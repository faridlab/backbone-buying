//! Guard matrix for the purchase-order lifecycle band (draft / sent / to_approve / purchase /
//! cancelled): every lifecycle verb, once through its happy path and once through its refusal,
//! plus the database trigger backstops that catch a raw write bypassing the service.
//!
//! Two layers are proven per guard: the service pre-check returns the typed `BuyingError`, and the
//! matching BEFORE-trigger on the table refuses the same write attempted as raw SQL (the service
//! guard could otherwise mask a trigger that never fires). Delivery/billing maturity is NOT part of
//! the band — it lives on the stored computes `receipt_status` / `invoice_status`, read here where
//! a guard interacts with the watermarks (the billed-lines cancel guard).
//!
//! Requires DATABASE_URL (:5433/backbone_buying, migrated with this module's migrations).

use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use backbone_buying::application::service::buying_write_service::{
    BuyingError, BuyingWriteService, NewLine, NewPurchaseOrder,
};

fn d(s: &str) -> Decimal { Decimal::from_str_exact(s).unwrap() }
fn day() -> chrono::NaiveDate { chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap() }
fn uq(p: &str) -> String { format!("{p}-{}", &Uuid::new_v4().simple().to_string()[..8]) }
async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/backbone_buying".to_string());
    PgPool::connect(&url).await.expect("connect DB")
}
fn line(item: Uuid, qty: &str, rate: &str) -> NewLine {
    NewLine { item_id: item, warehouse_id: None, description: None, quantity: d(qty), rate: d(rate), qty_received_method: None, purchase_method: None }
}

/// Create a same-currency (rate 1) draft PO: 10 × 100,000 = 1,000,000 total, no tax.
async fn po(w: &BuyingWriteService, company: Uuid, item: Uuid) -> Uuid {
    w.create_purchase_order(NewPurchaseOrder {
        po_number: uq("PO"), supplier_quotation_id: None, order_kind: None, company_id: company,
        branch_id: None, supplier_id: Uuid::new_v4(), order_date: day(), schedule_date: None,
        currency: None, currency_rate: None, agreement_id: None, project_id: None, tax_rate: Decimal::ZERO, notes: None,
        lines: vec![line(item, "10", "100000")],
    }).await.unwrap()
}

/// Create a USD draft PO carrying the order-time rate snapshot 1,000 (company currency IDR per
/// USD): 10 × 100,000 USD = 1,000,000 USD → converts to 1,000,000,000 IDR.
async fn fx_po(w: &BuyingWriteService, company: Uuid, item: Uuid) -> Uuid {
    w.create_purchase_order(NewPurchaseOrder {
        po_number: uq("PO"), supplier_quotation_id: None, order_kind: None, company_id: company,
        branch_id: None, supplier_id: Uuid::new_v4(), order_date: day(), schedule_date: None,
        currency: Some("USD".into()), currency_rate: Some(d("1000")), agreement_id: None,
        project_id: None, tax_rate: Decimal::ZERO, notes: None,
        lines: vec![line(item, "10", "100000")],
    }).await.unwrap()
}

async fn po_status(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT status::text FROM buying.purchase_orders WHERE id=$1").bind(id).fetch_one(pool).await.unwrap()
}
async fn line_id(pool: &PgPool, order: Uuid, item: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT id FROM buying.purchase_order_items WHERE order_id=$1 AND item_id=$2 AND (metadata->>'deleted_at') IS NULL")
        .bind(order).bind(item).fetch_one(pool).await.unwrap()
}
/// Whether a raw SQL statement failed with a database error carrying `needle` (the guard trigger's
/// message tag). Proves the DB backstop fired for a write that bypassed the service.
fn db_refusal(e: &sqlx::Error, needle: &str) -> bool {
    e.as_database_error().map(|d| d.message().contains(needle)).unwrap_or(false)
}

/// Configure two-step double validation with a 1,000,000,000 IDR threshold. Only the foreign-
/// currency test POs (raw total 1,000,000 USD, converted 1,000,000,000 IDR) reach it; the
/// same-currency POs used across the suite stay far below, so this row cannot park another test's
/// confirm (the test connection is a DB superuser and so is not fenced by the company RLS policy
/// the HTTP layer applies).
async fn seed_two_step(pool: &PgPool, company: Uuid) {
    sqlx::query(
        r#"INSERT INTO buying.purchase_company_settings
               (company_id, double_validation, double_validation_amount, company_currency)
           VALUES ($1, 'two_step', 1000000000, 'IDR')
           ON CONFLICT (company_id) WHERE (metadata->>'deleted_at') IS NULL DO UPDATE SET
               double_validation = EXCLUDED.double_validation,
               double_validation_amount = EXCLUDED.double_validation_amount,
               company_currency = EXCLUDED.company_currency"#,
    )
    .bind(company)
    .execute(pool).await.expect("seed two-step purchase settings");
}

