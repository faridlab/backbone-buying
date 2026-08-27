use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;

use super::DoubleValidation;
use super::AuditMetadata;

/// Strongly-typed ID for PurchaseCompanySetting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PurchaseCompanySettingId(pub Uuid);

impl PurchaseCompanySettingId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for PurchaseCompanySettingId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for PurchaseCompanySettingId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for PurchaseCompanySettingId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<PurchaseCompanySettingId> for Uuid {
    fn from(id: PurchaseCompanySettingId) -> Self { id.0 }
}

impl AsRef<Uuid> for PurchaseCompanySettingId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for PurchaseCompanySettingId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PurchaseCompanySetting {
    pub id: Uuid,
    pub company_id: Uuid,
    pub double_validation: DoubleValidation,
    pub double_validation_amount: Decimal,
    pub company_currency: String,
    pub send_reminder: bool,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl PurchaseCompanySetting {
    /// Create a builder for PurchaseCompanySetting
    pub fn builder() -> PurchaseCompanySettingBuilder {
        <PurchaseCompanySettingBuilder as Default>::default()
    }

    /// Create a new PurchaseCompanySetting with required fields
    pub fn new(company_id: Uuid, double_validation: DoubleValidation, double_validation_amount: Decimal, company_currency: String, send_reminder: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            double_validation,
            double_validation_amount,
            company_currency,
            send_reminder,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> PurchaseCompanySettingId {
        PurchaseCompanySettingId(self.id)
    }

    /// Get when this entity was created
    pub fn created_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.created_at.as_ref()
    }

    /// Get when this entity was last updated
    pub fn updated_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.updated_at.as_ref()
    }

    /// Check if this entity is soft deleted
    pub fn is_deleted(&self) -> bool {
        self.metadata.deleted_at.is_some()
    }

    /// Check if this entity is active (not deleted)
    pub fn is_active(&self) -> bool {
        self.metadata.deleted_at.is_none()
    }

    /// Get when this entity was deleted
    pub fn deleted_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.deleted_at.as_ref()
    }

    /// Get who created this entity
    pub fn created_by(&self) -> Option<&Uuid> {
        self.metadata.created_by.as_ref()
    }

    /// Get who last updated this entity
    pub fn updated_by(&self) -> Option<&Uuid> {
        self.metadata.updated_by.as_ref()
    }

    /// Get who deleted this entity
    pub fn deleted_by(&self) -> Option<&Uuid> {
        self.metadata.deleted_by.as_ref()
    }


    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "company_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.company_id = v; }
                }
                "double_validation" => {
                    if let Ok(v) = serde_json::from_value(value) { self.double_validation = v; }
                }
                "double_validation_amount" => {
                    if let Ok(v) = serde_json::from_value(value) { self.double_validation_amount = v; }
                }
                "company_currency" => {
                    if let Ok(v) = serde_json::from_value(value) { self.company_currency = v; }
                }
                "send_reminder" => {
                    if let Ok(v) = serde_json::from_value(value) { self.send_reminder = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for PurchaseCompanySetting {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "PurchaseCompanySetting"
    }
}

impl backbone_core::PersistentEntity for PurchaseCompanySetting {
    fn entity_id(&self) -> String {
        self.id.to_string()
    }
    fn set_entity_id(&mut self, id: String) {
        if let Ok(uuid) = uuid::Uuid::parse_str(&id) {
            self.id = uuid;
        }
    }
    fn created_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.created_at
    }
    fn set_created_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        self.metadata.created_at = Some(ts);
    }
    fn updated_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.updated_at
    }
    fn set_updated_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        self.metadata.updated_at = Some(ts);
    }
    fn deleted_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.deleted_at
    }
    fn set_deleted_at(&mut self, ts: Option<chrono::DateTime<chrono::Utc>>) {
        self.metadata.deleted_at = ts;
    }
}

impl backbone_orm::EntityRepoMeta for PurchaseCompanySetting {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("double_validation".to_string(), "double_validation".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["company_currency"]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for PurchaseCompanySetting entity
///
/// Provides a fluent API for constructing PurchaseCompanySetting instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct PurchaseCompanySettingBuilder {
    company_id: Option<Uuid>,
    double_validation: Option<DoubleValidation>,
    double_validation_amount: Option<Decimal>,
    company_currency: Option<String>,
    send_reminder: Option<bool>,
}

impl PurchaseCompanySettingBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the double_validation field (default: `DoubleValidation::default()`)
    pub fn double_validation(mut self, value: DoubleValidation) -> Self {
        self.double_validation = Some(value);
        self
    }

    /// Set the double_validation_amount field (default: `Decimal::from(5000)`)
    pub fn double_validation_amount(mut self, value: Decimal) -> Self {
        self.double_validation_amount = Some(value);
        self
    }

    /// Set the company_currency field (default: `"IDR".to_string()`)
    pub fn company_currency(mut self, value: String) -> Self {
        self.company_currency = Some(value);
        self
    }

    /// Set the send_reminder field (default: `true`)
    pub fn send_reminder(mut self, value: bool) -> Self {
        self.send_reminder = Some(value);
        self
    }

    /// Build the PurchaseCompanySetting entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<PurchaseCompanySetting, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;

        Ok(PurchaseCompanySetting {
            id: Uuid::new_v4(),
            company_id,
            double_validation: self.double_validation.unwrap_or_default(),
            double_validation_amount: self.double_validation_amount.unwrap_or(Decimal::from(5000)),
            company_currency: self.company_currency.unwrap_or("IDR".to_string()),
            send_reminder: self.send_reminder.unwrap_or(true),
            metadata: AuditMetadata::default(),
        })
    }
}
