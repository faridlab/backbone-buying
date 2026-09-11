//! The purchase-order grouping domain (hand-authored, user-owned).
//!
//! An `impl BuyingWriteService` chunk over the vocabulary in [`super::buying_write_service`].
//! Holds NO SQL — the one statement lives on
//! [`PurchaseOrderRepository::find_open_po_for_demand`], per the module's 4-layer rule.
//!
//! One rule lives here, and it is absolute: **two demands with different `project_id`s never
//! coalesce into one purchase order.** Purchase cost collection is per project; a merged PO
//! would post its receipt and invoice against one project while half its lines belong to
//! another. The rule is enforced structurally, not by a check at merge time: the candidate
//! lookup's key is `(supplier_id, project_id)` with `project_id` matched exactly
//! (`IS NOT DISTINCT FROM` — NULL matches NULL only), so an order bought for project A is not
//! in the candidate set of a demand for project B at all. Any future grouping/merge engine
//! MUST resolve its candidate through [`BuyingWriteService::find_open_po_for_demand`] — that
//! is the point of having exactly one named shape for it.
//!
//! The module carries no tenant axis of its own (ADR-0029): the composing service's tenancy
//! decorator fences the candidate read, so a tenant's demands never see another tenant's
//! orders to group into.
//!
//! "Open" means the still-editable band of the lifecycle (`draft` / `sent`) — the same band the
//! module's own line-edit guards treat as editable-in-the-draft-sense. A parked
//! (`to_approve`) or confirmed (`purchase`) commitment never silently absorbs new lines.

use uuid::Uuid;

use super::buying_write_service::{BuyingError, BuyingWriteService};

/// A demand's grouping key: which supplier, and which project the demand buys
/// for. Build one with [`PoDemand::new`] + [`PoDemand::for_project`] /
/// [`PoDemand::without_project`].
///
/// This is the only key shape the grouping domain accepts. Additional grouping dimensions a
/// future engine might need (currency, branch, agreement) extend this key AND the repo finder
/// together — never a second, parallel lookup path.
#[derive(Debug, Clone)]
pub struct PoDemand {
    pub supplier_id: Uuid,
    /// The project partition of the demand. `None` = an unassigned demand; it can only ever
    /// group with orders that also carry no project (exact-match, NULL with NULL).
    pub project_id: Option<Uuid>,
}

impl PoDemand {
    /// Start a demand key for one supplier, unassigned to any project.
    pub fn new(supplier_id: Uuid) -> Self {
        Self { supplier_id, project_id: None }
    }

    /// Assign the demand to a project (logical FK project.Project.id). Once set, the demand can
    /// only group with orders bought for that same project.
    pub fn for_project(mut self, project_id: Uuid) -> Self {
        self.project_id = Some(project_id);
        self
    }

    /// Explicitly leave the demand unassigned (matches only project-less orders).
    pub fn without_project(mut self) -> Self {
        self.project_id = None;
        self
    }
}

/// The open purchase order a demand resolved to — the merge/group candidate. Carries the
/// project partition key it matched on so callers can see (and assert) the partition.
#[derive(Debug, Clone)]
pub struct PoMergeCandidate {
    pub id: Uuid,
    pub po_number: String,
    /// The band state the candidate was found in (`draft` or `sent`).
    pub status: String,
    /// The project the order is bought for — always equal to the demand's `project_id`
    /// (including both being `None`): the exact-match guarantee.
    pub project_id: Option<Uuid>,
}

impl BuyingWriteService {
    /// Resolve the open PO a demand may group into — the ONE named lookup of the grouping
    /// domain.
    ///
    /// `Ok(None)` = no open order for this (supplier, project) key; a grouping engine
    /// then creates a fresh PO rather than reusing any other project's. The read rides the
    /// request-dedicated connection when the composing service bound one (carrying the
    /// decorator's fence variables), plainly on the pool otherwise (ADR-0029).
    pub async fn find_open_po_for_demand(&self, demand: &PoDemand) -> Result<Option<PoMergeCandidate>, BuyingError> {
        let row = self.repos
            .purchase_orders
            .find_open_po_for_demand(
                &self.db_pool,
                demand.supplier_id,
                demand.project_id,
            )
            .await?;
        Ok(row.map(|r| PoMergeCandidate {
            id: r.id,
            po_number: r.po_number,
            status: r.status,
            project_id: r.project_id,
        }))
    }
}
