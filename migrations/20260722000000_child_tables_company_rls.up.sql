-- ADR-0010 Decision A: direct company_id + FORCE RLS on buying's child tables.
-- Hand-written (NOT regenerated). Mirrors the ADR-0008 fence already on the parent
-- tables (material_requests / purchase_orders / request_for_quotations / supplier_quotations),
-- but extended to the five child tables whose only route to the company used to be a JOIN.
--
-- For each child: ADD COLUMN company_id UUID nullable → backfill from the parent (verified
-- parent FK below) → SET NOT NULL → ENABLE + FORCE
-- RLS → CREATE POLICY USING/WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid).
--
-- No hard FK to organization.companies is added: like every other catalog/billing/selling child,
-- company_id is a LOGICAL FK only (modules own their own schema, cross-module hard FKs are avoided
-- so each module stays independently deployable). RLS is the fence, not the FK.
--
-- Parent FK map (each child has exactly one parent FK → no ambiguity):
--   material_request_items.request_id     → buying.material_requests.id
--   purchase_order_items.order_id         → buying.purchase_orders.id
--   rfq_items.rfq_id                      → buying.request_for_quotations.id
--   rfq_suppliers.rfq_id                  → buying.request_for_quotations.id
--   supplier_quotation_items.quotation_id → buying.supplier_quotations.id

-- ===========================================================================
-- 1. material_request_items
-- ===========================================================================
ALTER TABLE buying.material_request_items ADD COLUMN IF NOT EXISTS company_id UUID;

UPDATE buying.material_request_items AS c
   SET company_id = p.company_id
  FROM buying.material_requests AS p
 WHERE c.request_id = p.id
   AND c.company_id IS NULL;

ALTER TABLE buying.material_request_items ALTER COLUMN company_id SET NOT NULL;

ALTER TABLE buying.material_request_items ENABLE ROW LEVEL SECURITY;
ALTER TABLE buying.material_request_items FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS material_request_items_company_isolation ON buying.material_request_items;
CREATE POLICY material_request_items_company_isolation ON buying.material_request_items
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- ===========================================================================
-- 2. purchase_order_items
-- ===========================================================================
ALTER TABLE buying.purchase_order_items ADD COLUMN IF NOT EXISTS company_id UUID;

UPDATE buying.purchase_order_items AS c
   SET company_id = p.company_id
  FROM buying.purchase_orders AS p
 WHERE c.order_id = p.id
   AND c.company_id IS NULL;

ALTER TABLE buying.purchase_order_items ALTER COLUMN company_id SET NOT NULL;

ALTER TABLE buying.purchase_order_items ENABLE ROW LEVEL SECURITY;
ALTER TABLE buying.purchase_order_items FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS purchase_order_items_company_isolation ON buying.purchase_order_items;
CREATE POLICY purchase_order_items_company_isolation ON buying.purchase_order_items
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- ===========================================================================
-- 3. rfq_items
-- ===========================================================================
ALTER TABLE buying.rfq_items ADD COLUMN IF NOT EXISTS company_id UUID;

UPDATE buying.rfq_items AS c
   SET company_id = p.company_id
  FROM buying.request_for_quotations AS p
 WHERE c.rfq_id = p.id
   AND c.company_id IS NULL;

ALTER TABLE buying.rfq_items ALTER COLUMN company_id SET NOT NULL;

ALTER TABLE buying.rfq_items ENABLE ROW LEVEL SECURITY;
ALTER TABLE buying.rfq_items FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS rfq_items_company_isolation ON buying.rfq_items;
CREATE POLICY rfq_items_company_isolation ON buying.rfq_items
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- ===========================================================================
-- 4. rfq_suppliers
-- ===========================================================================
ALTER TABLE buying.rfq_suppliers ADD COLUMN IF NOT EXISTS company_id UUID;

UPDATE buying.rfq_suppliers AS c
   SET company_id = p.company_id
  FROM buying.request_for_quotations AS p
 WHERE c.rfq_id = p.id
   AND c.company_id IS NULL;

ALTER TABLE buying.rfq_suppliers ALTER COLUMN company_id SET NOT NULL;

ALTER TABLE buying.rfq_suppliers ENABLE ROW LEVEL SECURITY;
ALTER TABLE buying.rfq_suppliers FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS rfq_suppliers_company_isolation ON buying.rfq_suppliers;
CREATE POLICY rfq_suppliers_company_isolation ON buying.rfq_suppliers
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- ===========================================================================
-- 5. supplier_quotation_items
-- ===========================================================================
ALTER TABLE buying.supplier_quotation_items ADD COLUMN IF NOT EXISTS company_id UUID;

UPDATE buying.supplier_quotation_items AS c
   SET company_id = p.company_id
  FROM buying.supplier_quotations AS p
 WHERE c.quotation_id = p.id
   AND c.company_id IS NULL;

ALTER TABLE buying.supplier_quotation_items ALTER COLUMN company_id SET NOT NULL;

ALTER TABLE buying.supplier_quotation_items ENABLE ROW LEVEL SECURITY;
ALTER TABLE buying.supplier_quotation_items FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS supplier_quotation_items_company_isolation ON buying.supplier_quotation_items;
CREATE POLICY supplier_quotation_items_company_isolation ON buying.supplier_quotation_items
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
