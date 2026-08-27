-- Reverse the agreements/supplier-prices fence and CHECKs.

ALTER TABLE buying.supplier_prices DROP CONSTRAINT IF EXISTS supplier_price_positive;
ALTER TABLE buying.purchase_agreement_lines DROP CONSTRAINT IF EXISTS agreement_line_rate_positive;
ALTER TABLE buying.purchase_agreement_lines DROP CONSTRAINT IF EXISTS agreement_line_quantity_positive;

DROP POLICY IF EXISTS supplier_prices_company_isolation ON buying.supplier_prices;
ALTER TABLE buying.supplier_prices NO FORCE ROW LEVEL SECURITY;
ALTER TABLE buying.supplier_prices DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS purchase_agreement_lines_company_isolation ON buying.purchase_agreement_lines;
ALTER TABLE buying.purchase_agreement_lines NO FORCE ROW LEVEL SECURITY;
ALTER TABLE buying.purchase_agreement_lines DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS purchase_agreements_company_isolation ON buying.purchase_agreements;
ALTER TABLE buying.purchase_agreements NO FORCE ROW LEVEL SECURITY;
ALTER TABLE buying.purchase_agreements DISABLE ROW LEVEL SECURITY;
