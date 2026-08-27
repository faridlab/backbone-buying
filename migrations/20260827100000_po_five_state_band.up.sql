-- Converge the purchase-order lifecycle onto the 5-state band
-- (draft / sent / to_approve / purchase / cancelled), moving delivery/billing
-- maturity out of `status` onto the stored computes receipt_status /
-- invoice_status, and adding the order-time currency snapshot, the
-- double-validation gate columns, the lock/acknowledge flags, the per-line
-- receipt/billing policy columns, and the cancel/delete guard triggers.
--
-- Legacy status mapping (Postgres cannot drop enum values, so the type is
-- recreated and the column retyped through a CASE):
--   draft                -> draft
--   to_receive           -> purchase   (watermark maturity now on receipt_status)
--   to_bill              -> purchase   (watermark maturity now on invoice_status)
--   to_receive_and_bill  -> purchase
--   completed            -> purchase   (fully-received-and-billed is now receipt_status='full'
--                                       AND invoice_status='invoiced')
--   closed               -> cancelled  (least-wrong legacy remap: manually closed was terminal)
--   anything else        -> cancelled
--
-- Currency-rate convention (pinned): currency_rate is the ORDER-TIME snapshot
-- in units of COMPANY currency per 1 unit of PO currency at order_date. The
-- double-validation gate compares total * currency_rate (PO total converted
-- INTO company currency) against the company-currency threshold.

-- 1. New enum types (unqualified, per the module's enum convention).
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'purchase_receipt_status') THEN
        CREATE TYPE purchase_receipt_status AS ENUM ('pending', 'partial', 'full');
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'purchase_invoice_status') THEN
        CREATE TYPE purchase_invoice_status AS ENUM ('no', 'to_invoice', 'invoiced');
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'qty_received_method') THEN
        CREATE TYPE qty_received_method AS ENUM ('stock_moves', 'manual');
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'purchase_method') THEN
        CREATE TYPE purchase_method AS ENUM ('on_received', 'purchase');
    END IF;
END
$$;

-- 2. Header columns (defaults first, backfill next, so NOT NULL holds throughout).
ALTER TABLE buying.purchase_orders
    ADD COLUMN IF NOT EXISTS receipt_status purchase_receipt_status NOT NULL DEFAULT 'pending',
    ADD COLUMN IF NOT EXISTS invoice_status purchase_invoice_status NOT NULL DEFAULT 'no',
    ADD COLUMN IF NOT EXISTS currency_rate NUMERIC(18, 6) NOT NULL DEFAULT 1 CHECK (currency_rate > 0),
    ADD COLUMN IF NOT EXISTS acknowledged BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS locked BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS date_approve DATE,
    ADD COLUMN IF NOT EXISTS agreement_id UUID;

-- Backfill delivery maturity from the line received watermarks: full iff every
-- live line fully received; partial iff any live line has received anything.
UPDATE buying.purchase_orders po
SET receipt_status = sub.rs
FROM (
    SELECT i.order_id,
           CASE
               WHEN bool_and(i.received_qty >= i.quantity) THEN 'full'::purchase_receipt_status
               WHEN bool_or(i.received_qty > 0) THEN 'partial'::purchase_receipt_status
               ELSE 'pending'::purchase_receipt_status
           END AS rs
    FROM buying.purchase_order_items i
    WHERE (i.metadata->>'deleted_at') IS NULL
    GROUP BY i.order_id
) sub
WHERE po.id = sub.order_id;

-- Backfill billing maturity from the line billed watermarks. This MUST run after the item
-- policy columns exist (step 3 below adds purchase_method): the capacity formula
-- CASE WHEN purchase_method = 'purchase' THEN quantity ELSE received_qty END is the SAME
-- one the runtime maturity recompute and the allocation caps use. Decision order:
-- to_invoice when any live line still has invoiceable quantity (a received-but-never-billed
-- PO IS to invoice); else invoiced when any line billed anything; else no. Existing rows are
-- all on_received, so the formula evaluates on received_qty for them.
-- (Relocated below step 3 — see the second invoice-status UPDATE.)

