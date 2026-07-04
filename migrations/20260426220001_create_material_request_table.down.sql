-- Down: drop buying.material_requests table
DROP TABLE IF EXISTS buying.material_requests CASCADE;
DROP FUNCTION IF EXISTS buying.material_requests_audit_timestamp() CASCADE;
