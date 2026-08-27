use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "purchase_receipt_status", rename_all = "snake_case")]
pub enum PurchaseReceiptStatus {
    Pending,
    Partial,
    Full,
}

impl std::fmt::Display for PurchaseReceiptStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Partial => write!(f, "partial"),
            Self::Full => write!(f, "full"),
        }
    }
}

impl FromStr for PurchaseReceiptStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pending" => Ok(Self::Pending),
            "partial" => Ok(Self::Partial),
            "full" => Ok(Self::Full),
            _ => Err(format!("Unknown PurchaseReceiptStatus variant: {}", s)),
        }
    }
}

impl Default for PurchaseReceiptStatus {
    fn default() -> Self {
        Self::Pending
    }
}
