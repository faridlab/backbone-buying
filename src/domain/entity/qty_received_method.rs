use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "qty_received_method", rename_all = "snake_case")]
pub enum QtyReceivedMethod {
    StockMoves,
    Manual,
}

impl std::fmt::Display for QtyReceivedMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StockMoves => write!(f, "stock_moves"),
            Self::Manual => write!(f, "manual"),
        }
    }
}

impl FromStr for QtyReceivedMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "stock_moves" => Ok(Self::StockMoves),
            "manual" => Ok(Self::Manual),
            _ => Err(format!("Unknown QtyReceivedMethod variant: {}", s)),
        }
    }
}

impl Default for QtyReceivedMethod {
    fn default() -> Self {
        Self::StockMoves
    }
}
