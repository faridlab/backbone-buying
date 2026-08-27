use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "purchase_invoice_status", rename_all = "snake_case")]
pub enum PurchaseInvoiceStatus {
    No,
    ToInvoice,
    Invoiced,
}

impl std::fmt::Display for PurchaseInvoiceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::No => write!(f, "no"),
            Self::ToInvoice => write!(f, "to_invoice"),
            Self::Invoiced => write!(f, "invoiced"),
        }
    }
}

impl FromStr for PurchaseInvoiceStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "no" => Ok(Self::No),
            "to_invoice" => Ok(Self::ToInvoice),
            "invoiced" => Ok(Self::Invoiced),
            _ => Err(format!("Unknown PurchaseInvoiceStatus variant: {}", s)),
        }
    }
}

impl Default for PurchaseInvoiceStatus {
    fn default() -> Self {
        Self::No
    }
}
