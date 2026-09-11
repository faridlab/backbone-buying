-- Hand-authored (user-owned). Not regenerated.
--
-- Strip every company-fence artifact from the buying tables (ADR-0029): the module is
-- tenant-agnostic; org scoping is installed by the COMPOSING service's tenancy decorator,
-- never by the module. Dropped here, per table: the company-leading indexes/uniques, the
-- <table>_company_isolation RLS policy, and the company_id column itself — plus the
-- (id, company_id) pair unique on purchase_orders and the composite FK that leaned on it.
--
-- Ordering guard (the decorator must run FIRST on any database with data): the module
-- never moves tenancy data. A table is safe to strip when EITHER
--   a) it carries org_unit_id with no NULLs — the decorator backfilled it from company_id —
--      or b) it is empty (a fresh database: the earlier chain files created it empty).
-- Otherwise the strip RAISEs, naming the decorator step, rather than dropping a column
-- that still holds the only tenancy key. The file is re-runnable (every drop is IF EXISTS
-- and the tracker has no checksums), so a failed run retries cleanly after the decorator
-- lands.
--
-- RLS enable/force flags are deliberately NOT touched: the decorator owns those now.
--
-- Uniqueness note: purchase_company_settings carried one row per company (a partial
-- UNIQUE index leading on company_id) and supplier_prices carried one live price row per
-- company+supplier+item+agreement-line. Those guarantees are re-declared org-scoped
-- (org_unit_id-leading) by the composing service's tenancy decorator; module-side the
-- write verbs remain the only writers, so the shapes stay enforced at composition time.

DO $$
DECLARE
    t text;
    has_org boolean;
    org_nulls bigint;
    total bigint;
    offenders text := '';
BEGIN
    FOREACH t IN ARRAY ARRAY[
        'material_requests', 'material_request_items',
        'request_for_quotations', 'rfq_items', 'rfq_suppliers',
        'supplier_quotations', 'supplier_quotation_items',
        'purchase_orders', 'purchase_order_items',
        'purchase_company_settings', 'supplier_reminder_settings',
        'purchase_agreements', 'purchase_agreement_lines', 'supplier_prices'
    ]
    LOOP
        IF to_regclass(format('buying.%I', t)) IS NULL THEN
            CONTINUE; -- chain not fully applied on this database; nothing to strip
        END IF;

        SELECT EXISTS (
                   SELECT 1 FROM information_schema.columns
                   WHERE table_schema = 'buying' AND table_name = t AND column_name = 'org_unit_id'
               )
        INTO has_org;

        EXECUTE format('SELECT count(*) FROM buying.%I', t) INTO total;

        IF has_org THEN
            EXECUTE format(
                'SELECT count(*) FROM buying.%I WHERE org_unit_id IS NULL', t)
            INTO org_nulls;
        ELSE
            org_nulls := total; -- no org column: every row's only tenancy key is company_id
        END IF;

        IF has_org AND org_nulls = 0 THEN
            CONTINUE; -- decorator backfilled: safe
        END IF;
        IF total = 0 THEN
            CONTINUE; -- empty table (fresh database): safe
        END IF;
        offenders := offenders || format(' buying.%s (%s rows, %s rows not covered by org_unit_id);', t, total, org_nulls);
    END LOOP;

    IF offenders <> '' THEN
        RAISE EXCEPTION 'refusing to strip company_id — these tables are not yet covered by the tenancy decorator:%. Apply the composing service''s tenancy decorator (it backfills org_unit_id from company_id) and re-run; it is the only step that moves tenancy data.', offenders;
    END IF;
END $$;

-- ── material_requests ─────────────────────────────────────────────────────────
DROP INDEX IF EXISTS buying.idx_material_requests_company_id_status;
DROP POLICY IF EXISTS material_requests_company_isolation ON buying.material_requests;
ALTER TABLE buying.material_requests DROP COLUMN IF EXISTS company_id;

-- ── material_request_items ────────────────────────────────────────────────────
DROP POLICY IF EXISTS material_request_items_company_isolation ON buying.material_request_items;
ALTER TABLE buying.material_request_items DROP COLUMN IF EXISTS company_id;

-- ── request_for_quotations ────────────────────────────────────────────────────
DROP INDEX IF EXISTS buying.idx_request_for_quotations_company_id_status;
DROP POLICY IF EXISTS request_for_quotations_company_isolation ON buying.request_for_quotations;
ALTER TABLE buying.request_for_quotations DROP COLUMN IF EXISTS company_id;

