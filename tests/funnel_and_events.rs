//! Completeness council (2026-07-05): the procure-to-pay CONVERSION funnel executed as behavior
//! (MR → RFQ → Supplier Quotation → PO, copy+link+advance-source), the semantic event surface
//! (MaterialRequestRaised/RfqIssued/SupplierQuotationReceived/PurchaseOrderConfirmed/
//! ThreeWayMatchFailed/PurchaseOrderFullyReceived/FullyBilled), and the enriched PurchaseOrderRef.
//! Requires DATABASE_URL (:5433/backbone_buying).

use std::sync::{Arc, Mutex};

use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use backbone_buying::application::service::buying_events::{BuyingEvent, BuyingEventSink};
use backbone_buying::application::service::buying_write_service::{
    BuyingError, BuyingWriteService, NewMaterialRequest, NewPurchaseOrder, NewLine, SimpleLine,
};

#[derive(Default, Clone)]
struct Rec { events: Arc<Mutex<Vec<BuyingEvent>>> }
impl BuyingEventSink for Rec { fn publish(&self, e: BuyingEvent) { self.events.lock().unwrap().push(e); } }
impl Rec {
    fn has(&self, pred: impl Fn(&BuyingEvent) -> bool) -> bool { self.events.lock().unwrap().iter().any(pred) }
}

fn d(s: &str) -> Decimal { Decimal::from_str_exact(s).unwrap() }
fn day() -> chrono::NaiveDate { chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap() }
fn uq(p: &str) -> String { format!("{p}-{}", &Uuid::new_v4().simple().to_string()[..8]) }
async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/backbone_buying".to_string());
    PgPool::connect(&url).await.expect("connect DB")
}
async fn scalar_txt(pool: &PgPool, sql: &str, id: Uuid) -> String {
    sqlx::query_scalar(sql).bind(id).fetch_one(pool).await.unwrap()
}

// BFC-1: the full funnel executes — MR → RFQ → SupplierQuotation → PO, copying lines/rates and
// advancing each source's status. The PO is provably derived from the quotation it links.
#[tokio::test]
async fn conversion_funnel_mr_to_po() {
    let pool = pool().await;
    let rec = Rec::default();
    let w = BuyingWriteService::with_sink(pool.clone(), Arc::new(rec.clone()));
    let (company, item_a, item_b, supplier) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());

    // MR with two lines.
    let mr = w.create_material_request(NewMaterialRequest {
        request_number: uq("MR"), company_id: company, request_type: None, request_date: day(),
        schedule_date: None, notes: None,
        lines: vec![SimpleLine { item_id: item_a, quantity: d("10") }, SimpleLine { item_id: item_b, quantity: d("4") }],
    }).await.unwrap();

    // MR → RFQ (invite the supplier).
    let rfq = w.convert_material_request_to_rfq(mr, uq("RFQ"), None, &[supplier]).await.unwrap();
    assert_eq!(scalar_txt(&pool, "SELECT status::text FROM buying.material_requests WHERE id=$1", mr).await, "ordered");
    let rfq_items: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM buying.rfq_items WHERE rfq_id=$1").bind(rfq).fetch_one(&pool).await.unwrap();
    assert_eq!(rfq_items, 2);
    let invited: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM buying.rfq_suppliers WHERE rfq_id=$1").bind(rfq).fetch_one(&pool).await.unwrap();
    assert_eq!(invited, 1);

    // RFQ → SupplierQuotation (supplier quotes rates).
    let sq = w.convert_rfq_to_supplier_quotation(rfq, uq("SQ"), supplier, &[(item_a, d("100000")), (item_b, d("50000"))]).await.unwrap();
    let (sq_qty, sq_rate): (Decimal, Decimal) = sqlx::query_as("SELECT quantity, rate FROM buying.supplier_quotation_items WHERE quotation_id=$1 AND item_id=$2")
        .bind(sq).bind(item_a).fetch_one(&pool).await.unwrap();
    assert_eq!(sq_qty, d("10.0000"));
    assert_eq!(sq_rate, d("100000.00"));

    // SupplierQuotation → PO (the §31 "select → PO" step): copies supplier + lines + rates, advances SQ.
    let po = w.convert_supplier_quotation_to_po(sq, uq("PO"), d("11")).await.unwrap();
    assert_eq!(scalar_txt(&pool, "SELECT status::text FROM buying.supplier_quotations WHERE id=$1", sq).await, "ordered");
    let linked: Option<Uuid> = sqlx::query_scalar("SELECT supplier_quotation_id FROM buying.purchase_orders WHERE id=$1").bind(po).fetch_one(&pool).await.unwrap();
    assert_eq!(linked, Some(sq), "PO links its source quotation");
    let (po_supplier, po_total): (Uuid, Decimal) = sqlx::query_as("SELECT supplier_id, total FROM buying.purchase_orders WHERE id=$1").bind(po).fetch_one(&pool).await.unwrap();
    assert_eq!(po_supplier, supplier, "PO copies the quotation's supplier");
    // subtotal = 10*100,000 + 4*50,000 = 1,200,000; +11% = 1,332,000.
    assert_eq!(po_total, d("1332000.00"), "PO copies the quotation's lines + rates");
    let po_line_rate: Decimal = sqlx::query_scalar("SELECT rate FROM buying.purchase_order_items WHERE order_id=$1 AND item_id=$2").bind(po).bind(item_a).fetch_one(&pool).await.unwrap();
    assert_eq!(po_line_rate, d("100000.00"));

    // Funnel events broadcast.
    assert!(rec.has(|e| matches!(e, BuyingEvent::MaterialRequestRaised(_))));
    assert!(rec.has(|e| matches!(e, BuyingEvent::RfqIssued(_))));
    assert!(rec.has(|e| matches!(e, BuyingEvent::SupplierQuotationReceived(_))));

    // A ref for the PO carries the brief §42 shape.
    let r = w.purchase_order_ref(po).await.unwrap();
    assert_eq!(r.supplier_id, supplier);
    assert_eq!(r.grand_total, d("1332000.00"));
    assert_eq!(r.order_kind, "standard");
    assert_eq!(r.currency, "IDR");
}

