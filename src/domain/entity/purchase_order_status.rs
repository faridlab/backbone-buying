use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "purchase_order_status", rename_all = "snake_case")]
pub enum PurchaseOrderStatus {
    Draft,
    Sent,
    ToApprove,
    Purchase,
    Cancelled,
}

impl std::fmt::Display for PurchaseOrderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Draft => write!(f, "draft"),
            Self::Sent => write!(f, "sent"),
            Self::ToApprove => write!(f, "to_approve"),
            Self::Purchase => write!(f, "purchase"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl FromStr for PurchaseOrderStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "draft" => Ok(Self::Draft),
            "sent" => Ok(Self::Sent),
            "to_approve" => Ok(Self::ToApprove),
            "purchase" => Ok(Self::Purchase),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!("Unknown PurchaseOrderStatus variant: {}", s)),
        }
    }
}

impl Default for PurchaseOrderStatus {
    fn default() -> Self {
        Self::Draft
    }
}
