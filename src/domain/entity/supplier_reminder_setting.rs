use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use super::AuditMetadata;

/// Strongly-typed ID for SupplierReminderSetting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SupplierReminderSettingId(pub Uuid);

impl SupplierReminderSettingId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for SupplierReminderSettingId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for SupplierReminderSettingId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for SupplierReminderSettingId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<SupplierReminderSettingId> for Uuid {
    fn from(id: SupplierReminderSettingId) -> Self { id.0 }
}

impl AsRef<Uuid> for SupplierReminderSettingId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for SupplierReminderSettingId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SupplierReminderSetting {
    pub id: Uuid,
    pub company_id: Uuid,
    pub supplier_id: Uuid,
    pub receipt_reminder_email: bool,
    pub reminder_days_before: i32,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl SupplierReminderSetting {
    /// Create a builder for SupplierReminderSetting
    pub fn builder() -> SupplierReminderSettingBuilder {
        <SupplierReminderSettingBuilder as Default>::default()
    }

    /// Create a new SupplierReminderSetting with required fields
    pub fn new(company_id: Uuid, supplier_id: Uuid, receipt_reminder_email: bool, reminder_days_before: i32) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            supplier_id,
            receipt_reminder_email,
            reminder_days_before,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> SupplierReminderSettingId {
        SupplierReminderSettingId(self.id)
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
                "supplier_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.supplier_id = v; }
                }
                "receipt_reminder_email" => {
                    if let Ok(v) = serde_json::from_value(value) { self.receipt_reminder_email = v; }
                }
                "reminder_days_before" => {
                    if let Ok(v) = serde_json::from_value(value) { self.reminder_days_before = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for SupplierReminderSetting {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "SupplierReminderSetting"
    }
}

impl backbone_core::PersistentEntity for SupplierReminderSetting {
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

impl backbone_orm::EntityRepoMeta for SupplierReminderSetting {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("supplier_id".to_string(), "uuid".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for SupplierReminderSetting entity
///
/// Provides a fluent API for constructing SupplierReminderSetting instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct SupplierReminderSettingBuilder {
    company_id: Option<Uuid>,
    supplier_id: Option<Uuid>,
    receipt_reminder_email: Option<bool>,
    reminder_days_before: Option<i32>,
}

impl SupplierReminderSettingBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the supplier_id field (required)
    pub fn supplier_id(mut self, value: Uuid) -> Self {
        self.supplier_id = Some(value);
        self
    }

    /// Set the receipt_reminder_email field (default: `true`)
    pub fn receipt_reminder_email(mut self, value: bool) -> Self {
        self.receipt_reminder_email = Some(value);
        self
    }

    /// Set the reminder_days_before field (default: `1`)
    pub fn reminder_days_before(mut self, value: i32) -> Self {
        self.reminder_days_before = Some(value);
        self
    }

    /// Build the SupplierReminderSetting entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<SupplierReminderSetting, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let supplier_id = self.supplier_id.ok_or_else(|| "supplier_id is required".to_string())?;

        Ok(SupplierReminderSetting {
            id: Uuid::new_v4(),
            company_id,
            supplier_id,
            receipt_reminder_email: self.receipt_reminder_email.unwrap_or(true),
            reminder_days_before: self.reminder_days_before.unwrap_or(1),
            metadata: AuditMetadata::default(),
        })
    }
}