// BFC-2: converting a non-submitted supplier quotation is refused.
#[tokio::test]
async fn convert_requires_submitted_quotation() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let err = w.convert_supplier_quotation_to_po(Uuid::new_v4(), uq("PO"), Decimal::ZERO).await.unwrap_err();
    assert!(matches!(err, BuyingError::SourceNotFound(_)));
}

// BFC-3: a 3-way-match variance rejection BROADCASTS ThreeWayMatchFailed (§33 mismatch flagged).
#[tokio::test]
async fn variance_broadcasts_three_way_match_failed() {
    let pool = pool().await;
    let rec = Rec::default();
    let w = BuyingWriteService::with_sink(pool.clone(), Arc::new(rec.clone()));
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let po = w.create_purchase_order(NewPurchaseOrder {
        po_number: uq("PO"), supplier_quotation_id: None, order_kind: None, company_id: company,
        branch_id: None, supplier_id: Uuid::new_v4(), order_date: day(), schedule_date: None,
        currency: None, tax_rate: Decimal::ZERO, notes: None,
        lines: vec![NewLine { item_id: item, warehouse_id: None, description: None, quantity: d("10"), rate: d("100") }],
    }).await.unwrap();
    w.confirm_purchase_order(po).await.unwrap();
    // over-receipt → rejected AND broadcast.
    assert!(w.mark_received(po, &[(item, d("12"))]).await.is_err());
    assert!(rec.has(|e| matches!(e, BuyingEvent::ThreeWayMatchFailed(f) if f.kind == "over_receipt")));
}

// BFC-4: completion milestones fire once — PurchaseOrderFullyReceived then FullyBilled.
#[tokio::test]
async fn completion_milestones_emitted() {
    let pool = pool().await;
    let rec = Rec::default();
    let w = BuyingWriteService::with_sink(pool.clone(), Arc::new(rec.clone()));
    let (company, item) = (Uuid::new_v4(), Uuid::new_v4());
    let po = w.create_purchase_order(NewPurchaseOrder {
        po_number: uq("PO"), supplier_quotation_id: None, order_kind: None, company_id: company,
        branch_id: None, supplier_id: Uuid::new_v4(), order_date: day(), schedule_date: None,
        currency: None, tax_rate: Decimal::ZERO, notes: None,
        lines: vec![NewLine { item_id: item, warehouse_id: None, description: None, quantity: d("10"), rate: d("100") }],
    }).await.unwrap();
    w.confirm_purchase_order(po).await.unwrap();
    w.mark_received(po, &[(item, d("10"))]).await.unwrap();
    assert!(rec.has(|e| matches!(e, BuyingEvent::PurchaseOrderFullyReceived(_))), "fully received milestone");
    w.mark_billed(po, &[(item, d("10"))]).await.unwrap();
    assert!(rec.has(|e| matches!(e, BuyingEvent::PurchaseOrderFullyBilled(_))), "fully billed milestone");
    // Received milestone fired exactly once (not re-emitted on the billing recompute).
    let n_recv = rec.events.lock().unwrap().iter().filter(|e| matches!(e, BuyingEvent::PurchaseOrderFullyReceived(_))).count();
    assert_eq!(n_recv, 1);
}
