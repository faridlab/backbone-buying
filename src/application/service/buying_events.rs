//! Buying domain events + the outbound receipt-request envelope (hand-authored, user-owned).
//!
//! Buying emits NO `AccountingPost` — it *drives* inventory's Purchase Receipt (asset post) and
//! billing's Purchase Invoice (A/P post) via events. `ReceiptRequestEnvelope` is the serialized
//! cross-module request an ACL maps into inventory's `ReceiptExpected` (adding the warehouse + GL
//! accounts inventory owns). Zero shared Rust type, zero Cargo edge.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A purchase order was confirmed (the supply commitment).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PurchaseOrderConfirmed {
    pub order_id: Uuid,
    pub company_id: Uuid,
    pub supplier_id: Uuid,
    pub grand_total: Decimal,
    pub currency: String,
}

/// A funnel document was raised/derived (material request → RFQ → supplier quotation → PO).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DocumentRaised {
    pub document_id: Uuid,
    pub company_id: Uuid,
    /// The source document it was converted from (None for a directly-created document).
    pub source_id: Option<Uuid>,
}

/// A 3-way-match variance was detected and REJECTED (over-receipt or over-billing). §33: mismatch
/// flagged before billing — broadcast so an async consumer can react, not just the synchronous caller.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThreeWayMatchFailed {
    pub order_id: Uuid,
    pub item_id: Uuid,
    /// "over_receipt" | "over_billing".
    pub kind: String,
}

/// A PO's watermark reached full completion (received or billed) — the signal downstream needs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PurchaseOrderMilestone {
    pub order_id: Uuid,
    pub company_id: Uuid,
}

/// One line of a receipt request (carries the unit cost for inventory's asset post).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReceiptRequestLine {
    pub item_id: Uuid,
    pub quantity: Decimal,
    pub rate: Decimal,
}

/// The cross-module request buying emits when a confirmed PO is ready to receive. Serialized (the
/// wire contract) — a composition layer maps it into inventory's `ReceiptExpected` (adding the
/// warehouse + Inventory/GR-IR accounts inventory owns).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReceiptRequestEnvelope {
    pub order_id: Uuid,
    pub company_id: Uuid,
    pub supplier_id: Uuid,
    pub currency: String,
    pub lines: Vec<ReceiptRequestLine>,
}

/// Exported reference DTO for a purchase order (the brief §42 shape) — richer than the generated
/// `{id}`-only ref. Built by `BuyingWriteService::purchase_order_ref`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PurchaseOrderRef {
    pub id: Uuid,
    pub supplier_id: Uuid,
    pub company_id: Uuid,
    pub order_kind: String,
    pub grand_total: Decimal,
    pub currency: String,
}

/// The buying domain-event union (discriminated) published on the module event bus.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum BuyingEvent {
    MaterialRequestRaised(DocumentRaised),
    RfqIssued(DocumentRaised),
    SupplierQuotationReceived(DocumentRaised),
    PurchaseOrderConfirmed(PurchaseOrderConfirmed),
    ReceiptRequested(ReceiptRequestEnvelope),
    ThreeWayMatchFailed(ThreeWayMatchFailed),
    PurchaseOrderFullyReceived(PurchaseOrderMilestone),
    PurchaseOrderFullyBilled(PurchaseOrderMilestone),
}

/// Sink for buying domain events. Fire-and-forget; a real adapter wires a bus, tests record.
pub trait BuyingEventSink: Send + Sync {
    fn publish(&self, event: BuyingEvent);
}

/// Default sink — emits structured tracing events.
pub struct LoggingSink;

impl BuyingEventSink for LoggingSink {
    fn publish(&self, event: BuyingEvent) {
        tracing::info!(target: "buying.events", ?event, "buying domain event");
    }
}
