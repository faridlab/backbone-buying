//! Validated write path for buying (hand-authored, user-owned) — the procure-to-pay intent
//! pipeline + the receipt seam. Mirrors selling's order-to-cash shape.
//!
//! Closes the CRUD-bypass: material requests / RFQs / supplier quotations / purchase orders are
//! transactional documents whose money must be consistent. Creates compute line amounts + totals
//! server-side (2dp half-up) and reject empty documents; header+lines are written in ONE
//! transaction. `build_receipt_request` emits a `ReceiptRequestEnvelope` (an ACL maps it into
//! inventory's `ReceiptExpected`); `mark_received` / `mark_billed` advance the 3-way-match
//! watermarks and recompute the PO status. Buying emits NO `AccountingPost`.

use backbone_orm::company_scope;
use rust_decimal::{Decimal, RoundingStrategy};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::infrastructure::persistence::{
    MatchWatermark, MaterialRequestItemRepository, MaterialRequestRepository,
    NewMaterialRequestItemRow, NewMaterialRequestRow, NewPurchaseOrderItemRow, NewPurchaseOrderRow,
    NewQuotationFromRfqRow, NewRfqItemRow, NewRfqRow, NewRfqSupplierRow,
    NewSupplierQuotationItemRow, NewSupplierQuotationRow, PurchaseOrderItemRepository,
    PurchaseOrderRepository, RequestForQuotationRepository, RfqItemRepository, RfqSupplierRepository,
    SupplierQuotationItemRepository, SupplierQuotationRepository,
};

use super::buying_events::{
    BuyingEvent, BuyingEventSink, DocumentRaised, LoggingSink, PurchaseOrderConfirmed,
    PurchaseOrderMilestone, PurchaseOrderRef, ReceiptRequestEnvelope, ReceiptRequestLine,
    ThreeWayMatchFailed,
};

fn money(v: Decimal) -> Decimal {
    v.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
}

// --- input structs -----------------------------------------------------------

#[derive(Debug, Clone)]
pub struct NewLine {
    pub item_id: Uuid,
    pub warehouse_id: Option<Uuid>,
    pub description: Option<String>,
    pub quantity: Decimal,
    pub rate: Decimal,
}

#[derive(Debug, Clone)]
pub struct SimpleLine {
    pub item_id: Uuid,
    pub quantity: Decimal,
}

#[derive(Debug, Clone)]
pub struct NewMaterialRequest {
    pub request_number: String,
    pub company_id: Uuid,
    pub request_type: Option<String>,
    pub request_date: chrono::NaiveDate,
    pub schedule_date: Option<chrono::NaiveDate>,
    pub notes: Option<String>,
    pub lines: Vec<SimpleLine>,
}

#[derive(Debug, Clone)]
pub struct NewSupplierQuotation {
    pub quotation_number: String,
    pub rfq_id: Option<Uuid>,
    pub company_id: Uuid,
    pub supplier_id: Uuid,
    pub quotation_date: chrono::NaiveDate,
    pub valid_till: Option<chrono::NaiveDate>,
    pub currency: Option<String>,
    pub lines: Vec<NewLine>,
}

#[derive(Debug, Clone)]
pub struct NewPurchaseOrder {
    pub po_number: String,
    pub supplier_quotation_id: Option<Uuid>,
    pub order_kind: Option<String>,
    pub company_id: Uuid,
    pub branch_id: Option<Uuid>,
    pub supplier_id: Uuid,
    pub order_date: chrono::NaiveDate,
    pub schedule_date: Option<chrono::NaiveDate>,
    pub currency: Option<String>,
    pub tax_rate: Decimal,
    pub notes: Option<String>,
    pub lines: Vec<NewLine>,
}

// --- errors ------------------------------------------------------------------

#[derive(Debug)]
pub enum BuyingError {
    EmptyDocument,
    NegativeQuantity,
    DuplicateNumber(String),
    OrderNotFound(Uuid),
    NotConfirmable(String),
    /// A source funnel document (MR / RFQ / SQ) was not found.
    SourceNotFound(Uuid),
    /// A source funnel document is not in a convertible state.
    SourceNotConvertible(String),
    /// 3-way match: cannot receive more than ordered (no over-receipt tolerance configured).
    OverReceipt { item_id: Uuid },
    /// 3-way match: cannot bill more than received (invoice ≤ receipt).
    OverBilling { item_id: Uuid },
    Db(sqlx::Error),
}

