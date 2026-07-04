-- Down: drop buying.material_request_items table
DROP TABLE IF EXISTS buying.material_request_items CASCADE;
DROP FUNCTION IF EXISTS buying.material_request_items_audit_timestamp() CASCADE;
