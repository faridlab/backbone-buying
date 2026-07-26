//! Purchase order lifecycle: confirm + the exported cross-module ref (hand-authored, user-owned).
//!
//! An `impl BuyingWriteService` chunk over the vocabulary in [`super::buying_write_service`]. Confirm
//! advances a draft PO to `to_receive_and_bill` (the supply commitment) and emits
//! `PurchaseOrderConfirmed` so downstream (inventory's receipt expectation, billing's invoice
//! expectation) can react. `purchase_order_ref` loads the brief §42 cross-module DTO a composing
//! service reads through the integration surface.
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on
//! `PurchaseOrderRepository`. Both paths are ID-only (no company argument); under HTTP the
//! request-dedicated connection carries the caller's `app.company_id`, so another company's PO
//! simply isn't found.

use uuid::Uuid;

use super::buying_events::{BuyingEvent, PurchaseOrderConfirmed, PurchaseOrderRef};
use super::buying_write_service::{BuyingError, BuyingWriteService};

impl BuyingWriteService {
    /// Confirm a draft PO → `to_receive_and_bill` (awaiting both receipt and billing). Emits
    /// `PurchaseOrderConfirmed`.
    pub async fn confirm_purchase_order(&self, order_id: Uuid) -> Result<(), BuyingError> {
        // RLS scope (ADR-0008), ID-only pattern: the UPDATE ... RETURNING rides the request-dedicated
        // connection, so it can only confirm a PO in the caller's own company.
        let row = self.repos.purchase_orders.confirm(&self.db_pool, order_id).await?
            .ok_or_else(|| BuyingError::NotConfirmable(order_id.to_string()))?;
        self.sink.publish(BuyingEvent::PurchaseOrderConfirmed(PurchaseOrderConfirmed {
            order_id, company_id: row.company_id, supplier_id: row.supplier_id,
            grand_total: row.total, currency: row.currency,
        }));
        Ok(())
    }

    /// Load the exported `PurchaseOrderRef` (the brief §42 cross-module DTO) for one PO.
    pub async fn purchase_order_ref(&self, order_id: Uuid) -> Result<PurchaseOrderRef, BuyingError> {
        // RLS scope (ADR-0008), ID-only pattern: read rides the request-dedicated connection.
        let row = self.repos.purchase_orders.fetch_ref(&self.db_pool, order_id).await?
            .ok_or(BuyingError::OrderNotFound(order_id))?;
        Ok(PurchaseOrderRef {
            id: order_id,
            supplier_id: row.supplier_id,
            company_id: row.company_id,
            order_kind: row.order_kind,
            grand_total: row.total,
            currency: row.currency,
        })
    }
}
