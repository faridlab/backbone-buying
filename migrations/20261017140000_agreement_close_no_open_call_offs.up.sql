-- An agreement is closed or cancelled only once no call-off order on it is still open (draft,
-- sent, to approve). The agreement verbs check it under a lock; this trigger holds it for any
-- other writer of the status, with the rule's code. It runs after the row lock is granted, so a
-- call-off that held the agreement while it committed is counted.
CREATE OR REPLACE FUNCTION buying.purchase_agreements_close_no_open_call_offs() RETURNS trigger AS $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM buying.purchase_orders po
         WHERE po.agreement_id = NEW.id
           AND po.status IN ('draft','sent','to_approve')
           AND (po.metadata->>'deleted_at') IS NULL
    ) THEN
        RAISE EXCEPTION 'agreement_has_draft_orders: agreement % still has open call-off orders', NEW.id
            USING ERRCODE = 'check_violation', CONSTRAINT = 'ck_purchase_agreements_agreement_close_no_draft_calloffs__agreement_has_draft_orders';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS purchase_agreements_close_no_open_call_offs ON buying.purchase_agreements;
CREATE TRIGGER purchase_agreements_close_no_open_call_offs
    BEFORE UPDATE OF status ON buying.purchase_agreements
    FOR EACH ROW
    WHEN (NEW.status::text IN ('done','cancelled') AND OLD.status IS DISTINCT FROM NEW.status)
    EXECUTE FUNCTION buying.purchase_agreements_close_no_open_call_offs();
