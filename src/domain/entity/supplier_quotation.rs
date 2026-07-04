use chrono::{DateTime, Utc, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::PurchaseDocStatus;
use super::AuditMetadata;

/// Strongly-typed ID for SupplierQuotation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SupplierQuotationId(pub Uuid);

impl SupplierQuotationId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for SupplierQuotationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for SupplierQuotationId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for SupplierQuotationId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<SupplierQuotationId> for Uuid {
    fn from(id: SupplierQuotationId) -> Self { id.0 }
}

impl AsRef<Uuid> for SupplierQuotationId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for SupplierQuotationId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SupplierQuotation {
    pub id: Uuid,
    pub quotation_number: String,
    pub rfq_id: Option<Uuid>,
    pub company_id: Uuid,
    pub supplier_id: Uuid,
    pub status: PurchaseDocStatus,
    pub quotation_date: NaiveDate,
    pub valid_till: Option<NaiveDate>,
    pub currency: String,
    pub notes: Option<String>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl SupplierQuotation {
    /// Create a builder for SupplierQuotation
    pub fn builder() -> SupplierQuotationBuilder {
        SupplierQuotationBuilder::default()
    }

    /// Create a new SupplierQuotation with required fields
    pub fn new(quotation_number: String, company_id: Uuid, supplier_id: Uuid, status: PurchaseDocStatus, quotation_date: NaiveDate, currency: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            quotation_number,
            rfq_id: None,
            company_id,
            supplier_id,
            status,
            quotation_date,
            valid_till: None,
            currency,
            notes: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> SupplierQuotationId {
        SupplierQuotationId(self.id)
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

    /// Set the rfq_id field (chainable)
    pub fn with_rfq_id(mut self, value: Uuid) -> Self {
        self.rfq_id = Some(value);
        self
    }

    /// Set the valid_till field (chainable)
    pub fn with_valid_till(mut self, value: NaiveDate) -> Self {
        self.valid_till = Some(value);
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
                "quotation_number" => {
                    if let Ok(v) = serde_json::from_value(value) { self.quotation_number = v; }
                }
                "rfq_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.rfq_id = v; }
                }
                "company_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.company_id = v; }
                }
                "supplier_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.supplier_id = v; }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) { self.status = v; }
                }
                "quotation_date" => {
                    if let Ok(v) = serde_json::from_value(value) { self.quotation_date = v; }
                }
                "valid_till" => {
                    if let Ok(v) = serde_json::from_value(value) { self.valid_till = v; }
                }
                "currency" => {
                    if let Ok(v) = serde_json::from_value(value) { self.currency = v; }
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

impl super::Entity for SupplierQuotation {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "SupplierQuotation"
    }
}

impl backbone_core::PersistentEntity for SupplierQuotation {
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

impl backbone_orm::EntityRepoMeta for SupplierQuotation {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("rfq_id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("supplier_id".to_string(), "uuid".to_string());
        m.insert("status".to_string(), "purchase_doc_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["quotation_number", "currency"]
    }
}

/// Builder for SupplierQuotation entity
///
/// Provides a fluent API for constructing SupplierQuotation instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct SupplierQuotationBuilder {
    quotation_number: Option<String>,
    rfq_id: Option<Uuid>,
    company_id: Option<Uuid>,
    supplier_id: Option<Uuid>,
    status: Option<PurchaseDocStatus>,
    quotation_date: Option<NaiveDate>,
    valid_till: Option<NaiveDate>,
    currency: Option<String>,
    notes: Option<String>,
}

impl SupplierQuotationBuilder {
    /// Set the quotation_number field (required)
    pub fn quotation_number(mut self, value: String) -> Self {
        self.quotation_number = Some(value);
        self
    }

    /// Set the rfq_id field (optional)
    pub fn rfq_id(mut self, value: Uuid) -> Self {
        self.rfq_id = Some(value);
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

    /// Set the status field (default: `PurchaseDocStatus::default()`)
    pub fn status(mut self, value: PurchaseDocStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Set the quotation_date field (required)
    pub fn quotation_date(mut self, value: NaiveDate) -> Self {
        self.quotation_date = Some(value);
        self
    }

    /// Set the valid_till field (optional)
    pub fn valid_till(mut self, value: NaiveDate) -> Self {
        self.valid_till = Some(value);
        self
    }

    /// Set the currency field (default: `"IDR".to_string()`)
    pub fn currency(mut self, value: String) -> Self {
        self.currency = Some(value);
        self
    }

    /// Set the notes field (optional)
    pub fn notes(mut self, value: String) -> Self {
        self.notes = Some(value);
        self
    }

    /// Build the SupplierQuotation entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<SupplierQuotation, String> {
        let quotation_number = self.quotation_number.ok_or_else(|| "quotation_number is required".to_string())?;
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let supplier_id = self.supplier_id.ok_or_else(|| "supplier_id is required".to_string())?;
        let quotation_date = self.quotation_date.ok_or_else(|| "quotation_date is required".to_string())?;

        Ok(SupplierQuotation {
            id: Uuid::new_v4(),
            quotation_number,
            rfq_id: self.rfq_id,
            company_id,
            supplier_id,
            status: self.status.unwrap_or(PurchaseDocStatus::default()),
            quotation_date,
            valid_till: self.valid_till,
            currency: self.currency.unwrap_or("IDR".to_string()),
            notes: self.notes,
            metadata: AuditMetadata::default(),
        })
    }
}
