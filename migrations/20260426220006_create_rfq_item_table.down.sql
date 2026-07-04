-- Down: drop buying.rfq_items table
DROP TABLE IF EXISTS buying.rfq_items CASCADE;
DROP FUNCTION IF EXISTS buying.rfq_items_audit_timestamp() CASCADE;
