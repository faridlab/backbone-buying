# Buying — Golden Cases (the numeric oracle)

Mirrors `tests/buying_golden_cases.rs`, `tests/integrity_probes.rs`, and the cross-module receipt
seam in `tests/receipt_seam.rs`. Money is exact IDR (2dp, half-up).

## Write path (`tests/buying_golden_cases.rs`)

| Case | Input | Expected |
|------|-------|----------|
| **BGC-1** | PO: 10 × 100,000, PPN Input 11% | subtotal `1,000,000`, tax `110,000`, total `1,110,000`. |
| **BGC-2** | confirm → receipt request | order → `to_receive_and_bill`; request asks 10 @ 100,000. |
| **BGC-3** | received 10 → billed 10 | `to_bill` after receipt; `completed` after billing (both watermarks). |
| **BGC-4** | partial receipt (4 of 10) | stays `to_receive_and_bill`; next request asks only 6 (the remainder). |
| **BGC-5** | material request + supplier quotation; empty/dup PO | creates persist; `empty_document` / `duplicate_number`. |
| **BGC-6** | PO `order_kind=subcontract` | persists as the subcontract subtype (supply-chain §6.1). |

## Guarded route surface (`tests/integrity_probes.rs`)

| Case | Input | Expected |
|------|-------|----------|
| **BIP-1** | `POST /purchase-orders/bulk` (generic) | `405/404` — generic bulk not exposed. |
| **BIP-2** | `DELETE /purchase-orders/{id}` (generic) | `405/404` — generic delete not exposed. |
| **BIP-3** | `POST /purchase-orders` well-formed | `201`. |
| **BIP-4** | `POST /purchase-orders` with `lines:[]` | `422 empty_document`. |

## Conversion funnel + event surface (`tests/funnel_and_events.rs`) — completeness council 2026-07-05

| Case | Input | Expected |
|------|-------|----------|
| **BFC-1** | MR (2 lines) → `convert_material_request_to_rfq` → `convert_rfq_to_supplier_quotation` (rates) → `convert_supplier_quotation_to_po` (11%) | each hop copies lines/rates + advances the source (`ordered`); PO links the SQ, copies supplier + lines, total `1,332,000`; `MaterialRequestRaised`/`RfqIssued`/`SupplierQuotationReceived` broadcast; `PurchaseOrderRef` carries the §42 shape. |
| **BFC-2** | convert a non-existent/non-submitted quotation | `404 source_not_found` / `422 source_not_convertible`. |
| **BFC-3** | over-receipt on a confirmed PO | rejected **and** `ThreeWayMatchFailed{kind=over_receipt}` broadcast (§33 mismatch flagged). |
| **BFC-4** | receive full → bill full | `PurchaseOrderFullyReceived` then `PurchaseOrderFullyBilled` fire once each. |

## Receipt seam — buying ↔ inventory ↔ accounting (`tests/receipt_seam.rs` + `scripts/receipt_seam_roundtrip.sh`)

| Case | Input | Expected |
|------|-------|----------|
| **RSEAM-1** | buying confirms PO (10 @ 100,000); emits `ReceiptRequested` → inventory receives → `StockReceived` → `mark_received` → simulated billing | asset journal `Dr Inventory 1,000,000 · Cr GR/IR 1,000,000`; PO `to_bill` after receipt, `completed` after billing; Bin holds 10 @ 100,000; `received_qty`=10. Zero normal Cargo edge. |
| **§5 round-trip** | regen BOTH buying + inventory, re-run | all seam ACL/consumer files byte-identical; RSEAM-1 still green — survives regen of both modules. |

## Conventions
- Buying **emits no `AccountingPost`** — it drives inventory's receipt (asset post) + billing's
  invoice (A/P) via events.
- 3-way-match: a PO completes only when every line is fully **received AND billed**.
- Subcontracting is a PO subtype (`order_kind`), not a separate module.
