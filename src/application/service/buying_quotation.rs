//! Supplier quotation step: create SQ, convert RFQ → SQ, convert SQ → PO (hand-authored, user-owned).
//!
//! An `impl BuyingWriteService` chunk over the vocabulary in [`super::buying_write_service`]. The
//! middle of the procurement funnel: suppliers' quoted rates come back as supplier quotations
//! (either entered directly or derived from an RFQ), and an accepted quotation is converted into a
//! draft Purchase Order — the hand-off to the order-create sibling.
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on
//! `SupplierQuotationRepository` / `SupplierQuotationItemRepository`, and the tx-taking repo methods
//! ride this service's transaction. `convert_supplier_quotation_to_po` delegates the actual PO
//! write to [`super::buying_order_create::BuyingWriteService::create_purchase_order`] and then
//! flips the SQ to `ordered` under that PO's company scope.

use backbone_orm::company_scope;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::infrastructure::persistence::{
    NewQuotationFromRfqRow, NewSupplierQuotationItemRow, NewSupplierQuotationRow,
};

use super::buying_events::{BuyingEvent, DocumentRaised};
use super::buying_write_service::{
    is_dup, price_document, BuyingError, BuyingWriteService, NewLine, NewPurchaseOrder,
    NewSupplierQuotation,
};

impl BuyingWriteService {
    // ---- Supplier Quotation -------------------------------------------------

    pub async fn create_supplier_quotation(&self, q: NewSupplierQuotation) -> Result<Uuid, BuyingError> {
        let (priced, _sub, _tax, _tot) = price_document(&q.lines, Decimal::ZERO)?;
        let id = Uuid::new_v4();
        let currency = q.currency.unwrap_or_else(|| "IDR".into());
        // RLS scope (ADR-0008): company is on the DTO — bind it onto our own transaction.
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, q.company_id).await?;
        let r = self.repos.supplier_quotations.insert_quotation(&mut tx, &NewSupplierQuotationRow {
            id,
            quotation_number: &q.quotation_number,
            rfq_id: q.rfq_id,
            company_id: q.company_id,
            supplier_id: q.supplier_id,
            quotation_date: q.quotation_date,
            valid_till: q.valid_till,
            currency: &currency,
        }).await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { BuyingError::DuplicateNumber(q.quotation_number) } else { e.into() });
        }
        for p in &priced {
            self.repos.supplier_quotation_items.insert_item(&mut tx, &NewSupplierQuotationItemRow {
                id: Uuid::new_v4(), quotation_id: id, company_id: q.company_id, item_id: p.item_id,
                quantity: p.quantity, rate: p.rate,
            }).await?;
        }
        tx.commit().await?;
        Ok(id)
    }

    /// Convert an RFQ into a supplier quotation for one supplier's quoted rates (copies the RFQ line
    /// quantities, applies the supplier's rates, links `rfq_id`). The RFQ→SupplierQuotation step.
    pub async fn convert_rfq_to_supplier_quotation(
        &self, rfq_id: Uuid, quotation_number: String, supplier_id: Uuid,
        quoted_rates: &[(Uuid, Decimal)], // (item_id, rate)
    ) -> Result<Uuid, BuyingError> {
        // RLS scope (ADR-0008), ID-only pattern — see `convert_material_request_to_rfq`.
        let rfq = self.repos.rfqs.fetch_source(&self.db_pool, rfq_id).await?
            .ok_or(BuyingError::SourceNotFound(rfq_id))?;
        let company_id = rfq.company_id;
        if rfq.status == "cancelled" {
            return Err(BuyingError::SourceNotConvertible(rfq_id.to_string()));
        }
        let items = self.repos.rfq_items.fetch_lines(&self.db_pool, rfq_id).await?;
        let rate_of = |item: Uuid| quoted_rates.iter().find(|(i, _)| *i == item).map(|(_, r)| *r).unwrap_or(Decimal::ZERO);

        let id = Uuid::new_v4();
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, company_id).await?;
        let r = self.repos.supplier_quotations.insert_quotation_from_rfq(&mut tx, &NewQuotationFromRfqRow {
            id,
            quotation_number: &quotation_number,
            rfq_id,
            company_id,
            supplier_id,
        }).await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { BuyingError::DuplicateNumber(quotation_number) } else { e.into() });
        }
        for it in &items {
            self.repos.supplier_quotation_items.insert_item(&mut tx, &NewSupplierQuotationItemRow {
                id: Uuid::new_v4(), quotation_id: id, company_id, item_id: it.item_id,
                quantity: it.quantity, rate: rate_of(it.item_id),
            }).await?;
        }
        tx.commit().await?;
        self.sink.publish(BuyingEvent::SupplierQuotationReceived(DocumentRaised {
            document_id: id, company_id, source_id: Some(rfq_id),
        }));
        Ok(id)
    }

    /// Convert an accepted supplier quotation into a draft Purchase Order — copies supplier + lines +
    /// rates, links `supplier_quotation_id`, advances the SQ to `ordered`. The §31 "select → PO" step.
    pub async fn convert_supplier_quotation_to_po(
        &self, quotation_id: Uuid, po_number: String, tax_rate: Decimal,
    ) -> Result<Uuid, BuyingError> {
        // RLS scope (ADR-0008), ID-only pattern — see `convert_material_request_to_rfq`.
        let sq = self.repos.supplier_quotations.fetch_source(&self.db_pool, quotation_id).await?
            .ok_or(BuyingError::SourceNotFound(quotation_id))?;
        if sq.status != "submitted" {
            return Err(BuyingError::SourceNotConvertible(quotation_id.to_string()));
        }
        let items = self.repos.supplier_quotation_items.fetch_lines(&self.db_pool, quotation_id).await?;
        if items.is_empty() {
            return Err(BuyingError::EmptyDocument);
        }
        let lines: Vec<NewLine> = items.iter().map(|it| NewLine {
            item_id: it.item_id,
            warehouse_id: None,
            description: None,
            quantity: it.quantity,
            rate: it.rate,
            qty_received_method: None,
            purchase_method: None,
        }).collect();

        let sq_company = sq.company_id;
        let order_id = self.create_purchase_order(NewPurchaseOrder {
            po_number,
            supplier_quotation_id: Some(quotation_id),
            order_kind: None,
            company_id: sq_company,
            branch_id: None,
            supplier_id: sq.supplier_id,
            order_date: chrono::Utc::now().date_naive(),
            schedule_date: None,
            currency: Some(sq.currency),
            currency_rate: None,
            agreement_id: None,
            project_id: None,
            tax_rate,
            notes: None,
            lines,
        }).await?;

        // The SQ's company was just read off its row — scope the status flip on it explicitly, so this
        // is correct for non-request callers too.
        company_scope::with_company_scope(
            Some(sq_company),
            self.repos.supplier_quotations.mark_ordered(&self.db_pool, quotation_id),
        ).await?;
        Ok(order_id)
    }
}