impl BuyingError {
    pub fn code(&self) -> String {
        match self {
            BuyingError::EmptyDocument => "empty_document".into(),
            BuyingError::NegativeQuantity => "negative_quantity".into(),
            BuyingError::DuplicateNumber(_) => "duplicate_number".into(),
            BuyingError::OrderNotFound(_) => "order_not_found".into(),
            BuyingError::NotConfirmable(_) => "not_confirmable".into(),
            BuyingError::SourceNotFound(_) => "source_not_found".into(),
            BuyingError::SourceNotConvertible(_) => "source_not_convertible".into(),
            BuyingError::OverReceipt { .. } => "over_receipt".into(),
            BuyingError::OverBilling { .. } => "over_billing".into(),
            BuyingError::Db(_) => "internal_error".into(),
        }
    }
    pub fn http_status(&self) -> u16 {
        match self {
            BuyingError::OrderNotFound(_) | BuyingError::SourceNotFound(_) => 404,
            BuyingError::Db(_) => 500,
            _ => 422,
        }
    }
}
impl std::fmt::Display for BuyingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code())
    }
}
impl std::error::Error for BuyingError {}
impl From<sqlx::Error> for BuyingError {
    fn from(e: sqlx::Error) -> Self { BuyingError::Db(e) }
}
fn is_dup(e: &sqlx::Error) -> bool {
    e.as_database_error().map(|d| d.is_unique_violation()).unwrap_or(false)
}

struct PricedLine {
    item_id: Uuid,
    warehouse_id: Option<Uuid>,
    description: Option<String>,
    quantity: Decimal,
    rate: Decimal,
    line_amount: Decimal,
}

/// Compute `line_amount = money(qty*rate)` per line + `(subtotal, tax_amount, total)`; reject empty/negative.
fn price_document(lines: &[NewLine], tax_rate: Decimal) -> Result<(Vec<PricedLine>, Decimal, Decimal, Decimal), BuyingError> {
    if lines.is_empty() {
        return Err(BuyingError::EmptyDocument);
    }
    let mut priced = Vec::with_capacity(lines.len());
    let mut subtotal = Decimal::ZERO;
    for l in lines {
        if l.quantity < Decimal::ZERO || l.rate < Decimal::ZERO {
            return Err(BuyingError::NegativeQuantity);
        }
        let line_amount = money(l.quantity * l.rate);
        subtotal += line_amount;
        priced.push(PricedLine {
            item_id: l.item_id, warehouse_id: l.warehouse_id, description: l.description.clone(),
            quantity: l.quantity, rate: l.rate, line_amount,
        });
    }
    let subtotal = money(subtotal);
    let tax_amount = money(subtotal * tax_rate / Decimal::from(100));
    let total = subtotal + tax_amount;
    Ok((priced, subtotal, tax_amount, total))
}

/// The repositories this service orchestrates. Bundled behind one `Arc` so the service stays cheap
/// to `Clone` (it is cloned per request) without requiring the repository newtypes to be `Clone`.
struct Repos {
    material_requests: MaterialRequestRepository,
    material_request_items: MaterialRequestItemRepository,
    rfqs: RequestForQuotationRepository,
    rfq_items: RfqItemRepository,
    rfq_suppliers: RfqSupplierRepository,
    supplier_quotations: SupplierQuotationRepository,
    supplier_quotation_items: SupplierQuotationItemRepository,
    purchase_orders: PurchaseOrderRepository,
    purchase_order_items: PurchaseOrderItemRepository,
}

impl Repos {
    fn new(db_pool: &PgPool) -> Self {
        Self {
            material_requests: MaterialRequestRepository::new(db_pool.clone()),
            material_request_items: MaterialRequestItemRepository::new(db_pool.clone()),
            rfqs: RequestForQuotationRepository::new(db_pool.clone()),
            rfq_items: RfqItemRepository::new(db_pool.clone()),
            rfq_suppliers: RfqSupplierRepository::new(db_pool.clone()),
            supplier_quotations: SupplierQuotationRepository::new(db_pool.clone()),
            supplier_quotation_items: SupplierQuotationItemRepository::new(db_pool.clone()),
            purchase_orders: PurchaseOrderRepository::new(db_pool.clone()),
            purchase_order_items: PurchaseOrderItemRepository::new(db_pool.clone()),
        }
    }
}

