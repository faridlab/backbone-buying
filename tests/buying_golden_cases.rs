//! Golden oracle for the buying write path (procure-to-pay intent). Buying-only (no inventory) —
//! the receipt seam is proven in `receipt_seam.rs`. Requires DATABASE_URL (:5433/backbone_buying).

use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use backbone_buying::application::service::buying_events::{BuyingEvent, BuyingEventSink};
use backbone_buying::application::service::buying_write_service::{
    BuyingError, BuyingWriteService, NewLine, NewMaterialRequest, NewPurchaseOrder,
    NewSupplierQuotation, SimpleLine,
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
async fn po(w: &BuyingWriteService, company: Uuid, item: Uuid, qty: &str, rate: &str, tax: &str) -> Uuid {
    w.create_purchase_order(NewPurchaseOrder {
        po_number: uq("PO"), supplier_quotation_id: None, order_kind: None, company_id: company,
        branch_id: None, supplier_id: Uuid::new_v4(), order_date: day(), schedule_date: None,
        currency: None, currency_rate: None, agreement_id: None, project_id: None, tax_rate: d(tax), notes: None,
        lines: vec![line(item, qty, rate)],
    }).await.unwrap()
}
async fn po_status(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT status::text FROM buying.purchase_orders WHERE id=$1").bind(id).fetch_one(pool).await.unwrap()
}
/// The PO's delivery/billing maturity, as the stored computes hold it: `(receipt_status,
/// invoice_status)` — `pending/partial/full` by receipt, `no/to_invoice/invoiced` by billing. The
/// lifecycle band itself (`status`) has no watermark states; maturity lives ONLY here.
async fn po_maturity(pool: &PgPool, id: Uuid) -> (String, String) {
    sqlx::query_as("SELECT receipt_status::text, invoice_status::text FROM buying.purchase_orders WHERE id=$1")
        .bind(id).fetch_one(pool).await.unwrap()
}

// BGC-1: PO line + total math — 10 × 100,000, PPN Input 11% → subtotal 1,000,000, tax 110,000, total 1,110,000.
#[tokio::test]
async fn po_line_and_total_math() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let id = po(&w, company, item, "10", "100000", "11").await;
    let row = sqlx::query("SELECT subtotal, tax_amount, total FROM buying.purchase_orders WHERE id=$1")
        .bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(row.get::<Decimal, _>("subtotal"), d("1000000"));
    assert_eq!(row.get::<Decimal, _>("tax_amount"), d("110000.00"));
    assert_eq!(row.get::<Decimal, _>("total"), d("1110000.00"));
}

// BGC-2: confirm → purchase; nothing received or billed yet (both computes at their floors); the
// receipt request asks for the un-received qty.
#[tokio::test]
async fn confirm_then_receipt_request() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let id = po(&w, company, item, "10", "100000", "0").await;
    w.confirm_purchase_order(id, false).await.unwrap();
    assert_eq!(po_status(&pool, id).await, "purchase");
    assert_eq!(po_maturity(&pool, id).await, ("pending".into(), "no".into()));
    let req = w.build_receipt_request(id).await.unwrap();
    assert_eq!(req.lines.len(), 1);
    assert_eq!(req.lines[0].quantity, d("10.0000"));
    assert_eq!(req.lines[0].rate, d("100000.00"));
}

// BGC-7 (council 2026-07-05): the 3-way-match invariant is enforced — over-receipt and over-billing
// are rejected (billed ≤ received ≤ ordered). Without this, a PO could certify payment for goods
// never received.
#[tokio::test]
async fn over_receipt_and_over_billing_rejected() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let id = po(&w, company, item, "10", "100000", "0").await;
    w.confirm_purchase_order(id, false).await.unwrap();

    // Receiving 12 against a PO of 10 is refused; no over-receipt.
    let e = w.mark_received(id, company, &[(item, d("12"))]).await.unwrap_err();
    assert!(matches!(e, BuyingError::OverReceipt { .. }));
    let rq0: Decimal = sqlx::query_scalar("SELECT received_qty FROM buying.purchase_order_items WHERE order_id=$1").bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(rq0, d("0.0000"), "rejected receipt leaves the watermark untouched");

    // Receive exactly 10; then billing 15 against 10 received is refused (invoice > receipt).
    w.mark_received(id, company, &[(item, d("10"))]).await.unwrap();
    let e = w.mark_billed(id, company, &[(item, d("15"))]).await.unwrap_err();
    assert!(matches!(e, BuyingError::OverBilling { .. }));
    let bq0: Decimal = sqlx::query_scalar("SELECT billed_qty FROM buying.purchase_order_items WHERE order_id=$1").bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(bq0, d("0.0000"), "rejected billing leaves the watermark untouched");
    // Billing exactly 10 completes it: invoice capacity exhausted, billing history present.
    w.mark_billed(id, company, &[(item, d("10"))]).await.unwrap();
    assert_eq!(po_status(&pool, id).await, "purchase");
    assert_eq!(po_maturity(&pool, id).await, ("full".into(), "invoiced".into()));
}

