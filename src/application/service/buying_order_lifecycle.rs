//! Purchase order lifecycle verbs + the exported cross-module ref (hand-authored, user-owned).
//!
//! An `impl BuyingWriteService` chunk over the vocabulary in [`super::buying_write_service`]. The
//! 5-state band (draft / sent / to_approve / purchase / cancelled) is driven by these verbs:
//!
//! - **confirm** — `draft`/`sent` → `purchase` (the supply commitment) or, when the double-validation
//!   gate refuses, parked in `to_approve` for a manager. `PurchaseOrderConfirmed` is published from
//!   BOTH entry paths exactly once (gate-passing confirm, manager approve).
//! - **approve / reset / cancel / send / lock / unlock / acknowledge / delete** — the band's other
//!   verbs; each is state-guarded in the repository and pre-checked here for the typed errors
//!   (G4 locked-cancel, G5 billed-cancel, G8 delete-requires-cancelled, G9 line-delete-requires-
//!   editable-order; the DB triggers are the backstop).
//!
//! `purchase_order_ref` loads the brief §42 cross-module DTO a composing service reads through the
//! integration surface.
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on
//! `PurchaseOrderRepository` / `PurchaseOrderItemRepository`. ID-only paths (no company argument);
//! under HTTP the request-dedicated connection carries the caller's `app.company_id`, so another
//! company's PO simply isn't found.

use uuid::Uuid;

use super::buying_events::{BuyingEvent, PurchaseOrderConfirmed, PurchaseOrderPendingApproval, PurchaseOrderRef};
use super::buying_write_service::{BuyingError, BuyingWriteService};

impl BuyingWriteService {
    /// Confirm a `draft`/`sent` PO. With the double-validation gate configured (`two_step`) and no
    /// manager claim, an over-threshold PO is parked in `to_approve` (`PurchaseOrderPendingApproval`)
    /// instead — `approve_purchase_order` finishes it. A gate-passing confirm enters `purchase` and
    /// publishes `PurchaseOrderConfirmed`.
    ///
    /// The gate threshold is denominated in the COMPANY currency; the comparison converts the PO
    /// total INTO company currency with the order-time `currency_rate` snapshot
    /// (`total * currency_rate >= threshold` needs a manager). Never the reverse division, never a
    /// silent rate of 1 — the snapshot is `NOT NULL CHECK (> 0)` at the DB and resolved at create.
    pub async fn confirm_purchase_order(&self, order_id: Uuid, is_manager: bool) -> Result<(), BuyingError> {
        // RLS scope (ADR-0008), ID-only pattern: the reads/UPDATEs ride the request-dedicated
        // connection, so they can only act on a PO in the caller's own company.
        let gate = self.repos.purchase_orders.fetch_gate_row(&self.db_pool, order_id).await?
            .ok_or(BuyingError::OrderNotFound(order_id))?;
        if gate.status != "draft" && gate.status != "sent" {
            return Err(BuyingError::NotConfirmable(order_id.to_string()));
        }

        // The company's gate configuration. An absent settings row = the schema defaults
        // (one_step): every confirm passes the gate.
        let settings = self.repos.purchase_company_settings.fetch_settings(&self.db_pool).await?;
        let two_step = settings.as_ref().map(|s| s.double_validation == "two_step").unwrap_or(false);
        let threshold = settings.as_ref().map(|s| s.double_validation_amount);

        let needs_manager = two_step
            && !is_manager
            && gate.total * gate.currency_rate >= threshold.unwrap_or(rust_decimal::Decimal::ZERO);

        if needs_manager {
            let company_id = self.repos.purchase_orders.park_for_approval(&self.db_pool, order_id).await?
                .ok_or_else(|| BuyingError::NotConfirmable(order_id.to_string()))?;
            self.sink.publish(BuyingEvent::PurchaseOrderPendingApproval(PurchaseOrderPendingApproval {
                order_id, company_id,
            }));
            return Ok(());
        }

        self.enter_purchase_and_publish(order_id, &["draft", "sent"]).await
    }

    /// Approve a `to_approve` PO (the manager leg of the double-validation gate): enters `purchase`
    /// and publishes the SAME `PurchaseOrderConfirmed` a gate-passing confirm publishes — one event,
    /// two entry paths, exactly once each.
    ///
    /// The gate is RE-CHECKED here (the amount may have changed while the order was parked, and a
    /// non-manager must not be able to walk an over-threshold PO through the approve verb): a
    /// non-manager claim on an over-threshold PO refuses loudly with `NotApprovable`. The threshold
    /// comparison is the same company-currency conversion as confirm — `total * currency_rate`
    /// against the company-currency threshold, never a raw amount compare.
    pub async fn approve_purchase_order(&self, order_id: Uuid, is_manager: bool) -> Result<(), BuyingError> {
        let gate = self.repos.purchase_orders.fetch_gate_row(&self.db_pool, order_id).await?
            .ok_or(BuyingError::OrderNotFound(order_id))?;
        if gate.status != "to_approve" {
            return Err(BuyingError::NotApprovable(order_id.to_string()));
        }

        let settings = self.repos.purchase_company_settings.fetch_settings(&self.db_pool).await?;
        let two_step = settings.as_ref().map(|s| s.double_validation == "two_step").unwrap_or(false);
        let threshold = settings.as_ref().map(|s| s.double_validation_amount);

        if two_step
            && !is_manager
            && gate.total * gate.currency_rate >= threshold.unwrap_or(rust_decimal::Decimal::ZERO)
        {
            return Err(BuyingError::NotApprovable(order_id.to_string()));
        }

        self.enter_purchase_and_publish(order_id, &["to_approve"]).await
    }

