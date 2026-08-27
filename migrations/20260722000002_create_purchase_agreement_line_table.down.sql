-- Down: drop buying.purchase_agreement_lines table
DROP TABLE IF EXISTS buying.purchase_agreement_lines CASCADE;
DROP FUNCTION IF EXISTS buying.purchase_agreement_lines_audit_timestamp() CASCADE;
