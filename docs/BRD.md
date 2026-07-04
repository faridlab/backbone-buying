# BRD — backbone-buying

> Business Requirements & Rules. Tier 2 · Supply Chain. Date: 2026-07-05. Pairs with
> `docs/business-flows/golden-cases.md`.

## Documents
Material Request (demand) · Request for Quotation (sourcing fan-out) · Supplier Quotation (offer) ·
Purchase Order (+items; the commitment, with 3-way-match watermarks).

## Business rules
**BR-1 (server-side money).** `line_amount = money(qty·rate)`; `subtotal = Σ line_amount`;
`tax_amount = money(subtotal·tax_rate/100)`; `total = subtotal + tax_amount`. 2dp half-up.

**BR-2 (non-empty / non-negative).** ≥1 line; no negative qty/rate. → `empty_document` /
`negative_quantity`.

**BR-3 (unique numbers).** MR/SQ/PO numbers unique (soft-delete aware). → `duplicate_number`.

**BR-4 (PO lifecycle — ADR-001).** `draft → to_receive_and_bill` on confirm; recomputes to
`to_receive` (billed, awaiting goods) / `to_bill` (received, awaiting invoice) / `completed` from the
two watermarks. `completed` requires every line **fully received AND fully billed**.

**BR-5 (3-way-match watermarks).** `received_qty` advances on inventory's `StockReceived`
(`mark_received`); `billed_qty` on billing's `PurchaseInvoicePosted` (`mark_billed`).

**BR-6 (receipt seam — ADR-002).** A confirmed PO emits a `ReceiptRequestEnvelope` an ACL maps into
inventory's `ReceiptExpected`; inventory receives (asset posts) and reports `StockReceived`, routed
back to `mark_received`. Buying holds no normal Cargo dependency on inventory. The request asks only
the un-received remainder per line.

**BR-7 (no AccountingPost).** Buying posts nothing; the asset post (inventory) and A/P post (billing)
are downstream.

**BR-8 (subcontracting subtype).** `order_kind = subcontract` on the PO (service + supplied BOM);
physical legs are inventory Stock Entries (deferred orchestration).

## Events
`PurchaseOrderConfirmed`, `ReceiptRequested`. (Consumed: `StockReceived`, `PurchaseInvoicePosted`.)

## Deferred (with reason)
A/P post (billing), projections (catalog/party-gated), scorecard, RFQ comparison, subcontract legs,
tax (backbone-tax), landed cost, multi-currency.
