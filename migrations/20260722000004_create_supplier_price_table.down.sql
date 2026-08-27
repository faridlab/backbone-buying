-- Down: drop buying.supplier_prices table
DROP TABLE IF EXISTS buying.supplier_prices CASCADE;
DROP FUNCTION IF EXISTS buying.supplier_prices_audit_timestamp() CASCADE;
