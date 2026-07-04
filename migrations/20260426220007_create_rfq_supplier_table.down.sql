-- Down: drop buying.rfq_suppliers table
DROP TABLE IF EXISTS buying.rfq_suppliers CASCADE;
DROP FUNCTION IF EXISTS buying.rfq_suppliers_audit_timestamp() CASCADE;
