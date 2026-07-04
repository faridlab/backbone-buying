-- Down: drop buying.request_for_quotations table
DROP TABLE IF EXISTS buying.request_for_quotations CASCADE;
DROP FUNCTION IF EXISTS buying.request_for_quotations_audit_timestamp() CASCADE;
