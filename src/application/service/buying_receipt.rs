//! The receipt seam + the 3-way-match watermarks (hand-authored, user-owned).
//!
//! An `impl BuyingWriteService` chunk over the vocabulary in [`super::buying_write_service`]. Three
//! concerns, one file, because they share the private `allocate` / `recompute_order_status` helpers:
//!
//! - **Receipt request** — `build_receipt_request` emits the cross-module envelope an ACL maps into
//!   inventory's `ReceiptExpected` (buying emits NO `AccountingPost`; it drives inventory's asset
//!   post and billing's A/P post via events).
//! - **Watermark allocation** — `mark_received` / `mark_billed` are the inbound handlers for
//!   inventory's `StockReceived` and billing's `PurchaseInvoicePosted`. They allocate the qty across
//!   a PO's lines (fill-in-order), capped so `received_qty ≤ quantity` and `billed_qty ≤ received_qty`
//!   hold per line. Over-receipt / over-billing is rejected and broadcast as `ThreeWayMatchFailed`.
//! - **Status recompute** — after each allocation, the PO status is recomputed from its two
//!   watermarks; the transition into full receipt / full billing emits the milestone events.
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on
//! `PurchaseOrderRepository` / `PurchaseOrderItemRepository`. Both the locking read and the watermark
//! bumps take the caller's transaction, so the `FOR UPDATE` lock taken by the capacity read is still
//! held when the bumps run.

use backbone_orm::company_scope;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::infrastructure::persistence::MatchWatermark;

use super::buying_events::{
    BuyingEvent, PurchaseOrderMilestone, ReceiptRequestEnvelope, ReceiptRequestLine, ThreeWayMatchFailed,
};
use super::buying_write_service::{BuyingError, BuyingWriteService};

impl BuyingWriteService {
    // ---- Receipt seam (buying -> inventory) --------------------------------

    /// Build the cross-module receipt request for a confirmed PO (the envelope buying emits; an ACL
    /// maps it into inventory's `ReceiptExpected`). Requests the not-yet-received quantity per line.
    /// Emits `ReceiptRequested`.
    pub async fn build_receipt_request(&self, order_id: Uuid) -> Result<ReceiptRequestEnvelope, BuyingError> {
        // RLS scope (ADR-0008), ID-only pattern: read-only, reads ride the request-dedicated connection.
        let hdr = self.repos.purchase_orders.fetch_header(&self.db_pool, order_id).await?
            .ok_or(BuyingError::OrderNotFound(order_id))?;
        if hdr.status == "draft" {
            return Err(BuyingError::NotConfirmable(order_id.to_string()));
        }
        let rows = self.repos.purchase_order_items.fetch_remaining(&self.db_pool, order_id).await?;
        let lines: Vec<ReceiptRequestLine> = rows.iter().map(|r| ReceiptRequestLine {
            item_id: r.item_id, quantity: r.remaining, rate: r.rate,
        }).collect();
        let env = ReceiptRequestEnvelope {
            order_id, company_id: hdr.company_id, supplier_id: hdr.supplier_id,
            currency: hdr.currency, lines,
        };
        self.sink.publish(BuyingEvent::ReceiptRequested(env.clone()));
        Ok(env)
    }

