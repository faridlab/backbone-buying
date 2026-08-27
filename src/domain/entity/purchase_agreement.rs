use chrono::{DateTime, Utc, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::AgreementKind;
use super::PurchaseAgreementStatus;
use super::AuditMetadata;

/// Strongly-typed ID for PurchaseAgreement
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PurchaseAgreementId(pub Uuid);

impl PurchaseAgreementId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for PurchaseAgreementId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for PurchaseAgreementId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for PurchaseAgreementId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<PurchaseAgreementId> for Uuid {
    fn from(id: PurchaseAgreementId) -> Self { id.0 }
}

impl AsRef<Uuid> for PurchaseAgreementId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for PurchaseAgreementId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PurchaseAgreement {
    pub id: Uuid,
    pub agreement_number: String,
    pub agreement_kind: AgreementKind,
    pub status: PurchaseAgreementStatus,
    pub company_id: Uuid,
    pub supplier_id: Uuid,
    pub currency: String,
    pub date_start: Option<NaiveDate>,
    pub date_end: Option<NaiveDate>,
    pub notes: Option<String>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl PurchaseAgreement {
    /// Create a builder for PurchaseAgreement
    pub fn builder() -> PurchaseAgreementBuilder {
        <PurchaseAgreementBuilder as Default>::default()
    }

    /// Create a new PurchaseAgreement with required fields
    pub fn new(agreement_number: String, agreement_kind: AgreementKind, status: PurchaseAgreementStatus, company_id: Uuid, supplier_id: Uuid, currency: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            agreement_number,
            agreement_kind,
            status,
            company_id,
            supplier_id,
            currency,
            date_start: None,
            date_end: None,
            notes: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> PurchaseAgreementId {
        PurchaseAgreementId(self.id)
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

    /// Get the current status
    pub fn status(&self) -> &PurchaseAgreementStatus {
        &self.status
    }


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the date_start field (chainable)
    pub fn with_date_start(mut self, value: NaiveDate) -> Self {
        self.date_start = Some(value);
        self
    }

    /// Set the date_end field (chainable)
    pub fn with_date_end(mut self, value: NaiveDate) -> Self {
        self.date_end = Some(value);
        self
    }

    /// Set the notes field (chainable)
    pub fn with_notes(mut self, value: String) -> Self {
        self.notes = Some(value);
        self
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "agreement_number" => {
                    if let Ok(v) = serde_json::from_value(value) { self.agreement_number = v; }
                }
                "agreement_kind" => {
                    if let Ok(v) = serde_json::from_value(value) { self.agreement_kind = v; }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) { self.status = v; }
                }
                "company_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.company_id = v; }
                }
                "supplier_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.supplier_id = v; }
                }
                "currency" => {
                    if let Ok(v) = serde_json::from_value(value) { self.currency = v; }
                }
                "date_start" => {
                    if let Ok(v) = serde_json::from_value(value) { self.date_start = v; }
                }
                "date_end" => {
                    if let Ok(v) = serde_json::from_value(value) { self.date_end = v; }
                }
                "notes" => {
                    if let Ok(v) = serde_json::from_value(value) { self.notes = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for PurchaseAgreement {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "PurchaseAgreement"
    }
}

impl backbone_core::PersistentEntity for PurchaseAgreement {
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

impl backbone_orm::EntityRepoMeta for PurchaseAgreement {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("supplier_id".to_string(), "uuid".to_string());
        m.insert("agreement_kind".to_string(), "agreement_kind".to_string());
        m.insert("status".to_string(), "purchase_agreement_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["agreement_number", "currency"]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for PurchaseAgreement entity
///
/// Provides a fluent API for constructing PurchaseAgreement instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct PurchaseAgreementBuilder {
    agreement_number: Option<String>,
    agreement_kind: Option<AgreementKind>,
    status: Option<PurchaseAgreementStatus>,
    company_id: Option<Uuid>,
    supplier_id: Option<Uuid>,
    currency: Option<String>,
    date_start: Option<NaiveDate>,
    date_end: Option<NaiveDate>,
    notes: Option<String>,
}

impl PurchaseAgreementBuilder {
    /// Set the agreement_number field (required)
    pub fn agreement_number(mut self, value: String) -> Self {
        self.agreement_number = Some(value);
        self
    }

    /// Set the agreement_kind field (default: `AgreementKind::default()`)
    pub fn agreement_kind(mut self, value: AgreementKind) -> Self {
        self.agreement_kind = Some(value);
        self
    }

    /// Set the status field (default: `PurchaseAgreementStatus::default()`)
    pub fn status(mut self, value: PurchaseAgreementStatus) -> Self {
        self.status = Some(value);
        self
    }

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

    /// Set the currency field (default: `"IDR".to_string()`)
    pub fn currency(mut self, value: String) -> Self {
        self.currency = Some(value);
        self
    }

    /// Set the date_start field (optional)
    pub fn date_start(mut self, value: NaiveDate) -> Self {
        self.date_start = Some(value);
        self
    }

    /// Set the date_end field (optional)
    pub fn date_end(mut self, value: NaiveDate) -> Self {
        self.date_end = Some(value);
        self
    }

    /// Set the notes field (optional)
    pub fn notes(mut self, value: String) -> Self {
        self.notes = Some(value);
        self
    }

    /// Build the PurchaseAgreement entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<PurchaseAgreement, String> {
        let agreement_number = self.agreement_number.ok_or_else(|| "agreement_number is required".to_string())?;
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let supplier_id = self.supplier_id.ok_or_else(|| "supplier_id is required".to_string())?;

        Ok(PurchaseAgreement {
            id: Uuid::new_v4(),
            agreement_number,
            agreement_kind: self.agreement_kind.unwrap_or_default(),
            status: self.status.unwrap_or_default(),
            company_id,
            supplier_id,
            currency: self.currency.unwrap_or("IDR".to_string()),
            date_start: self.date_start,
            date_end: self.date_end,
            notes: self.notes,
            metadata: AuditMetadata::default(),
        })
    }
}
