use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "order_kind", rename_all = "snake_case")]
pub enum OrderKind {
    Standard,
    Subcontract,
}

impl std::fmt::Display for OrderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Standard => write!(f, "standard"),
            Self::Subcontract => write!(f, "subcontract"),
        }
    }
}

impl FromStr for OrderKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "standard" => Ok(Self::Standard),
            "subcontract" => Ok(Self::Subcontract),
            _ => Err(format!("Unknown OrderKind variant: {}", s)),
        }
    }
}

impl Default for OrderKind {
    fn default() -> Self {
        Self::Standard
    }
}
