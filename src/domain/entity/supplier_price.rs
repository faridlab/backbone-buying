use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;
use super::AuditMetadata;

/// Strongly-typed ID for SupplierPrice
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SupplierPriceId(pub Uuid);

impl SupplierPriceId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for SupplierPriceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for SupplierPriceId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for SupplierPriceId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<SupplierPriceId> for Uuid {
    fn from(id: SupplierPriceId) -> Self { id.0 }
}

impl AsRef<Uuid> for SupplierPriceId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for SupplierPriceId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SupplierPrice {
    pub id: Uuid,
    pub company_id: Uuid,
    pub supplier_id: Uuid,
    pub item_id: Uuid,
    pub price: Decimal,
    pub currency: String,
    pub agreement_id: Uuid,
    pub agreement_line_id: Uuid,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl SupplierPrice {
    /// Create a builder for SupplierPrice
    pub fn builder() -> SupplierPriceBuilder {
        <SupplierPriceBuilder as Default>::default()
    }

    /// Create a new SupplierPrice with required fields
    pub fn new(company_id: Uuid, supplier_id: Uuid, item_id: Uuid, price: Decimal, currency: String, agreement_id: Uuid, agreement_line_id: Uuid) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            supplier_id,
            item_id,
            price,
            currency,
            agreement_id,
            agreement_line_id,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> SupplierPriceId {
        SupplierPriceId(self.id)
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
                "item_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.item_id = v; }
                }
                "price" => {
                    if let Ok(v) = serde_json::from_value(value) { self.price = v; }
                }
                "currency" => {
                    if let Ok(v) = serde_json::from_value(value) { self.currency = v; }
                }
                "agreement_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.agreement_id = v; }
                }
                "agreement_line_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.agreement_line_id = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for SupplierPrice {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "SupplierPrice"
    }
}

impl backbone_core::PersistentEntity for SupplierPrice {
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

impl backbone_orm::EntityRepoMeta for SupplierPrice {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("supplier_id".to_string(), "uuid".to_string());
        m.insert("item_id".to_string(), "uuid".to_string());
        m.insert("agreement_id".to_string(), "uuid".to_string());
        m.insert("agreement_line_id".to_string(), "uuid".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["currency"]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for SupplierPrice entity
///
/// Provides a fluent API for constructing SupplierPrice instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct SupplierPriceBuilder {
    company_id: Option<Uuid>,
    supplier_id: Option<Uuid>,
    item_id: Option<Uuid>,
    price: Option<Decimal>,
    currency: Option<String>,
    agreement_id: Option<Uuid>,
    agreement_line_id: Option<Uuid>,
}

impl SupplierPriceBuilder {
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

    /// Set the item_id field (required)
    pub fn item_id(mut self, value: Uuid) -> Self {
        self.item_id = Some(value);
        self
    }

    /// Set the price field (required)
    pub fn price(mut self, value: Decimal) -> Self {
        self.price = Some(value);
        self
    }

    /// Set the currency field (default: `"IDR".to_string()`)
    pub fn currency(mut self, value: String) -> Self {
        self.currency = Some(value);
        self
    }

    /// Set the agreement_id field (required)
    pub fn agreement_id(mut self, value: Uuid) -> Self {
        self.agreement_id = Some(value);
        self
    }

    /// Set the agreement_line_id field (required)
    pub fn agreement_line_id(mut self, value: Uuid) -> Self {
        self.agreement_line_id = Some(value);
        self
    }

    /// Build the SupplierPrice entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<SupplierPrice, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let supplier_id = self.supplier_id.ok_or_else(|| "supplier_id is required".to_string())?;
        let item_id = self.item_id.ok_or_else(|| "item_id is required".to_string())?;
        let price = self.price.ok_or_else(|| "price is required".to_string())?;
        let agreement_id = self.agreement_id.ok_or_else(|| "agreement_id is required".to_string())?;
        let agreement_line_id = self.agreement_line_id.ok_or_else(|| "agreement_line_id is required".to_string())?;

        Ok(SupplierPrice {
            id: Uuid::new_v4(),
            company_id,
            supplier_id,
            item_id,
            price,
            currency: self.currency.unwrap_or("IDR".to_string()),
            agreement_id,
            agreement_line_id,
            metadata: AuditMetadata::default(),
        })
    }
}
