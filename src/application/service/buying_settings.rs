//! Purchase settings upserts (hand-authored, user-owned).
//!
//! An `impl BuyingWriteService` chunk over the vocabulary in [`super::buying_write_service`]. The
//! purchase posture settings (double-validation gate + reminder switch + the home
//! currency the rate snapshot resolves against) and the supplier-level reminder overrides are
//! one-live-row documents: these verbs upsert them through the module's plain write path. The
//! module carries no tenant axis of its own (ADR-0029) — under a composed tenancy decorator the
//! statements ride the request-dedicated connection carrying the decorator's fence variables, so
//! the rows seen and written are the caller's own scope's; plainly on the pool otherwise.
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on
//! `PurchaseCompanySettingRepository` / `SupplierReminderSettingRepository`.

use rust_decimal::Decimal;
use uuid::Uuid;

use crate::infrastructure::persistence::{SettingsUpsert, SupplierReminderUpsert};

use super::buying_write_service::{BuyingError, BuyingWriteService};

impl BuyingWriteService {
    /// Read the purchase settings (the reminder job's G7 gate; hosts may also use
    /// it to render the current configuration). `Ok(None)` = not configured — the schema defaults
    /// (one_step, reminders on) apply.
    pub async fn company_purchase_settings(
        &self,
    ) -> Result<Option<crate::infrastructure::persistence::CompanyPurchaseSettingsRow>, sqlx::Error> {
        self.repos.purchase_company_settings.fetch_settings(&self.db_pool).await
    }

    /// Upsert the purchase settings. `double_validation` is `one_step`/`two_step`;
    /// `double_validation_amount` is denominated in the home currency (the gate converts the PO
    /// total INTO home currency with the order-time `currency_rate` snapshot before comparing).
    pub async fn upsert_purchase_company_settings(
        &self,
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
        self.repos.purchase_company_settings.upsert_settings(
            &self.db_pool,
            &SettingsUpsert {
                double_validation: &double_validation,
                double_validation_amount,
                company_currency: &company_currency,
                send_reminder,
            },
        ).await?;
        Ok(())
    }

    /// Upsert one supplier's reminder overrides: whether receipt-reminder
    /// emails are on, and how many days before `schedule_date` the reminder fires. An absent row is
    /// the enabled default (on, 1 day) — the settings-level `send_reminder=false` is the only
    /// off-switch (G7).
    pub async fn upsert_supplier_reminder_settings(
        &self,
        supplier_id: Uuid,
        receipt_reminder_email: bool,
        reminder_days_before: i32,
    ) -> Result<(), BuyingError> {
        if reminder_days_before < 0 {
            return Err(BuyingError::NegativeQuantity);
        }
        self.repos.supplier_reminder_settings.upsert_for_supplier(
            &self.db_pool,
            &SupplierReminderUpsert {
                supplier_id,
                receipt_reminder_email,
                reminder_days_before,
            },
        ).await?;
        Ok(())
    }
}
