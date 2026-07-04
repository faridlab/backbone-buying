-- Hand-authored constraint (council 2026-07-05, ADR-001): the 3-way-match invariant
-- 0 <= billed_qty <= received_qty <= quantity per PO line. The write path allocates within these
-- bounds and rejects over_receipt / over_billing; this CHECK is the belt-and-suspenders DB backstop
-- so no path (generic CRUD, direct SQL) can certify billing for goods never received.
ALTER TABLE buying.purchase_order_items
    ADD CONSTRAINT po_items_three_way_match
    CHECK (billed_qty <= received_qty AND received_qty <= quantity);
