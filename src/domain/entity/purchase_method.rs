use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "purchase_method", rename_all = "snake_case")]
pub enum PurchaseMethod {
    OnReceived,
    Purchase,
}

impl std::fmt::Display for PurchaseMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OnReceived => write!(f, "on_received"),
            Self::Purchase => write!(f, "purchase"),
        }
    }
}

impl FromStr for PurchaseMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "on_received" => Ok(Self::OnReceived),
            "purchase" => Ok(Self::Purchase),
            _ => Err(format!("Unknown PurchaseMethod variant: {}", s)),
        }
    }
}

impl Default for PurchaseMethod {
    fn default() -> Self {
        Self::OnReceived
    }
}
