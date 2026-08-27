//! Buying domain events + the outbound receipt-request envelope (hand-authored, user-owned).
//!
//! Buying emits NO `AccountingPost` — it *drives* inventory's Purchase Receipt (asset post) and
//! billing's Purchase Invoice (A/P post) via events. `ReceiptRequestEnvelope` is the serialized
//! cross-module request an ACL maps into inventory's `ReceiptExpected` (adding the warehouse + GL
//! accounts inventory owns). Zero shared Rust type, zero Cargo edge.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Serde default for the `order_kind` fields added to already-published payloads: events serialized
/// before the field existed deserialize as `standard`. Additive, backward-compatible.
fn default_order_kind() -> String {
    "standard".into()
}

/// A purchase order was confirmed (the supply commitment). `order_kind` lets consumers distinguish a
/// subcontract PO (service + supplied-material BOM) from a standard one without a read-back.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PurchaseOrderConfirmed {
    pub order_id: Uuid,
    pub company_id: Uuid,
    pub supplier_id: Uuid,
    pub grand_total: Decimal,
    pub currency: String,
    #[serde(default = "default_order_kind")]
    pub order_kind: String,
}

/// A confirm was parked in `to_approve` by the double-validation gate (over-threshold PO, no
/// manager claim). Not a confirmation — the supply commitment is not yet made.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PurchaseOrderPendingApproval {
    pub order_id: Uuid,
    pub company_id: Uuid,
}

/// A funnel document was raised/derived (material request → RFQ → supplier quotation → PO).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DocumentRaised {
    pub document_id: Uuid,
    pub company_id: Uuid,
    /// The source document it was converted from (None for a directly-created document).
    pub source_id: Option<Uuid>,
}

/// A 3-way-match variance was detected and REJECTED (over-receipt, over-billing, over-return, or
/// over-credit). §33: mismatch flagged before billing — broadcast so an async consumer can react, not just
/// the synchronous caller.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThreeWayMatchFailed {
    pub order_id: Uuid,
    pub item_id: Uuid,
    /// "over_receipt" | "over_billing" | "over_return" | "over_credit".
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
    #[serde(default = "default_order_kind")]
    pub order_kind: String,
    pub lines: Vec<ReceiptRequestLine>,
}

/// One line of a recorded receipt — exactly the quantity THIS receipt applied to one PO line
/// (post fill-in-order allocation), not a cumulative watermark.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PurchaseReceiptLine {
    pub item_id: Uuid,
    pub quantity: Decimal,
    pub rate: Decimal,
}

/// Per-receipt feedback signal: a receipt was applied to a PO. The completion milestones
/// (`PurchaseOrderFullyReceived` etc.) only fire at watermark completion; this fires on EVERY
/// receipt, so per-receipt consumers (e.g. subcontracting progress) don't wait for the last one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PurchaseReceiptRecorded {
    pub order_id: Uuid,
    pub company_id: Uuid,
    #[serde(default = "default_order_kind")]
    pub order_kind: String,
    pub lines: Vec<PurchaseReceiptLine>,
}

/// One bill-line ↔ PO-line pairing proposed by manual bill matching. `bill_line_ref` is the
/// composing billing document's own line reference (buying holds no bill rows — it never crosses
/// the module boundary, so the ref stays opaque here).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BillLineMatch {
    pub bill_line_ref: String,
    pub po_item_id: Uuid,
    pub quantity: Decimal,
}

/// A manual bill-line match was accepted. The `purchase_line_id` write on the bill lines is
/// billing's surface through its own write path; the composing ACL reacts to this event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BillLinesMatched {
    pub order_id: Uuid,
    pub company_id: Uuid,
    pub matches: Vec<BillLineMatch>,
}

/// A confirmed PO's expected receipt is due within the supplier's reminder window. Buying has NO
/// mail stack — this event IS the notification port; a composing service wires the email send.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PurchaseReceiptReminderDue {
    pub order_id: Uuid,
    pub company_id: Uuid,
    pub supplier_id: Uuid,
    pub schedule_date: NaiveDate,
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
    /// A purchase return reduced `received_qty` (goods sent back to the supplier). Reverse direction of
    /// [`BuyingEvent::PurchaseOrderFullyReceived`]; may drop the PO's `receipt_status` compute back
    /// from `full` to `partial`.
    PurchaseReturned(PurchaseOrderMilestone),
    /// A credit note reduced `billed_qty` (supplier-issued invoice correction). Reverse direction of
    /// [`BuyingEvent::PurchaseOrderFullyBilled`]; may move the PO's `invoice_status` compute back
    /// from `invoiced` to `to_invoice`.
    CreditNoted(PurchaseOrderMilestone),
    /// Confirm parked in `to_approve` by the double-validation gate.
    PurchaseOrderPendingApproval(PurchaseOrderPendingApproval),
    /// A receipt was applied (fires per receipt, not only at completion).
    PurchaseReceiptRecorded(PurchaseReceiptRecorded),
    /// A manual bill-line match was accepted (the seam vocabulary for billing's
    /// `purchase_line_id` write).
    BillLinesMatched(BillLinesMatched),
    /// A confirmed PO's receipt falls inside the supplier reminder window (notification port).
    PurchaseReceiptReminderDue(PurchaseReceiptReminderDue),
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