-- ── rfq_items ─────────────────────────────────────────────────────────────────
DROP POLICY IF EXISTS rfq_items_company_isolation ON buying.rfq_items;
ALTER TABLE buying.rfq_items DROP COLUMN IF EXISTS company_id;

-- ── rfq_suppliers ─────────────────────────────────────────────────────────────
DROP POLICY IF EXISTS rfq_suppliers_company_isolation ON buying.rfq_suppliers;
ALTER TABLE buying.rfq_suppliers DROP COLUMN IF EXISTS company_id;

-- ── supplier_quotations ───────────────────────────────────────────────────────
DROP INDEX IF EXISTS buying.idx_supplier_quotations_company_id_supplier_id_status;
DROP POLICY IF EXISTS supplier_quotations_company_isolation ON buying.supplier_quotations;
ALTER TABLE buying.supplier_quotations DROP COLUMN IF EXISTS company_id;

-- ── supplier_quotation_items ──────────────────────────────────────────────────
DROP POLICY IF EXISTS supplier_quotation_items_company_isolation ON buying.supplier_quotation_items;
ALTER TABLE buying.supplier_quotation_items DROP COLUMN IF EXISTS company_id;

-- ── purchase_orders ───────────────────────────────────────────────────────────
DROP INDEX IF EXISTS buying.idx_purchase_orders_company_id_supplier_id_status;
DROP INDEX IF EXISTS buying.idx_purchase_orders_company_supplier_project;
DROP POLICY IF EXISTS purchase_orders_company_isolation ON buying.purchase_orders;
-- The (id, company_id) pair unique (po five-state-band chain) and the items table's
-- composite FK leaning on it go together — the FK first, or the unique drop is refused
-- (objects depend on it).
ALTER TABLE buying.purchase_order_items DROP CONSTRAINT IF EXISTS fk_purchase_order_items_order_company;
ALTER TABLE buying.purchase_orders DROP CONSTRAINT IF EXISTS po_id_company_unique;
ALTER TABLE buying.purchase_orders DROP COLUMN IF EXISTS company_id;

-- ── purchase_order_items ──────────────────────────────────────────────────────
DROP POLICY IF EXISTS purchase_order_items_company_isolation ON buying.purchase_order_items;
-- The composite FK (order_id, company_id) → purchase_orders (id, company_id) was dropped
-- with the pair unique above; the plain order_id link remains the parent reference.
ALTER TABLE buying.purchase_order_items DROP COLUMN IF EXISTS company_id;

-- ── purchase_company_settings ─────────────────────────────────────────────────
DROP INDEX IF EXISTS buying.idx_purchase_company_settings_company_id;
DROP POLICY IF EXISTS purchase_company_settings_company_isolation ON buying.purchase_company_settings;
ALTER TABLE buying.purchase_company_settings DROP COLUMN IF EXISTS company_id;

-- ── supplier_reminder_settings ────────────────────────────────────────────────
DROP INDEX IF EXISTS buying.idx_supplier_reminder_settings_company_id_supplier_id;
DROP POLICY IF EXISTS supplier_reminder_settings_company_isolation ON buying.supplier_reminder_settings;
ALTER TABLE buying.supplier_reminder_settings DROP COLUMN IF EXISTS company_id;

-- ── purchase_agreements ───────────────────────────────────────────────────────
DROP INDEX IF EXISTS buying.idx_purchase_agreements_company_id_supplier_id_status;
DROP POLICY IF EXISTS purchase_agreements_company_isolation ON buying.purchase_agreements;
ALTER TABLE buying.purchase_agreements DROP COLUMN IF EXISTS company_id;

-- ── purchase_agreement_lines ──────────────────────────────────────────────────
DROP POLICY IF EXISTS purchase_agreement_lines_company_isolation ON buying.purchase_agreement_lines;
ALTER TABLE buying.purchase_agreement_lines DROP COLUMN IF EXISTS company_id;

-- ── supplier_prices ───────────────────────────────────────────────────────────
DROP INDEX IF EXISTS buying.idx_supplier_prices_company_id_supplier_id_item_id_agreement_line_id;
DROP POLICY IF EXISTS supplier_prices_company_isolation ON buying.supplier_prices;
ALTER TABLE buying.supplier_prices DROP COLUMN IF EXISTS company_id;