// BGC-3: 3-way-match watermarks gate the billing maturity compute — fully received →
// invoice_status to_invoice (received, awaiting billing); +fully billed → invoiced. The lifecycle
// band stays `purchase` throughout: delivery/billing maturity is carried by the computes.
#[tokio::test]
async fn watermarks_gate_completion() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let id = po(&w, company, item, "10", "100000", "0").await;
    w.confirm_purchase_order(id, false).await.unwrap();

    w.mark_received(id, company, &[(item, d("10"))]).await.unwrap();
    assert_eq!(po_status(&pool, id).await, "purchase");
    assert_eq!(po_maturity(&pool, id).await, ("full".into(), "to_invoice".into()), "received, awaiting billing");
    w.mark_billed(id, company, &[(item, d("10"))]).await.unwrap();
    assert_eq!(po_status(&pool, id).await, "purchase");
    assert_eq!(po_maturity(&pool, id).await, ("full".into(), "invoiced".into()), "received AND billed → invoiced");
    let (rq, bq): (Decimal, Decimal) = sqlx::query_as("SELECT received_qty, billed_qty FROM buying.purchase_order_items WHERE order_id=$1")
        .bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(rq, d("10.0000"));
    assert_eq!(bq, d("10.0000"));
}

// BGC-4: partial receipt keeps the order awaiting both goods and billing — receipt_status partial
// (not everything received), invoice_status to_invoice (the received portion is invoiceable); the
// next receipt request asks only the remainder.
#[tokio::test]
async fn partial_receipt_requests_remainder() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let id = po(&w, company, item, "10", "100000", "0").await;
    w.confirm_purchase_order(id, false).await.unwrap();
    w.mark_received(id, company, &[(item, d("4"))]).await.unwrap();
    assert_eq!(po_status(&pool, id).await, "purchase");
    assert_eq!(po_maturity(&pool, id).await, ("partial".into(), "to_invoice".into()), "partial receipt, still awaiting both");
    let req = w.build_receipt_request(id).await.unwrap();
    assert_eq!(req.lines[0].quantity, d("6.0000"), "requests only the un-received remainder");
}

// BGC-5: material request + supplier quotation creates; validation gates.
#[tokio::test]
async fn intent_creates_and_validation() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let mr = w.create_material_request(NewMaterialRequest {
        request_number: uq("MR"), company_id: company, request_type: None, request_date: day(),
        schedule_date: None, notes: None, lines: vec![SimpleLine { item_id: item, quantity: d("5") }],
    }).await.unwrap();
    let cnt: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM buying.material_request_items WHERE request_id=$1").bind(mr).fetch_one(&pool).await.unwrap();
    assert_eq!(cnt, 1);

    let sq = w.create_supplier_quotation(NewSupplierQuotation {
        quotation_number: uq("SQ"), rfq_id: None, company_id: company, supplier_id: Uuid::new_v4(),
        quotation_date: day(), valid_till: None, currency: None,
        lines: vec![line(item, "5", "90000")],
    }).await.unwrap();
    let rate: Decimal = sqlx::query_scalar("SELECT rate FROM buying.supplier_quotation_items WHERE quotation_id=$1").bind(sq).fetch_one(&pool).await.unwrap();
    assert_eq!(rate, d("90000.00"));

    // empty PO / negative rate rejected
    let e = w.create_purchase_order(NewPurchaseOrder {
        po_number: uq("PO"), supplier_quotation_id: None, order_kind: None, company_id: company,
        branch_id: None, supplier_id: Uuid::new_v4(), order_date: day(), schedule_date: None,
        currency: None, currency_rate: None, agreement_id: None, project_id: None, tax_rate: Decimal::ZERO, notes: None, lines: vec![],
    }).await.unwrap_err();
    assert!(matches!(e, BuyingError::EmptyDocument));
    // duplicate PO number
    let num = uq("DUP");
    let mut a = NewPurchaseOrder { po_number: num.clone(), supplier_quotation_id: None, order_kind: None,
        company_id: company, branch_id: None, supplier_id: Uuid::new_v4(), order_date: day(), schedule_date: None,
        currency: None, currency_rate: None, agreement_id: None, project_id: None, tax_rate: Decimal::ZERO, notes: None, lines: vec![line(item, "1", "10")] };
    w.create_purchase_order(a.clone()).await.unwrap();
    a.po_number = num;
    assert!(matches!(w.create_purchase_order(a).await.unwrap_err(), BuyingError::DuplicateNumber(_)));
}