#[derive(Clone)]
pub struct BuyingWriteService {
    db_pool: PgPool,
    repos: Arc<Repos>,
    sink: Arc<dyn BuyingEventSink>,
}

impl BuyingWriteService {
    pub fn new(db_pool: PgPool) -> Self {
        Self::with_sink(db_pool, Arc::new(LoggingSink))
    }
    pub fn with_sink(db_pool: PgPool, sink: Arc<dyn BuyingEventSink>) -> Self {
        let repos = Arc::new(Repos::new(&db_pool));
        Self { db_pool, repos, sink }
    }

    // ---- Material Request ---------------------------------------------------

    pub async fn create_material_request(&self, m: NewMaterialRequest) -> Result<Uuid, BuyingError> {
        if m.lines.is_empty() { return Err(BuyingError::EmptyDocument); }
        for l in &m.lines { if l.quantity < Decimal::ZERO { return Err(BuyingError::NegativeQuantity); } }
        let id = Uuid::new_v4();
        let rt = m.request_type.unwrap_or_else(|| "purchase".into());
        // RLS scope (ADR-0008): company is on the DTO — bind it onto our own transaction so the
        // header+lines insert passes the WITH CHECK fence. Explicit `company_id` binds stay as
        // defense-in-depth.
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, m.company_id).await?;
        let r = self.repos.material_requests.insert_material_request(&mut tx, &NewMaterialRequestRow {
            id,
            request_number: &m.request_number,
            company_id: m.company_id,
            request_type: &rt,
            request_date: m.request_date,
            schedule_date: m.schedule_date,
            notes: m.notes.as_deref(),
        }).await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { BuyingError::DuplicateNumber(m.request_number) } else { e.into() });
        }
        for l in &m.lines {
            self.repos.material_request_items.insert_item(&mut tx, &NewMaterialRequestItemRow {
                id: Uuid::new_v4(), request_id: id, company_id: m.company_id, item_id: l.item_id, quantity: l.quantity,
            }).await?;
        }
        tx.commit().await?;
        self.sink.publish(BuyingEvent::MaterialRequestRaised(DocumentRaised {
            document_id: id, company_id: m.company_id, source_id: None,
        }));
        Ok(id)
    }

    /// Convert a material request into an RFQ to the invited suppliers (copies the requested lines,
    /// links `material_request_id`, marks the MR `ordered`). The MR→RFQ funnel step.
    pub async fn convert_material_request_to_rfq(
        &self, request_id: Uuid, rfq_number: String, response_due: Option<chrono::NaiveDate>,
        supplier_ids: &[Uuid],
    ) -> Result<Uuid, BuyingError> {
        // RLS scope (ADR-0008), ID-only pattern: identified by the MR id alone — the reads ride the
        // request-dedicated connection (which carries the caller's `app.company_id`), so another
        // company's MR simply isn't found. The company read off the row then binds our transaction.
        let mr = self.repos.material_requests.fetch_source(&self.db_pool, request_id).await?
            .ok_or(BuyingError::SourceNotFound(request_id))?;
        let company_id = mr.company_id;
        if mr.status == "cancelled" {
            return Err(BuyingError::SourceNotConvertible(request_id.to_string()));
        }
        let items = self.repos.material_request_items.fetch_lines(&self.db_pool, request_id).await?;

        let id = Uuid::new_v4();
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, company_id).await?;
        let r = self.repos.rfqs.insert_rfq(&mut tx, &NewRfqRow {
            id,
            rfq_number: &rfq_number,
            material_request_id: request_id,
            company_id,
            response_due,
        }).await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { BuyingError::DuplicateNumber(rfq_number) } else { e.into() });
        }
        for it in &items {
            self.repos.rfq_items.insert_item(&mut tx, &NewRfqItemRow {
                id: Uuid::new_v4(), rfq_id: id, company_id, item_id: it.item_id, quantity: it.quantity,
            }).await?;
        }
        for sup in supplier_ids {
            self.repos.rfq_suppliers.insert_supplier(&mut tx, &NewRfqSupplierRow {
                id: Uuid::new_v4(), rfq_id: id, company_id, supplier_id: *sup,
            }).await?;
        }
        self.repos.material_requests.mark_ordered(&mut tx, request_id).await?;
        tx.commit().await?;
        self.sink.publish(BuyingEvent::RfqIssued(DocumentRaised {
            document_id: id, company_id, source_id: Some(request_id),
        }));
        Ok(id)
    }

    /// Convert an RFQ into a supplier quotation for one supplier's quoted rates (copies the RFQ line
    /// quantities, applies the supplier's rates, links `rfq_id`). The RFQ→SupplierQuotation step.
    pub async fn convert_rfq_to_supplier_quotation(
        &self, rfq_id: Uuid, quotation_number: String, supplier_id: Uuid,
        quoted_rates: &[(Uuid, Decimal)], // (item_id, rate)
    ) -> Result<Uuid, BuyingError> {
        // RLS scope (ADR-0008), ID-only pattern — see `convert_material_request_to_rfq`.
        let rfq = self.repos.rfqs.fetch_source(&self.db_pool, rfq_id).await?
            .ok_or(BuyingError::SourceNotFound(rfq_id))?;
        let company_id = rfq.company_id;
        if rfq.status == "cancelled" {
            return Err(BuyingError::SourceNotConvertible(rfq_id.to_string()));
        }
        let items = self.repos.rfq_items.fetch_lines(&self.db_pool, rfq_id).await?;
        let rate_of = |item: Uuid| quoted_rates.iter().find(|(i, _)| *i == item).map(|(_, r)| *r).unwrap_or(Decimal::ZERO);

        let id = Uuid::new_v4();
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, company_id).await?;
        let r = self.repos.supplier_quotations.insert_quotation_from_rfq(&mut tx, &NewQuotationFromRfqRow {
            id,
            quotation_number: &quotation_number,
            rfq_id,
            company_id,
            supplier_id,
        }).await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { BuyingError::DuplicateNumber(quotation_number) } else { e.into() });
        }
        for it in &items {
            self.repos.supplier_quotation_items.insert_item(&mut tx, &NewSupplierQuotationItemRow {
                id: Uuid::new_v4(), quotation_id: id, company_id, item_id: it.item_id,
                quantity: it.quantity, rate: rate_of(it.item_id),
            }).await?;
        }
        tx.commit().await?;
        self.sink.publish(BuyingEvent::SupplierQuotationReceived(DocumentRaised {
            document_id: id, company_id, source_id: Some(rfq_id),
        }));
        Ok(id)
    }

    /// Convert an accepted supplier quotation into a draft Purchase Order — copies supplier + lines +
    /// rates, links `supplier_quotation_id`, advances the SQ to `ordered`. The §31 "select → PO" step.
    pub async fn convert_supplier_quotation_to_po(
        &self, quotation_id: Uuid, po_number: String, tax_rate: Decimal,
    ) -> Result<Uuid, BuyingError> {
        // RLS scope (ADR-0008), ID-only pattern — see `convert_material_request_to_rfq`.
        let sq = self.repos.supplier_quotations.fetch_source(&self.db_pool, quotation_id).await?
            .ok_or(BuyingError::SourceNotFound(quotation_id))?;
        if sq.status != "submitted" {
            return Err(BuyingError::SourceNotConvertible(quotation_id.to_string()));
        }
        let items = self.repos.supplier_quotation_items.fetch_lines(&self.db_pool, quotation_id).await?;
        if items.is_empty() {
            return Err(BuyingError::EmptyDocument);
        }
        let lines: Vec<NewLine> = items.iter().map(|it| NewLine {
            item_id: it.item_id,
            warehouse_id: None,
            description: None,
            quantity: it.quantity,
            rate: it.rate,
        }).collect();

        let sq_company = sq.company_id;
        let order_id = self.create_purchase_order(NewPurchaseOrder {
            po_number,
            supplier_quotation_id: Some(quotation_id),
            order_kind: None,
            company_id: sq_company,
            branch_id: None,
            supplier_id: sq.supplier_id,
            order_date: chrono::Utc::now().date_naive(),
            schedule_date: None,
            currency: Some(sq.currency),
            tax_rate,
            notes: None,
            lines,
        }).await?;

        // The SQ's company was just read off its row — scope the status flip on it explicitly, so this
        // is correct for non-request callers too.
        company_scope::with_company_scope(
            Some(sq_company),
            self.repos.supplier_quotations.mark_ordered(&self.db_pool, quotation_id),
        ).await?;
        Ok(order_id)
    }

    /// Load the exported `PurchaseOrderRef` (the brief §42 cross-module DTO) for one PO.
    pub async fn purchase_order_ref(&self, order_id: Uuid) -> Result<PurchaseOrderRef, BuyingError> {
        // RLS scope (ADR-0008), ID-only pattern: read rides the request-dedicated connection.
        let row = self.repos.purchase_orders.fetch_ref(&self.db_pool, order_id).await?
            .ok_or(BuyingError::OrderNotFound(order_id))?;
        Ok(PurchaseOrderRef {
            id: order_id,
            supplier_id: row.supplier_id,
            company_id: row.company_id,
            order_kind: row.order_kind,
            grand_total: row.total,
            currency: row.currency,
        })
    }

    // ---- Supplier Quotation -------------------------------------------------

    pub async fn create_supplier_quotation(&self, q: NewSupplierQuotation) -> Result<Uuid, BuyingError> {
        let (priced, _sub, _tax, _tot) = price_document(&q.lines, Decimal::ZERO)?;
        let id = Uuid::new_v4();
        let currency = q.currency.unwrap_or_else(|| "IDR".into());
        // RLS scope (ADR-0008): company is on the DTO — bind it onto our own transaction.
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, q.company_id).await?;
        let r = self.repos.supplier_quotations.insert_quotation(&mut tx, &NewSupplierQuotationRow {
            id,
            quotation_number: &q.quotation_number,
            rfq_id: q.rfq_id,
            company_id: q.company_id,
            supplier_id: q.supplier_id,
            quotation_date: q.quotation_date,
            valid_till: q.valid_till,
            currency: &currency,
        }).await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { BuyingError::DuplicateNumber(q.quotation_number) } else { e.into() });
        }
        for p in &priced {
            self.repos.supplier_quotation_items.insert_item(&mut tx, &NewSupplierQuotationItemRow {
                id: Uuid::new_v4(), quotation_id: id, company_id: q.company_id, item_id: p.item_id,
                quantity: p.quantity, rate: p.rate,
            }).await?;
        }
        tx.commit().await?;
        Ok(id)
    }

    // ---- Purchase Order -----------------------------------------------------

    pub async fn create_purchase_order(&self, o: NewPurchaseOrder) -> Result<Uuid, BuyingError> {
        let (priced, subtotal, tax_amount, total) = price_document(&o.lines, o.tax_rate)?;
        let id = Uuid::new_v4();
        let currency = o.currency.unwrap_or_else(|| "IDR".into());
        let kind = o.order_kind.unwrap_or_else(|| "standard".into());
        // RLS scope (ADR-0008): company is on the DTO — bind it onto our own transaction.
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, o.company_id).await?;
        let r = self.repos.purchase_orders.insert_purchase_order(&mut tx, &NewPurchaseOrderRow {
            id,
            po_number: &o.po_number,
            supplier_quotation_id: o.supplier_quotation_id,
            order_kind: &kind,
            company_id: o.company_id,
            branch_id: o.branch_id,
            supplier_id: o.supplier_id,
            order_date: o.order_date,
            schedule_date: o.schedule_date,
            currency: &currency,
            subtotal,
            tax_rate: o.tax_rate,
            tax_amount,
            total,
            notes: o.notes.as_deref(),
        }).await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { BuyingError::DuplicateNumber(o.po_number) } else { e.into() });
        }
        for p in &priced {
            self.repos.purchase_order_items.insert_item(&mut tx, &NewPurchaseOrderItemRow {
                id: Uuid::new_v4(), order_id: id, company_id: o.company_id, item_id: p.item_id, warehouse_id: p.warehouse_id,
                description: p.description.as_deref(), quantity: p.quantity, rate: p.rate,
                line_amount: p.line_amount,
            }).await?;
        }
        tx.commit().await?;
        Ok(id)
    }

    /// Confirm a draft PO → `to_receive_and_bill` (awaiting both receipt and billing). Emits
    /// `PurchaseOrderConfirmed`.
    pub async fn confirm_purchase_order(&self, order_id: Uuid) -> Result<(), BuyingError> {
        // RLS scope (ADR-0008), ID-only pattern: the UPDATE ... RETURNING rides the request-dedicated
        // connection, so it can only confirm a PO in the caller's own company.
        let row = self.repos.purchase_orders.confirm(&self.db_pool, order_id).await?
            .ok_or_else(|| BuyingError::NotConfirmable(order_id.to_string()))?;
        self.sink.publish(BuyingEvent::PurchaseOrderConfirmed(PurchaseOrderConfirmed {
            order_id, company_id: row.company_id, supplier_id: row.supplier_id,
            grand_total: row.total, currency: row.currency,
        }));
        Ok(())
    }

    // ---- Receipt seam (buying -> inventory) --------------------------------

    /// Build the cross-module receipt request for a confirmed PO (the envelope buying emits; an ACL
    /// maps it into inventory's `ReceiptExpected`). Requests the not-yet-received quantity per line.
    /// Emits `ReceiptRequested`.
    pub async fn build_receipt_request(&self, order_id: Uuid) -> Result<ReceiptRequestEnvelope, BuyingError> {
        // RLS scope (ADR-0008), ID-only pattern: read-only, reads ride the request-dedicated connection.
        let hdr = self.repos.purchase_orders.fetch_header(&self.db_pool, order_id).await?
            .ok_or(BuyingError::OrderNotFound(order_id))?;
        if hdr.status == "draft" {
            return Err(BuyingError::NotConfirmable(order_id.to_string()));
        }
        let rows = self.repos.purchase_order_items.fetch_remaining(&self.db_pool, order_id).await?;
        let lines: Vec<ReceiptRequestLine> = rows.iter().map(|r| ReceiptRequestLine {
            item_id: r.item_id, quantity: r.remaining, rate: r.rate,
        }).collect();
        let env = ReceiptRequestEnvelope {
            order_id, company_id: hdr.company_id, supplier_id: hdr.supplier_id,
            currency: hdr.currency, lines,
        };
        self.sink.publish(BuyingEvent::ReceiptRequested(env.clone()));
        Ok(env)
    }

    /// Record a receipt against a PO (inbound handler for inventory's `StockReceived`): allocate the
    /// received quantity across the item's PO lines, filling each up to `quantity` (the 3-way-match
    /// ceiling — no over-receipt tolerance configured, council 2026-07-05). Rejects over-receipt.
    pub async fn mark_received(&self, order_id: Uuid, receipts: &[(Uuid, Decimal)]) -> Result<(), BuyingError> {
        // RLS scope (ADR-0008): this method carries NO company — it is identified by the PO id alone.
        // Under HTTP the request-dedicated connection supplies the scope. When driven by an EVENT
        // (inventory's `StockReceived`), the CALLER must wrap this in
        // `with_company_scope(Some(event.company_id))` — the event carries the company — or the
        // allocation reads/writes below fail closed.
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_current_company(&mut tx).await?;
        for (item_id, qty) in receipts {
            // capacity per line = quantity - received_qty
            if let Err(e) = self.allocate(&mut tx, order_id, *item_id, *qty, MatchWatermark::Received,
                BuyingError::OverReceipt { item_id: *item_id }).await {
                drop(tx); // roll back — no partial receipt
                if matches!(e, BuyingError::OverReceipt { .. }) {
                    // §33: broadcast the variance so an async consumer sees it, not just the caller.
                    self.sink.publish(BuyingEvent::ThreeWayMatchFailed(ThreeWayMatchFailed {
                        order_id, item_id: *item_id, kind: "over_receipt".into(),
                    }));
                }
                return Err(e);
            }
        }
        tx.commit().await?;
        self.recompute_order_status(order_id).await
    }

    /// Record billing against a PO (inbound handler for billing's `PurchaseInvoicePosted`): allocate
    /// the billed quantity across the item's lines, capped at `received_qty` (invoice ≤ receipt —
    /// the 3-way-match invariant). Rejects over-billing.
    pub async fn mark_billed(&self, order_id: Uuid, billed: &[(Uuid, Decimal)]) -> Result<(), BuyingError> {
        // RLS scope (ADR-0008): no company on this method — see `mark_received`. When driven by an
        // EVENT (billing's `PurchaseInvoicePosted`), the CALLER must wrap this in
        // `with_company_scope(Some(event.company_id))`.
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_current_company(&mut tx).await?;
        for (item_id, qty) in billed {
            // capacity per line = received_qty - billed_qty
            if let Err(e) = self.allocate(&mut tx, order_id, *item_id, *qty, MatchWatermark::Billed,
                BuyingError::OverBilling { item_id: *item_id }).await {
                drop(tx); // roll back — no partial billing
                if matches!(e, BuyingError::OverBilling { .. }) {
                    self.sink.publish(BuyingEvent::ThreeWayMatchFailed(ThreeWayMatchFailed {
                        order_id, item_id: *item_id, kind: "over_billing".into(),
                    }));
                }
                return Err(e);
            }
        }
        tx.commit().await?;
        self.recompute_order_status(order_id).await
    }

    /// Allocate `qty` of `item_id` across a PO's lines, advancing `watermark` up to each line's cap
    /// (fill-in-order). Rejects with `over_err` if the total remaining capacity is exceeded — so
    /// `received_qty <= quantity` and `billed_qty <= received_qty` hold per line (3-way match).
    /// Correct even when a PO has several lines of the same item.
    ///
    /// The DECISION lives here (the service owns the business rule); the lock/read/bump SQL lives in
    /// `PurchaseOrderItemRepository`. Both repo calls take the caller's `tx`, so the `FOR UPDATE` lock
    /// taken by the capacity read is still held when the bumps run.
    async fn allocate(
        &self, tx: &mut sqlx::PgConnection, order_id: Uuid, item_id: Uuid, mut qty: Decimal,
        watermark: MatchWatermark, over_err: BuyingError,
    ) -> Result<(), BuyingError> {
        let lines = self.repos.purchase_order_items
            .lock_lines_for_allocation(&mut *tx, order_id, item_id, watermark).await?;
        let total_cap: Decimal = lines.iter().map(|r| r.capacity).sum();
        if qty > total_cap {
            return Err(over_err);
        }
        for line in &lines {
            if qty <= Decimal::ZERO { break; }
            let cap = line.capacity;
            if cap <= Decimal::ZERO { continue; }
            let take = if qty < cap { qty } else { cap };
            self.repos.purchase_order_items
                .add_to_watermark(&mut *tx, line.id, watermark, take).await?;
            qty -= take;
        }
        Ok(())
    }

    /// Recompute a PO's status from its two 3-way-match watermarks: `completed` iff every line is
    /// fully received AND fully billed; else `to_receive` / `to_bill` / `to_receive_and_bill`. Emits
    /// `PurchaseOrderFullyReceived` / `PurchaseOrderFullyBilled` on the transition into each milestone.
    async fn recompute_order_status(&self, order_id: Uuid) -> Result<(), BuyingError> {
        // RLS scope (ADR-0008), ID-only pattern: no company argument — this runs under whatever scope
        // its caller (`mark_received` / `mark_billed`) established, i.e. the request connection under
        // HTTP or the event caller's `with_company_scope`.
        let row = self.repos.purchase_orders.fetch_match_watermarks(&self.db_pool, order_id).await?;
        let company_id = row.company_id;
        let prior = row.prior;
        let received_all = row.received_all.unwrap_or(false);
        let billed_all = row.billed_all.unwrap_or(false);
        let next = match (received_all, billed_all) {
            (true, true) => "completed",
            (true, false) => "to_bill",
            (false, true) => "to_receive",
            (false, false) => "to_receive_and_bill",
        };
        // The PO's company was just read off the row above — scope the status flip on it explicitly.
        company_scope::with_company_scope(
            Some(company_id),
            self.repos.purchase_orders.update_status(&self.db_pool, order_id, next),
        ).await?;

        // Milestone events on the FIRST transition into full receipt / full billing.
        let was_received = matches!(prior.as_str(), "to_bill" | "completed");
        let was_billed = matches!(prior.as_str(), "to_receive" | "completed");
        if received_all && !was_received {
            self.sink.publish(BuyingEvent::PurchaseOrderFullyReceived(PurchaseOrderMilestone { order_id, company_id }));
        }
        if billed_all && !was_billed {
            self.sink.publish(BuyingEvent::PurchaseOrderFullyBilled(PurchaseOrderMilestone { order_id, company_id }));
        }
        Ok(())
    }
}
