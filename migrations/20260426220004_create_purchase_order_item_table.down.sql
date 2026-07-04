-- Down: drop buying.purchase_order_items table
DROP TABLE IF EXISTS buying.purchase_order_items CASCADE;
DROP FUNCTION IF EXISTS buying.purchase_order_items_audit_timestamp() CASCADE;
