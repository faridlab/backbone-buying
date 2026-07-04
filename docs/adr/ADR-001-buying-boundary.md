# ADR-001: Buying owns procure-to-pay intent; it drives receipt + billing, emits no AccountingPost

**Status**: Accepted — Applied 2026-07-05
**Deciders**: Farid (owner), build session 2026-07-05
**Related**: `docs/erp/modules/backbone-buying.md`, `docs/erp/supply-chain.md` (§6.1), ADR-002 (receipt seam)

## Context

`backbone-buying` is the supply-pipeline mirror of `backbone-selling`: Material Request → RFQ →
Supplier Quotation → Purchase Order. It owns *intent* — demand, sourcing, supplier commitment, and
the 3-way-match watermarks — but posts nothing to the GL: the goods-receipt asset post
(`Dr Inventory · Cr GR/IR`) is `backbone-inventory`'s (Purchase Receipt), and A/P is billing's
(Purchase Invoice). Buying holds no masters; supplier/item/company/branch/warehouse are logical FKs.

## Decision

1. **Four documents, one funnel.** `MaterialRequest → RequestForQuotation → SupplierQuotation →
   PurchaseOrder` (+ line children). MR/RFQ/SQ are the sourcing funnel (validated creates + links);
   the **PurchaseOrder** is the commitment that drives receipt and billing. Money is computed
   server-side (`line_amount = money(qty·rate)`, subtotal/tax/total, 2dp half-up); generic CRUD is
   not mounted on the guarded surface.
2. **3-way-match watermarks gate completion.** `PurchaseOrderItem` carries `received_qty` and
   `billed_qty`; the PO recomputes: `to_receive_and_bill` on confirm → `to_receive` (billed,
   awaiting goods) / `to_bill` (received, awaiting invoice) / `completed` (both). `received_qty`
   advances on inventory's `StockReceived` (`mark_received`); `billed_qty` on billing's
   `PurchaseInvoicePosted` (`mark_billed`). A PO can complete only when fully received AND billed.
3. **Subcontracting folds in as a PO subtype (supply-chain §6.1).** `order_kind ∈ {standard,
   subcontract}` on the PO — no separate module. A subcontracting order *is* a PO whose lines are a
   service + supplied-material BOM; the physical legs (issue raw / receive FG) are inventory Stock
   Entries, orchestrated later. The enum + field exist now; the physical orchestration is deferred.
4. **Buying emits no AccountingPost.** It drives receipt + billing via events (ADR-002); the asset
   and A/P journals belong to inventory and billing respectively.

## Consequences

- The intent math + watermark gating are locked by `tests/buying_golden_cases.rs` (6 cases incl.
  partial-receipt remainder + subcontract subtype) and the guarded surface by
  `tests/integrity_probes.rs` (4).
- Buying is independently composable: it needs only a Postgres pool and a `BuyingEventSink`.
- Deferred (per the brief): PurchasableItem/Supplier' projections (ACL, catalog/party-gated),
  SupplierScorecard, RFQ multi-supplier comparison, subcontract physical-leg orchestration, PPh/PPN
  Input (backbone-tax at invoice time), landed-cost/import duty, real multi-currency.
