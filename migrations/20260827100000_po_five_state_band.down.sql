-- Reverse the 5-state band migration. The 7-value band is restored with a
-- best-effort reverse map (maturity detail was lost in the forward collapse):
--   draft     -> draft
--   sent      -> draft
--   to_approve-> draft
--   purchase  -> to_receive_and_bill
--   cancelled -> cancelled

-- Drop the guard triggers first so the column/type work below is unobstructed.
DROP TRIGGER IF EXISTS po_item_write_guards ON buying.purchase_order_items;
DROP FUNCTION IF EXISTS buying.po_item_write_guards();

DROP TRIGGER IF EXISTS po_hard_delete_guard ON buying.purchase_orders;
DROP FUNCTION IF EXISTS buying.po_hard_delete_guard();

DROP TRIGGER IF EXISTS po_write_guards ON buying.purchase_orders;
DROP FUNCTION IF EXISTS buying.po_write_guards();

-- Restore the original single-column FK before dropping the composite shape.
ALTER TABLE buying.purchase_order_items DROP CONSTRAINT IF EXISTS fk_purchase_order_items_order_company;
ALTER TABLE buying.purchase_orders DROP CONSTRAINT IF EXISTS po_id_company_unique;
ALTER TABLE buying.purchase_order_items
    ADD CONSTRAINT fk_purchase_order_items_order_id FOREIGN KEY (order_id) REFERENCES buying.purchase_orders (id);

-- Restore the single-formula 3-way-match CHECK (drops the purchase_method CASE).
ALTER TABLE buying.purchase_order_items DROP CONSTRAINT IF EXISTS po_items_three_way_match;
ALTER TABLE buying.purchase_order_items
    ADD CONSTRAINT po_items_three_way_match
    CHECK (billed_qty <= received_qty AND received_qty <= quantity);

-- Recreate the 7-value status band (default dropped for the retyping, re-set after).
CREATE TYPE purchase_order_status_old AS ENUM ('draft', 'to_receive', 'to_bill', 'to_receive_and_bill', 'completed', 'closed', 'cancelled');

ALTER TABLE buying.purchase_orders ALTER COLUMN status DROP DEFAULT;

ALTER TABLE buying.purchase_orders
    ALTER COLUMN status TYPE purchase_order_status_old
    USING (CASE status::text
        WHEN 'draft' THEN 'draft'
        WHEN 'sent' THEN 'draft'
        WHEN 'to_approve' THEN 'draft'
        WHEN 'purchase' THEN 'to_receive_and_bill'
        ELSE 'cancelled'
    END)::purchase_order_status_old;

DROP TYPE purchase_order_status;
ALTER TYPE purchase_order_status_old RENAME TO purchase_order_status;

ALTER TABLE buying.purchase_orders ALTER COLUMN status SET DEFAULT 'draft';

-- Drop the item policy columns.
ALTER TABLE buying.purchase_order_items
    DROP COLUMN IF EXISTS qty_received_method,
    DROP COLUMN IF EXISTS purchase_method;

-- Drop the band's header columns.
ALTER TABLE buying.purchase_orders
    DROP COLUMN IF EXISTS agreement_id,
    DROP COLUMN IF EXISTS date_approve,
    DROP COLUMN IF EXISTS locked,
    DROP COLUMN IF EXISTS acknowledged,
    DROP COLUMN IF EXISTS currency_rate,
    DROP COLUMN IF EXISTS invoice_status,
    DROP COLUMN IF EXISTS receipt_status;
