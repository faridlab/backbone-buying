use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "double_validation", rename_all = "snake_case")]
pub enum DoubleValidation {
    OneStep,
    TwoStep,
}

impl std::fmt::Display for DoubleValidation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OneStep => write!(f, "one_step"),
            Self::TwoStep => write!(f, "two_step"),
        }
    }
}

impl FromStr for DoubleValidation {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "one_step" => Ok(Self::OneStep),
            "two_step" => Ok(Self::TwoStep),
            _ => Err(format!("Unknown DoubleValidation variant: {}", s)),
        }
    }
}

impl Default for DoubleValidation {
    fn default() -> Self {
        Self::OneStep
    }
}
