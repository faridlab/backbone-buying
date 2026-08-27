-- Company RLS fence for the buying settings tables (ADR-0008), mirroring
-- 20260426220010: ENABLE + FORCE ROW LEVEL SECURITY + company_isolation policy
-- scoped on app.company_id. The tables themselves (and their audit-timestamp
-- triggers) are created by the generated create-table migrations; this adds
-- only the fence. An unset app.company_id sees zero rows.

ALTER TABLE buying.purchase_company_settings ENABLE ROW LEVEL SECURITY;
ALTER TABLE buying.purchase_company_settings FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS purchase_company_settings_company_isolation ON buying.purchase_company_settings;
CREATE POLICY purchase_company_settings_company_isolation ON buying.purchase_company_settings
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

ALTER TABLE buying.supplier_reminder_settings ENABLE ROW LEVEL SECURITY;
ALTER TABLE buying.supplier_reminder_settings FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS supplier_reminder_settings_company_isolation ON buying.supplier_reminder_settings;
CREATE POLICY supplier_reminder_settings_company_isolation ON buying.supplier_reminder_settings
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
