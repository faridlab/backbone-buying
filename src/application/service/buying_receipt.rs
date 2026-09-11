//! The receipt seam + the 3-way-match watermarks (hand-authored, user-owned).
//!
//! An `impl BuyingWriteService` chunk over the vocabulary in [`super::buying_write_service`]. Three
//! concerns, one file, because they share the private `allocate` / `deallocate` /
//! `recompute_order_maturity` helpers:
//!
//! - **Receipt request** — `build_receipt_request` emits the cross-module envelope an ACL maps into
//!   inventory's `ReceiptExpected` (buying emits NO `AccountingPost`; it drives inventory's asset
//!   post and billing's A/P post via events).
//! - **Watermark allocation** — `mark_received` / `mark_billed` are the inbound handlers for
//!   inventory's `StockReceived` and billing's `PurchaseInvoicePosted`. They allocate the qty across
//!   a PO's lines (fill-in-order), capped so `received_qty ≤ quantity` and `billed_qty ≤` the line's
//!   billing capacity (`received_qty` for on_received lines, `quantity` for order-driven
//!   `purchase`-method lines) hold per line. Over-receipt / over-billing is rejected and broadcast
//!   as `ThreeWayMatchFailed`. `mark_returned` / `mark_credited` mirror them in reverse.
//! - **Maturity recompute** — after each allocation, the PO's stored computes
//!   (`receipt_status` / `invoice_status`) are recomputed from its two watermarks; the transition
//!   into full receipt / invoiced emits the milestone events. EVERY receipt also publishes
//!   `PurchaseReceiptRecorded` with the per-line applied quantity, so per-receipt consumers don't
//!   wait for the last one.
//!
//! The two receipt tiers: `stock_moves` lines are advanced by the seam (inventory's receipts); a
//! `manual` line is advanced by an operator through `set_manual_line_receipt` (absolute set, same
//! caps, same event). The seam's allocation skips manual lines; the manual verb refuses
//! `stock_moves` lines.
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on
//! `PurchaseOrderRepository` / `PurchaseOrderItemRepository`. Both the locking read and the watermark
//! bumps take the caller's transaction, so the `FOR UPDATE` lock taken by the capacity read is still
//! held when the bumps run.

use rust_decimal::Decimal;
use uuid::Uuid;

use crate::infrastructure::persistence::{MatchWatermark, MaturityRow, ReverseWatermark};

use super::buying_events::{
    BuyingEvent, PurchaseOrderMilestone, PurchaseReceiptLine, PurchaseReceiptRecorded,
    ReceiptRequestEnvelope, ReceiptRequestLine, ThreeWayMatchFailed,
};
use super::buying_write_service::{
    legacy_company_echo, relay_ambient_scope, BuyingError, BuyingWriteService,
};

impl BuyingWriteService {
    // ---- Receipt seam (buying -> inventory) --------------------------------

    /// Build the cross-module receipt request for a PO in the operational state (`purchase` — the
    /// envelope buying emits; an ACL maps it into inventory's `ReceiptExpected`). Requests the
    /// not-yet-received quantity per `stock_moves` line. Emits `ReceiptRequested`.
    pub async fn build_receipt_request(&self, order_id: Uuid) -> Result<ReceiptRequestEnvelope, BuyingError> {
        // Id-only read — the composed decorator owns visibility.
        let hdr = self.repos.purchase_orders.fetch_header(&self.db_pool, order_id).await?
            .ok_or(BuyingError::OrderNotFound(order_id))?;
        if hdr.status != "purchase" {
            return Err(BuyingError::NotConfirmable(order_id.to_string()));
        }
        let rows = self.repos.purchase_order_items.fetch_remaining(&self.db_pool, order_id).await?;
        let lines: Vec<ReceiptRequestLine> = rows.iter().map(|r| ReceiptRequestLine {
            item_id: r.item_id, quantity: r.remaining, rate: r.rate,
        }).collect();
        let env = ReceiptRequestEnvelope {
            order_id, company_id: legacy_company_echo(), supplier_id: hdr.supplier_id,
            currency: hdr.currency, order_kind: hdr.order_kind, lines,
        };
        self.sink.publish(BuyingEvent::ReceiptRequested(env.clone()));
        Ok(env)
    }

