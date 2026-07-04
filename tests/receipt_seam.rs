//! The procure-to-pay receipt seam, end-to-end across THREE modules: **buying → inventory →
//! accounting → buying** — the mirror of selling's delivery seam. Zero normal Cargo edges
//! (inventory + accounting are dev-deps only).
//!
//! Flow: buying confirms a PO → emits `ReceiptRequestEnvelope`; an ACL maps it into inventory's
//! `ReceiptExpected` (adding warehouse + GL accounts) → a draft Purchase Receipt; inventory submits
//! it → **asset post** (Dr Inventory · Cr GR/IR) into the REAL ledger + a `StockReceived` event; an
//! ACL routes `StockReceived` → buying `mark_received` → `received_qty` advances → PO `to_bill`;
//! simulated billing → `completed`. All three schemas co-locate in one DB.
//! Requires DATABASE_URL (:5433/backbone_buying with all three schemas migrated).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use backbone_buying::application::service::buying_write_service::{
    BuyingWriteService, NewLine, NewPurchaseOrder,
};

use backbone_inventory::application::service::inventory_gl::{
    AccountingPostEnvelope as InvEnv, GlPostAck as InvAck, GlPostRejected as InvRej, GlPostSink as InvSink,
};
use backbone_inventory::application::service::inventory_events::{InventoryEvent, InventoryEventSink};
use backbone_inventory::application::service::inventory_intake::{DeliveryIntake, ReceiptExpected, ReceiptRequestLine as InvReceiptLine};
use backbone_inventory::application::service::inventory_write_service::{InventoryWriteService, NewWarehouse};

use backbone_accounting::application::service::posting_service::{PostingLine, PostingRequest, PostingService};

struct GlAdapter { svc: PostingService }
#[async_trait::async_trait]
impl InvSink for GlAdapter {
    async fn post(&self, e: &InvEnv) -> Result<InvAck, InvRej> {
        let mut r = PostingRequest::original(e.company_id, &e.source_type, e.source_id, e.posting_date);
        r.source_reference = e.source_reference.clone();
        r.lines = e.lines.iter().map(|l| PostingLine {
            account_id: l.account_id, debit: l.debit, credit: l.credit,
            party_type: l.party_type.clone(), party_id: l.party_id,
            cost_center_id: None, project_id: None, department_id: None, description: l.description.clone(),
        }).collect();
        match self.svc.post(r, None).await {
            Ok(x) => Ok(InvAck { post_id: x.post_id, journal_id: x.journal_id, idempotent_reuse: x.idempotent_reuse }),
            Err(x) => Err(InvRej { code: x.code().to_string(), message: x.to_string() }),
        }
    }
}

#[derive(Default, Clone)]
struct RecordingInvSink { events: Arc<Mutex<Vec<InventoryEvent>>> }
impl InventoryEventSink for RecordingInvSink {
    fn publish(&self, e: InventoryEvent) { self.events.lock().unwrap().push(e); }
}

