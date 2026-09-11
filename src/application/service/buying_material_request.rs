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

use rust_decimal::Decimal;
use uuid::Uuid;

use crate::infrastructure::persistence::{
    NewMaterialRequestItemRow, NewMaterialRequestRow, NewRfqItemRow, NewRfqRow, NewRfqSupplierRow,
};

use super::buying_events::{BuyingEvent, DocumentRaised};
use super::buying_write_service::{
    is_dup, legacy_company_echo, relay_ambient_scope, BuyingError, BuyingWriteService,
    NewMaterialRequest,
};

impl BuyingWriteService {
    // ---- Material Request ---------------------------------------------------

    pub async fn create_material_request(&self, m: NewMaterialRequest) -> Result<Uuid, BuyingError> {
        if m.lines.is_empty() { return Err(BuyingError::EmptyDocument); }
        for l in &m.lines { if l.quantity < Decimal::ZERO { return Err(BuyingError::NegativeQuantity); } }
        let id = Uuid::new_v4();
        let rt = m.request_type.unwrap_or_else(|| "purchase".into());
        let mut tx = self.db_pool.begin().await?;
        // Relay the caller's ambient org scope onto this transaction (ADR-0029): the composing
        // service's decorator set it task-locally; the fresh transaction carries none of it.
        relay_ambient_scope(&mut tx).await?;
        let r = self.repos.material_requests.insert_material_request(&mut tx, &NewMaterialRequestRow {
            id,
            request_number: &m.request_number,
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
                id: Uuid::new_v4(), request_id: id, item_id: l.item_id, quantity: l.quantity,
            }).await?;
        }
        tx.commit().await?;
        self.sink.publish(BuyingEvent::MaterialRequestRaised(DocumentRaised {
            document_id: id, company_id: legacy_company_echo(), source_id: None,
        }));
        Ok(id)
    }

    /// Convert a material request into an RFQ to the invited suppliers (copies the requested lines,
    /// links `material_request_id`, marks the MR `ordered`). The MR→RFQ funnel step. The reads are
    /// id-only: identified by the MR id alone, with the composed decorator owning isolation —
    /// another scope's MR simply isn't visible under the fence.
    pub async fn convert_material_request_to_rfq(
        &self, request_id: Uuid, rfq_number: String, response_due: Option<chrono::NaiveDate>,
        supplier_ids: &[Uuid],
    ) -> Result<Uuid, BuyingError> {
        let mr = self.repos.material_requests.fetch_source(&self.db_pool, request_id).await?
            .ok_or(BuyingError::SourceNotFound(request_id))?;
        if mr.status == "cancelled" {
            return Err(BuyingError::SourceNotConvertible(request_id.to_string()));
        }
        let items = self.repos.material_request_items.fetch_lines(&self.db_pool, request_id).await?;

        let id = Uuid::new_v4();
        let mut tx = self.db_pool.begin().await?;
        // Relay the caller's ambient org scope onto this transaction (ADR-0029).
        relay_ambient_scope(&mut tx).await?;
        let r = self.repos.rfqs.insert_rfq(&mut tx, &NewRfqRow {
            id,
            rfq_number: &rfq_number,
            material_request_id: request_id,
            response_due,
        }).await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { BuyingError::DuplicateNumber(rfq_number) } else { e.into() });
        }
        for it in &items {
            self.repos.rfq_items.insert_item(&mut tx, &NewRfqItemRow {
                id: Uuid::new_v4(), rfq_id: id, item_id: it.item_id, quantity: it.quantity,
            }).await?;
        }
        for sup in supplier_ids {
            self.repos.rfq_suppliers.insert_supplier(&mut tx, &NewRfqSupplierRow {
                id: Uuid::new_v4(), rfq_id: id, supplier_id: *sup,
            }).await?;
        }
        self.repos.material_requests.mark_ordered(&mut tx, request_id).await?;
        tx.commit().await?;
        self.sink.publish(BuyingEvent::RfqIssued(DocumentRaised {
            document_id: id, company_id: legacy_company_echo(), source_id: Some(request_id),
        }));
        Ok(id)
    }
}
