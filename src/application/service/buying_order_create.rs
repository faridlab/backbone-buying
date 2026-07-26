//! Purchase order creation: header + lines with server-owned totals (hand-authored, user-owned).
//!
//! An `impl BuyingWriteService` chunk over the vocabulary in [`super::buying_write_service`]. The
//! money is computed server-side (`price_document`: 2dp half-up line amounts, subtotal, tax,
//! total); header + lines are written in ONE transaction so a PO is never half-written. This is
//! also the seam the quotation sibling's `convert_supplier_quotation_to_po` delegates into.
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on
//! `PurchaseOrderRepository` / `PurchaseOrderItemRepository`, and the tx-taking repo methods ride
//! this service's transaction.

use backbone_orm::company_scope;
use uuid::Uuid;

use crate::infrastructure::persistence::{NewPurchaseOrderItemRow, NewPurchaseOrderRow};

use super::buying_write_service::{
    is_dup, price_document, BuyingError, BuyingWriteService, NewPurchaseOrder,
};

impl BuyingWriteService {
    // ---- Purchase Order (create) -------------------------------------------

    pub async fn create_purchase_order(&self, o: NewPurchaseOrder) -> Result<Uuid, BuyingError> {
        let (priced, subtotal, tax_amount, total) = price_document(&o.lines, o.tax_rate)?;
        let id = Uuid::new_v4();
        let currency = o.currency.unwrap_or_else(|| "IDR".into());
        let kind = o.order_kind.unwrap_or_else(|| "standard".into());
        // RLS scope (ADR-0008): company is on the DTO — bind it onto our own transaction.
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, o.company_id).await?;
        let r = self.repos.purchase_orders.insert_purchase_order(&mut tx, &NewPurchaseOrderRow {
            id,
            po_number: &o.po_number,
            supplier_quotation_id: o.supplier_quotation_id,
            order_kind: &kind,
            company_id: o.company_id,
            branch_id: o.branch_id,
            supplier_id: o.supplier_id,
            order_date: o.order_date,
            schedule_date: o.schedule_date,
            currency: &currency,
            subtotal,
            tax_rate: o.tax_rate,
            tax_amount,
            total,
            notes: o.notes.as_deref(),
        }).await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { BuyingError::DuplicateNumber(o.po_number) } else { e.into() });
        }
        for p in &priced {
            self.repos.purchase_order_items.insert_item(&mut tx, &NewPurchaseOrderItemRow {
                id: Uuid::new_v4(), order_id: id, company_id: o.company_id, item_id: p.item_id, warehouse_id: p.warehouse_id,
                description: p.description.as_deref(), quantity: p.quantity, rate: p.rate,
                line_amount: p.line_amount,
            }).await?;
        }
        tx.commit().await?;
        Ok(id)
    }
}
