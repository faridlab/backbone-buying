-- Down: drop buying.supplier_quotations table
DROP TABLE IF EXISTS buying.supplier_quotations CASCADE;
DROP FUNCTION IF EXISTS buying.supplier_quotations_audit_timestamp() CASCADE;