    /// Record a receipt against a PO (inbound handler for inventory's `StockReceived`): allocate the
    /// received quantity across the item's PO lines, filling each up to `quantity` (the 3-way-match
    /// ceiling — no over-receipt tolerance configured, council 2026-07-05). Rejects over-receipt.
    pub async fn mark_received(
        &self,
        order_id: Uuid,
        company_id: Uuid,
        receipts: &[(Uuid, Decimal)],
    ) -> Result<(), BuyingError> {
        // RLS scope (ADR-0008): company on the parameter — scope the received-qty writes + status
        // recompute so they run with `app.company_id` set. The inbound handler for inventory's
        // `StockReceived` passes the event's company; an event/job caller can no longer forget to.
        company_scope::with_company_scope(Some(company_id), async move {
            let mut tx = self.db_pool.begin().await?;
            company_scope::bind_company_on(&mut tx, company_id).await?;
            for (item_id, qty) in receipts {
                // capacity per line = quantity - received_qty
                if let Err(e) = self.allocate(&mut tx, order_id, *item_id, *qty, MatchWatermark::Received,
                    BuyingError::OverReceipt { item_id: *item_id }).await {
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
            self.recompute_order_status(order_id).await?;
            Ok(())
        }).await
    }

    /// Record billing against a PO (inbound handler for billing's `PurchaseInvoicePosted`): allocate
    /// the billed quantity across the item's lines, capped at `received_qty` (invoice ≤ receipt —
    /// the 3-way-match invariant). Rejects over-billing.
    pub async fn mark_billed(
        &self,
        order_id: Uuid,
        company_id: Uuid,
        billed: &[(Uuid, Decimal)],
    ) -> Result<(), BuyingError> {
        // RLS scope (ADR-0008): company on the parameter — the allocation tx binds it explicitly
        // (`bind_company_on`), and the status recompute runs inside the scope. The inbound handler for
        // billing's `PurchaseInvoicePosted` passes the event's company; an event/job caller can no
        // longer forget to scope the `FOR UPDATE` reads inside `allocate`.
        company_scope::with_company_scope(Some(company_id), async move {
            let mut tx = self.db_pool.begin().await?;
            company_scope::bind_company_on(&mut tx, company_id).await?;
            for (item_id, qty) in billed {
                // capacity per line = received_qty - billed_qty
                if let Err(e) = self.allocate(&mut tx, order_id, *item_id, *qty, MatchWatermark::Billed,
                    BuyingError::OverBilling { item_id: *item_id }).await {
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
            self.recompute_order_status(order_id).await?;
            Ok(())
        }).await
    }

    /// Allocate `qty` of `item_id` across a PO's lines, advancing `watermark` up to each line's cap
    /// (fill-in-order). Rejects with `over_err` if the total remaining capacity is exceeded — so
    /// `received_qty <= quantity` and `billed_qty <= received_qty` hold per line (3-way match).
    /// Correct even when a PO has several lines of the same item.
    ///
    /// The DECISION lives here (the service owns the business rule); the lock/read/bump SQL lives in
    /// `PurchaseOrderItemRepository`. Both repo calls take the caller's `tx`, so the `FOR UPDATE` lock
    /// taken by the capacity read is still held when the bumps run.
    async fn allocate(
        &self, tx: &mut sqlx::PgConnection, order_id: Uuid, item_id: Uuid, mut qty: Decimal,
        watermark: MatchWatermark, over_err: BuyingError,
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
            qty -= take;
        }
        Ok(())
    }

    /// Recompute a PO's status from its two 3-way-match watermarks: `completed` iff every line is
    /// fully received AND fully billed; else `to_receive` / `to_bill` / `to_receive_and_bill`. Emits
    /// `PurchaseOrderFullyReceived` / `PurchaseOrderFullyBilled` on the transition into each milestone.
    async fn recompute_order_status(&self, order_id: Uuid) -> Result<(), BuyingError> {
        // RLS scope (ADR-0008), ID-only pattern: no company argument — this runs under whatever scope
        // its caller (`mark_received` / `mark_billed`) established, i.e. the request connection under
        // HTTP or the event caller's `with_company_scope`.
        let row = self.repos.purchase_orders.fetch_match_watermarks(&self.db_pool, order_id).await?;
        let company_id = row.company_id;
        let prior = row.prior;
        let received_all = row.received_all.unwrap_or(false);
        let billed_all = row.billed_all.unwrap_or(false);
        let next = match (received_all, billed_all) {
            (true, true) => "completed",
            (true, false) => "to_bill",
            (false, true) => "to_receive",
            (false, false) => "to_receive_and_bill",
        };
        // The PO's company was just read off the row above — scope the status flip on it explicitly.
        company_scope::with_company_scope(
            Some(company_id),
            self.repos.purchase_orders.update_status(&self.db_pool, order_id, next),
        ).await?;

        // Milestone events on the FIRST transition into full receipt / full billing.
        let was_received = matches!(prior.as_str(), "to_bill" | "completed");
        let was_billed = matches!(prior.as_str(), "to_receive" | "completed");
        if received_all && !was_received {
            self.sink.publish(BuyingEvent::PurchaseOrderFullyReceived(PurchaseOrderMilestone { order_id, company_id }));
        }
        if billed_all && !was_billed {
            self.sink.publish(BuyingEvent::PurchaseOrderFullyBilled(PurchaseOrderMilestone { order_id, company_id }));
        }
        Ok(())
    }
}
