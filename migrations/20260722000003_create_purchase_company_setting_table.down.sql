-- Down: drop buying.purchase_company_settings table
DROP TABLE IF EXISTS buying.purchase_company_settings CASCADE;
DROP FUNCTION IF EXISTS buying.purchase_company_settings_audit_timestamp() CASCADE;
