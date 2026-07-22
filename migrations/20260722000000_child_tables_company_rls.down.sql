-- Reverse ADR-0010 Decision A on the five child tables.
-- Order: drop POLICY → DISABLE RLS (no un-FORCE; FORCE has no separate form) → drop FK →
-- drop COLUMN (cascades through NOT NULL + any dependent objects).

-- 5. supplier_quotation_items
DROP POLICY IF EXISTS supplier_quotation_items_company_isolation ON buying.supplier_quotation_items;
ALTER TABLE buying.supplier_quotation_items DISABLE ROW LEVEL SECURITY;
ALTER TABLE buying.supplier_quotation_items NO FORCE ROW LEVEL SECURITY;
ALTER TABLE buying.supplier_quotation_items DROP CONSTRAINT IF EXISTS fk_supplier_quotation_items_company_id;
ALTER TABLE buying.supplier_quotation_items DROP COLUMN IF EXISTS company_id;

-- 4. rfq_suppliers
DROP POLICY IF EXISTS rfq_suppliers_company_isolation ON buying.rfq_suppliers;
ALTER TABLE buying.rfq_suppliers DISABLE ROW LEVEL SECURITY;
ALTER TABLE buying.rfq_suppliers NO FORCE ROW LEVEL SECURITY;
ALTER TABLE buying.rfq_suppliers DROP CONSTRAINT IF EXISTS fk_rfq_suppliers_company_id;
ALTER TABLE buying.rfq_suppliers DROP COLUMN IF EXISTS company_id;

-- 3. rfq_items
DROP POLICY IF EXISTS rfq_items_company_isolation ON buying.rfq_items;
ALTER TABLE buying.rfq_items DISABLE ROW LEVEL SECURITY;
ALTER TABLE buying.rfq_items NO FORCE ROW LEVEL SECURITY;
ALTER TABLE buying.rfq_items DROP CONSTRAINT IF EXISTS fk_rfq_items_company_id;
ALTER TABLE buying.rfq_items DROP COLUMN IF EXISTS company_id;

-- 2. purchase_order_items
DROP POLICY IF EXISTS purchase_order_items_company_isolation ON buying.purchase_order_items;
ALTER TABLE buying.purchase_order_items DISABLE ROW LEVEL SECURITY;
ALTER TABLE buying.purchase_order_items NO FORCE ROW LEVEL SECURITY;
ALTER TABLE buying.purchase_order_items DROP CONSTRAINT IF EXISTS fk_purchase_order_items_company_id;
ALTER TABLE buying.purchase_order_items DROP COLUMN IF EXISTS company_id;

-- 1. material_request_items
DROP POLICY IF EXISTS material_request_items_company_isolation ON buying.material_request_items;
ALTER TABLE buying.material_request_items DISABLE ROW LEVEL SECURITY;
ALTER TABLE buying.material_request_items NO FORCE ROW LEVEL SECURITY;
ALTER TABLE buying.material_request_items DROP CONSTRAINT IF EXISTS fk_material_request_items_company_id;
ALTER TABLE buying.material_request_items DROP COLUMN IF EXISTS company_id;
