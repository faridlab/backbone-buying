//! Manual bill-line matching (hand-authored, user-owned).
//!
//! An `impl BuyingWriteService` chunk over the vocabulary in [`super::buying_write_service`]. The
//! composing billing document proposes pairings of ITS bill lines onto the PO's lines; buying
//! validates the proposed quantities against each line's remaining billing capacity (the SAME
//! per-line `purchase_method` CASE the DB CHECK, the allocation caps, and the maturity recompute
//! use) and applies them as billed watermarks in ONE transaction. Buying holds no bill rows — the
//! bill line crosses the boundary as an opaque reference it echoes back in the
//! `BillLinesMatched` event, which the composing ACL turns into the `purchase_line_id` write on
//! the bill side.
//!
//! G6: an empty proposal list refuses (`NoLinesSelected`) — a silent no-op match would let a
//! caller believe a bill was matched when nothing was.
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on
//! `PurchaseOrderRepository` / `PurchaseOrderItemRepository`.

use backbone_orm::company_scope;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::infrastructure::persistence::MatchWatermark;

use super::buying_events::{BillLineMatch, BillLinesMatched, BuyingEvent, ThreeWayMatchFailed};
use super::buying_write_service::{BuyingError, BuyingWriteService};

/// One proposed bill-line ↔ PO-line pairing. `po_item_id` names the exact
/// `buying.purchase_order_items` row (no fill-in-order spread: the operator chose the pairing).
#[derive(Debug, Clone)]
pub struct BillLineProposal {
    pub bill_line_ref: String,
    pub po_item_id: Uuid,
    pub quantity: Decimal,
}

impl BuyingWriteService {
    /// Apply a manual bill match: validate every proposal against its line's remaining billing
    /// capacity, bump the billed watermarks in one transaction, recompute the PO's billing
    /// maturity, and publish `BillLinesMatched` (the composing billing service's signal to stamp
    /// its own bill lines with `purchase_line_id`).
    ///
    /// Refuses: empty proposal list (G6), a PO not in the operational state, a proposal naming a
    /// line outside this PO, and any proposal exceeding its line's capacity (`OverBilling` —
    /// broadcast as `ThreeWayMatchFailed` too, exactly like the automatic handler).
    pub async fn match_bill_lines(
        &self,
        order_id: Uuid,
        company_id: Uuid,
        proposals: &[BillLineProposal],
    ) -> Result<(), BuyingError> {
        if proposals.is_empty() {
            return Err(BuyingError::NoLinesSelected);
        }
        for p in proposals {
            if p.quantity <= Decimal::ZERO {
                return Err(BuyingError::NegativeQuantity);
            }
        }

        company_scope::with_company_scope(Some(company_id), async move {
            let hdr = self.repos.purchase_orders.fetch_header(&self.db_pool, order_id).await?
                .ok_or(BuyingError::OrderNotFound(order_id))?;
            if hdr.status != "purchase" {
                return Err(BuyingError::NotConfirmable(order_id.to_string()));
            }

            let mut tx = self.db_pool.begin().await?;
            company_scope::bind_company_on(&mut tx, company_id).await?;
            for p in proposals {
                let line = self.repos.purchase_order_items
                    .lock_line_for_matching(&mut tx, order_id, p.po_item_id).await?
                    .ok_or(BuyingError::OrderNotFound(p.po_item_id))?;
                if p.quantity > line.capacity {
                    drop(tx); // roll back — no partial match
                    self.sink.publish(BuyingEvent::ThreeWayMatchFailed(ThreeWayMatchFailed {
                        order_id, item_id: line.item_id, kind: "over_billing".into(),
                    }));
                    return Err(BuyingError::OverBilling { item_id: line.item_id });
                }
                self.repos.purchase_order_items
                    .add_to_watermark(&mut tx, p.po_item_id, MatchWatermark::Billed, p.quantity).await?;
            }
            tx.commit().await?;

            self.recompute_order_maturity(order_id).await?;
            self.sink.publish(BuyingEvent::BillLinesMatched(BillLinesMatched {
                order_id,
                company_id,
                matches: proposals.iter().map(|p| BillLineMatch {
                    bill_line_ref: p.bill_line_ref.clone(),
                    po_item_id: p.po_item_id,
                    quantity: p.quantity,
                }).collect(),
            }));
            Ok(())
        }).await
    }
}
