# ADR-004: Reverse 3-way-match watermarks (purchase returns + credit notes)

**Status**: Accepted — Applied 2026-07-28
**Deciders**: Farid (owner), build session 2026-07-28
**Related**: [ADR-002](ADR-002-receipt-seam.md) (receipt seam), framework ADR-0008/0010 (company RLS), council [2026-07-28 bounded-context-cleanliness](../council/2026-07-28-module-backbone-buying-bounded-context-cleanliness.md)

## Context

Through v0.3.2 the 3-way-match watermarks on `purchase_order_items` — `received_qty` and `billed_qty` —
were **monotonic**: they only advanced via `add_to_watermark` (`SET col = col + $2`). The 2026-07-28
council's domain-expert seat flagged that real procure-to-pay has two first-class reverse operations the
model could not represent: a **purchase return** (goods sent back to the supplier; reduces `received_qty`)
and a **credit note** (supplier-issued invoice correction; reduces `billed_qty`). Without them a `completed`
PO could not be reopened for either, and downstream (inventory's asset, billing's A/P) had no buying-side
handler to reverse against.

## Decision

Add reverse-allocation, symmetric to `mark_received` / `mark_billed`, with the invariants preserved by the
existing DB CHECK (`billed_qty <= received_qty AND received_qty <= quantity`, plus non-negativity):

1. **`mark_returned` / `mark_credited`** (`buying_receipt.rs`) are the inbound handlers for inventory's
   `StockReturned` and billing's `PurchaseCreditPosted`, mirroring `mark_received`/`mark_billed`. A private
   `deallocate` mirrors `allocate` (fill-in-order, `FOR UPDATE` lock held across the read → decrement).
2. **A return is capped at the un-billed received portion** (`received_qty - billed_qty`), not at
   `received_qty`. Decrementing `received_qty` below `billed_qty` would violate the CHECK, so
   **already-billed goods must be credited first** — an over-return is rejected (`OverReturn`) and broadcast
   (`ThreeWayMatchFailed { kind: "over_return" }`). No surprise cascade / implicit money movement. A credit
   note decrements `billed_qty` up to `billed_qty` (always CHECK-safe).
3. **No new PO status states.** A return/credit moves a PO back into `to_receive` / `to_bill` /
   `to_receive_and_bill`. `update_status` is broadened to allow `completed → to_*` (safe no-op for the
   forward path, which recomputes `completed → completed`). Terminal `closed` / `cancelled` stay excluded.
4. **No migration.** `received_qty` / `billed_qty` are already decrementable; the CHECK is the backstop.
5. **Events** `PurchaseReturned` / `CreditNoted` (both `PurchaseOrderMilestone`-shaped) on success;
   **errors** `OverReturn` / `OverCredit`. The forward milestone logic (entering fully-received/billed) is
   unchanged — reversals emit their own events, they do not "un-emit".

## Consequences

- A `completed` PO can now be reopened by a return or credit note; `received_qty` / `billed_qty` are no
  longer monotonic.
- The 3-way-match invariants survive reversal: `billed ≤ received ≤ ordered`, non-negative, enforced both
  in the service (capacity caps) and the DB (CHECK).
- Returns of already-billed goods require an explicit credit-first sequencing (rejected otherwise). A
  cascade that auto-credits on return was deliberately deferred — keep as a future option if the explicit
  two-step proves too rigid.
- Composition-layer routing of `StockReturned` / `PurchaseCreditPosted` → `mark_returned` / `mark_credited`
  is out of scope here (the buying-side handlers exist; wiring is the composition service's job, like the
  receipt seam in ADR-002).
