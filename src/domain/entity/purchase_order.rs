use chrono::{DateTime, Utc, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;

use super::OrderKind;
use super::PurchaseOrderStatus;
use super::AuditMetadata;

/// Strongly-typed ID for PurchaseOrder
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PurchaseOrderId(pub Uuid);

impl PurchaseOrderId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for PurchaseOrderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for PurchaseOrderId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for PurchaseOrderId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<PurchaseOrderId> for Uuid {
    fn from(id: PurchaseOrderId) -> Self { id.0 }
}

impl AsRef<Uuid> for PurchaseOrderId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for PurchaseOrderId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PurchaseOrder {
    pub id: Uuid,
    pub po_number: String,
    pub supplier_quotation_id: Option<Uuid>,
    pub order_kind: OrderKind,
    pub company_id: Uuid,
    pub branch_id: Option<Uuid>,
    pub supplier_id: Uuid,
    pub status: PurchaseOrderStatus,
    pub order_date: NaiveDate,
    pub schedule_date: Option<NaiveDate>,
    pub currency: String,
    pub subtotal: Decimal,
    pub tax_rate: Decimal,
    pub tax_amount: Decimal,
    pub total: Decimal,
    pub notes: Option<String>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl PurchaseOrder {
    /// Create a builder for PurchaseOrder
    pub fn builder() -> PurchaseOrderBuilder {
        PurchaseOrderBuilder::default()
    }

    /// Create a new PurchaseOrder with required fields
    pub fn new(po_number: String, order_kind: OrderKind, company_id: Uuid, supplier_id: Uuid, status: PurchaseOrderStatus, order_date: NaiveDate, currency: String, subtotal: Decimal, tax_rate: Decimal, tax_amount: Decimal, total: Decimal) -> Self {
        Self {
            id: Uuid::new_v4(),
            po_number,
            supplier_quotation_id: None,
            order_kind,
            company_id,
            branch_id: None,
            supplier_id,
            status,
            order_date,
            schedule_date: None,
            currency,
            subtotal,
            tax_rate,
            tax_amount,
            total,
            notes: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> PurchaseOrderId {
        PurchaseOrderId(self.id)
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
    pub fn status(&self) -> &PurchaseOrderStatus {
        &self.status
    }


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the supplier_quotation_id field (chainable)
    pub fn with_supplier_quotation_id(mut self, value: Uuid) -> Self {
        self.supplier_quotation_id = Some(value);
        self
    }

    /// Set the branch_id field (chainable)
    pub fn with_branch_id(mut self, value: Uuid) -> Self {
        self.branch_id = Some(value);
        self
    }

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
                "po_number" => {
                    if let Ok(v) = serde_json::from_value(value) { self.po_number = v; }
                }
                "supplier_quotation_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.supplier_quotation_id = v; }
                }
                "order_kind" => {
                    if let Ok(v) = serde_json::from_value(value) { self.order_kind = v; }
                }
                "company_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.company_id = v; }
                }
                "branch_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.branch_id = v; }
                }
                "supplier_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.supplier_id = v; }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) { self.status = v; }
                }
                "order_date" => {
                    if let Ok(v) = serde_json::from_value(value) { self.order_date = v; }
                }
                "schedule_date" => {
                    if let Ok(v) = serde_json::from_value(value) { self.schedule_date = v; }
                }
                "currency" => {
                    if let Ok(v) = serde_json::from_value(value) { self.currency = v; }
                }
                "subtotal" => {
                    if let Ok(v) = serde_json::from_value(value) { self.subtotal = v; }
                }
                "tax_rate" => {
                    if let Ok(v) = serde_json::from_value(value) { self.tax_rate = v; }
                }
                "tax_amount" => {
                    if let Ok(v) = serde_json::from_value(value) { self.tax_amount = v; }
                }
                "total" => {
                    if let Ok(v) = serde_json::from_value(value) { self.total = v; }
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

impl super::Entity for PurchaseOrder {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "PurchaseOrder"
    }
}

impl backbone_core::PersistentEntity for PurchaseOrder {
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

impl backbone_orm::EntityRepoMeta for PurchaseOrder {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("supplier_quotation_id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("branch_id".to_string(), "uuid".to_string());
        m.insert("supplier_id".to_string(), "uuid".to_string());
        m.insert("order_kind".to_string(), "order_kind".to_string());
        m.insert("status".to_string(), "purchase_order_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["po_number", "currency"]
    }
}

/// Builder for PurchaseOrder entity
///
/// Provides a fluent API for constructing PurchaseOrder instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct PurchaseOrderBuilder {
    po_number: Option<String>,
    supplier_quotation_id: Option<Uuid>,
    order_kind: Option<OrderKind>,
    company_id: Option<Uuid>,
    branch_id: Option<Uuid>,
    supplier_id: Option<Uuid>,
    status: Option<PurchaseOrderStatus>,
    order_date: Option<NaiveDate>,
    schedule_date: Option<NaiveDate>,
    currency: Option<String>,
    subtotal: Option<Decimal>,
    tax_rate: Option<Decimal>,
    tax_amount: Option<Decimal>,
    total: Option<Decimal>,
    notes: Option<String>,
}

impl PurchaseOrderBuilder {
    /// Set the po_number field (required)
    pub fn po_number(mut self, value: String) -> Self {
        self.po_number = Some(value);
        self
    }

    /// Set the supplier_quotation_id field (optional)
    pub fn supplier_quotation_id(mut self, value: Uuid) -> Self {
        self.supplier_quotation_id = Some(value);
        self
    }

    /// Set the order_kind field (default: `OrderKind::default()`)
    pub fn order_kind(mut self, value: OrderKind) -> Self {
        self.order_kind = Some(value);
        self
    }

    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the branch_id field (optional)
    pub fn branch_id(mut self, value: Uuid) -> Self {
        self.branch_id = Some(value);
        self
    }

    /// Set the supplier_id field (required)
    pub fn supplier_id(mut self, value: Uuid) -> Self {
        self.supplier_id = Some(value);
        self
    }

    /// Set the status field (default: `PurchaseOrderStatus::default()`)
    pub fn status(mut self, value: PurchaseOrderStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Set the order_date field (required)
    pub fn order_date(mut self, value: NaiveDate) -> Self {
        self.order_date = Some(value);
        self
    }

    /// Set the schedule_date field (optional)
    pub fn schedule_date(mut self, value: NaiveDate) -> Self {
        self.schedule_date = Some(value);
        self
    }

    /// Set the currency field (default: `"IDR".to_string()`)
    pub fn currency(mut self, value: String) -> Self {
        self.currency = Some(value);
        self
    }

    /// Set the subtotal field (default: `Decimal::from(0)`)
    pub fn subtotal(mut self, value: Decimal) -> Self {
        self.subtotal = Some(value);
        self
    }

    /// Set the tax_rate field (default: `Decimal::from(0)`)
    pub fn tax_rate(mut self, value: Decimal) -> Self {
        self.tax_rate = Some(value);
        self
    }

    /// Set the tax_amount field (default: `Decimal::from(0)`)
    pub fn tax_amount(mut self, value: Decimal) -> Self {
        self.tax_amount = Some(value);
        self
    }

    /// Set the total field (default: `Decimal::from(0)`)
    pub fn total(mut self, value: Decimal) -> Self {
        self.total = Some(value);
        self
    }

    /// Set the notes field (optional)
    pub fn notes(mut self, value: String) -> Self {
        self.notes = Some(value);
        self
    }

    /// Build the PurchaseOrder entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<PurchaseOrder, String> {
        let po_number = self.po_number.ok_or_else(|| "po_number is required".to_string())?;
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let supplier_id = self.supplier_id.ok_or_else(|| "supplier_id is required".to_string())?;
        let order_date = self.order_date.ok_or_else(|| "order_date is required".to_string())?;

        Ok(PurchaseOrder {
            id: Uuid::new_v4(),
            po_number,
            supplier_quotation_id: self.supplier_quotation_id,
            order_kind: self.order_kind.unwrap_or(OrderKind::default()),
            company_id,
            branch_id: self.branch_id,
            supplier_id,
            status: self.status.unwrap_or(PurchaseOrderStatus::default()),
            order_date,
            schedule_date: self.schedule_date,
            currency: self.currency.unwrap_or("IDR".to_string()),
            subtotal: self.subtotal.unwrap_or(Decimal::from(0)),
            tax_rate: self.tax_rate.unwrap_or(Decimal::from(0)),
            tax_amount: self.tax_amount.unwrap_or(Decimal::from(0)),
            total: self.total.unwrap_or(Decimal::from(0)),
            notes: self.notes,
            metadata: AuditMetadata::default(),
        })
    }
}