fn d(s: &str) -> Decimal { Decimal::from_str_exact(s).unwrap() }
fn day() -> chrono::NaiveDate { chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap() }
fn uq(p: &str) -> String { format!("{p}-{}", &Uuid::new_v4().simple().to_string()[..8]) }
async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/backbone_buying".to_string());
    PgPool::connect(&url).await.expect("connect DB")
}
async fn seed_coa(pool: &PgPool) -> (Uuid, HashMap<&'static str, Uuid>) {
    let company = Uuid::new_v4();
    let coa: &[(&str, &str, &str, &str, &str)] = &[
        ("1300", "Persediaan", "asset", "inventory", "debit"),
        ("2150", "GR/IR", "liability", "current_liability", "credit"),
    ];
    let mut m = HashMap::new();
    for (code, name, at, st, nb) in coa {
        let id = Uuid::new_v4();
        sqlx::query(r#"INSERT INTO accounting.accounts (id, company_id, account_number, account_code, name, account_type, account_subtype, normal_balance, is_header, is_detail, status)
            VALUES ($1,$2,$3,$4,$5,$6::account_type,$7::account_subtype,$8::normal_balance,false,true,'active'::account_status)"#)
            .bind(id).bind(company).bind(code).bind(code).bind(name).bind(at).bind(st).bind(nb)
            .execute(pool).await.expect("seed acct");
        m.insert(*code, id);
    }
    (company, m)
}

/// RSEAM-1: procure-to-pay + goods receipt across buying, inventory, and the real ledger.
#[tokio::test]
async fn procure_to_pay_receipt_across_three_modules() {
    let pool = pool().await;
    let (company, coa) = seed_coa(&pool).await;
    let item = Uuid::new_v4();
    let supplier = Uuid::new_v4();

    let buying = BuyingWriteService::new(pool.clone());
    let recorder = RecordingInvSink::default();
    let inventory = InventoryWriteService::with_sink(pool.clone(), Arc::new(recorder.clone()));
    let intake = DeliveryIntake::new(pool.clone());
    let gl = GlAdapter { svc: PostingService::new(pool.clone()) };

    let wh = inventory.create_warehouse(NewWarehouse { company_id: company, code: uq("WH"), name: "Main".into(), warehouse_type: None, parent_warehouse_id: None, is_group: false }).await.unwrap();

    // 1) buying: PO for 10 @ 100,000, confirm.
    let po = buying.create_purchase_order(NewPurchaseOrder {
        po_number: uq("PO"), supplier_quotation_id: None, order_kind: None, company_id: company,
        branch_id: None, supplier_id: supplier, order_date: day(), schedule_date: None, currency: None,
        tax_rate: Decimal::ZERO, notes: None,
        lines: vec![NewLine { item_id: item, warehouse_id: Some(wh), description: None, quantity: d("10"), rate: d("100000") }],
    }).await.unwrap();
    buying.confirm_purchase_order(po).await.unwrap();
    assert_eq!(po_status(&pool, po).await, "to_receive_and_bill");

    // 2) buying emits a receipt request; ACL maps it into inventory's ReceiptExpected.
    let req = buying.build_receipt_request(po).await.unwrap();
    assert_eq!(req.lines.len(), 1);
    let pr = intake.on_receipt_expected(ReceiptExpected {
        receipt_number: uq("PR"), company_id: req.company_id, branch_id: None, supplier_id: req.supplier_id,
        source_po_id: Some(req.order_id), warehouse_id: wh, posting_date: day(),
        inventory_account_id: coa["1300"], grir_account_id: coa["2150"],
        lines: req.lines.iter().map(|l| InvReceiptLine { item_id: l.item_id, quantity: l.quantity, rate: l.rate }).collect(),
    }).await.unwrap();

    // 3) inventory submits the receipt → asset post into the REAL ledger + StockReceived.
    let out = inventory.submit_purchase_receipt(pr, &gl).await.unwrap();
    assert!(out.posted);
    // Asset journal: Dr Inventory 1,000,000 · Cr GR/IR 1,000,000.
    assert_eq!(journal_totals(&pool, out.journal_id.unwrap()).await, (d("1000000"), d("1000000")));

    // 4) ACL: StockReceived (source_po_id = our PO) → buying.mark_received.
    let evts = recorder.events.lock().unwrap().clone();
    let received = evts.iter().find_map(|e| match e {
        InventoryEvent::StockReceived(s) if s.source_po_id == Some(po) => Some(s.clone()), _ => None,
    }).expect("StockReceived for our PO");
    assert_eq!(received.total_value, d("1000000.00"));
    buying.mark_received(po, &[(item, d("10"))]).await.unwrap();
    assert_eq!(po_status(&pool, po).await, "to_bill", "received, awaiting billing");

    // 5) simulated billing completes the 3-way match.
    buying.mark_billed(po, &[(item, d("10"))]).await.unwrap();
    assert_eq!(po_status(&pool, po).await, "completed");

    // inventory Bin holds the received stock at the PO rate.
    let (bin_qty, bin_val): (Decimal, Decimal) = sqlx::query_as("SELECT actual_qty, stock_value FROM inventory.bins WHERE company_id=$1 AND item_id=$2 AND warehouse_id=$3").bind(company).bind(item).bind(wh).fetch_one(&pool).await.unwrap();
    assert_eq!(bin_qty, d("10.0000"));
    assert_eq!(bin_val, d("1000000.00"));
    // buying watermark
    let rq: Decimal = sqlx::query_scalar("SELECT received_qty FROM buying.purchase_order_items WHERE order_id=$1").bind(po).fetch_one(&pool).await.unwrap();
    assert_eq!(rq, d("10.0000"));
}

async fn po_status(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT status::text FROM buying.purchase_orders WHERE id=$1").bind(id).fetch_one(pool).await.unwrap()
}
async fn journal_totals(pool: &PgPool, jid: Uuid) -> (Decimal, Decimal) {
    let r = sqlx::query("SELECT total_debit, total_credit FROM accounting.journals WHERE id=$1").bind(jid).fetch_one(pool).await.unwrap();
    (r.get("total_debit"), r.get("total_credit"))
}
