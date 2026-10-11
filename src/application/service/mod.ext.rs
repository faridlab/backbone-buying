// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

// Hand-authored (user-owned): domain events + the validated write path + receipt seam.
pub mod buying_events;
pub mod buying_write_service;
// The write surface, chunked: each is an `impl BuyingWriteService` block over the vocabulary that
// stays in `buying_write_service` (so the `buying_write_service::{NewLine, ...}` import paths are
// unchanged).
pub mod buying_material_request;
pub mod buying_quotation;
pub mod buying_order_create;
pub mod buying_order_lifecycle;
pub mod buying_receipt;
pub mod buying_agreement;
pub mod buying_bill_matching;
pub mod buying_settings;
pub mod buying_po_grouping;
pub use buying_events::{
    BuyingEvent, BuyingEventSink, DocumentRaised, PurchaseOrderConfirmed, PurchaseOrderMilestone,
    PurchaseOrderRef, ReceiptRequestEnvelope, ReceiptRequestLine, ThreeWayMatchFailed,
};
pub use buying_write_service::{
    BuyingError, BuyingWriteService, NewLine, NewMaterialRequest, NewPurchaseOrder,
    NewSupplierQuotation, SimpleLine,
};
pub use buying_po_grouping::{PoDemand, PoMergeCandidate};