    /// Record a receipt against a PO (inbound handler for inventory's `StockReceived`): allocate the
    /// received quantity across the item's `stock_moves` PO lines, filling each up to `quantity`
    /// (the 3-way-match ceiling — no over-receipt tolerance configured). Rejects over-receipt.
    /// Publishes `PurchaseReceiptRecorded` with the per-line applied quantity + rate, and stamps
    /// `acknowledged` (goods landed — no further reminder needed).
    pub async fn mark_received(
        &self,
        order_id: Uuid,
        receipts: &[(Uuid, Decimal)],
    ) -> Result<(), BuyingError> {
        let mut tx = self.db_pool.begin().await?;
        // Relay the caller's ambient org scope onto this transaction (ADR-0029): the composing
        // service's decorator set it task-locally; the fresh transaction carries none of it.
        relay_ambient_scope(&mut tx).await?;
        let mut applied: Vec<PurchaseReceiptLine> = Vec::new();
        for (item_id, qty) in receipts {
            // capacity per line = quantity - received_qty
            if let Err(e) = self.allocate(&mut tx, order_id, *item_id, *qty, MatchWatermark::Received,
                BuyingError::OverReceipt { item_id: *item_id }, &mut applied).await {
                drop(tx); // roll back — no partial receipt
                if matches!(e, BuyingError::OverReceipt { .. }) {
                    // §33: broadcast the variance so an async consumer sees it, not just the caller.
                    self.sink.publish(BuyingEvent::ThreeWayMatchFailed(ThreeWayMatchFailed {
                        order_id, item_id: *item_id, kind: "over_receipt".into(),
                    }));
                }
                return Err(e);
            }
        }
        tx.commit().await?;

        // Goods landed: the supplier needs no further chaser for this PO.
        let _ = self.repos.purchase_orders.set_acknowledged(&self.db_pool, order_id).await?;

        let maturity = self.recompute_order_maturity(order_id).await?;
        self.sink.publish(BuyingEvent::PurchaseReceiptRecorded(PurchaseReceiptRecorded {
            order_id, company_id: legacy_company_echo(),
            order_kind: maturity.order_kind,
            lines: applied,
        }));
        Ok(())
    }

    /// Record billing against a PO (inbound handler for billing's `PurchaseInvoicePosted`): allocate
    /// the billed quantity across the item's lines, capped at each line's billing capacity —
    /// `received_qty` for `on_received` lines (invoice ≤ receipt), `quantity` for order-driven
    /// `purchase`-method lines, which may bill ahead of receipt. Rejects over-billing.
    pub async fn mark_billed(
        &self,
        order_id: Uuid,
        billed: &[(Uuid, Decimal)],
    ) -> Result<(), BuyingError> {
        let mut tx = self.db_pool.begin().await?;
        // Relay the caller's ambient org scope onto this transaction (ADR-0029).
        relay_ambient_scope(&mut tx).await?;
        for (item_id, qty) in billed {
            // capacity per line = billing capacity - billed_qty (the purchase_method CASE)
            if let Err(e) = self.allocate(&mut tx, order_id, *item_id, *qty, MatchWatermark::Billed,
                BuyingError::OverBilling { item_id: *item_id }, &mut Vec::new()).await {
                drop(tx); // roll back — no partial billing
                if matches!(e, BuyingError::OverBilling { .. }) {
                    self.sink.publish(BuyingEvent::ThreeWayMatchFailed(ThreeWayMatchFailed {
                        order_id, item_id: *item_id, kind: "over_billing".into(),
                    }));
                }
                return Err(e);
            }
        }
        tx.commit().await?;
        self.recompute_order_maturity(order_id).await?;
        Ok(())
    }

    /// Record a purchase return against a PO (inbound handler for inventory's `StockReturned`): reverse-
    /// allocate the returned qty across the item's PO lines, decrementing `received_qty`, capped at the
    /// un-billed received portion. Rejects over-return — already-billed goods must be credited
    /// ([`Self::mark_credited`]) before they can be returned (the `po_items_three_way_match` CHECK is
    /// the backstop). Broadcasts `ThreeWayMatchFailed` on over-return; `PurchaseReturned` on success.
    /// Mirrors [`Self::mark_received`] in the reverse direction.
    pub async fn mark_returned(
        &self,
        order_id: Uuid,
        returns: &[(Uuid, Decimal)],
    ) -> Result<(), BuyingError> {
        let mut tx = self.db_pool.begin().await?;
        // Relay the caller's ambient org scope onto this transaction (ADR-0029).
        relay_ambient_scope(&mut tx).await?;
        for (item_id, qty) in returns {
            // reverse capacity per line = received_qty - billed_qty (un-billed received goods),
            // clamped at 0 (a purchase-method line may have billed ahead of receipt)
            if let Err(e) = self.deallocate(&mut tx, order_id, *item_id, *qty, ReverseWatermark::Returned,
                BuyingError::OverReturn { item_id: *item_id }).await {
                drop(tx); // roll back — no partial return
                if matches!(e, BuyingError::OverReturn { .. }) {
                    // §33: broadcast the variance so an async consumer sees it, not just the caller.
                    self.sink.publish(BuyingEvent::ThreeWayMatchFailed(ThreeWayMatchFailed {
                        order_id, item_id: *item_id, kind: "over_return".into(),
                    }));
                }
                return Err(e);
            }
        }
        tx.commit().await?;
        self.recompute_order_maturity(order_id).await?;
        self.sink.publish(BuyingEvent::PurchaseReturned(PurchaseOrderMilestone { order_id, company_id: legacy_company_echo() }));
        Ok(())
    }

