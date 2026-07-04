//! Validated write path for buying (hand-authored, user-owned) — the procure-to-pay intent
//! pipeline + the receipt seam. Mirrors selling's order-to-cash shape.
//!
//! Closes the CRUD-bypass: material requests / RFQs / supplier quotations / purchase orders are
//! transactional documents whose money must be consistent. Creates compute line amounts + totals
//! server-side (2dp half-up) and reject empty documents; header+lines are written in ONE
//! transaction. `build_receipt_request` emits a `ReceiptRequestEnvelope` (an ACL maps it into
//! inventory's `ReceiptExpected`); `mark_received` / `mark_billed` advance the 3-way-match
//! watermarks and recompute the PO status. Buying emits NO `AccountingPost`.

use rust_decimal::{Decimal, RoundingStrategy};
use sqlx::{PgPool, Row};
use std::sync::Arc;
use uuid::Uuid;

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

#[derive(Clone)]
pub struct BuyingWriteService {
    db_pool: PgPool,
    sink: Arc<dyn BuyingEventSink>,
}

impl BuyingWriteService {
    pub fn new(db_pool: PgPool) -> Self {
        Self { db_pool, sink: Arc::new(LoggingSink) }
    }
    pub fn with_sink(db_pool: PgPool, sink: Arc<dyn BuyingEventSink>) -> Self {
        Self { db_pool, sink }
    }

    // ---- Material Request ---------------------------------------------------

