//! Blanket purchase agreements + their call-off orders (hand-authored, user-owned).
//!
//! An `impl BuyingWriteService` chunk over the vocabulary in [`super::buying_write_service`]. A
//! blanket agreement is a draft-time negotiated price book: header + lines created as `draft`,
//! confirmed into `open` (which MINTS the supplier-price rows — the only writer they have, the
//! PXB-1 correction), re-priced while open (line + minted price in ONE transaction), closed or
//! cancelled (which unlinks its prices), and consumed through call-off POs that take their prices
//! from the agreement lines and advance `qty_ordered` up to the blanket quantity.
//!
//! Close/cancel refuses while a call-off PO in a pre-confirmed state (`draft`/`sent`/`to_approve`)
//! still hangs off the agreement — cancelling the price source under a live draft call-off would
//! strand it without prices.
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on
//! `PurchaseAgreementRepository` / `PurchaseAgreementLineRepository` / `SupplierPriceRepository`,
//! and the tx-taking repo methods ride this service's transaction.

use backbone_orm::company_scope;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::infrastructure::persistence::{NewAgreementLineRow, NewAgreementRow};

use super::buying_write_service::{is_dup, BuyingError, BuyingWriteService, NewPurchaseOrder};

/// One negotiated line of a new blanket agreement.
#[derive(Debug, Clone)]
pub struct NewAgreementLine {
    pub item_id: Uuid,
    /// The blanket quantity this line covers across ALL of its call-offs.
    pub quantity: Decimal,
    /// The negotiated unit rate.
    pub rate: Decimal,
}

/// The create-input for a blanket agreement. Created as `draft`.
#[derive(Debug, Clone)]
pub struct NewPurchaseAgreement {
    pub agreement_number: String,
    pub company_id: Uuid,
    pub supplier_id: Uuid,
    pub currency: Option<String>,
    pub date_start: Option<chrono::NaiveDate>,
    pub date_end: Option<chrono::NaiveDate>,
    pub notes: Option<String>,
    pub lines: Vec<NewAgreementLine>,
}

/// A call-off against an open blanket: which agreement lines to draw on, and how much. The PO
/// number and dates belong to the PO being created.
#[derive(Debug, Clone)]
pub struct CallOffLine {
    pub agreement_line_id: Uuid,
    pub quantity: Decimal,
    pub warehouse_id: Option<Uuid>,
}
#[derive(Debug, Clone)]
pub struct NewCallOffOrder {
    pub po_number: String,
    pub company_id: Uuid,
    pub branch_id: Option<Uuid>,
    pub agreement_id: Uuid,
    pub order_date: chrono::NaiveDate,
    pub schedule_date: Option<chrono::NaiveDate>,
    pub tax_rate: Decimal,
    pub notes: Option<String>,
    pub lines: Vec<CallOffLine>,
}