// BGC-6: subcontract order_kind persists (subcontracting folds in as a PO subtype).
#[tokio::test]
async fn subcontract_order_kind() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let id = w.create_purchase_order(NewPurchaseOrder {
        po_number: uq("SCO"), supplier_quotation_id: None, order_kind: Some("subcontract".into()),
        company_id: company, branch_id: None, supplier_id: Uuid::new_v4(), order_date: day(),
        schedule_date: None, currency: None, currency_rate: None, agreement_id: None, project_id: None, tax_rate: Decimal::ZERO, notes: None,
        lines: vec![line(item, "1", "50000")],
    }).await.unwrap();
    let kind: String = sqlx::query_scalar("SELECT order_kind::text FROM buying.purchase_orders WHERE id=$1").bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(kind, "subcontract");
}

// BGC-8 (council 2026-07-28): reverse watermarks — a credit note decrements billed_qty and drags a
// fully-billed PO back to to_invoice; a purchase return decrements received_qty and drops
// receipt_status back to partial. The 3-way-match invariants survive reversal
// (billed ≤ received ≤ ordered; non-negative).
async fn wms(pool: &PgPool, id: Uuid) -> (Decimal, Decimal) {
    let (rq, bq): (Decimal, Decimal) = sqlx::query_as(
        "SELECT received_qty, billed_qty FROM buying.purchase_order_items WHERE order_id=$1")
        .bind(id).fetch_one(pool).await.unwrap();
    (rq, bq)
}

// A credit note for 3 of 10 billed reopens the billing maturity: billed_qty 10→7, invoice_status
// invoiced→to_invoice (delivery stays full — all goods were received).
#[tokio::test]
async fn credit_note_reopens_completed() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let id = po(&w, company, item, "10", "100000", "0").await;
    w.confirm_purchase_order(id, false).await.unwrap();
    w.mark_received(id, company, &[(item, d("10"))]).await.unwrap();
    w.mark_billed(id, company, &[(item, d("10"))]).await.unwrap();
    assert_eq!(po_status(&pool, id).await, "purchase");
    assert_eq!(po_maturity(&pool, id).await, ("full".into(), "invoiced".into()));

    w.mark_credited(id, company, &[(item, d("3"))]).await.unwrap();
    assert_eq!(wms(&pool, id).await, (d("10.0000"), d("7.0000")), "credit decrements billed_qty only");
    assert_eq!(po_status(&pool, id).await, "purchase");
    assert_eq!(po_maturity(&pool, id).await, ("full".into(), "to_invoice".into()), "received all, no longer fully billed");
}

// A return of 4 of 10 received (none billed) reopens delivery: received_qty 10→6,
// receipt_status full→partial (billing maturity stays to_invoice — nothing billed, goods to invoice).
#[tokio::test]
async fn purchase_return_reopens_po() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let id = po(&w, company, item, "10", "100000", "0").await;
    w.confirm_purchase_order(id, false).await.unwrap();
    w.mark_received(id, company, &[(item, d("10"))]).await.unwrap();
    assert_eq!(po_status(&pool, id).await, "purchase");
    assert_eq!(po_maturity(&pool, id).await, ("full".into(), "to_invoice".into()));

    w.mark_returned(id, company, &[(item, d("4"))]).await.unwrap();
    assert_eq!(wms(&pool, id).await, (d("6.0000"), d("0.0000")), "return decrements received_qty only");
    assert_eq!(po_status(&pool, id).await, "purchase");
    assert_eq!(po_maturity(&pool, id).await, ("partial".into(), "to_invoice".into()), "no longer fully received");
}

