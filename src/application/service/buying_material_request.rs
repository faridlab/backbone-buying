//! Funnel entry: material request + the MR → RFQ conversion (hand-authored, user-owned).
//!
//! An `impl BuyingWriteService` chunk over the vocabulary in [`super::buying_write_service`]. The
//! procurement funnel starts here — a material request is the demand signal; converting it into an
//! RFQ fans it out to invited suppliers. Both writes are transactional (header + lines in one unit)
//! and emit `MaterialRequestRaised` / `RfqIssued` so downstream can track the funnel.
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on
//! `MaterialRequestRepository` / `MaterialRequestItemRepository` / `RequestForQuotationRepository` /
//! `RfqItemRepository` / `RfqSupplierRepository`, and the tx-taking repo methods ride this service's
//! transaction so the header + lines commit together.

use backbone_orm::company_scope;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::infrastructure::persistence::{
    NewMaterialRequestItemRow, NewMaterialRequestRow, NewRfqItemRow, NewRfqRow, NewRfqSupplierRow,
};

use super::buying_events::{BuyingEvent, DocumentRaised};
use super::buying_write_service::{is_dup, BuyingError, BuyingWriteService, NewMaterialRequest};

impl BuyingWriteService {
    // ---- Material Request ---------------------------------------------------

    pub async fn create_material_request(&self, m: NewMaterialRequest) -> Result<Uuid, BuyingError> {
        if m.lines.is_empty() { return Err(BuyingError::EmptyDocument); }
        for l in &m.lines { if l.quantity < Decimal::ZERO { return Err(BuyingError::NegativeQuantity); } }
        let id = Uuid::new_v4();
        let rt = m.request_type.unwrap_or_else(|| "purchase".into());
        // RLS scope (ADR-0008): company is on the DTO — bind it onto our own transaction so the
        // header+lines insert passes the WITH CHECK fence. Explicit `company_id` binds stay as
        // defense-in-depth.
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, m.company_id).await?;
        let r = self.repos.material_requests.insert_material_request(&mut tx, &NewMaterialRequestRow {
            id,
            request_number: &m.request_number,
            company_id: m.company_id,
            request_type: &rt,
            request_date: m.request_date,
            schedule_date: m.schedule_date,
            notes: m.notes.as_deref(),
        }).await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { BuyingError::DuplicateNumber(m.request_number) } else { e.into() });
        }
        for l in &m.lines {
            self.repos.material_request_items.insert_item(&mut tx, &NewMaterialRequestItemRow {
                id: Uuid::new_v4(), request_id: id, company_id: m.company_id, item_id: l.item_id, quantity: l.quantity,
            }).await?;
        }
        tx.commit().await?;
        self.sink.publish(BuyingEvent::MaterialRequestRaised(DocumentRaised {
            document_id: id, company_id: m.company_id, source_id: None,
        }));
        Ok(id)
    }

    /// Convert a material request into an RFQ to the invited suppliers (copies the requested lines,
    /// links `material_request_id`, marks the MR `ordered`). The MR→RFQ funnel step.
    pub async fn convert_material_request_to_rfq(
        &self, request_id: Uuid, rfq_number: String, response_due: Option<chrono::NaiveDate>,
        supplier_ids: &[Uuid],
    ) -> Result<Uuid, BuyingError> {
        // RLS scope (ADR-0008), ID-only pattern: identified by the MR id alone — the reads ride the
        // request-dedicated connection (which carries the caller's `app.company_id`), so another
        // company's MR simply isn't found. The company read off the row then binds our transaction.
        let mr = self.repos.material_requests.fetch_source(&self.db_pool, request_id).await?
            .ok_or(BuyingError::SourceNotFound(request_id))?;
        let company_id = mr.company_id;
        if mr.status == "cancelled" {
            return Err(BuyingError::SourceNotConvertible(request_id.to_string()));
        }
        let items = self.repos.material_request_items.fetch_lines(&self.db_pool, request_id).await?;

        let id = Uuid::new_v4();
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, company_id).await?;
        let r = self.repos.rfqs.insert_rfq(&mut tx, &NewRfqRow {
            id,
            rfq_number: &rfq_number,
            material_request_id: request_id,
            company_id,
            response_due,
        }).await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { BuyingError::DuplicateNumber(rfq_number) } else { e.into() });
        }
        for it in &items {
            self.repos.rfq_items.insert_item(&mut tx, &NewRfqItemRow {
                id: Uuid::new_v4(), rfq_id: id, company_id, item_id: it.item_id, quantity: it.quantity,
            }).await?;
        }
        for sup in supplier_ids {
            self.repos.rfq_suppliers.insert_supplier(&mut tx, &NewRfqSupplierRow {
                id: Uuid::new_v4(), rfq_id: id, company_id, supplier_id: *sup,
            }).await?;
        }
        self.repos.material_requests.mark_ordered(&mut tx, request_id).await?;
        tx.commit().await?;
        self.sink.publish(BuyingEvent::RfqIssued(DocumentRaised {
            document_id: id, company_id, source_id: Some(request_id),
        }));
        Ok(id)
    }
}
