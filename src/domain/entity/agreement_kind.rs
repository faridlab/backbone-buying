use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "agreement_kind", rename_all = "snake_case")]
pub enum AgreementKind {
    Blanket,
}

impl std::fmt::Display for AgreementKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Blanket => write!(f, "blanket"),
        }
    }
}

impl FromStr for AgreementKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "blanket" => Ok(Self::Blanket),
            _ => Err(format!("Unknown AgreementKind variant: {}", s)),
        }
    }
}

impl Default for AgreementKind {
    fn default() -> Self {
        Self::Blanket
    }
}
