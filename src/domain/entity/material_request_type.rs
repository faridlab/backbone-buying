use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "material_request_type", rename_all = "snake_case")]
pub enum MaterialRequestType {
    Purchase,
    Transfer,
    Subcontract,
}

impl std::fmt::Display for MaterialRequestType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Purchase => write!(f, "purchase"),
            Self::Transfer => write!(f, "transfer"),
            Self::Subcontract => write!(f, "subcontract"),
        }
    }
}

impl FromStr for MaterialRequestType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "purchase" => Ok(Self::Purchase),
            "transfer" => Ok(Self::Transfer),
            "subcontract" => Ok(Self::Subcontract),
            _ => Err(format!("Unknown MaterialRequestType variant: {}", s)),
        }
    }
}

impl Default for MaterialRequestType {
    fn default() -> Self {
        Self::Purchase
    }
}