impl BuyingWriteService {
    /// Create a blanket agreement as `draft` (header + lines, one transaction). No supplier prices
    /// exist yet — they are minted by [`Self::confirm_purchase_agreement`].
    pub async fn create_purchase_agreement(&self, a: NewPurchaseAgreement) -> Result<Uuid, BuyingError> {
        if a.lines.is_empty() {
            return Err(BuyingError::EmptyDocument);
        }
        for l in &a.lines {
            if l.quantity <= Decimal::ZERO || l.rate <= Decimal::ZERO {
                return Err(BuyingError::NegativeQuantity);
            }
        }
        let id = Uuid::new_v4();
        let currency = a.currency.clone().unwrap_or_else(|| "IDR".into());
        // RLS scope (ADR-0008): company is on the DTO — bind it onto our own transaction.
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, a.company_id).await?;
        let r = self.repos.purchase_agreements.insert_agreement(&mut tx, &NewAgreementRow {
            id,
            agreement_number: &a.agreement_number,
            company_id: a.company_id,
            supplier_id: a.supplier_id,
            currency: &currency,
            date_start: a.date_start,
            date_end: a.date_end,
            notes: a.notes.as_deref(),
        }).await;
        if let Err(e) = r {
            return Err(if is_dup(&e) { BuyingError::DuplicateNumber(a.agreement_number) } else { e.into() });
        }
        for l in &a.lines {
            self.repos.purchase_agreement_lines.insert_line(&mut tx, &NewAgreementLineRow {
                id: Uuid::new_v4(), agreement_id: id, company_id: a.company_id,
                item_id: l.item_id, quantity: l.quantity, rate: l.rate,
            }).await?;
        }
        tx.commit().await?;
        Ok(id)
    }

    /// Confirm a `draft` blanket into `open` — and mint its supplier prices: one
    /// `buying.supplier_prices` row per agreement line, in the SAME transaction. This verb (and the
    /// open re-price below) is the ONLY writer of supplier prices (PXB-1: no CRUD route, no other
    /// path). Refuses from any other state (`AgreementNotConvertible`).
    pub async fn confirm_purchase_agreement(&self, agreement_id: Uuid) -> Result<(), BuyingError> {
        let state = self.repos.purchase_agreements.fetch_state(&self.db_pool, agreement_id).await?
            .ok_or(BuyingError::AgreementNotFound(agreement_id))?;
        if state.status != "draft" {
            return Err(BuyingError::AgreementNotConvertible(agreement_id.to_string()));
        }
        let lines = self.repos.purchase_agreement_lines.fetch_lines(&self.db_pool, agreement_id).await?;
        if lines.is_empty() {
            return Err(BuyingError::EmptyDocument);
        }

        let company_id = state.company_id;
        let supplier_id = state.supplier_id;
        let currency = state.currency;
        company_scope::with_company_scope(Some(company_id), async move {
            let mut tx = self.db_pool.begin().await?;
            company_scope::bind_company_on(&mut tx, company_id).await?;
            self.repos.purchase_agreements
                .transition(&mut tx, agreement_id, "open", &["draft"]).await?
                .ok_or_else(|| BuyingError::AgreementNotConvertible(agreement_id.to_string()))?;
            for l in &lines {
                self.repos.supplier_prices.upsert_for_agreement_line(
                    &mut tx, company_id, supplier_id, l.item_id, l.rate,
                    &currency, agreement_id, l.id,
                ).await?;
            }
            tx.commit().await?;
            Ok(())
        }).await
    }

    /// Re-price one line of an OPEN agreement: the line's rate and its minted supplier-price row
    /// move together, in one transaction. Refuses on a draft (nothing is minted yet — edit the
    /// line) and on done/cancelled.
    pub async fn update_agreement_line_price(
        &self,
        line_id: Uuid,
        rate: Decimal,
    ) -> Result<(), BuyingError> {
        if rate <= Decimal::ZERO {
            return Err(BuyingError::NegativeQuantity);
        }
        let (agreement_id, status) = self.repos.purchase_agreement_lines.fetch_parent_status(&self.db_pool, line_id).await?
            .ok_or(BuyingError::AgreementNotFound(line_id))?;
        if status != "open" {
            return Err(BuyingError::AgreementNotConvertible(status));
        }
        let line = self.repos.purchase_agreement_lines.fetch_line(&self.db_pool, line_id).await?
            .ok_or(BuyingError::AgreementNotFound(line_id))?;
        let state = self.repos.purchase_agreements.fetch_state(&self.db_pool, agreement_id).await?
            .ok_or(BuyingError::AgreementNotFound(agreement_id))?;

        let company_id = line.company_id;
        company_scope::with_company_scope(Some(company_id), async {
            let mut tx = self.db_pool.begin().await?;
            company_scope::bind_company_on(&mut tx, company_id).await?;
            self.repos.purchase_agreement_lines.set_line_rate(&mut tx, line_id, rate).await?;
            self.repos.supplier_prices.upsert_for_agreement_line(
                &mut tx, company_id, state.supplier_id, line.item_id, rate,
                &state.currency, agreement_id, line_id,
            ).await?;
            tx.commit().await?;
            Ok(())
        }).await
    }

    /// Replace a DRAFT agreement's line set (the resequence: the line order IS the document, so
    /// re-sequencing rewrites the set). One transaction: old lines soft-deleted, the new ordered
    /// set inserted. Refuses on non-draft.
    pub async fn resequence_agreement_lines(
        &self,
        agreement_id: Uuid,
        lines: Vec<NewAgreementLine>,
    ) -> Result<(), BuyingError> {
        if lines.is_empty() {
            return Err(BuyingError::EmptyDocument);
        }
        for l in &lines {
            if l.quantity <= Decimal::ZERO || l.rate <= Decimal::ZERO {
                return Err(BuyingError::NegativeQuantity);
            }
        }
        let state = self.repos.purchase_agreements.fetch_state(&self.db_pool, agreement_id).await?
            .ok_or(BuyingError::AgreementNotFound(agreement_id))?;
        if state.status != "draft" {
            return Err(BuyingError::AgreementNotConvertible(agreement_id.to_string()));
        }
        let company_id = state.company_id;
        company_scope::with_company_scope(Some(company_id), async {
            let mut tx = self.db_pool.begin().await?;
            company_scope::bind_company_on(&mut tx, company_id).await?;
            self.repos.purchase_agreement_lines.soft_delete_all_for_agreement(&mut tx, agreement_id).await?;
            for l in &lines {
                self.repos.purchase_agreement_lines.insert_line(&mut tx, &NewAgreementLineRow {
                    id: Uuid::new_v4(), agreement_id, company_id,
                    item_id: l.item_id, quantity: l.quantity, rate: l.rate,
                }).await?;
            }
            tx.commit().await?;
            Ok(())
        }).await
    }

    /// Close (`open` → `done`) or cancel (`draft`/`open` → `cancelled`) an agreement. Both refuse
    /// while a call-off PO in a pre-confirmed state hangs off it, and both UNLINK the agreement's
    /// supplier-price rows (a closed blanket no longer prices new call-offs) — status flip and
    /// unlink in one transaction.
    async fn retire_agreement(&self, agreement_id: Uuid, to: &str, from: &[&str]) -> Result<(), BuyingError> {
        let state = self.repos.purchase_agreements.fetch_state(&self.db_pool, agreement_id).await?
            .ok_or(BuyingError::AgreementNotFound(agreement_id))?;
        if state.draft_order_count > 0 {
            return Err(BuyingError::AgreementHasDraftOrders(agreement_id));
        }
        if !from.contains(&state.status.as_str()) {
            return Err(BuyingError::AgreementNotConvertible(agreement_id.to_string()));
        }
        let company_id = state.company_id;
        company_scope::with_company_scope(Some(company_id), async {
            let mut tx = self.db_pool.begin().await?;
            company_scope::bind_company_on(&mut tx, company_id).await?;
            self.repos.purchase_agreements.transition(&mut tx, agreement_id, to, from).await?
                .ok_or_else(|| BuyingError::AgreementNotConvertible(agreement_id.to_string()))?;
            self.repos.supplier_prices.soft_delete_for_agreement(&mut tx, agreement_id).await?;
            tx.commit().await?;
            Ok(())
        }).await
    }

    /// Close an open blanket (`done`).
    pub async fn close_purchase_agreement(&self, agreement_id: Uuid) -> Result<(), BuyingError> {
        self.retire_agreement(agreement_id, "done", &["open"]).await
    }

    /// Cancel a blanket (draft or open).
    pub async fn cancel_purchase_agreement(&self, agreement_id: Uuid) -> Result<(), BuyingError> {
        self.retire_agreement(agreement_id, "cancelled", &["draft", "open"]).await
    }

    /// Create a call-off PO against an OPEN blanket agreement: prices come from the agreement
    /// lines (not from the request), the PO carries `agreement_id`, and each drawn line's
    /// `qty_ordered` advances — refused (`AgreementExceeded`) past the blanket quantity. PO header
    /// + lines + blanket consumption commit as ONE transaction (delegates the create shape to
    /// [`Self::create_purchase_order`] semantics, inlined here so the blanket consumption rides the
    /// same unit of work).
    pub async fn create_call_off_po(&self, o: NewCallOffOrder) -> Result<Uuid, BuyingError> {
        if o.lines.is_empty() {
            return Err(BuyingError::NoLinesSelected);
        }
        let state = self.repos.purchase_agreements.fetch_state(&self.db_pool, o.agreement_id).await?
            .ok_or(BuyingError::AgreementNotFound(o.agreement_id))?;
        if state.status != "open" {
            return Err(BuyingError::AgreementNotConvertible(o.agreement_id.to_string()));
        }
        let agreement_lines = self.repos.purchase_agreement_lines.fetch_lines(&self.db_pool, o.agreement_id).await?;
        let by_id: std::collections::HashMap<Uuid, _> =
            agreement_lines.into_iter().map(|l| (l.id, l)).collect();

        // Resolve + cap-check every draw BEFORE writing anything.
        let mut draws: Vec<(&crate::infrastructure::persistence::AgreementLineRow, &CallOffLine)> =
            Vec::with_capacity(o.lines.len());
        for c in &o.lines {
            let al = by_id.get(&c.agreement_line_id)
                .ok_or(BuyingError::AgreementNotFound(c.agreement_line_id))?;
            if c.quantity <= Decimal::ZERO {
                return Err(BuyingError::NegativeQuantity);
            }
            if al.qty_ordered + c.quantity > al.quantity {
                return Err(BuyingError::AgreementExceeded(c.agreement_line_id));
            }
            draws.push((al, c));
        }

        // The call-off PO + its blanket consumption commit as ONE unit of work: header, lines,
        // and the qty_ordered increments ride the same transaction, so a PO can never exist with
        // its draw uncounted (an uncounted draw would under-count the blanket cap). The create
        // shape (server-owned totals, currency gate, line-method defaults, duplicate-number
        // mapping) is the same one `create_purchase_order` applies, driven here onto this
        // transaction because that verb commits its own.
        let po_lines: Vec<super::buying_write_service::NewLine> = draws.iter().map(|(al, c)| super::buying_write_service::NewLine {
            item_id: al.item_id,
            warehouse_id: c.warehouse_id,
            description: None,
            quantity: c.quantity,
            rate: al.rate,
            qty_received_method: None,
            purchase_method: None,
        }).collect();
        let new_order = NewPurchaseOrder {
            po_number: o.po_number,
            supplier_quotation_id: None,
            order_kind: Some("standard".into()),
            company_id: o.company_id,
            branch_id: o.branch_id,
            supplier_id: state.supplier_id,
            order_date: o.order_date,
            schedule_date: o.schedule_date,
            currency: Some(state.currency.clone()),
            currency_rate: None,
            agreement_id: Some(o.agreement_id),
            tax_rate: o.tax_rate,
            notes: o.notes.clone(),
            lines: po_lines,
        };
        let (priced, subtotal, tax_amount, total) = super::buying_write_service::price_document(&new_order.lines, new_order.tax_rate)?;
        let po_id = Uuid::new_v4();

        let company_id = o.company_id;
        company_scope::with_company_scope(Some(company_id), async move {
            // The same currency gate create_purchase_order applies: the agreement's currency
            // against the company's, with a loud refusal when a rate snapshot is missing.
            let settings = self.repos.purchase_company_settings.fetch_settings(&self.db_pool).await?;
            let company_currency = settings.map(|s| s.company_currency).unwrap_or_else(|| "IDR".into());
            let rate = super::buying_order_create::resolve_rate(&new_order, &company_currency)?;
            let currency = new_order.currency.clone().unwrap_or(company_currency);
            let kind = new_order.order_kind.clone().unwrap_or_else(|| "standard".into());

            let mut tx = self.db_pool.begin().await?;
            company_scope::bind_company_on(&mut tx, company_id).await?;
            let r = self.repos.purchase_orders.insert_purchase_order(&mut tx, &crate::infrastructure::persistence::NewPurchaseOrderRow {
                id: po_id,
                po_number: &new_order.po_number,
                supplier_quotation_id: new_order.supplier_quotation_id,
                order_kind: &kind,
                company_id,
                branch_id: new_order.branch_id,
                supplier_id: new_order.supplier_id,
                order_date: new_order.order_date,
                schedule_date: new_order.schedule_date,
                currency: &currency,
                currency_rate: rate,
                agreement_id: new_order.agreement_id,
                subtotal,
                tax_rate: new_order.tax_rate,
                tax_amount,
                total,
                notes: new_order.notes.as_deref(),
            }).await;
            if let Err(e) = r {
                return Err(if super::buying_write_service::is_dup(&e) { BuyingError::DuplicateNumber(new_order.po_number.clone()) } else { e.into() });
            }
            for (p, l) in priced.iter().zip(new_order.lines.iter()) {
                let (rm, pm) = super::buying_order_create::line_method_pair(&l.qty_received_method, &l.purchase_method)?;
                self.repos.purchase_order_items.insert_item(&mut tx, &crate::infrastructure::persistence::NewPurchaseOrderItemRow {
                    id: Uuid::new_v4(), order_id: po_id, company_id, item_id: p.item_id, warehouse_id: p.warehouse_id,
                    description: p.description.as_deref(), quantity: p.quantity, rate: p.rate,
                    line_amount: p.line_amount, qty_received_method: rm, purchase_method: pm,
                }).await?;
            }
            // The blanket consumption rides the SAME transaction — this is the call-off's
            // defining atomicity: no PO without its counted draw, no counted draw without a PO.
            for (_, c) in &draws {
                self.repos.purchase_agreement_lines
                    .increment_qty_ordered(&mut tx, c.agreement_line_id, c.quantity).await?;
            }
            tx.commit().await?;
            Ok(po_id)
        }).await
    }
}
