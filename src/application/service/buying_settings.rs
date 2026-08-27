//! Purchase settings upserts (hand-authored, user-owned).
//!
//! An `impl BuyingWriteService` chunk over the vocabulary in [`super::buying_write_service`]. The
//! company-level purchase settings (double-validation gate + reminder switch + the company
//! currency the rate snapshot resolves against) and the supplier-level reminder overrides are
//! one-row-per-tenant documents: these verbs upsert them through the RLS-fenced write path, with
//! the company derived from the caller's scope (HTTP: the signed token; the verbs below take it
//! as a parameter so a job/event caller cannot forget it either).
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on
//! `PurchaseCompanySettingRepository` / `SupplierReminderSettingRepository`.

use backbone_orm::company_scope;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::infrastructure::persistence::{SettingsUpsert, SupplierReminderUpsert};

use super::buying_write_service::{BuyingError, BuyingWriteService};

impl BuyingWriteService {
    /// Read the caller's company purchase settings (the reminder job's G7 gate; hosts may also use
    /// it to render the current configuration). `Ok(None)` = not configured — the schema defaults
    /// (one_step, reminders on) apply.
    pub async fn company_purchase_settings(
        &self,
    ) -> Result<Option<crate::infrastructure::persistence::CompanyPurchaseSettingsRow>, sqlx::Error> {
        self.repos.purchase_company_settings.fetch_settings(&self.db_pool).await
    }

    /// Upsert the caller's company purchase settings. `double_validation` is `one_step`/`two_step`;
    /// `double_validation_amount` is denominated in the COMPANY currency (the gate converts the PO
    /// total INTO company currency with the order-time `currency_rate` snapshot before comparing).
    pub async fn upsert_purchase_company_settings(
        &self,
        company_id: Uuid,
        double_validation: String,
        double_validation_amount: Decimal,
        company_currency: String,
        send_reminder: bool,
    ) -> Result<(), BuyingError> {
        if !matches!(double_validation.as_str(), "one_step" | "two_step") {
            return Err(BuyingError::InvalidLineMethod(double_validation));
        }
        if double_validation_amount < Decimal::ZERO {
            return Err(BuyingError::NegativeQuantity);
        }
        if company_currency.is_empty() || company_currency.len() > 3 {
            return Err(BuyingError::InvalidLineMethod(company_currency));
        }
        // RLS scope (ADR-0008): company on the parameter — the upsert rides it, so the row written
        // is always the caller's own company's (HTTP: the same scope the token-derived tenant set).
        company_scope::with_company_scope(Some(company_id), async {
            self.repos.purchase_company_settings.upsert_settings(
                &self.db_pool,
                &SettingsUpsert {
                    double_validation: &double_validation,
                    double_validation_amount,
                    company_currency: &company_currency,
                    send_reminder,
                },
                company_id,
            ).await?;
            Ok(())
        }).await
    }

    /// Upsert one supplier's reminder overrides for the caller's company: whether receipt-reminder
    /// emails are on, and how many days before `schedule_date` the reminder fires. An absent row is
    /// the enabled default (on, 1 day) — the COMPANY-level `send_reminder=false` is the only
    /// off-switch (G7).
    pub async fn upsert_supplier_reminder_settings(
        &self,
        company_id: Uuid,
        supplier_id: Uuid,
        receipt_reminder_email: bool,
        reminder_days_before: i32,
    ) -> Result<(), BuyingError> {
        if reminder_days_before < 0 {
            return Err(BuyingError::NegativeQuantity);
        }
        // RLS scope (ADR-0008), as above.
        company_scope::with_company_scope(Some(company_id), async {
            self.repos.supplier_reminder_settings.upsert_for_supplier(
                &self.db_pool,
                &SupplierReminderUpsert {
                    supplier_id,
                    receipt_reminder_email,
                    reminder_days_before,
                },
                company_id,
            ).await?;
            Ok(())
        }).await
    }
}
