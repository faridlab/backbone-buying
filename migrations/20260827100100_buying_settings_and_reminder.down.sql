-- Reverse the settings-table RLS fence.

DROP POLICY IF EXISTS purchase_company_settings_company_isolation ON buying.purchase_company_settings;
ALTER TABLE buying.purchase_company_settings NO FORCE ROW LEVEL SECURITY;
ALTER TABLE buying.purchase_company_settings DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS supplier_reminder_settings_company_isolation ON buying.supplier_reminder_settings;
ALTER TABLE buying.supplier_reminder_settings NO FORCE ROW LEVEL SECURITY;
ALTER TABLE buying.supplier_reminder_settings DISABLE ROW LEVEL SECURITY;