// All received goods are billed → returnable portion is 0; a return is refused (credit first).
#[tokio::test]
async fn over_return_on_billed_goods_rejected() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let id = po(&w, company, item, "10", "100000", "0").await;
    w.confirm_purchase_order(id, false).await.unwrap();
    w.mark_received(id, company, &[(item, d("10"))]).await.unwrap();
    w.mark_billed(id, company, &[(item, d("10"))]).await.unwrap();

    let e = w.mark_returned(id, company, &[(item, d("1"))]).await.unwrap_err();
    assert!(matches!(e, BuyingError::OverReturn { .. }));
    assert_eq!(wms(&pool, id).await, (d("10.0000"), d("10.0000")), "rejected return leaves watermarks untouched");
}

// Only 7 of 10 received were billed → crediting 10 is refused.
#[tokio::test]
async fn over_credit_rejected() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let id = po(&w, company, item, "10", "100000", "0").await;
    w.confirm_purchase_order(id, false).await.unwrap();
    w.mark_received(id, company, &[(item, d("10"))]).await.unwrap();
    w.mark_billed(id, company, &[(item, d("7"))]).await.unwrap();

    let e = w.mark_credited(id, company, &[(item, d("10"))]).await.unwrap_err();
    assert!(matches!(e, BuyingError::OverCredit { .. }));
    let (_, bq) = wms(&pool, id).await;
    assert_eq!(bq, d("7.0000"), "rejected credit leaves billed_qty untouched");
}

// Credit 3 first (frees 3 received from billing), then return 3 — billed ≤ received holds throughout.
#[tokio::test]
async fn return_after_credit_chain() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let id = po(&w, company, item, "10", "100000", "0").await;
    w.confirm_purchase_order(id, false).await.unwrap();
    w.mark_received(id, company, &[(item, d("10"))]).await.unwrap();
    w.mark_billed(id, company, &[(item, d("10"))]).await.unwrap();

    w.mark_credited(id, company, &[(item, d("3"))]).await.unwrap();
    w.mark_returned(id, company, &[(item, d("3"))]).await.unwrap();
    assert_eq!(wms(&pool, id).await, (d("7.0000"), d("7.0000")), "credit then return keeps billed ≤ received");
    assert_eq!(po_status(&pool, id).await, "purchase");
    // received 7 of 10 → partial; billed 7 of the 7 received → invoiced (nothing left to invoice).
    assert_eq!(po_maturity(&pool, id).await, ("partial".into(), "invoiced".into()));
}

// --- the double-validation gate (multi-currency) -----------------------------
//
// The gate threshold is denominated in the COMPANY currency: the comparison converts the PO total
// INTO company currency with the order-time `currency_rate` snapshot (`total * currency_rate >=
// threshold`). A two_step-configured company parks an over-threshold PO in `to_approve` on a
// non-manager confirm; the manager approve verb re-checks the SAME conversion and walks it into
// `purchase`. The boundary is inclusive (`>=`): at-threshold parks, one step under passes.

#[derive(Default, Clone)]
struct Rec { events: Arc<Mutex<Vec<BuyingEvent>>> }
impl BuyingEventSink for Rec { fn publish(&self, e: BuyingEvent) { self.events.lock().unwrap().push(e); } }
impl Rec {
    fn count(&self, pred: impl Fn(&BuyingEvent) -> bool) -> usize {
        self.events.lock().unwrap().iter().filter(|e| pred(e)).count()
    }
}

/// Configure two-step double validation for `company`, threshold in the company currency (IDR).
/// Deliberately high: every other PO total in this suite is far below it, so this settings row
/// cannot flip another test's confirm into a park even though the test connection (a DB superuser)
/// is not fenced by the company RLS policy the HTTP layer applies.
async fn seed_two_step(pool: &PgPool, company: Uuid, threshold: Decimal) {
    sqlx::query(
        r#"INSERT INTO buying.purchase_company_settings
               (company_id, double_validation, double_validation_amount, company_currency)
           VALUES ($1, 'two_step', $2, 'IDR')
           ON CONFLICT (company_id) WHERE (metadata->>'deleted_at') IS NULL DO UPDATE SET
               double_validation = EXCLUDED.double_validation,
               double_validation_amount = EXCLUDED.double_validation_amount,
               company_currency = EXCLUDED.company_currency"#,
    )
    .bind(company).bind(threshold)
    .execute(pool).await.expect("seed two-step purchase settings");
}

