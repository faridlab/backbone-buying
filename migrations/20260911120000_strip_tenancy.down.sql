-- Hand-authored (user-owned). Not regenerated.
--
-- Best-effort restore sketch for the tenancy strip (ADR-0029). This is a breaking module
-- release against dev-stage databases: the down re-adds the company_id column as nullable
-- with its plain indexes/uniques and the company isolation policy shape, but restores NO
-- data — rows written after the strip (or after the decorator re-keyed them) carry
-- org_unit_id only. The composing service's tenancy decorator remains the live fence;
-- treat this down as a schema-shape sketch for archaeology, not a usable rollback.

ALTER TABLE buying.material_requests         ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE buying.material_request_items    ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE buying.request_for_quotations    ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE buying.rfq_items                 ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE buying.rfq_suppliers             ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE buying.supplier_quotations       ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE buying.supplier_quotation_items  ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE buying.purchase_orders           ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE buying.purchase_order_items      ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE buying.purchase_company_settings ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE buying.supplier_reminder_settings ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE buying.purchase_agreements       ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE buying.purchase_agreement_lines  ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE buying.supplier_prices           ADD COLUMN IF NOT EXISTS company_id uuid;

CREATE INDEX IF NOT EXISTS idx_material_requests_company_id_status
    ON buying.material_requests (company_id, status);
CREATE INDEX IF NOT EXISTS idx_request_for_quotations_company_id_status
    ON buying.request_for_quotations (company_id, status);
CREATE INDEX IF NOT EXISTS idx_supplier_quotations_company_id_supplier_id_status
    ON buying.supplier_quotations (company_id, supplier_id, status);
CREATE INDEX IF NOT EXISTS idx_purchase_orders_company_id_supplier_id_status
    ON buying.purchase_orders (company_id, supplier_id, status);
CREATE INDEX IF NOT EXISTS idx_purchase_orders_company_supplier_project
    ON buying.purchase_orders (company_id, supplier_id, project_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_purchase_company_settings_company_id
    ON buying.purchase_company_settings (company_id) WHERE (metadata->>'deleted_at') IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_supplier_reminder_settings_company_id_supplier_id
    ON buying.supplier_reminder_settings (company_id, supplier_id) WHERE (metadata->>'deleted_at') IS NULL;
CREATE INDEX IF NOT EXISTS idx_purchase_agreements_company_id_supplier_id_status
    ON buying.purchase_agreements (company_id, supplier_id, status);
CREATE UNIQUE INDEX IF NOT EXISTS idx_supplier_prices_company_id_supplier_id_item_id_agreement_line_id
    ON buying.supplier_prices (company_id, supplier_id, item_id, agreement_line_id) WHERE (metadata->>'deleted_at') IS NULL;

-- The (id, company_id) pair unique and the items table's composite FK that leaned on it
-- (po five-state-band chain). Only valid while both columns exist and every pair is distinct
-- — another reason the down is a sketch, not a rollback.
ALTER TABLE buying.purchase_orders
    ADD CONSTRAINT po_id_company_unique UNIQUE (id, company_id);
ALTER TABLE buying.purchase_order_items
    ADD CONSTRAINT fk_purchase_order_items_order_company
    FOREIGN KEY (order_id, company_id) REFERENCES buying.purchase_orders (id, company_id);

CREATE POLICY material_requests_company_isolation ON buying.material_requests
    FOR ALL USING (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
CREATE POLICY material_request_items_company_isolation ON buying.material_request_items
    FOR ALL USING (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
CREATE POLICY request_for_quotations_company_isolation ON buying.request_for_quotations
    FOR ALL USING (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
CREATE POLICY rfq_items_company_isolation ON buying.rfq_items
    FOR ALL USING (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
CREATE POLICY rfq_suppliers_company_isolation ON buying.rfq_suppliers
    FOR ALL USING (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
CREATE POLICY supplier_quotations_company_isolation ON buying.supplier_quotations
    FOR ALL USING (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
CREATE POLICY supplier_quotation_items_company_isolation ON buying.supplier_quotation_items
    FOR ALL USING (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
CREATE POLICY purchase_orders_company_isolation ON buying.purchase_orders
    FOR ALL USING (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
CREATE POLICY purchase_order_items_company_isolation ON buying.purchase_order_items
    FOR ALL USING (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
CREATE POLICY purchase_company_settings_company_isolation ON buying.purchase_company_settings
    FOR ALL USING (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
CREATE POLICY supplier_reminder_settings_company_isolation ON buying.supplier_reminder_settings
    FOR ALL USING (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
CREATE POLICY purchase_agreements_company_isolation ON buying.purchase_agreements
    FOR ALL USING (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
CREATE POLICY purchase_agreement_lines_company_isolation ON buying.purchase_agreement_lines
    FOR ALL USING (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
CREATE POLICY supplier_prices_company_isolation ON buying.supplier_prices
    FOR ALL USING (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