    /// Record a credit note against a PO (inbound handler for billing's `PurchaseCreditPosted`): reverse-
    /// allocate the credited qty across the item's PO lines, decrementing `billed_qty`, capped at
    /// `billed_qty` (always CHECK-safe — reducing `billed_qty` preserves the billing-capacity bound).
    /// Rejects over-credit. Broadcasts `ThreeWayMatchFailed` on over-credit; `CreditNoted` on success.
    /// Mirrors [`Self::mark_billed`] in the reverse direction.
    pub async fn mark_credited(
        &self,
        order_id: Uuid,
        credits: &[(Uuid, Decimal)],
    ) -> Result<(), BuyingError> {
        let mut tx = self.db_pool.begin().await?;
        // Relay the caller's ambient org scope onto this transaction (ADR-0029).
        relay_ambient_scope(&mut tx).await?;
        for (item_id, qty) in credits {
            // reverse capacity per line = billed_qty
            if let Err(e) = self.deallocate(&mut tx, order_id, *item_id, *qty, ReverseWatermark::Credited,
                BuyingError::OverCredit { item_id: *item_id }).await {
                drop(tx); // roll back — no partial credit
                if matches!(e, BuyingError::OverCredit { .. }) {
                    self.sink.publish(BuyingEvent::ThreeWayMatchFailed(ThreeWayMatchFailed {
                        order_id, item_id: *item_id, kind: "over_credit".into(),
                    }));
                }
                return Err(e);
            }
        }
        tx.commit().await?;
        self.recompute_order_maturity(order_id).await?;
        self.sink.publish(BuyingEvent::CreditNoted(PurchaseOrderMilestone { order_id, company_id: legacy_company_echo() }));
        Ok(())
    }

    /// Set a `manual`-method line's received quantity directly (the operator tier of the two-tier
    /// `qty_received_method`): absolute set, not an increment, capped at the ordered `quantity` by
    /// the DB CHECK. Refuses (`InvalidLineMethod`) a `stock_moves` line — those advance through the
    /// receipt seam only. Publishes `PurchaseReceiptRecorded` (single line) and recomputes maturity,
    /// exactly like a seam receipt.
    pub async fn set_manual_line_receipt(
        &self,
        order_id: Uuid,
        line_id: Uuid,
        qty: Decimal,
    ) -> Result<(), BuyingError> {
        let row = self.repos.purchase_order_items
            .set_manual_line_receipt(&self.db_pool, order_id, line_id, qty).await?
            .ok_or_else(|| BuyingError::InvalidLineMethod(line_id.to_string()))?;

        let maturity = self.recompute_order_maturity(order_id).await?;
        self.sink.publish(BuyingEvent::PurchaseReceiptRecorded(PurchaseReceiptRecorded {
            order_id, company_id: legacy_company_echo(),
            order_kind: maturity.order_kind,
            lines: vec![PurchaseReceiptLine { item_id: row.item_id, quantity: qty, rate: row.rate }],
        }));
        Ok(())
    }

    /// Allocate `qty` of `item_id` across a PO's lines, advancing `watermark` up to each line's cap
    /// (fill-in-order), recording what each line took into `applied`. Rejects with `over_err` if the
    /// total remaining capacity is exceeded — so `received_qty <= quantity` and the per-line billing
    /// capacity bound hold (3-way match). Correct even when a PO has several lines of the same item.
    ///
    /// The DECISION lives here (the service owns the business rule); the lock/read/bump SQL lives in
    /// `PurchaseOrderItemRepository`. Both repo calls take the caller's `tx`, so the `FOR UPDATE` lock
    /// taken by the capacity read is still held when the bumps run.
    async fn allocate(
        &self, tx: &mut sqlx::PgConnection, order_id: Uuid, item_id: Uuid, mut qty: Decimal,
        watermark: MatchWatermark, over_err: BuyingError,
        applied: &mut Vec<PurchaseReceiptLine>,
    ) -> Result<(), BuyingError> {
        let lines = self.repos.purchase_order_items
            .lock_lines_for_allocation(&mut *tx, order_id, item_id, watermark).await?;
        let total_cap: Decimal = lines.iter().map(|r| r.capacity).sum();
        if qty > total_cap {
            return Err(over_err);
        }
        for line in &lines {
            if qty <= Decimal::ZERO { break; }
            let cap = line.capacity;
            if cap <= Decimal::ZERO { continue; }
            let take = if qty < cap { qty } else { cap };
            self.repos.purchase_order_items
                .add_to_watermark(&mut *tx, line.id, watermark, take).await?;
            if matches!(watermark, MatchWatermark::Received) {
                applied.push(PurchaseReceiptLine { item_id: line.item_id, quantity: take, rate: line.rate });
            }
            qty -= take;
        }
        Ok(())
    }