-- Stamp date_approve for rows already in an operational (legacy-confirmed) state.
UPDATE buying.purchase_orders
SET date_approve = order_date
WHERE status::text IN ('to_receive', 'to_bill', 'to_receive_and_bill', 'completed');

-- 3. Item policy columns.
ALTER TABLE buying.purchase_order_items
    ADD COLUMN IF NOT EXISTS qty_received_method qty_received_method NOT NULL DEFAULT 'stock_moves',
    ADD COLUMN IF NOT EXISTS purchase_method purchase_method NOT NULL DEFAULT 'on_received';

-- Backfill billing maturity (runs here because it reads purchase_method): to_invoice iff any
-- live line still has invoiceable quantity under the capacity formula; else invoiced iff any
-- line billed anything; else no.
UPDATE buying.purchase_orders po
SET invoice_status = sub.ist
FROM (
    SELECT i.order_id,
           CASE
               WHEN bool_or((CASE WHEN i.purchase_method = 'purchase' THEN i.quantity ELSE i.received_qty END) - i.billed_qty <> 0)
                   THEN 'to_invoice'::purchase_invoice_status
               WHEN bool_or(i.billed_qty > 0)
                   THEN 'invoiced'::purchase_invoice_status
               ELSE 'no'::purchase_invoice_status
           END AS ist
    FROM buying.purchase_order_items i
    WHERE (i.metadata->>'deleted_at') IS NULL
    GROUP BY i.order_id
) sub
WHERE po.id = sub.order_id;

-- 4. Recreate purchase_order_status with the 5-state band. The column DEFAULT
-- must be dropped before the retyping (a default cannot auto-cast across the
-- type change) and re-applied after.
CREATE TYPE purchase_order_status_new AS ENUM ('draft', 'sent', 'to_approve', 'purchase', 'cancelled');

ALTER TABLE buying.purchase_orders ALTER COLUMN status DROP DEFAULT;

ALTER TABLE buying.purchase_orders
    ALTER COLUMN status TYPE purchase_order_status_new
    USING (CASE status::text
        WHEN 'draft' THEN 'draft'
        WHEN 'to_receive' THEN 'purchase'
        WHEN 'to_bill' THEN 'purchase'
        WHEN 'to_receive_and_bill' THEN 'purchase'
        WHEN 'completed' THEN 'purchase'
        WHEN 'closed' THEN 'cancelled'
        ELSE 'cancelled'
    END)::purchase_order_status_new;

DROP TYPE purchase_order_status;
ALTER TYPE purchase_order_status_new RENAME TO purchase_order_status;

ALTER TABLE buying.purchase_orders ALTER COLUMN status SET DEFAULT 'draft';

-- 5. Swap the 3-way-match CHECK to the two-formula bound: received caps at the
-- ordered quantity; billed caps at received_qty for on_received lines but at
-- quantity for purchase-method (order-driven service) lines, which may bill
-- ahead of receipt. All existing rows are on_received, so the swap is safe.
ALTER TABLE buying.purchase_order_items DROP CONSTRAINT IF EXISTS po_items_three_way_match;
ALTER TABLE buying.purchase_order_items
    ADD CONSTRAINT po_items_three_way_match
    CHECK (received_qty <= quantity
       AND billed_qty <= CASE WHEN purchase_method = 'purchase' THEN quantity ELSE received_qty END);

-- 6. Company consistency (header/line): the line's company can no longer diverge
-- from its header's. Branch stays header-only by port decision (lines carry no
-- branch dimension in this module).
ALTER TABLE buying.purchase_orders
    ADD CONSTRAINT po_id_company_unique UNIQUE (id, company_id);

ALTER TABLE buying.purchase_order_items DROP CONSTRAINT IF EXISTS fk_purchase_order_items_order_id;
ALTER TABLE buying.purchase_order_items
    ADD CONSTRAINT fk_purchase_order_items_order_company
    FOREIGN KEY (order_id, company_id) REFERENCES buying.purchase_orders (id, company_id);

-- 7. Guard triggers (DB backstops; the service pre-checks are the first line).
--    The guards fire ONLY on the narrow transitions they police — the cancel
--    transition and the live->soft-deleted transition — so watermark bumps,
--    maturity recomputes, and other status flips pass through untouched.