    /// The shared tail of both entry paths into `purchase`: flip the state (stamping
    /// `date_approve`), then publish `PurchaseOrderConfirmed` with the `order_kind` consumers
    /// (subcontract progress, MTO) key on.
    async fn enter_purchase_and_publish(&self, order_id: Uuid, from: &[&str]) -> Result<(), BuyingError> {
        let row = self.repos.purchase_orders.enter_purchase(&self.db_pool, order_id, from).await?
            .ok_or_else(|| BuyingError::NotConfirmable(order_id.to_string()))?;
        self.sink.publish(BuyingEvent::PurchaseOrderConfirmed(PurchaseOrderConfirmed {
            order_id, company_id: row.company_id, supplier_id: row.supplier_id,
            grand_total: row.total, currency: row.currency, order_kind: row.order_kind,
        }));
        Ok(())
    }

    /// Reset a non-draft PO back to `draft` (rework). `date_approve` is kept: it records that an
    /// approval once happened, not the current state.
    pub async fn reset_purchase_order(&self, order_id: Uuid) -> Result<(), BuyingError> {
        self.repos.purchase_orders.reset_to_draft(&self.db_pool, order_id).await?
            .ok_or_else(|| BuyingError::NotCancelable(order_id.to_string()))?;
        Ok(())
    }

    /// Cancel a live PO. Typed refusals (the service pre-checks; the G4/G5 triggers are the DB
    /// backstop): a locked order refuses (`OrderLocked`), any live billed line refuses
    /// (`OrderBilled`), and a state with no cancel edge refuses (`NotCancelable`).
    pub async fn cancel_purchase_order(&self, order_id: Uuid) -> Result<(), BuyingError> {
        let gate = self.repos.purchase_orders.fetch_gate_row(&self.db_pool, order_id).await?
            .ok_or(BuyingError::OrderNotFound(order_id))?;
        if gate.locked {
            return Err(BuyingError::OrderLocked(order_id));
        }
        if gate.has_live_billed_lines {
            return Err(BuyingError::OrderBilled(order_id));
        }
        if !matches!(gate.status.as_str(), "draft" | "sent" | "to_approve" | "purchase") {
            return Err(BuyingError::NotCancelable(order_id.to_string()));
        }
        self.repos.purchase_orders.cancel(&self.db_pool, order_id).await?
            .ok_or_else(|| BuyingError::NotCancelable(order_id.to_string()))?;
        Ok(())
    }

    /// `draft` → `sent`: the PO was printed/sent to the supplier, so it is no longer editable.
    pub async fn send_purchase_order(&self, order_id: Uuid) -> Result<(), BuyingError> {
        self.repos.purchase_orders.mark_sent(&self.db_pool, order_id).await?
            .ok_or_else(|| BuyingError::NotConfirmable(order_id.to_string()))?;
        Ok(())
    }

    /// Lock a PO (freeze it against cancel: G4). Orthogonal to the lifecycle band.
    pub async fn lock_purchase_order(&self, order_id: Uuid) -> Result<(), BuyingError> {
        self.repos.purchase_orders.set_locked(&self.db_pool, order_id, true).await?
            .ok_or(BuyingError::OrderNotFound(order_id))?;
        Ok(())
    }

    /// Unlock a PO (releases the G4 cancel guard).
    pub async fn unlock_purchase_order(&self, order_id: Uuid) -> Result<(), BuyingError> {
        self.repos.purchase_orders.set_locked(&self.db_pool, order_id, false).await?
            .ok_or(BuyingError::OrderNotFound(order_id))?;
        Ok(())
    }

    /// Acknowledge a `purchase` PO (an operator confirmed the supplier's delivery promise — this is
    /// what suppresses the receipt reminder while the PO waits).
    pub async fn acknowledge_purchase_order(&self, order_id: Uuid) -> Result<(), BuyingError> {
        self.repos.purchase_orders.set_acknowledged(&self.db_pool, order_id).await?
            .ok_or_else(|| BuyingError::NotAcknowledgable(order_id.to_string()))?;
        Ok(())
    }

    /// Soft-delete a CANCELLED PO (G8 — a live order is never deletable; the trigger is the DB
    /// backstop, the state guard in the repository is the first line).
    pub async fn delete_purchase_order(&self, order_id: Uuid) -> Result<(), BuyingError> {
        self.repos.purchase_orders.soft_delete_cancelled(&self.db_pool, order_id).await?
            .ok_or_else(|| BuyingError::NotDeletable(order_id.to_string()))?;
        Ok(())
    }

    /// Soft-delete one PO line (G9: only while the parent order is editable — `draft`/`sent`; the
    /// trigger on the lines table is the DB backstop).
    pub async fn delete_purchase_order_line(&self, order_id: Uuid, line_id: Uuid) -> Result<(), BuyingError> {
        self.repos.purchase_order_items.soft_delete_line(&self.db_pool, order_id, line_id).await?
            .ok_or_else(|| BuyingError::NotDeletable(line_id.to_string()))?;
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
