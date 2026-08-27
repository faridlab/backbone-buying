//! Validated write path for buying (hand-authored, user-owned) — the procure-to-pay intent
//! pipeline + the receipt seam. Mirrors selling's order-to-cash shape.
//!
//! Closes the CRUD-bypass: material requests / RFQs / supplier quotations / purchase orders are
//! transactional documents whose money must be consistent. Creates compute line amounts + totals
//! server-side (2dp half-up) and reject empty documents; header+lines are written in ONE
//! transaction. `build_receipt_request` emits a `ReceiptRequestEnvelope` (an ACL maps it into
//! inventory's `ReceiptExpected`); `mark_received` / `mark_billed` advance the 3-way-match
//! watermarks and recompute the PO status. Buying emits NO `AccountingPost`.
//!
//! **Layering (the module's 4-layer rule):** this service ORCHESTRATES — it validates, computes the
//! money, owns the unit of work (`begin`/`commit`), drives the repositories, and publishes events.
//! It holds no SQL: every statement lives on the repositories in `infrastructure::persistence`,
//! whose custom methods take the caller's transaction so a cross-entity write (header + lines, or
//! the receipt watermark bumps + status recompute) commits as one unit. The RLS scope wrappers
//! (ADR-0008) stay HERE, in the service, because the service is what knows the company; tx-taking
//! repo methods ride the bind this service already made.
//!
//! **This file is the hub:** it holds the module's vocabulary (input structs, `Repos`, errors,
//! shared money helpers) and the constructors. The rest of the write surface is chunked into
//! focused siblings, each an `impl BuyingWriteService` block over these same types:
//!
//! - [`super::buying_material_request`] — funnel entry: raise a material request, convert MR → RFQ.
//! - [`super::buying_quotation`] — supplier quotation step: create SQ, convert RFQ → SQ, convert SQ → PO.
//! - [`super::buying_order_create`] — `create_purchase_order` (header + lines, server-owned totals).
//! - [`super::buying_order_lifecycle`] — `confirm_purchase_order`, `purchase_order_ref`.
//! - [`super::buying_receipt`] — the receipt seam + 3-way match: `build_receipt_request`,
//!   `mark_received`, `mark_billed`, and the private allocate/recompute helpers they share.

use rust_decimal::{Decimal, RoundingStrategy};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::infrastructure::persistence::{
    MaterialRequestItemRepository, MaterialRequestRepository, PurchaseAgreementLineRepository,
    PurchaseAgreementRepository, PurchaseCompanySettingRepository, PurchaseOrderItemRepository,
    PurchaseOrderRepository, RequestForQuotationRepository, RfqItemRepository, RfqSupplierRepository,
    SupplierPriceRepository, SupplierQuotationItemRepository, SupplierQuotationRepository,
    SupplierReminderSettingRepository,
};

use super::buying_events::{BuyingEventSink, LoggingSink};

