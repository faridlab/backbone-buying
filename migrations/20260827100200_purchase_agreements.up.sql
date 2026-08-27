-- Agreements + supplier prices: the RLS fence and the negotiated-data CHECKs.
-- The tables themselves (and their audit-timestamp triggers) are created by the
-- generated create-table migrations; this adds:
--   * the ADR-0008 company fence on the three tables (the agreement lines carry
--     their own direct company_id per ADR-0010, exactly like purchase_order_items),
--   * the agreement-line CHECKs: every line needs quantity > 0 and rate > 0 —
--     a zero-qty or zero-rate line is a data error, caught at the door (the
--     confirm verb validates it first; the CHECK is the belt-and-suspenders).

ALTER TABLE buying.purchase_agreements ENABLE ROW LEVEL SECURITY;
ALTER TABLE buying.purchase_agreements FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS purchase_agreements_company_isolation ON buying.purchase_agreements;
CREATE POLICY purchase_agreements_company_isolation ON buying.purchase_agreements
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

ALTER TABLE buying.purchase_agreement_lines ENABLE ROW LEVEL SECURITY;
ALTER TABLE buying.purchase_agreement_lines FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS purchase_agreement_lines_company_isolation ON buying.purchase_agreement_lines;
CREATE POLICY purchase_agreement_lines_company_isolation ON buying.purchase_agreement_lines
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

ALTER TABLE buying.supplier_prices ENABLE ROW LEVEL SECURITY;
ALTER TABLE buying.supplier_prices FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS supplier_prices_company_isolation ON buying.supplier_prices;
CREATE POLICY supplier_prices_company_isolation ON buying.supplier_prices
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- Negotiated-data CHECKs on the agreement lines.
ALTER TABLE buying.purchase_agreement_lines DROP CONSTRAINT IF EXISTS agreement_line_quantity_positive;
ALTER TABLE buying.purchase_agreement_lines
    ADD CONSTRAINT agreement_line_quantity_positive CHECK (quantity > 0);

ALTER TABLE buying.purchase_agreement_lines DROP CONSTRAINT IF EXISTS agreement_line_rate_positive;
ALTER TABLE buying.purchase_agreement_lines
    ADD CONSTRAINT agreement_line_rate_positive CHECK (rate > 0);

-- Supplier prices mirror the agreement line rate, so they are positive too.
ALTER TABLE buying.supplier_prices DROP CONSTRAINT IF EXISTS supplier_price_positive;
ALTER TABLE buying.supplier_prices
    ADD CONSTRAINT supplier_price_positive CHECK (price > 0);
