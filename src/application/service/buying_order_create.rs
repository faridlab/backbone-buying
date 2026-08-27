//! Purchase order creation: header + lines with server-owned totals (hand-authored, user-owned).
//!
//! An `impl BuyingWriteService` chunk over the vocabulary in [`super::buying_write_service`]. The
//! money is computed server-side (`price_document`: 2dp half-up line amounts, subtotal, tax,
//! total); header + lines are written in ONE transaction so a PO is never half-written. This is
//! also the seam the quotation sibling's `convert_supplier_quotation_to_po` and the agreement
//! sibling's `create_call_off_po` delegate into.
//!
//! The create gate resolves the currency-rate snapshot: a PO denominated in the company currency
//! fixes rate 1 regardless of what was supplied; any other currency REQUIRES a positive rate and
//! refuses loudly otherwise (`CurrencyRateRequired`) — a silent rate-1 default would mis-classify
//! every foreign-currency PO at the double-validation gate.
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on
//! `PurchaseOrderRepository` / `PurchaseOrderItemRepository`, and the tx-taking repo methods ride
//! this service's transaction.

use backbone_orm::company_scope;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::infrastructure::persistence::{NewPurchaseOrderItemRow, NewPurchaseOrderRow};

use super::buying_write_service::{
    is_dup, price_document, BuyingError, BuyingWriteService, NewPurchaseOrder,
};

/// Resolve the order-time currency-rate snapshot against the company's currency (the pinned
/// convention: COMPANY currency units per 1 PO-currency unit).
///
/// Same-currency POs fix 1 — whatever the caller sent. Foreign-currency POs must carry a positive
/// rate or the create refuses (`CurrencyRateRequired`); this is deliberately NOT defaulted, because
/// the double-validation gate converts the PO total into company currency with this same snapshot.
pub(super) fn resolve_rate(o: &NewPurchaseOrder, company_currency: &str) -> Result<Decimal, BuyingError> {
    let currency = o.currency.clone().unwrap_or_else(|| company_currency.to_string());
    if currency == company_currency {
        return Ok(Decimal::ONE);
    }
    match o.currency_rate {
        Some(r) if r > Decimal::ZERO => Ok(r),
        _ => Err(BuyingError::CurrencyRateRequired),
    }
}

/// Validate a line-method pair against the enum bands. `None` = the schema defaults
/// (`stock_moves` / `on_received`).
pub(super) fn line_method_pair(
    qty_received_method: &Option<String>,
    purchase_method: &Option<String>,
) -> Result<(&'static str, &'static str), BuyingError> {
    let rm = match qty_received_method.as_deref() {
        None | Some("stock_moves") => "stock_moves",
        Some("manual") => "manual",
        Some(other) => return Err(BuyingError::InvalidLineMethod(other.into())),
    };
    let pm = match purchase_method.as_deref() {
        None | Some("on_received") => "on_received",
        Some("purchase") => "purchase",
        Some(other) => return Err(BuyingError::InvalidLineMethod(other.into())),
    };
    Ok((rm, pm))
}

impl BuyingWriteService {
    // ---- Purchase Order (create) -------------------------------------------

    pub async fn create_purchase_order(&self, o: NewPurchaseOrder) -> Result<Uuid, BuyingError> {
        let (priced, subtotal, tax_amount, total) = price_document(&o.lines, o.tax_rate)?;
        let id = Uuid::new_v4();

        // The create gate: resolve the company currency (settings row, schema default IDR), then
        // the rate snapshot. Both reads ride the request-dedicated connection under the caller's
        // scope; the company for the rate decision is the PO's own company.
        company_scope::with_company_scope(Some(o.company_id), async {
            let settings = self.repos.purchase_company_settings.fetch_settings(&self.db_pool).await?;
            let company_currency = settings
                .map(|s| s.company_currency)
                .unwrap_or_else(|| "IDR".into());
            let rate = resolve_rate(&o, &company_currency)?;
            let currency = o.currency.clone().unwrap_or(company_currency);

            let kind = o.order_kind.clone().unwrap_or_else(|| "standard".into());
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
                currency_rate: rate,
                agreement_id: o.agreement_id,
                subtotal,
                tax_rate: o.tax_rate,
                tax_amount,
                total,
                notes: o.notes.as_deref(),
            }).await;
            if let Err(e) = r {
                return Err(if is_dup(&e) { BuyingError::DuplicateNumber(o.po_number.clone()) } else { e.into() });
            }
            for (p, l) in priced.iter().zip(o.lines.iter()) {
                let (rm, pm) = line_method_pair(&l.qty_received_method, &l.purchase_method)?;
                self.repos.purchase_order_items.insert_item(&mut tx, &NewPurchaseOrderItemRow {
                    id: Uuid::new_v4(), order_id: id, company_id: o.company_id, item_id: p.item_id, warehouse_id: p.warehouse_id,
                    description: p.description.as_deref(), quantity: p.quantity, rate: p.rate,
                    line_amount: p.line_amount, qty_received_method: rm, purchase_method: pm,
                }).await?;
            }
            tx.commit().await?;
            Ok(id)
        }).await
    }
}
