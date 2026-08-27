-- Down: drop buying.supplier_reminder_settings table
DROP TABLE IF EXISTS buying.supplier_reminder_settings CASCADE;
DROP FUNCTION IF EXISTS buying.supplier_reminder_settings_audit_timestamp() CASCADE;
