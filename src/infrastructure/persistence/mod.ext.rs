// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

// The hand-written buying SQL's parameter/projection types. Every repository listed here is declared
// `user_owned` in metaphor.codegen.yaml — see the write service, which orchestrates them.
pub use material_request_repository::{MaterialRequestSourceRow, NewMaterialRequestRow};
pub use material_request_item_repository::{NewMaterialRequestItemRow, RequestedLineRow};
pub use purchase_order_repository::{
    ConfirmedOrderRow, MaturityRow, NewPurchaseOrderRow, OrderGateRow, PoDemandCandidateRow,
    PurchaseOrderHeaderRow, PurchaseOrderRefRow,
};
pub use purchase_order_item_repository::{
    AllocatableLineRow, ManualReceiptRow, MatchWatermark, NewPurchaseOrderItemRow, RemainingLineRow,
    ReverseWatermark,
};
pub use purchase_company_setting_repository::{CompanyPurchaseSettingsRow, SettingsUpsert};
pub use purchase_agreement_repository::{AgreementStateRow, NewAgreementRow};
pub use purchase_agreement_line_repository::{AgreementLineRow, NewAgreementLineRow};
pub use supplier_reminder_setting_repository::{SupplierReminderRow, SupplierReminderUpsert};
pub use request_for_quotation_repository::{NewRfqRow, RfqSourceRow};
pub use rfq_item_repository::{NewRfqItemRow, RfqLineRow};
pub use rfq_supplier_repository::NewRfqSupplierRow;
pub use supplier_quotation_repository::{
    NewQuotationFromRfqRow, NewSupplierQuotationRow, SupplierQuotationSourceRow,
};
pub use supplier_quotation_item_repository::{NewSupplierQuotationItemRow, QuotedLineRow};
