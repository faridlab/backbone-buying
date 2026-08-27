-- Down: drop buying.purchase_agreements table
DROP TABLE IF EXISTS buying.purchase_agreements CASCADE;
DROP FUNCTION IF EXISTS buying.purchase_agreements_audit_timestamp() CASCADE;