pub(super) fn money(v: Decimal) -> Decimal {
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
    /// How `received_qty` advances on this line: `stock_moves` (the receipt seam allocates it —
    /// default) or `manual` (an operator sets it; the seam allocation skips the line).
    pub qty_received_method: Option<String>,
    /// Billing capacity formula: `on_received` (billed caps at received — default) or `purchase`
    /// (billed caps at quantity; order-driven service lines may bill ahead of receipt).
    pub purchase_method: Option<String>,
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
    /// Order-time exchange-rate snapshot, COMPANY currency per 1 PO-currency unit. REQUIRED
    /// (loudly) whenever the PO currency differs from the company currency; same-currency POs
    /// fix 1 regardless of what was supplied.
    pub currency_rate: Option<Decimal>,
    /// Blanket call-off source (set by `create_call_off_po`; direct POs leave it None).
    pub agreement_id: Option<Uuid>,
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
    /// The double-validation gate refused: an over-threshold PO may only be approved by a manager.
    NotApprovable(String),
    /// Cancel refused: the order is locked (G4).
    OrderLocked(Uuid),
    /// Cancel refused: the order has billed lines (G5).
    OrderBilled(Uuid),
    /// Cancel/reset refused from the order's current state.
    NotCancelable(String),
    /// A foreign-currency PO arrived without an order-time exchange-rate snapshot. Refused loudly:
    /// a silent rate-1 default would mis-classify every foreign-currency PO at the gate.
    CurrencyRateRequired,
    /// Acknowledge refused: the order is not in the operational (purchase) state.
    NotAcknowledgable(String),
    /// Delete refused: the order (or its line) is not in a deletable state (G8/G9).
    NotDeletable(String),
    /// A PO line's receipt method cannot take the requested write (e.g. a seam allocation or
    /// manual receipt aimed at a line whose method is the other tier).
    InvalidLineMethod(String),
    /// A source funnel document (MR / RFQ / SQ) was not found.
    SourceNotFound(Uuid),
    /// A source funnel document is not in a convertible state.
    SourceNotConvertible(String),
    /// An agreement (or its line) was not found.
    AgreementNotFound(Uuid),
    /// An agreement verb was refused from the agreement's current state (e.g. confirming a done
    /// agreement, editing prices on a draft, re-sequencing a non-draft).
    AgreementNotConvertible(String),
    /// Close/cancel refused: a call-off PO in a pre-confirmed state still hangs off the agreement.
    AgreementHasDraftOrders(Uuid),
    /// A call-off would push an agreement line past its blanket quantity.
    AgreementExceeded(Uuid),
    /// A matching/matching-style verb arrived with an empty selection (G6).
    NoLinesSelected,
    /// 3-way match: cannot receive more than ordered (no over-receipt tolerance configured).
    OverReceipt { item_id: Uuid },
    /// 3-way match: cannot bill more than the line's billing capacity (received for
    /// on_received lines; ordered for order-driven service lines).
    OverBilling { item_id: Uuid },
    /// 3-way match: cannot return more than the un-billed received portion (credit the billed goods first).
    OverReturn { item_id: Uuid },
    /// 3-way match: cannot credit more than has been billed.
    OverCredit { item_id: Uuid },
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
            BuyingError::NotApprovable(_) => "not_approvable".into(),
            BuyingError::OrderLocked(_) => "order_locked".into(),
            BuyingError::OrderBilled(_) => "order_billed".into(),
            BuyingError::NotCancelable(_) => "not_cancelable".into(),
            BuyingError::CurrencyRateRequired => "currency_rate_required".into(),
            BuyingError::NotAcknowledgable(_) => "not_acknowledgable".into(),
            BuyingError::NotDeletable(_) => "not_deletable".into(),
            BuyingError::InvalidLineMethod(_) => "invalid_line_method".into(),
            BuyingError::SourceNotFound(_) => "source_not_found".into(),
            BuyingError::SourceNotConvertible(_) => "source_not_convertible".into(),
            BuyingError::AgreementNotFound(_) => "agreement_not_found".into(),
            BuyingError::AgreementNotConvertible(_) => "agreement_not_convertible".into(),
            BuyingError::AgreementHasDraftOrders(_) => "agreement_has_draft_orders".into(),
            BuyingError::AgreementExceeded(_) => "agreement_exceeded".into(),
            BuyingError::NoLinesSelected => "no_lines_selected".into(),
            BuyingError::OverReceipt { .. } => "over_receipt".into(),
            BuyingError::OverBilling { .. } => "over_billing".into(),
            BuyingError::OverReturn { .. } => "over_return".into(),
            BuyingError::OverCredit { .. } => "over_credit".into(),
            BuyingError::Db(_) => "internal_error".into(),
        }
    }
    pub fn http_status(&self) -> u16 {
        match self {
            BuyingError::OrderNotFound(_)
            | BuyingError::SourceNotFound(_)
            | BuyingError::AgreementNotFound(_) => 404,
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
pub(super) fn is_dup(e: &sqlx::Error) -> bool {
    e.as_database_error().map(|d| d.is_unique_violation()).unwrap_or(false)
}

pub(super) struct PricedLine {
    pub(super) item_id: Uuid,
    pub(super) warehouse_id: Option<Uuid>,
    pub(super) description: Option<String>,
    pub(super) quantity: Decimal,
    pub(super) rate: Decimal,
    pub(super) line_amount: Decimal,
}

/// Compute `line_amount = money(qty*rate)` per line + `(subtotal, tax_amount, total)`; reject empty/negative.
pub(super) fn price_document(lines: &[NewLine], tax_rate: Decimal) -> Result<(Vec<PricedLine>, Decimal, Decimal, Decimal), BuyingError> {
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
pub(super) struct Repos {
    pub(super) material_requests: MaterialRequestRepository,
    pub(super) material_request_items: MaterialRequestItemRepository,
    pub(super) rfqs: RequestForQuotationRepository,
    pub(super) rfq_items: RfqItemRepository,
    pub(super) rfq_suppliers: RfqSupplierRepository,
    pub(super) supplier_quotations: SupplierQuotationRepository,
    pub(super) supplier_quotation_items: SupplierQuotationItemRepository,
    pub(super) purchase_orders: PurchaseOrderRepository,
    pub(super) purchase_order_items: PurchaseOrderItemRepository,
    pub(super) purchase_agreements: PurchaseAgreementRepository,
    pub(super) purchase_agreement_lines: PurchaseAgreementLineRepository,
    pub(super) supplier_prices: SupplierPriceRepository,
    pub(super) purchase_company_settings: PurchaseCompanySettingRepository,
    pub(super) supplier_reminder_settings: SupplierReminderSettingRepository,
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
            purchase_agreements: PurchaseAgreementRepository::new(db_pool.clone()),
            purchase_agreement_lines: PurchaseAgreementLineRepository::new(db_pool.clone()),
            supplier_prices: SupplierPriceRepository::new(db_pool.clone()),
            purchase_company_settings: PurchaseCompanySettingRepository::new(db_pool.clone()),
            supplier_reminder_settings: SupplierReminderSettingRepository::new(db_pool.clone()),
        }
    }
}

#[derive(Clone)]
pub struct BuyingWriteService {
    pub(super) db_pool: PgPool,
    pub(super) repos: Arc<Repos>,
    pub(super) sink: Arc<dyn BuyingEventSink>,
}

impl BuyingWriteService {
    pub fn new(db_pool: PgPool) -> Self {
        Self::with_sink(db_pool, Arc::new(LoggingSink))
    }
    pub fn with_sink(db_pool: PgPool, sink: Arc<dyn BuyingEventSink>) -> Self {
        let repos = Arc::new(Repos::new(&db_pool));
        Self { db_pool, repos, sink }
    }
    /// The event sink this service publishes through — jobs and hosts reuse it so one composition
    /// emits through one sink.
    pub fn event_sink(&self) -> &Arc<dyn BuyingEventSink> {
        &self.sink
    }
}
