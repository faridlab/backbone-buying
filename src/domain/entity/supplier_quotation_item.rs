use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;
use super::AuditMetadata;

/// Strongly-typed ID for SupplierQuotationItem
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SupplierQuotationItemId(pub Uuid);

impl SupplierQuotationItemId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for SupplierQuotationItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for SupplierQuotationItemId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for SupplierQuotationItemId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<SupplierQuotationItemId> for Uuid {
    fn from(id: SupplierQuotationItemId) -> Self { id.0 }
}

impl AsRef<Uuid> for SupplierQuotationItemId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for SupplierQuotationItemId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SupplierQuotationItem {
    pub id: Uuid,
    pub quotation_id: Uuid,
    pub item_id: Uuid,
    pub quantity: Decimal,
    pub rate: Decimal,
    pub lead_time_days: Option<i32>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl SupplierQuotationItem {
    /// Create a builder for SupplierQuotationItem
    pub fn builder() -> SupplierQuotationItemBuilder {
        SupplierQuotationItemBuilder::default()
    }

    /// Create a new SupplierQuotationItem with required fields
    pub fn new(quotation_id: Uuid, item_id: Uuid, quantity: Decimal, rate: Decimal) -> Self {
        Self {
            id: Uuid::new_v4(),
            quotation_id,
            item_id,
            quantity,
            rate,
            lead_time_days: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> SupplierQuotationItemId {
        SupplierQuotationItemId(self.id)
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
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the lead_time_days field (chainable)
    pub fn with_lead_time_days(mut self, value: i32) -> Self {
        self.lead_time_days = Some(value);
        self
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "quotation_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.quotation_id = v; }
                }
                "item_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.item_id = v; }
                }
                "quantity" => {
                    if let Ok(v) = serde_json::from_value(value) { self.quantity = v; }
                }
                "rate" => {
                    if let Ok(v) = serde_json::from_value(value) { self.rate = v; }
                }
                "lead_time_days" => {
                    if let Ok(v) = serde_json::from_value(value) { self.lead_time_days = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for SupplierQuotationItem {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "SupplierQuotationItem"
    }
}

impl backbone_core::PersistentEntity for SupplierQuotationItem {
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

impl backbone_orm::EntityRepoMeta for SupplierQuotationItem {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("quotation_id".to_string(), "uuid".to_string());
        m.insert("item_id".to_string(), "uuid".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
    fn relations() -> &'static [(&'static str, &'static str, &'static str)] {
        &[("quotation", "supplier_quotations", "quotationId")]
    }
}

/// Builder for SupplierQuotationItem entity
///
/// Provides a fluent API for constructing SupplierQuotationItem instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct SupplierQuotationItemBuilder {
    quotation_id: Option<Uuid>,
    item_id: Option<Uuid>,
    quantity: Option<Decimal>,
    rate: Option<Decimal>,
    lead_time_days: Option<i32>,
}

impl SupplierQuotationItemBuilder {
    /// Set the quotation_id field (required)
    pub fn quotation_id(mut self, value: Uuid) -> Self {
        self.quotation_id = Some(value);
        self
    }

    /// Set the item_id field (required)
    pub fn item_id(mut self, value: Uuid) -> Self {
        self.item_id = Some(value);
        self
    }

    /// Set the quantity field (required)
    pub fn quantity(mut self, value: Decimal) -> Self {
        self.quantity = Some(value);
        self
    }

    /// Set the rate field (required)
    pub fn rate(mut self, value: Decimal) -> Self {
        self.rate = Some(value);
        self
    }

    /// Set the lead_time_days field (optional)
    pub fn lead_time_days(mut self, value: i32) -> Self {
        self.lead_time_days = Some(value);
        self
    }

    /// Build the SupplierQuotationItem entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<SupplierQuotationItem, String> {
        let quotation_id = self.quotation_id.ok_or_else(|| "quotation_id is required".to_string())?;
        let item_id = self.item_id.ok_or_else(|| "item_id is required".to_string())?;
        let quantity = self.quantity.ok_or_else(|| "quantity is required".to_string())?;
        let rate = self.rate.ok_or_else(|| "rate is required".to_string())?;

        Ok(SupplierQuotationItem {
            id: Uuid::new_v4(),
            quotation_id,
            item_id,
            quantity,
            rate,
            lead_time_days: self.lead_time_days,
            metadata: AuditMetadata::default(),
        })
    }
}
