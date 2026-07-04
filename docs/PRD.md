# PRD — backbone-buying

> Tier 2 · Supply Chain · Indonesia-first ERP. Status: built. Date: 2026-07-05.

## Problem & intent
Businesses that buy need a controlled procure-to-pay pipeline — request → source → commit → receive
→ pay — without every app re-implementing sourcing, PO integrity, and 3-way match. `backbone-buying`
owns that pipeline as an independent module; it drives inventory (goods receipt) and billing
(purchase invoice) via events, mirroring selling's order-to-cash.

## Goals
- Own **Material Request → RFQ → Supplier Quotation → Purchase Order** (+ line children).
- Compute money **server-side**; guarded surface (no generic mutation).
- Drive **goods receipt** across the buying↔inventory seam (`ReceiptRequested` → inventory Purchase
  Receipt → `StockReceived` → `received_qty`), with zero normal Cargo edge.
- Track **3-way-match watermarks** (`received_qty`, `billed_qty`) and complete the PO only when both
  are satisfied.
- Fold **subcontracting** in as a PO subtype (`order_kind`), no separate module.

## Non-goals (this phase / deferred)
Goods-receipt asset post (inventory's), A/P post (billing's), PurchasableItem/Supplier' projections,
SupplierScorecard, RFQ comparison UI, subcontract physical-leg orchestration, PPh/PPN Input
(backbone-tax), landed cost/import duty, multi-currency.

## Personas
Buyer (raises MR/RFQ, places POs), Finance (relies on 3-way match before paying), Integrating
engineer (consumes PO events, wires the receipt seam).

## Success criteria
- Procure-to-pay math + watermark gating locked by a numeric oracle (6 cases).
- The receipt seam proven end-to-end against the real ledger (RSEAM-1) + survives regen of both
  modules (§5).
- Indonesia-ready: PPh/PPN Input hooks on the supplier projection (deferred), import-duty flag for
  landed cost.