-- G4 + G5 + G8 on the header.
CREATE OR REPLACE FUNCTION buying.po_write_guards() RETURNS trigger AS $$
BEGIN
    -- Cancel transition: locked orders refuse (G4); any live billed line refuses (G5).
    IF NEW.status = 'cancelled' AND OLD.status <> 'cancelled' THEN
        IF NEW.locked THEN
            RAISE EXCEPTION 'po_cancel_locked: purchase order % is locked', OLD.id
                USING ERRCODE = 'check_violation';
        END IF;
        IF EXISTS (SELECT 1 FROM buying.purchase_order_items i
                   WHERE i.order_id = OLD.id
                     AND (i.metadata->>'deleted_at') IS NULL
                     AND i.billed_qty > 0) THEN
            RAISE EXCEPTION 'po_cancel_billed: purchase order % has billed lines', OLD.id
                USING ERRCODE = 'check_violation';
        END IF;
    END IF;

    -- Soft-delete transition: a live order may only be deleted once cancelled (G8).
    IF (OLD.metadata->>'deleted_at') IS NULL
       AND (NEW.metadata->>'deleted_at') IS NOT NULL
       AND NEW.status <> 'cancelled' THEN
        RAISE EXCEPTION 'po_delete_requires_cancelled: purchase order % is not cancelled', OLD.id
            USING ERRCODE = 'check_violation';
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS po_write_guards ON buying.purchase_orders;
CREATE TRIGGER po_write_guards BEFORE UPDATE ON buying.purchase_orders
    FOR EACH ROW EXECUTE FUNCTION buying.po_write_guards();

-- Hard delete (empty_trash path): only cancelled rows may leave the table.
CREATE OR REPLACE FUNCTION buying.po_hard_delete_guard() RETURNS trigger AS $$
BEGIN
    IF OLD.status <> 'cancelled' THEN
        RAISE EXCEPTION 'po_delete_requires_cancelled: purchase order % is not cancelled', OLD.id
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS po_hard_delete_guard ON buying.purchase_orders;
CREATE TRIGGER po_hard_delete_guard BEFORE DELETE ON buying.purchase_orders
    FOR EACH ROW EXECUTE FUNCTION buying.po_hard_delete_guard();

-- G9 on the lines: soft-delete or hard-delete of a line requires its parent
-- order to be in draft/sent. Fires only on the delete transitions — the
-- watermark/maturity UPDATEs the receipt seam makes pass through untouched.
CREATE OR REPLACE FUNCTION buying.po_item_write_guards() RETURNS trigger AS $$
DECLARE
    parent_status text;
BEGIN
    IF TG_OP = 'DELETE' THEN
        SELECT po.status::text INTO parent_status
        FROM buying.purchase_orders po WHERE po.id = OLD.order_id;
        IF parent_status IS NOT NULL AND parent_status NOT IN ('draft', 'sent') THEN
            RAISE EXCEPTION 'po_item_delete_requires_editable_order: line % belongs to order % in state %', OLD.id, OLD.order_id, parent_status
                USING ERRCODE = 'check_violation';
        END IF;
        RETURN OLD;
    ELSE
        IF (OLD.metadata->>'deleted_at') IS NULL AND (NEW.metadata->>'deleted_at') IS NOT NULL THEN
            SELECT po.status::text INTO parent_status
            FROM buying.purchase_orders po WHERE po.id = NEW.order_id;
            IF parent_status IS NOT NULL AND parent_status NOT IN ('draft', 'sent') THEN
                RAISE EXCEPTION 'po_item_delete_requires_editable_order: line % belongs to order % in state %', NEW.id, NEW.order_id, parent_status
                    USING ERRCODE = 'check_violation';
            END IF;
        END IF;
        RETURN NEW;
    END IF;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS po_item_write_guards ON buying.purchase_order_items;
CREATE TRIGGER po_item_write_guards BEFORE UPDATE OR DELETE ON buying.purchase_order_items
    FOR EACH ROW EXECUTE FUNCTION buying.po_item_write_guards();
