-- Down: drop enum types for buying module
DROP TYPE IF EXISTS purchase_doc_status CASCADE;
DROP TYPE IF EXISTS order_kind CASCADE;
DROP TYPE IF EXISTS purchase_order_status CASCADE;
DROP TYPE IF EXISTS material_request_type CASCADE;