// T2: the approve verb re-checks the double-validation gate with the same currency conversion the
// confirm used. A non-manager claim cannot walk an over-threshold PO through; a manager claim
// finishes the parked confirm into `purchase`.
#[tokio::test]
async fn approve_verb_rechecks_the_gate() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    seed_two_step(&pool, company).await;

    // Non-manager confirm parks the over-threshold (converted) PO.
    let id = fx_po(&w, company, item).await;
    w.confirm_purchase_order(id, false).await.unwrap();
    assert_eq!(po_status(&pool, id).await, "to_approve");

    // REFUSAL: the approve verb re-checks — a non-manager is refused, the PO stays parked.
    assert!(matches!(w.approve_purchase_order(id, false).await.unwrap_err(), BuyingError::NotApprovable { .. }));
    assert_eq!(po_status(&pool, id).await, "to_approve");

    // HAPPY: the manager claim finishes the confirm into the operational state.
    w.approve_purchase_order(id, true).await.unwrap();
    assert_eq!(po_status(&pool, id).await, "purchase");
}

// T3: reset walks a cancelled PO back to draft (rework of a cancelled order is reachable); a PO
// already in draft has no reset edge and refuses.
#[tokio::test]
async fn reset_verb_returns_cancelled_to_draft() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let id = po(&w, company, item).await;

    // HAPPY: cancelled → draft.
    w.cancel_purchase_order(id).await.unwrap();
    assert_eq!(po_status(&pool, id).await, "cancelled");
    w.reset_purchase_order(id).await.unwrap();
    assert_eq!(po_status(&pool, id).await, "draft");

    // REFUSAL: a draft PO has nothing to reset from.
    assert!(matches!(w.reset_purchase_order(id).await.unwrap_err(), BuyingError::NotCancelable { .. }));
    assert_eq!(po_status(&pool, id).await, "draft");
}

// T4: cancel terminates a live order; a cancelled order has no cancel edge (double cancel refused).
#[tokio::test]
async fn cancel_verb_terminates_a_live_order() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let id = po(&w, company, item).await;

    // HAPPY: purchase → cancelled.
    w.confirm_purchase_order(id, false).await.unwrap();
    w.cancel_purchase_order(id).await.unwrap();
    assert_eq!(po_status(&pool, id).await, "cancelled");

    // REFUSAL: cancelling again has no edge to walk.
    assert!(matches!(w.cancel_purchase_order(id).await.unwrap_err(), BuyingError::NotCancelable { .. }));
    assert_eq!(po_status(&pool, id).await, "cancelled");
}

// G5: cancel refuses while any live line is billed; once the billing is fully credited back, the
// same cancel succeeds. The watermark trail (not the lifecycle band) is what the guard reads.
#[tokio::test]
async fn cancel_refused_while_billed() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let id = po(&w, company, item).await;
    w.confirm_purchase_order(id, false).await.unwrap();
    w.mark_received(id, company, &[(item, d("10"))]).await.unwrap();
    w.mark_billed(id, company, &[(item, d("3"))]).await.unwrap();

    // REFUSAL: billed_qty > 0 on a live line → typed refusal, order stays live.
    assert!(matches!(w.cancel_purchase_order(id).await.unwrap_err(), BuyingError::OrderBilled { .. }));
    assert_eq!(po_status(&pool, id).await, "purchase");

    // HAPPY: credit the billing back to zero → the same cancel goes through.
    w.mark_credited(id, company, &[(item, d("3"))]).await.unwrap();
    w.cancel_purchase_order(id).await.unwrap();
    assert_eq!(po_status(&pool, id).await, "cancelled");
}

// G4: a locked order refuses cancel — through the service pre-check AND through the DB trigger when
// the cancel is attempted as a raw write. Unlocking releases the guard.
#[tokio::test]
async fn locked_order_refuses_cancel() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let id = po(&w, company, item).await;
    w.confirm_purchase_order(id, false).await.unwrap();
    w.lock_purchase_order(id).await.unwrap();

    // REFUSAL (service layer): the typed error, order stays operational.
    assert!(matches!(w.cancel_purchase_order(id).await.unwrap_err(), BuyingError::OrderLocked { .. }));
    assert_eq!(po_status(&pool, id).await, "purchase");

    // REFUSAL (DB backstop): the same cancel as a raw UPDATE — the po_write_guards trigger raises,
    // so a caller bypassing the service still cannot cancel a locked order.
    let raw = sqlx::query("UPDATE buying.purchase_orders SET status='cancelled'::purchase_order_status WHERE id=$1")
        .bind(id).execute(&pool).await;
    assert!(raw.as_ref().err().map(|e| db_refusal(e, "po_cancel_locked")).unwrap_or(false),
        "the DB trigger must refuse a raw cancel of a locked order");

    // HAPPY: unlock releases the guard and the cancel proceeds.
    w.unlock_purchase_order(id).await.unwrap();
    w.cancel_purchase_order(id).await.unwrap();
    assert_eq!(po_status(&pool, id).await, "cancelled");
}