// BGC-9: the gate compares the CONVERTED amount. The PO's raw total (1,000,000 USD) sits far below
// the threshold (1,000,000,000 IDR) — only the order-time rate snapshot (1,000 IDR per USD) brings
// it to the boundary, so a park here proves the conversion happened, and one rate-unit step under
// passes straight through. The approve verb then re-checks the same conversion: a non-manager claim
// is refused, a manager claim finishes the confirm.
#[tokio::test]
async fn double_validation_gate_converts_currency() {
    let pool = pool().await;
    let rec = Rec::default();
    let w = BuyingWriteService::with_sink(pool.clone(), Arc::new(rec.clone()));
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    seed_two_step(&pool, company, d("1000000000")).await;

    // AT the threshold (inclusive boundary): 10 × 100,000 USD = 1,000,000 USD total;
    // 1,000,000 × 1,000 = 1,000,000,000 IDR = threshold → a non-manager confirm parks it.
    let at = w.create_purchase_order(NewPurchaseOrder {
        po_number: uq("PO"), supplier_quotation_id: None, order_kind: None, company_id: company,
        branch_id: None, supplier_id: Uuid::new_v4(), order_date: day(), schedule_date: None,
        currency: Some("USD".into()), currency_rate: Some(d("1000")), agreement_id: None,
        project_id: None, tax_rate: Decimal::ZERO, notes: None, lines: vec![line(item, "10", "100000")],
    }).await.unwrap();
    let snap: Decimal = sqlx::query_scalar("SELECT currency_rate FROM buying.purchase_orders WHERE id=$1")
        .bind(at).fetch_one(&pool).await.unwrap();
    assert_eq!(snap, d("1000.000000"), "the order-time rate snapshot rides the PO row");
    w.confirm_purchase_order(at, false).await.unwrap();
    assert_eq!(po_status(&pool, at).await, "to_approve", "converted total AT the threshold needs a manager");
    assert_eq!(rec.count(|e| matches!(e, BuyingEvent::PurchaseOrderPendingApproval(p) if p.order_id == at)), 1,
        "the park publishes PurchaseOrderPendingApproval");
    assert_eq!(rec.count(|e| matches!(e, BuyingEvent::PurchaseOrderConfirmed(c) if c.order_id == at)), 0,
        "a parked PO is NOT confirmed yet");

    // Just UNDER the threshold: 10 × 99,999 USD = 999,990 USD; × 1,000 = 999,990,000 IDR — one
    // rate-unit step below. The same non-manager confirm passes the gate straight into purchase.
    let under = w.create_purchase_order(NewPurchaseOrder {
        po_number: uq("PO"), supplier_quotation_id: None, order_kind: None, company_id: company,
        branch_id: None, supplier_id: Uuid::new_v4(), order_date: day(), schedule_date: None,
        currency: Some("USD".into()), currency_rate: Some(d("1000")), agreement_id: None,
        project_id: None, tax_rate: Decimal::ZERO, notes: None, lines: vec![line(item, "10", "99999")],
    }).await.unwrap();
    w.confirm_purchase_order(under, false).await.unwrap();
    assert_eq!(po_status(&pool, under).await, "purchase", "one step under the converted threshold passes the gate");
    assert_eq!(rec.count(|e| matches!(e, BuyingEvent::PurchaseOrderConfirmed(c) if c.order_id == under)), 1);

    // The approve verb re-checks the gate with the same conversion: a non-manager claim cannot
    // walk the over-threshold PO through; a manager claim finishes it into purchase — confirming
    // the parked PO exactly once, on the manager leg.
    assert!(matches!(w.approve_purchase_order(at, false).await.unwrap_err(), BuyingError::NotApprovable { .. }),
        "non-manager approve re-refuses the over-threshold PO");
    assert_eq!(po_status(&pool, at).await, "to_approve", "the refused approve leaves the PO parked");
    w.approve_purchase_order(at, true).await.unwrap();
    assert_eq!(po_status(&pool, at).await, "purchase");
    assert_eq!(rec.count(|e| matches!(e, BuyingEvent::PurchaseOrderConfirmed(c) if c.order_id == at)), 1,
        "manager approve confirms exactly once");
}

