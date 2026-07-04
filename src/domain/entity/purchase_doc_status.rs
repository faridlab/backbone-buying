use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "purchase_doc_status", rename_all = "snake_case")]
pub enum PurchaseDocStatus {
    Draft,
    Submitted,
    Ordered,
    Cancelled,
}

impl std::fmt::Display for PurchaseDocStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Draft => write!(f, "draft"),
            Self::Submitted => write!(f, "submitted"),
            Self::Ordered => write!(f, "ordered"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl FromStr for PurchaseDocStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "draft" => Ok(Self::Draft),
            "submitted" => Ok(Self::Submitted),
            "ordered" => Ok(Self::Ordered),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!("Unknown PurchaseDocStatus variant: {}", s)),
        }
    }
}

impl Default for PurchaseDocStatus {
    fn default() -> Self {
        Self::Draft
    }
}