// G8: soft-delete and hard-delete both require the order to be cancelled first — the service
// refuses on a live order, the DB triggers refuse a raw attempt, and after a real cancel the
// soft-delete succeeds.
#[tokio::test]
async fn delete_requires_cancelled_order() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let id = po(&w, company, item).await;
    w.confirm_purchase_order(id, false).await.unwrap();

    // REFUSAL (service layer): a live order is not deletable.
    assert!(matches!(w.delete_purchase_order(id).await.unwrap_err(), BuyingError::NotDeletable { .. }));
    assert_eq!(po_status(&pool, id).await, "purchase");

    // REFUSAL (DB backstops): both the soft-delete shape (metadata stamp on a live row) and the
    // hard-delete shape (DELETE on a live row) are refused by the triggers.
    let soft = sqlx::query("UPDATE buying.purchase_orders SET metadata = jsonb_set(metadata, '{deleted_at}', to_jsonb(NOW())) WHERE id=$1")
        .bind(id).execute(&pool).await;
    assert!(soft.as_ref().err().map(|e| db_refusal(e, "po_delete_requires_cancelled")).unwrap_or(false),
        "the soft-delete guard trigger must refuse a live order");
    let hard = sqlx::query("DELETE FROM buying.purchase_orders WHERE id=$1")
        .bind(id).execute(&pool).await;
    assert!(hard.as_ref().err().map(|e| db_refusal(e, "po_delete_requires_cancelled")).unwrap_or(false),
        "the hard-delete guard trigger must refuse a live order");

    // HAPPY: cancel, then the soft-delete lands (deleted_at stamped).
    w.cancel_purchase_order(id).await.unwrap();
    w.delete_purchase_order(id).await.unwrap();
    let deleted: Option<String> = sqlx::query_scalar("SELECT metadata->>'deleted_at' FROM buying.purchase_orders WHERE id=$1")
        .bind(id).fetch_one(&pool).await.unwrap();
    assert!(deleted.is_some(), "the cancelled order is soft-deleted");

    // REFUSAL: an already-deleted order has nothing left to delete.
    assert!(matches!(w.delete_purchase_order(id).await.unwrap_err(), BuyingError::NotDeletable { .. }));
}

// G9: a PO line is deletable only while its parent order is editable (draft/sent) — through the
// service and through the lines trigger on a raw write; once the order is confirmed, the line is
// locked to the order's history.
#[tokio::test]
async fn line_delete_requires_editable_order() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item_a, item_b) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let id = w.create_purchase_order(NewPurchaseOrder {
        po_number: uq("PO"), supplier_quotation_id: None, order_kind: None, company_id: company,
        branch_id: None, supplier_id: Uuid::new_v4(), order_date: day(), schedule_date: None,
        currency: None, currency_rate: None, agreement_id: None, project_id: None, tax_rate: Decimal::ZERO, notes: None,
        lines: vec![line(item_a, "10", "100000"), line(item_b, "5", "50000")],
    }).await.unwrap();

    // HAPPY: while the order is draft, deleting a line soft-deletes exactly that line.
    let la = line_id(&pool, id, item_a).await;
    w.delete_purchase_order_line(id, la).await.unwrap();
    let la_deleted: Option<String> = sqlx::query_scalar("SELECT metadata->>'deleted_at' FROM buying.purchase_order_items WHERE id=$1")
        .bind(la).fetch_one(&pool).await.unwrap();
    assert!(la_deleted.is_some(), "the draft order's line is soft-deleted");
    let live: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM buying.purchase_order_items WHERE order_id=$1 AND (metadata->>'deleted_at') IS NULL")
        .bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(live, 1, "the sibling line stays live");

    // REFUSAL: once the order is confirmed, its remaining line is not deletable.
    w.confirm_purchase_order(id, false).await.unwrap();
    let lb = line_id(&pool, id, item_b).await;
    assert!(matches!(w.delete_purchase_order_line(id, lb).await.unwrap_err(), BuyingError::NotDeletable { .. }));

    // REFUSAL (DB backstop): the same soft-delete as a raw UPDATE — the po_item_write_guards
    // trigger raises, so a caller bypassing the service still cannot delete the line.
    let raw = sqlx::query("UPDATE buying.purchase_order_items SET metadata = jsonb_set(metadata, '{deleted_at}', to_jsonb(NOW())) WHERE id=$1")
        .bind(lb).execute(&pool).await;
    assert!(raw.as_ref().err().map(|e| db_refusal(e, "po_item_delete_requires_editable_order")).unwrap_or(false),
        "the line-delete guard trigger must refuse a non-editable parent order");
}

// T6: send walks a draft PO to sent (printed/sent to the supplier, no longer editable); a PO
// already out of draft has no send edge and refuses.
#[tokio::test]
async fn send_verb_walks_draft_to_sent() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let id = po(&w, company, item).await;

    // HAPPY: draft → sent.
    w.send_purchase_order(id).await.unwrap();
    assert_eq!(po_status(&pool, id).await, "sent");

    // REFUSAL: an already-sent PO cannot be sent again.
    assert!(matches!(w.send_purchase_order(id).await.unwrap_err(), BuyingError::NotConfirmable { .. }));
    assert_eq!(po_status(&pool, id).await, "sent");
}
