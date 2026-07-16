-- Down: remove the company RLS fence for buying module

-- Reverse the company RLS fence for buying.material_requests
DROP POLICY IF EXISTS material_requests_company_isolation ON buying.material_requests;
ALTER TABLE buying.material_requests NO FORCE ROW LEVEL SECURITY;
ALTER TABLE buying.material_requests DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for buying.purchase_orders
DROP POLICY IF EXISTS purchase_orders_company_isolation ON buying.purchase_orders;
ALTER TABLE buying.purchase_orders NO FORCE ROW LEVEL SECURITY;
ALTER TABLE buying.purchase_orders DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for buying.request_for_quotations
DROP POLICY IF EXISTS request_for_quotations_company_isolation ON buying.request_for_quotations;
ALTER TABLE buying.request_for_quotations NO FORCE ROW LEVEL SECURITY;
ALTER TABLE buying.request_for_quotations DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for buying.supplier_quotations
DROP POLICY IF EXISTS supplier_quotations_company_isolation ON buying.supplier_quotations;
ALTER TABLE buying.supplier_quotations NO FORCE ROW LEVEL SECURITY;
ALTER TABLE buying.supplier_quotations DISABLE ROW LEVEL SECURITY;

