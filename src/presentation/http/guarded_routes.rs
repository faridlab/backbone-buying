//! Guarded route composition — the RECOMMENDED way to mount the buying module.
//!
//! Hand-authored (user-owned). Read documents + **validated create** (material-request /
//! supplier-quotation / purchase-order) + confirm; generic create/update/delete CRUD is NOT
//! mounted, so a caller cannot write a PO with inconsistent totals or bypass the write path.
//! `BuyingWriteService` is built from the pool (regen-safe). The receipt seam
//! (`build_receipt_request`) needs a composition layer, so it is service/job-driven, not an HTTP route.

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::service::buying_write_service::{
    BuyingError, BuyingWriteService, NewLine, NewPurchaseOrder,
};
use crate::BuyingModule;

use super::{
    create_material_request_read_routes, create_purchase_order_read_routes,
    create_supplier_quotation_read_routes,
};

#[derive(Debug, Serialize)]
struct ErrorBody { error: String, message: String }
#[derive(Debug, Serialize)]
struct IdResponse { id: Uuid }
fn err(e: BuyingError) -> axum::response::Response {
    let s = StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (s, Json(ErrorBody { error: e.code(), message: e.to_string() })).into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LineBody {
    item_id: Uuid,
    #[serde(default)] warehouse_id: Option<Uuid>,
    #[serde(default)] description: Option<String>,
    quantity: Decimal,
    rate: Decimal,
}
impl From<LineBody> for NewLine {
    fn from(b: LineBody) -> Self {
        NewLine { item_id: b.item_id, warehouse_id: b.warehouse_id, description: b.description, quantity: b.quantity, rate: b.rate }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreatePoBody {
    po_number: String,
    #[serde(default)] supplier_quotation_id: Option<Uuid>,
    #[serde(default)] order_kind: Option<String>,
    company_id: Uuid,
    #[serde(default)] branch_id: Option<Uuid>,
    supplier_id: Uuid,
    order_date: chrono::NaiveDate,
    #[serde(default)] schedule_date: Option<chrono::NaiveDate>,
    #[serde(default)] currency: Option<String>,
    #[serde(default)] tax_rate: Decimal,
    #[serde(default)] notes: Option<String>,
    lines: Vec<LineBody>,
}
async fn create_po(State(svc): State<Arc<BuyingWriteService>>, Json(b): Json<CreatePoBody>) -> axum::response::Response {
    let o = NewPurchaseOrder {
        po_number: b.po_number, supplier_quotation_id: b.supplier_quotation_id, order_kind: b.order_kind,
        company_id: b.company_id, branch_id: b.branch_id, supplier_id: b.supplier_id, order_date: b.order_date,
        schedule_date: b.schedule_date, currency: b.currency, tax_rate: b.tax_rate, notes: b.notes,
        lines: b.lines.into_iter().map(Into::into).collect(),
    };
    match svc.create_purchase_order(o).await {
        Ok(id) => (StatusCode::CREATED, Json(IdResponse { id })).into_response(),
        Err(e) => err(e),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfirmBody { order_id: Uuid }
async fn confirm_po(State(svc): State<Arc<BuyingWriteService>>, Json(b): Json<ConfirmBody>) -> axum::response::Response {
    match svc.confirm_purchase_order(b.order_id).await {
        Ok(()) => (StatusCode::OK, Json(IdResponse { id: b.order_id })).into_response(),
        Err(e) => err(e),
    }
}

fn write_routes(svc: Arc<BuyingWriteService>) -> Router {
    Router::new()
        .route("/purchase-orders", post(create_po))
        .route("/purchase-orders/confirm", post(confirm_po))
        .with_state(svc)
}

/// Mount the buying module: read documents + validated creates. Generic mutation is not mounted.
/// **Prefer this over `BuyingModule::all_crud_routes()` for any real deployment.**
pub fn create_guarded_buying_routes(m: &BuyingModule, pool: PgPool) -> Router {
    let write = Arc::new(BuyingWriteService::new(pool));
    Router::new()
        .merge(create_material_request_read_routes(m.material_request_service.clone()))
        .merge(create_supplier_quotation_read_routes(m.supplier_quotation_service.clone()))
        .merge(create_purchase_order_read_routes(m.purchase_order_service.clone()))
        .merge(write_routes(write))
}
