# Extension Guide — backbone-buying

> Public contract per `docs/erp/extension-contract.md`. Stable path:
> `backbone_buying::application::service::*` (the generated `exports/` tree is unwired scaffolding).

## Public surface
**A. Domain events** (`buying_events`, the 8-variant `BuyingEvent`): `MaterialRequestRaised`,
`RfqIssued`, `SupplierQuotationReceived`, `PurchaseOrderConfirmed`, `ReceiptRequested`
(`ReceiptRequestEnvelope`), `ThreeWayMatchFailed` (over-receipt/over-billing variance),
`PurchaseOrderFullyReceived`, `PurchaseOrderFullyBilled`.

**A2. Exported DTO** — `PurchaseOrderRef {id, supplier_id, company_id, order_kind, grand_total,
currency}` via `BuyingWriteService::purchase_order_ref`. **Conversions** `convert_*` execute the
MR→RFQ→SQ→PO funnel (copy+link+advance).
**B. The receipt-request wire type** — `ReceiptRequestEnvelope { order_id, company_id, supplier_id,
currency, lines[] }`; an ACL maps it into inventory's `ReceiptExpected`; inbound `StockReceived`
routes to `BuyingWriteService::mark_received`.

## How a consumer extends
1. Subscribe to a domain event — implement `BuyingEventSink` in your crate / a `*_custom.rs` and pass
   it to `BuyingWriteService::with_sink`.
2. Wire the receipt seam — map `ReceiptRequested` → inventory `ReceiptExpected` (supply warehouse +
   GL accounts); route `StockReceived` → `mark_received`.
3. Keep logic in `user_owned`/`*_custom.rs` — survives regen (proven by
   `scripts/receipt_seam_roundtrip.sh`).

## Not a contract
Generated CRUD events; internal repositories/services; `// <<< CUSTOM` blocks (own edits only).

## Deferred surfaces
PurchasableItem/Supplier' projections, `PurchaseInvoicePosted` intake (billing), subcontract legs,
multi-currency — additive when built.
