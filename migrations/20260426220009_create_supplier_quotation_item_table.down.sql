-- Down: drop buying.supplier_quotation_items table
DROP TABLE IF EXISTS buying.supplier_quotation_items CASCADE;
DROP FUNCTION IF EXISTS buying.supplier_quotation_items_audit_timestamp() CASCADE;
