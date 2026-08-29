-- Down: remove the project anchoring from purchase orders.
-- (The index drop is schema-qualified: psql runs migrations under the default
-- search_path, where an unqualified DROP INDEX would not find an index that
-- lives in the buying schema.)

DROP INDEX IF EXISTS buying.idx_purchase_orders_company_supplier_project;

ALTER TABLE buying.purchase_orders
    DROP COLUMN IF EXISTS project_id;
