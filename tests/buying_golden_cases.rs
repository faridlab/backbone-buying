//! Golden oracle for the buying write path (procure-to-pay intent). Buying-only (no inventory) —
//! the receipt seam is proven in `receipt_seam.rs`. Requires DATABASE_URL (:5433/backbone_buying).

use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use uuid::Uuid;

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
    NewLine { item_id: item, warehouse_id: None, description: None, quantity: d(qty), rate: d(rate) }
}
async fn po(w: &BuyingWriteService, company: Uuid, item: Uuid, qty: &str, rate: &str, tax: &str) -> Uuid {
    w.create_purchase_order(NewPurchaseOrder {
        po_number: uq("PO"), supplier_quotation_id: None, order_kind: None, company_id: company,
        branch_id: None, supplier_id: Uuid::new_v4(), order_date: day(), schedule_date: None,
        currency: None, tax_rate: d(tax), notes: None,
        lines: vec![line(item, qty, rate)],
    }).await.unwrap()
}
async fn po_status(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT status::text FROM buying.purchase_orders WHERE id=$1").bind(id).fetch_one(pool).await.unwrap()
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

// BGC-2: confirm → to_receive_and_bill; the receipt request asks for the un-received qty.
#[tokio::test]
async fn confirm_then_receipt_request() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let id = po(&w, company, item, "10", "100000", "0").await;
    w.confirm_purchase_order(id).await.unwrap();
    assert_eq!(po_status(&pool, id).await, "to_receive_and_bill");
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
    w.confirm_purchase_order(id).await.unwrap();

    // Receiving 12 against a PO of 10 is refused; no over-receipt.
    let e = w.mark_received(id, &[(item, d("12"))]).await.unwrap_err();
    assert!(matches!(e, BuyingError::OverReceipt { .. }));
    let rq0: Decimal = sqlx::query_scalar("SELECT received_qty FROM buying.purchase_order_items WHERE order_id=$1").bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(rq0, d("0.0000"), "rejected receipt leaves the watermark untouched");

    // Receive exactly 10; then billing 15 against 10 received is refused (invoice > receipt).
    w.mark_received(id, &[(item, d("10"))]).await.unwrap();
    let e = w.mark_billed(id, &[(item, d("15"))]).await.unwrap_err();
    assert!(matches!(e, BuyingError::OverBilling { .. }));
    let bq0: Decimal = sqlx::query_scalar("SELECT billed_qty FROM buying.purchase_order_items WHERE order_id=$1").bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(bq0, d("0.0000"), "rejected billing leaves the watermark untouched");
    // Billing exactly 10 completes it.
    w.mark_billed(id, &[(item, d("10"))]).await.unwrap();
    assert_eq!(po_status(&pool, id).await, "completed");
}

// BGC-3: 3-way-match watermarks gate completion — received → to_bill; +billed → completed.
#[tokio::test]
async fn watermarks_gate_completion() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let id = po(&w, company, item, "10", "100000", "0").await;
    w.confirm_purchase_order(id).await.unwrap();

    w.mark_received(id, &[(item, d("10"))]).await.unwrap();
    assert_eq!(po_status(&pool, id).await, "to_bill", "received, awaiting billing");
    w.mark_billed(id, &[(item, d("10"))]).await.unwrap();
    assert_eq!(po_status(&pool, id).await, "completed", "received AND billed → completed");
    let (rq, bq): (Decimal, Decimal) = sqlx::query_as("SELECT received_qty, billed_qty FROM buying.purchase_order_items WHERE order_id=$1")
        .bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(rq, d("10.0000"));
    assert_eq!(bq, d("10.0000"));
}

// BGC-4: partial receipt stays to_receive_and_bill; the next receipt request asks only the remainder.
#[tokio::test]
async fn partial_receipt_requests_remainder() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let id = po(&w, company, item, "10", "100000", "0").await;
    w.confirm_purchase_order(id).await.unwrap();
    w.mark_received(id, &[(item, d("4"))]).await.unwrap();
    assert_eq!(po_status(&pool, id).await, "to_receive_and_bill", "partial receipt, still awaiting both");
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
        currency: None, tax_rate: Decimal::ZERO, notes: None, lines: vec![],
    }).await.unwrap_err();
    assert!(matches!(e, BuyingError::EmptyDocument));
    // duplicate PO number
    let num = uq("DUP");
    let mut a = NewPurchaseOrder { po_number: num.clone(), supplier_quotation_id: None, order_kind: None,
        company_id: company, branch_id: None, supplier_id: Uuid::new_v4(), order_date: day(), schedule_date: None,
        currency: None, tax_rate: Decimal::ZERO, notes: None, lines: vec![line(item, "1", "10")] };
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
        schedule_date: None, currency: None, tax_rate: Decimal::ZERO, notes: None,
        lines: vec![line(item, "1", "50000")],
    }).await.unwrap();
    let kind: String = sqlx::query_scalar("SELECT order_kind::text FROM buying.purchase_orders WHERE id=$1").bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(kind, "subcontract");
}