    pub async fn create_material_request(&self, m: NewMaterialRequest) -> Result<Uuid, BuyingError> {
        if m.lines.is_empty() { return Err(BuyingError::EmptyDocument); }
        for l in &m.lines { if l.quantity < Decimal::ZERO { return Err(BuyingError::NegativeQuantity); } }
        let id = Uuid::new_v4();
        let rt = m.request_type.unwrap_or_else(|| "purchase".into());
        let mut tx = self.db_pool.begin().await?;
        let r = sqlx::query(
            r#"INSERT INTO buying.material_requests
                (id, request_number, company_id, request_type, status, request_date, schedule_date, notes)
               VALUES ($1,$2,$3,$4::material_request_type,'draft'::purchase_doc_status,$5,$6,$7)"#,
        )
        .bind(id).bind(&m.request_number).bind(m.company_id).bind(&rt).bind(m.request_date).bind(m.schedule_date).bind(&m.notes)
        .execute(&mut *tx).await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { BuyingError::DuplicateNumber(m.request_number) } else { e.into() });
        }
        for l in &m.lines {
            sqlx::query("INSERT INTO buying.material_request_items (id, request_id, item_id, quantity) VALUES ($1,$2,$3,$4)")
                .bind(Uuid::new_v4()).bind(id).bind(l.item_id).bind(l.quantity).execute(&mut *tx).await?;
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
        let mr = sqlx::query(
            r#"SELECT company_id, status::text AS st FROM buying.material_requests
               WHERE id=$1 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(request_id).fetch_optional(&self.db_pool).await?
        .ok_or(BuyingError::SourceNotFound(request_id))?;
        let company_id: Uuid = mr.get("company_id");
        if mr.get::<String, _>("st") == "cancelled" {
            return Err(BuyingError::SourceNotConvertible(request_id.to_string()));
        }
        let items = sqlx::query(
            r#"SELECT item_id, quantity FROM buying.material_request_items
               WHERE request_id=$1 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(request_id).fetch_all(&self.db_pool).await?;

        let id = Uuid::new_v4();
        let mut tx = self.db_pool.begin().await?;
        let r = sqlx::query(
            r#"INSERT INTO buying.request_for_quotations
                (id, rfq_number, material_request_id, company_id, status, rfq_date, response_due)
               VALUES ($1,$2,$3,$4,'submitted'::purchase_doc_status,CURRENT_DATE,$5)"#,
        )
        .bind(id).bind(&rfq_number).bind(request_id).bind(company_id).bind(response_due)
        .execute(&mut *tx).await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { BuyingError::DuplicateNumber(rfq_number) } else { e.into() });
        }
        for it in &items {
            sqlx::query("INSERT INTO buying.rfq_items (id, rfq_id, item_id, quantity) VALUES ($1,$2,$3,$4)")
                .bind(Uuid::new_v4()).bind(id).bind(it.get::<Uuid, _>("item_id")).bind(it.get::<Decimal, _>("quantity"))
                .execute(&mut *tx).await?;
        }
        for sup in supplier_ids {
            sqlx::query("INSERT INTO buying.rfq_suppliers (id, rfq_id, supplier_id) VALUES ($1,$2,$3)")
                .bind(Uuid::new_v4()).bind(id).bind(sup).execute(&mut *tx).await?;
        }
        sqlx::query("UPDATE buying.material_requests SET status='ordered'::purchase_doc_status WHERE id=$1")
            .bind(request_id).execute(&mut *tx).await?;
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
        let rfq = sqlx::query(
            r#"SELECT company_id, status::text AS st FROM buying.request_for_quotations
               WHERE id=$1 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(rfq_id).fetch_optional(&self.db_pool).await?
        .ok_or(BuyingError::SourceNotFound(rfq_id))?;
        let company_id: Uuid = rfq.get("company_id");
        if rfq.get::<String, _>("st") == "cancelled" {
            return Err(BuyingError::SourceNotConvertible(rfq_id.to_string()));
        }
        let items = sqlx::query(
            r#"SELECT item_id, quantity FROM buying.rfq_items
               WHERE rfq_id=$1 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(rfq_id).fetch_all(&self.db_pool).await?;
        let rate_of = |item: Uuid| quoted_rates.iter().find(|(i, _)| *i == item).map(|(_, r)| *r).unwrap_or(Decimal::ZERO);

        let id = Uuid::new_v4();
        let mut tx = self.db_pool.begin().await?;
        let r = sqlx::query(
            r#"INSERT INTO buying.supplier_quotations
                (id, quotation_number, rfq_id, company_id, supplier_id, status, quotation_date, currency)
               VALUES ($1,$2,$3,$4,$5,'submitted'::purchase_doc_status,CURRENT_DATE,'IDR')"#,
        )
        .bind(id).bind(&quotation_number).bind(rfq_id).bind(company_id).bind(supplier_id)
        .execute(&mut *tx).await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { BuyingError::DuplicateNumber(quotation_number) } else { e.into() });
        }
        for it in &items {
            let item: Uuid = it.get("item_id");
            sqlx::query("INSERT INTO buying.supplier_quotation_items (id, quotation_id, item_id, quantity, rate) VALUES ($1,$2,$3,$4,$5)")
                .bind(Uuid::new_v4()).bind(id).bind(item).bind(it.get::<Decimal, _>("quantity")).bind(rate_of(item))
                .execute(&mut *tx).await?;
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
        let sq = sqlx::query(
            r#"SELECT company_id, supplier_id, currency, status::text AS st
               FROM buying.supplier_quotations WHERE id=$1 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(quotation_id).fetch_optional(&self.db_pool).await?
        .ok_or(BuyingError::SourceNotFound(quotation_id))?;
        if sq.get::<String, _>("st") != "submitted" {
            return Err(BuyingError::SourceNotConvertible(quotation_id.to_string()));
        }
        let items = sqlx::query(
            r#"SELECT item_id, quantity, rate FROM buying.supplier_quotation_items
               WHERE quotation_id=$1 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(quotation_id).fetch_all(&self.db_pool).await?;
        if items.is_empty() {
            return Err(BuyingError::EmptyDocument);
        }
        let lines: Vec<NewLine> = items.iter().map(|it| NewLine {
            item_id: it.get("item_id"),
            warehouse_id: None,
            description: None,
            quantity: it.get("quantity"),
            rate: it.get("rate"),
        }).collect();

        let order_id = self.create_purchase_order(NewPurchaseOrder {
            po_number,
            supplier_quotation_id: Some(quotation_id),
            order_kind: None,
            company_id: sq.get("company_id"),
            branch_id: None,
            supplier_id: sq.get("supplier_id"),
            order_date: chrono::Utc::now().date_naive(),
            schedule_date: None,
            currency: Some(sq.get("currency")),
            tax_rate,
            notes: None,
            lines,
        }).await?;

        sqlx::query("UPDATE buying.supplier_quotations SET status='ordered'::purchase_doc_status WHERE id=$1")
            .bind(quotation_id).execute(&self.db_pool).await?;
        Ok(order_id)
    }

    /// Load the exported `PurchaseOrderRef` (the brief §42 cross-module DTO) for one PO.
    pub async fn purchase_order_ref(&self, order_id: Uuid) -> Result<PurchaseOrderRef, BuyingError> {
        let row = sqlx::query(
            r#"SELECT supplier_id, company_id, order_kind::text AS kind, total, currency
               FROM buying.purchase_orders WHERE id=$1 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(order_id).fetch_optional(&self.db_pool).await?
        .ok_or(BuyingError::OrderNotFound(order_id))?;
        Ok(PurchaseOrderRef {
            id: order_id,
            supplier_id: row.get("supplier_id"),
            company_id: row.get("company_id"),
            order_kind: row.get("kind"),
            grand_total: row.get("total"),
            currency: row.get("currency"),
        })
    }

    // ---- Supplier Quotation -------------------------------------------------

    pub async fn create_supplier_quotation(&self, q: NewSupplierQuotation) -> Result<Uuid, BuyingError> {
        let (priced, _sub, _tax, _tot) = price_document(&q.lines, Decimal::ZERO)?;
        let id = Uuid::new_v4();
        let currency = q.currency.unwrap_or_else(|| "IDR".into());
        let mut tx = self.db_pool.begin().await?;
        let r = sqlx::query(
            r#"INSERT INTO buying.supplier_quotations
                (id, quotation_number, rfq_id, company_id, supplier_id, status, quotation_date, valid_till, currency)
               VALUES ($1,$2,$3,$4,$5,'submitted'::purchase_doc_status,$6,$7,$8)"#,
        )
        .bind(id).bind(&q.quotation_number).bind(q.rfq_id).bind(q.company_id).bind(q.supplier_id)
        .bind(q.quotation_date).bind(q.valid_till).bind(&currency)
        .execute(&mut *tx).await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { BuyingError::DuplicateNumber(q.quotation_number) } else { e.into() });
        }
        for p in &priced {
            sqlx::query("INSERT INTO buying.supplier_quotation_items (id, quotation_id, item_id, quantity, rate) VALUES ($1,$2,$3,$4,$5)")
                .bind(Uuid::new_v4()).bind(id).bind(p.item_id).bind(p.quantity).bind(p.rate).execute(&mut *tx).await?;
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
        let mut tx = self.db_pool.begin().await?;
        let r = sqlx::query(
            r#"INSERT INTO buying.purchase_orders
                (id, po_number, supplier_quotation_id, order_kind, company_id, branch_id, supplier_id,
                 status, order_date, schedule_date, currency, subtotal, tax_rate, tax_amount, total, notes)
               VALUES ($1,$2,$3,$4::order_kind,$5,$6,$7,'draft'::purchase_order_status,$8,$9,$10,$11,$12,$13,$14,$15)"#,
        )
        .bind(id).bind(&o.po_number).bind(o.supplier_quotation_id).bind(&kind).bind(o.company_id)
        .bind(o.branch_id).bind(o.supplier_id).bind(o.order_date).bind(o.schedule_date).bind(&currency)
        .bind(subtotal).bind(o.tax_rate).bind(tax_amount).bind(total).bind(&o.notes)
        .execute(&mut *tx).await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { BuyingError::DuplicateNumber(o.po_number) } else { e.into() });
        }
        for p in &priced {
            sqlx::query(
                r#"INSERT INTO buying.purchase_order_items
                    (id, order_id, item_id, warehouse_id, description, quantity, rate, line_amount)
                   VALUES ($1,$2,$3,$4,$5,$6,$7,$8)"#,
            )
            .bind(Uuid::new_v4()).bind(id).bind(p.item_id).bind(p.warehouse_id).bind(&p.description)
            .bind(p.quantity).bind(p.rate).bind(p.line_amount)
            .execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(id)
    }

    /// Confirm a draft PO → `to_receive_and_bill` (awaiting both receipt and billing). Emits
    /// `PurchaseOrderConfirmed`.
    pub async fn confirm_purchase_order(&self, order_id: Uuid) -> Result<(), BuyingError> {
        let row = sqlx::query(
            r#"UPDATE buying.purchase_orders SET status='to_receive_and_bill'::purchase_order_status
               WHERE id=$1 AND status='draft'::purchase_order_status AND (metadata->>'deleted_at') IS NULL
               RETURNING company_id, supplier_id, total, currency"#,
        )
        .bind(order_id).fetch_optional(&self.db_pool).await?
        .ok_or_else(|| BuyingError::NotConfirmable(order_id.to_string()))?;
        self.sink.publish(BuyingEvent::PurchaseOrderConfirmed(PurchaseOrderConfirmed {
            order_id, company_id: row.get("company_id"), supplier_id: row.get("supplier_id"),
            grand_total: row.get("total"), currency: row.get("currency"),
        }));
        Ok(())
    }

    // ---- Receipt seam (buying -> inventory) --------------------------------

    /// Build the cross-module receipt request for a confirmed PO (the envelope buying emits; an ACL
    /// maps it into inventory's `ReceiptExpected`). Requests the not-yet-received quantity per line.
    /// Emits `ReceiptRequested`.
    pub async fn build_receipt_request(&self, order_id: Uuid) -> Result<ReceiptRequestEnvelope, BuyingError> {
        let hdr = sqlx::query(
            r#"SELECT company_id, supplier_id, currency, status::text AS st
               FROM buying.purchase_orders WHERE id=$1 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(order_id).fetch_optional(&self.db_pool).await?
        .ok_or(BuyingError::OrderNotFound(order_id))?;
        if hdr.get::<String, _>("st") == "draft" {
            return Err(BuyingError::NotConfirmable(order_id.to_string()));
        }
        let rows = sqlx::query(
            r#"SELECT item_id, rate, (quantity - received_qty) AS remaining
               FROM buying.purchase_order_items
               WHERE order_id=$1 AND (metadata->>'deleted_at') IS NULL AND (quantity - received_qty) > 0"#,
        )
        .bind(order_id).fetch_all(&self.db_pool).await?;
        let lines: Vec<ReceiptRequestLine> = rows.iter().map(|r| ReceiptRequestLine {
            item_id: r.get("item_id"), quantity: r.get("remaining"), rate: r.get("rate"),
        }).collect();
        let env = ReceiptRequestEnvelope {
            order_id, company_id: hdr.get("company_id"), supplier_id: hdr.get("supplier_id"),
            currency: hdr.get("currency"), lines,
        };
        self.sink.publish(BuyingEvent::ReceiptRequested(env.clone()));
        Ok(env)
    }

    /// Record a receipt against a PO (inbound handler for inventory's `StockReceived`): allocate the
    /// received quantity across the item's PO lines, filling each up to `quantity` (the 3-way-match
    /// ceiling — no over-receipt tolerance configured, council 2026-07-05). Rejects over-receipt.
    pub async fn mark_received(&self, order_id: Uuid, receipts: &[(Uuid, Decimal)]) -> Result<(), BuyingError> {
        let mut tx = self.db_pool.begin().await?;
        for (item_id, qty) in receipts {
            // capacity per line = quantity - received_qty
            if let Err(e) = Self::allocate(&mut tx, order_id, *item_id, *qty, "received_qty", "quantity",
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
        let mut tx = self.db_pool.begin().await?;
        for (item_id, qty) in billed {
            // capacity per line = received_qty - billed_qty
            if let Err(e) = Self::allocate(&mut tx, order_id, *item_id, *qty, "billed_qty", "received_qty",
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

    /// Allocate `qty` of `item_id` across a PO's lines, adding to `col` up to each line's `cap_col`
    /// (fill-in-order). Rejects with `over_err` if the total remaining capacity is exceeded — so
    /// `received_qty <= quantity` and `billed_qty <= received_qty` hold per line (3-way match).
    /// Correct even when a PO has several lines of the same item.
    async fn allocate(
        tx: &mut sqlx::PgConnection, order_id: Uuid, item_id: Uuid, mut qty: Decimal,
        col: &str, cap_col: &str, over_err: BuyingError,
    ) -> Result<(), BuyingError> {
        let sql = format!(
            "SELECT id, ({cap_col} - {col}) AS capacity FROM buying.purchase_order_items \
             WHERE order_id=$1 AND item_id=$2 AND (metadata->>'deleted_at') IS NULL ORDER BY id FOR UPDATE"
        );
        let lines = sqlx::query(&sql).bind(order_id).bind(item_id).fetch_all(&mut *tx).await?;
        let total_cap: Decimal = lines.iter().map(|r| r.get::<Decimal, _>("capacity")).sum();
        if qty > total_cap {
            return Err(over_err);
        }
        for line in &lines {
            if qty <= Decimal::ZERO { break; }
            let cap: Decimal = line.get("capacity");
            if cap <= Decimal::ZERO { continue; }
            let take = if qty < cap { qty } else { cap };
            let upd = format!("UPDATE buying.purchase_order_items SET {col} = {col} + $2 WHERE id=$1");
            sqlx::query(&upd).bind(line.get::<Uuid, _>("id")).bind(take).execute(&mut *tx).await?;
            qty -= take;
        }
        Ok(())
    }

    /// Recompute a PO's status from its two 3-way-match watermarks: `completed` iff every line is
    /// fully received AND fully billed; else `to_receive` / `to_bill` / `to_receive_and_bill`. Emits
    /// `PurchaseOrderFullyReceived` / `PurchaseOrderFullyBilled` on the transition into each milestone.
    async fn recompute_order_status(&self, order_id: Uuid) -> Result<(), BuyingError> {
        let row = sqlx::query(
            r#"SELECT po.company_id, po.status::text AS prior,
                      bool_and(i.received_qty >= i.quantity) AS received_all,
                      bool_and(i.billed_qty >= i.quantity) AS billed_all
               FROM buying.purchase_order_items i
               JOIN buying.purchase_orders po ON po.id = i.order_id
               WHERE i.order_id=$1 AND (i.metadata->>'deleted_at') IS NULL
               GROUP BY po.company_id, po.status"#,
        )
        .bind(order_id).fetch_one(&self.db_pool).await?;
        let company_id: Uuid = row.get("company_id");
        let prior: String = row.get("prior");
        let received_all = row.get::<Option<bool>, _>("received_all").unwrap_or(false);
        let billed_all = row.get::<Option<bool>, _>("billed_all").unwrap_or(false);
        let next = match (received_all, billed_all) {
            (true, true) => "completed",
            (true, false) => "to_bill",
            (false, true) => "to_receive",
            (false, false) => "to_receive_and_bill",
        };
        sqlx::query(
            r#"UPDATE buying.purchase_orders SET status=$2::purchase_order_status
               WHERE id=$1 AND status = ANY(ARRAY['to_receive','to_bill','to_receive_and_bill']::purchase_order_status[])"#,
        )
        .bind(order_id).bind(next).execute(&self.db_pool).await?;

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
