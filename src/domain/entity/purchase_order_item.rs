use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;
use super::AuditMetadata;

/// Strongly-typed ID for PurchaseOrderItem
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PurchaseOrderItemId(pub Uuid);

impl PurchaseOrderItemId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for PurchaseOrderItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for PurchaseOrderItemId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for PurchaseOrderItemId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<PurchaseOrderItemId> for Uuid {
    fn from(id: PurchaseOrderItemId) -> Self { id.0 }
}

impl AsRef<Uuid> for PurchaseOrderItemId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for PurchaseOrderItemId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PurchaseOrderItem {
    pub id: Uuid,
    pub order_id: Uuid,
    pub company_id: Uuid,
    pub item_id: Uuid,
    pub warehouse_id: Option<Uuid>,
    pub description: Option<String>,
    pub quantity: Decimal,
    pub rate: Decimal,
    pub line_amount: Decimal,
    pub received_qty: Decimal,
    pub billed_qty: Decimal,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl PurchaseOrderItem {
    /// Create a builder for PurchaseOrderItem
    pub fn builder() -> PurchaseOrderItemBuilder {
        PurchaseOrderItemBuilder::default()
    }

    /// Create a new PurchaseOrderItem with required fields
    pub fn new(order_id: Uuid, company_id: Uuid, item_id: Uuid, quantity: Decimal, rate: Decimal, line_amount: Decimal, received_qty: Decimal, billed_qty: Decimal) -> Self {
        Self {
            id: Uuid::new_v4(),
            order_id,
            company_id,
            item_id,
            warehouse_id: None,
            description: None,
            quantity,
            rate,
            line_amount,
            received_qty,
            billed_qty,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> PurchaseOrderItemId {
        PurchaseOrderItemId(self.id)
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

    /// Set the warehouse_id field (chainable)
    pub fn with_warehouse_id(mut self, value: Uuid) -> Self {
        self.warehouse_id = Some(value);
        self
    }

    /// Set the description field (chainable)
    pub fn with_description(mut self, value: String) -> Self {
        self.description = Some(value);
        self
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "order_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.order_id = v; }
                }
                "company_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.company_id = v; }
                }
                "item_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.item_id = v; }
                }
                "warehouse_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.warehouse_id = v; }
                }
                "description" => {
                    if let Ok(v) = serde_json::from_value(value) { self.description = v; }
                }
                "quantity" => {
                    if let Ok(v) = serde_json::from_value(value) { self.quantity = v; }
                }
                "rate" => {
                    if let Ok(v) = serde_json::from_value(value) { self.rate = v; }
                }
                "line_amount" => {
                    if let Ok(v) = serde_json::from_value(value) { self.line_amount = v; }
                }
                "received_qty" => {
                    if let Ok(v) = serde_json::from_value(value) { self.received_qty = v; }
                }
                "billed_qty" => {
                    if let Ok(v) = serde_json::from_value(value) { self.billed_qty = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for PurchaseOrderItem {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "PurchaseOrderItem"
    }
}

impl backbone_core::PersistentEntity for PurchaseOrderItem {
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

impl backbone_orm::EntityRepoMeta for PurchaseOrderItem {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("order_id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("item_id".to_string(), "uuid".to_string());
        m.insert("warehouse_id".to_string(), "uuid".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
    fn relations() -> &'static [(&'static str, &'static str, &'static str)] {
        &[("order", "purchase_orders", "orderId")]
    }
}

/// Builder for PurchaseOrderItem entity
///
/// Provides a fluent API for constructing PurchaseOrderItem instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct PurchaseOrderItemBuilder {
    order_id: Option<Uuid>,
    company_id: Option<Uuid>,
    item_id: Option<Uuid>,
    warehouse_id: Option<Uuid>,
    description: Option<String>,
    quantity: Option<Decimal>,
    rate: Option<Decimal>,
    line_amount: Option<Decimal>,
    received_qty: Option<Decimal>,
    billed_qty: Option<Decimal>,
}

impl PurchaseOrderItemBuilder {
    /// Set the order_id field (required)
    pub fn order_id(mut self, value: Uuid) -> Self {
        self.order_id = Some(value);
        self
    }

    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the item_id field (required)
    pub fn item_id(mut self, value: Uuid) -> Self {
        self.item_id = Some(value);
        self
    }

    /// Set the warehouse_id field (optional)
    pub fn warehouse_id(mut self, value: Uuid) -> Self {
        self.warehouse_id = Some(value);
        self
    }

    /// Set the description field (optional)
    pub fn description(mut self, value: String) -> Self {
        self.description = Some(value);
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

    /// Set the line_amount field (default: `Decimal::from(0)`)
    pub fn line_amount(mut self, value: Decimal) -> Self {
        self.line_amount = Some(value);
        self
    }

    /// Set the received_qty field (default: `Decimal::from(0)`)
    pub fn received_qty(mut self, value: Decimal) -> Self {
        self.received_qty = Some(value);
        self
    }

    /// Set the billed_qty field (default: `Decimal::from(0)`)
    pub fn billed_qty(mut self, value: Decimal) -> Self {
        self.billed_qty = Some(value);
        self
    }

    /// Build the PurchaseOrderItem entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<PurchaseOrderItem, String> {
        let order_id = self.order_id.ok_or_else(|| "order_id is required".to_string())?;
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let item_id = self.item_id.ok_or_else(|| "item_id is required".to_string())?;
        let quantity = self.quantity.ok_or_else(|| "quantity is required".to_string())?;
        let rate = self.rate.ok_or_else(|| "rate is required".to_string())?;

        Ok(PurchaseOrderItem {
            id: Uuid::new_v4(),
            order_id,
            company_id,
            item_id,
            warehouse_id: self.warehouse_id,
            description: self.description,
            quantity,
            rate,
            line_amount: self.line_amount.unwrap_or(Decimal::from(0)),
            received_qty: self.received_qty.unwrap_or(Decimal::from(0)),
            billed_qty: self.billed_qty.unwrap_or(Decimal::from(0)),
            metadata: AuditMetadata::default(),
        })
    }
}