    /// The reverse of [`Self::allocate`]: remove `qty` of `item_id` across a PO's lines, decrementing
    /// `rw`'s column down to each line's reverse capacity (fill-in-order). Rejects with `over_err` if the
    /// total reverse capacity is exceeded — so `received_qty <= quantity`, the billing-capacity bound,
    /// and non-negativity all hold per line (the DB CHECK is the backstop). Correct even when a PO has
    /// several lines of the same item.
    ///
    /// As with `allocate`, the DECISION lives here (the service owns the business rule); the lock/read/
    /// bump SQL lives in `PurchaseOrderItemRepository`. Both repo calls take the caller's `tx`, so the
    /// `FOR UPDATE` lock taken by the capacity read is still held when the decrements run.
    async fn deallocate(
        &self, tx: &mut sqlx::PgConnection, order_id: Uuid, item_id: Uuid, mut qty: Decimal,
        rw: ReverseWatermark, over_err: BuyingError,
    ) -> Result<(), BuyingError> {
        let lines = self.repos.purchase_order_items
            .lock_lines_for_reverse(&mut *tx, order_id, item_id, rw).await?;
        let total_cap: Decimal = lines.iter().map(|r| r.capacity).sum();
        if qty > total_cap {
            return Err(over_err);
        }
        for line in &lines {
            if qty <= Decimal::ZERO { break; }
            let cap = line.capacity;
            if cap <= Decimal::ZERO { continue; }
            let take = if qty < cap { qty } else { cap };
            self.repos.purchase_order_items
                .subtract_from_watermark(&mut *tx, line.id, rw, take).await?;
            qty -= take;
        }
        Ok(())
    }

    /// Recompute a PO's stored maturity computes from its 3-way-match watermarks:
    /// `receipt_status` = full / partial / pending by delivery; `invoice_status` = invoiced /
    /// to_invoice / no by billing. The `any_to_invoice` aggregate carries the SAME per-line
    /// `purchase_method` CASE as the billing cap and the DB CHECK. Emits
    /// `PurchaseOrderFullyReceived` / `PurchaseOrderFullyBilled` on the FIRST transition into each
    /// milestone. Guarded to the operational state in the repository — a draft or cancelled PO is
    /// never dragged forward by a stray recompute.
    ///
    /// `pub(super)`: the bill-matching sibling shares it (it bumps billed watermarks too).
    pub(super) async fn recompute_order_maturity(&self, order_id: Uuid) -> Result<MaturityRow, BuyingError> {
        // Id-only read + write — this runs under whatever ambient scope its caller relayed onto
        // their transaction; the composed decorator owns isolation.
        let row = self.repos.purchase_orders.fetch_maturity(&self.db_pool, order_id).await?;
        let received_all = row.received_all.unwrap_or(false);
        let any_received = row.any_received.unwrap_or(false);
        let any_to_invoice = row.any_to_invoice.unwrap_or(false);
        let any_billed = row.any_billed.unwrap_or(false);

        let receipt_status = if received_all { "full" } else if any_received { "partial" } else { "pending" };
        // Decision order per the maturity contract: any invoiceable quantity remaining -> to_invoice
        // (a received-but-never-billed PO IS to invoice); else any billing history -> invoiced; else no.
        let invoice_status = if any_to_invoice { "to_invoice" } else if any_billed { "invoiced" } else { "no" };

        self.repos.purchase_orders
            .update_maturity(&self.db_pool, order_id, receipt_status, invoice_status).await?;

        // Milestone events on the FIRST transition into full receipt / invoiced.
        let was_full = row.prior_receipt == "full";
        let was_invoiced = row.prior_invoice == "invoiced";
        if received_all && !was_full {
            self.sink.publish(BuyingEvent::PurchaseOrderFullyReceived(PurchaseOrderMilestone { order_id, company_id: legacy_company_echo() }));
        }
        if invoice_status == "invoiced" && !was_invoiced {
            self.sink.publish(BuyingEvent::PurchaseOrderFullyBilled(PurchaseOrderMilestone { order_id, company_id: legacy_company_echo() }));
        }
        Ok(row)
    }
}
