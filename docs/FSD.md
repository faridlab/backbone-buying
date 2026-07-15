# FSD — backbone-buying

> Functional Spec. Tier 2 · Supply Chain. Date: 2026-07-05.

## Entities (schema/models/*.model.yaml — SSoT)
MaterialRequest(+items) · RequestForQuotation(+items,+suppliers) · SupplierQuotation(+items) ·
PurchaseOrder(+items: `received_qty`/`billed_qty`, `order_kind`). Cross-module ids are logical FKs
(`@exclude_from_foreign_key_check`): supplier→party, item→catalog, company/branch→organization,
warehouse→inventory.

## Services (application/service — hand-authored, user_owned)
- `BuyingWriteService` — validated creates (`create_material_request` / `create_supplier_quotation`
  / `create_purchase_order`); the **conversion funnel** `convert_material_request_to_rfq` /
  `convert_rfq_to_supplier_quotation` / `convert_supplier_quotation_to_po` (copy lines/rates + link +
  advance the source's status); `purchase_order_ref` → enriched `PurchaseOrderRef`; `confirm_purchase_order` → `to_receive_and_bill` + emits
  `PurchaseOrderConfirmed`; `build_receipt_request` → `ReceiptRequestEnvelope` (un-received
  remainder) + emits `ReceiptRequested`; `mark_received` / `mark_billed` advance watermarks →
  `recompute_order_status`.
- `buying_events` — the 8-variant `BuyingEvent` (MaterialRequestRaised / RfqIssued /
  SupplierQuotationReceived / PurchaseOrderConfirmed / ReceiptRequested / ThreeWayMatchFailed /
  PurchaseOrderFullyReceived / PurchaseOrderFullyBilled) + `BuyingEventSink` + `ReceiptRequestEnvelope`
  + `PurchaseOrderRef`.

## HTTP surface (presentation/http/guarded_routes.rs)
`create_guarded_buying_routes(&BuyingModule, pool, TenantVerifier)` — read documents + validated
`POST /purchase-orders` + `POST /purchase-orders/confirm`. No generic mutation. The writes are
tenant-guarded: `company_id`/`branch_id` come from the signed Bearer token (`TenantContext`), never
from the request body. The receipt seam is service/job-driven.

## State machines
- MR/RFQ/SQ: `draft → submitted → ordered` / `cancelled` (`PurchaseDocStatus`).
- PurchaseOrder: `draft → to_receive_and_bill`, recomputing to `to_receive`/`to_bill`/`completed`
  from the two watermarks; `closed`/`cancelled`.

## Integration seams
- **Receipt seam (proven):** `build_receipt_request` → ACL → inventory `ReceiptExpected` → asset
  post; inventory `StockReceived` → `mark_received`. Zero normal Cargo edge. ADR-002,
  `tests/receipt_seam.rs`, `scripts/receipt_seam_roundtrip.sh`.
- **Inbound (future):** `ItemCreated`/`PartyCreated` → projections; `PurchaseInvoicePosted` (billing)
  → `mark_billed`.

## Test oracle
`buying_golden_cases` (7), `integrity_probes` (4), `receipt_seam` (1, real ledger + §5),
`funnel_and_events` (4, MR→RFQ→SQ→PO conversion + event surface + ref). **16 tests.**
