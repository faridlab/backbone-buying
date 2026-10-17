-- Supplier prices are written only by the agreement verbs: confirming an agreement mints them,
-- re-pricing an open line propagates to them, closing or cancelling unlinks them. Those writes mark
-- their transaction (`app.buying_agreement_write`); this trigger refuses any other write, from a
-- generic service, another module or raw SQL, with the rule's code. The composing service's
-- tenancy decorator owns org_unit_id: an update that changes nothing else (its backfill) passes.
CREATE OR REPLACE FUNCTION buying.supplier_prices_agreement_owned() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'UPDATE' AND (to_jsonb(NEW) - 'org_unit_id') = (to_jsonb(OLD) - 'org_unit_id') THEN
        RETURN NEW;
    END IF;
    IF current_setting('app.buying_agreement_write', true) IS DISTINCT FROM '1' THEN
        RAISE EXCEPTION 'supplier_price_agreement_owned: supplier prices change only through their agreement'
            USING ERRCODE = 'check_violation', CONSTRAINT = 'ck_supplier_prices_agreement_owned__supplier_price_agreement_owned';
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS supplier_prices_agreement_owned ON buying.supplier_prices;
CREATE TRIGGER supplier_prices_agreement_owned
    BEFORE INSERT OR UPDATE OR DELETE ON buying.supplier_prices
    FOR EACH ROW EXECUTE FUNCTION buying.supplier_prices_agreement_owned();
