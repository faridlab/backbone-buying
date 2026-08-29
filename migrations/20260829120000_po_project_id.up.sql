-- Project anchoring on purchase orders (the procurement side of project cost
-- collection): a nullable LOGICAL FK to project.Project.id. No DB constraint —
-- buying and project are separate module schemas by design (the ecosystem's
-- cross-module convention), so the reference stays application-level.
--
-- The column also carries the never-merge-across-projects rule for the PO
-- grouping domain: any find-or-create/merge candidate lookup MUST key on
-- (company_id, supplier_id, project_id) with project_id matched exactly
-- (NULL matches NULL only). The backing index covers exactly that lookup.

ALTER TABLE buying.purchase_orders
    ADD COLUMN IF NOT EXISTS project_id UUID;

CREATE INDEX IF NOT EXISTS idx_purchase_orders_company_supplier_project
    ON buying.purchase_orders (company_id, supplier_id, project_id);
