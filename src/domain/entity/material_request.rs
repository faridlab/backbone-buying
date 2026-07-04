use chrono::{DateTime, Utc, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::MaterialRequestType;
use super::PurchaseDocStatus;
use super::AuditMetadata;

/// Strongly-typed ID for MaterialRequest
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MaterialRequestId(pub Uuid);

impl MaterialRequestId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for MaterialRequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for MaterialRequestId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for MaterialRequestId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<MaterialRequestId> for Uuid {
    fn from(id: MaterialRequestId) -> Self { id.0 }
}

impl AsRef<Uuid> for MaterialRequestId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for MaterialRequestId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MaterialRequest {
    pub id: Uuid,
    pub request_number: String,
    pub company_id: Uuid,
    pub request_type: MaterialRequestType,
    pub status: PurchaseDocStatus,
    pub request_date: NaiveDate,
    pub schedule_date: Option<NaiveDate>,
    pub notes: Option<String>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl MaterialRequest {
    /// Create a builder for MaterialRequest
    pub fn builder() -> MaterialRequestBuilder {
        MaterialRequestBuilder::default()
    }

    /// Create a new MaterialRequest with required fields
    pub fn new(request_number: String, company_id: Uuid, request_type: MaterialRequestType, status: PurchaseDocStatus, request_date: NaiveDate) -> Self {
        Self {
            id: Uuid::new_v4(),
            request_number,
            company_id,
            request_type,
            status,
            request_date,
            schedule_date: None,
            notes: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> MaterialRequestId {
        MaterialRequestId(self.id)
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
    pub fn status(&self) -> &PurchaseDocStatus {
        &self.status
    }


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the schedule_date field (chainable)
    pub fn with_schedule_date(mut self, value: NaiveDate) -> Self {
        self.schedule_date = Some(value);
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
                "request_number" => {
                    if let Ok(v) = serde_json::from_value(value) { self.request_number = v; }
                }
                "company_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.company_id = v; }
                }
                "request_type" => {
                    if let Ok(v) = serde_json::from_value(value) { self.request_type = v; }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) { self.status = v; }
                }
                "request_date" => {
                    if let Ok(v) = serde_json::from_value(value) { self.request_date = v; }
                }
                "schedule_date" => {
                    if let Ok(v) = serde_json::from_value(value) { self.schedule_date = v; }
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

impl super::Entity for MaterialRequest {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "MaterialRequest"
    }
}

impl backbone_core::PersistentEntity for MaterialRequest {
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

impl backbone_orm::EntityRepoMeta for MaterialRequest {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("request_type".to_string(), "material_request_type".to_string());
        m.insert("status".to_string(), "purchase_doc_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["request_number"]
    }
}

/// Builder for MaterialRequest entity
///
/// Provides a fluent API for constructing MaterialRequest instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct MaterialRequestBuilder {
    request_number: Option<String>,
    company_id: Option<Uuid>,
    request_type: Option<MaterialRequestType>,
    status: Option<PurchaseDocStatus>,
    request_date: Option<NaiveDate>,
    schedule_date: Option<NaiveDate>,
    notes: Option<String>,
}

impl MaterialRequestBuilder {
    /// Set the request_number field (required)
    pub fn request_number(mut self, value: String) -> Self {
        self.request_number = Some(value);
        self
    }

    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the request_type field (default: `MaterialRequestType::default()`)
    pub fn request_type(mut self, value: MaterialRequestType) -> Self {
        self.request_type = Some(value);
        self
    }

    /// Set the status field (default: `PurchaseDocStatus::default()`)
    pub fn status(mut self, value: PurchaseDocStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Set the request_date field (required)
    pub fn request_date(mut self, value: NaiveDate) -> Self {
        self.request_date = Some(value);
        self
    }

    /// Set the schedule_date field (optional)
    pub fn schedule_date(mut self, value: NaiveDate) -> Self {
        self.schedule_date = Some(value);
        self
    }

    /// Set the notes field (optional)
    pub fn notes(mut self, value: String) -> Self {
        self.notes = Some(value);
        self
    }

    /// Build the MaterialRequest entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<MaterialRequest, String> {
        let request_number = self.request_number.ok_or_else(|| "request_number is required".to_string())?;
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let request_date = self.request_date.ok_or_else(|| "request_date is required".to_string())?;

        Ok(MaterialRequest {
            id: Uuid::new_v4(),
            request_number,
            company_id,
            request_type: self.request_type.unwrap_or(MaterialRequestType::default()),
            status: self.status.unwrap_or(PurchaseDocStatus::default()),
            request_date,
            schedule_date: self.schedule_date,
            notes: self.notes,
            metadata: AuditMetadata::default(),
        })
    }
}
