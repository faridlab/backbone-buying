# ADR-002: The buying↔inventory receipt seam (procure-to-pay, end-to-end)

**Status**: Accepted — Applied 2026-07-05 (proven end-to-end; mirror of selling's delivery seam)
**Deciders**: Farid (owner), build session 2026-07-05
**Related**: selling ADR-004 (delivery seam), inventory ADR-001/002, `docs/erp/extension-contract.md` §5

## Context

Inventory proved the goods-receipt asset post; selling proved the delivery seam. This ADR records the
symmetric **receipt seam**: a confirmed Purchase Order gets received by inventory (asset posts) and
the PO's received watermark advances — the procure-to-pay counterpart of order-to-cash.

## Decision

1. **Every cross-module hop is a serialized envelope mapped by an ACL — zero normal Cargo edges.**
   - Buying emits `ReceiptRequestEnvelope { order_id, company_id, supplier_id, lines[] }` (each line
     carries the unit `rate` inventory needs to value the receipt); a composition layer maps it into
     inventory's `ReceiptExpected` (adding the **warehouse + Inventory/GR-IR accounts inventory
     owns**) → inventory creates a **draft** Purchase Receipt.
   - Inventory `submit_purchase_receipt(sink)` writes the SLE + Bin and emits the asset
     `AccountingPost` (`Dr Inventory · Cr GR/IR`) into the real ledger, plus a
     `StockReceived { source_po_id, total_value }` event.
   - The composition routes `StockReceived` → buying `mark_received(po, lines)` → advances
     `received_qty`.
   The shipped buying library has **no normal dependency** on inventory or accounting
   (`cargo tree -e normal -i backbone-inventory`/`-i backbone-accounting` are empty; both are
   dev-dependencies for the seam test only).
2. **Inventory grew a `ReceiptExpected` intake** (`DeliveryIntake::on_receipt_expected`) — the exact
   mirror of `on_delivery_requested`, turning a buying request into a draft Purchase Receipt.
3. **Physical + financial stay decoupled and eventually consistent** (inventory ADR-002): the SLE+Bin
   commit first; the asset post is emitted after and is idempotent + repost-recoverable.

## Consequences

- **Proven, not asserted:** `tests/receipt_seam.rs` runs the full round-trip — buying confirms a PO
  (10 @ 100,000), emits a receipt request, inventory receives (asset journal `Dr Inventory 1,000,000
  · Cr GR/IR 1,000,000`), buying's `received_qty` advances to `to_bill`, simulated billing →
  `completed`; the Bin holds 10 @ 100,000.
- **Extension-contract §5 discharged for the seam:** `scripts/receipt_seam_roundtrip.sh` regenerates
  **both** modules and asserts every ACL/consumer file is byte-identical and the seam stays green.
- This is the **second proven cross-module fulfillment seam** (after selling↔inventory delivery),
  from the opposite direction — the composition pattern is now demonstrated symmetrically.
- Residual / parking lot: a real event bus + procurement service to own the ACL in production; the
  A/P (Purchase Invoice) post (billing); subcontract physical-leg orchestration; landed cost.
