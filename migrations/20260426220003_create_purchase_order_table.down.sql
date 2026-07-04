-- Down: drop buying.purchase_orders table
DROP TABLE IF EXISTS buying.purchase_orders CASCADE;
DROP FUNCTION IF EXISTS buying.purchase_orders_audit_timestamp() CASCADE;
