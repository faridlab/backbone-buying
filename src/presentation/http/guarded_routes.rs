//! Guarded route composition — the RECOMMENDED way to mount the buying module.
//!
//! Hand-authored (user-owned). Read documents + **validated creates** (material-request /
//! supplier-quotation / purchase-order / blanket-agreement) + the PO lifecycle verbs + the
//! agreement verbs + manual bill matching + the settings upserts; generic create/update/delete
//! CRUD is NOT mounted, so a caller cannot write a PO with inconsistent totals, mint supplier
//! prices outside the agreement verbs, or bypass the write path. `BuyingWriteService` is built
//! from the pool (regen-safe). The receipt seam (`build_receipt_request`) needs a composition
//! layer, so it is service/job-driven, not an HTTP route.

use std::sync::Arc;

use axum::{
    extract::State, http::StatusCode, middleware::from_fn_with_state, response::IntoResponse,
    routing::post, Json, Router,
};
use backbone_auth::company::{company_auth, CompanyContext, CompanyVerifier};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::service::buying_agreement::{
    NewAgreementLine, NewCallOffOrder, NewPurchaseAgreement,
};
use crate::application::service::buying_bill_matching::BillLineProposal;
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
fn ok_id(id: Uuid) -> axum::response::Response {
    (StatusCode::OK, Json(IdResponse { id })).into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LineBody {
    item_id: Uuid,
    #[serde(default)] warehouse_id: Option<Uuid>,
    #[serde(default)] description: Option<String>,
    quantity: Decimal,
    rate: Decimal,
    /// Receipt tier: `stock_moves` (the receipt seam advances it — default) or `manual`.
    #[serde(default)] qty_received_method: Option<String>,
    /// Billing capacity formula: `on_received` (default) or `purchase`.
    #[serde(default)] purchase_method: Option<String>,
}
impl From<LineBody> for NewLine {
    fn from(b: LineBody) -> Self {
        NewLine {
            item_id: b.item_id, warehouse_id: b.warehouse_id, description: b.description,
            quantity: b.quantity, rate: b.rate,
            qty_received_method: b.qty_received_method, purchase_method: b.purchase_method,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreatePoBody {
    po_number: String,
    #[serde(default)] supplier_quotation_id: Option<Uuid>,
    #[serde(default)] order_kind: Option<String>,
    // No `company_id` / `branch_id`: the tenant is derived from the signed token via
    // `CompanyContext`, never from the request body — a client must not be able to name the tenant
    // it writes into.
    supplier_id: Uuid,
    order_date: chrono::NaiveDate,
    #[serde(default)] schedule_date: Option<chrono::NaiveDate>,
    #[serde(default)] currency: Option<String>,
    /// Order-time rate snapshot (company currency per 1 PO-currency unit). Required whenever the
    /// PO currency differs from the company currency; same-currency POs fix 1.
    #[serde(default)] currency_rate: Option<Decimal>,
    #[serde(default)] tax_rate: Decimal,
    #[serde(default)] notes: Option<String>,
    lines: Vec<LineBody>,
}
async fn create_po(
    State(svc): State<Arc<BuyingWriteService>>,
    tenant: CompanyContext,
    Json(b): Json<CreatePoBody>,
) -> axum::response::Response {
    let o = NewPurchaseOrder {
        po_number: b.po_number, supplier_quotation_id: b.supplier_quotation_id, order_kind: b.order_kind,
        company_id: tenant.company_id, branch_id: tenant.branch_id, supplier_id: b.supplier_id, order_date: b.order_date,
        schedule_date: b.schedule_date, currency: b.currency, currency_rate: b.currency_rate,
        agreement_id: None, tax_rate: b.tax_rate, notes: b.notes,
        lines: b.lines.into_iter().map(Into::into).collect(),
    };
    match svc.create_purchase_order(o).await {
        Ok(id) => (StatusCode::CREATED, Json(IdResponse { id })).into_response(),
        Err(e) => err(e),
    }
}

// ---- PO lifecycle verbs ------------------------------------------------------

/// The confirm/approve body. `isManager` claims the manager leg of the double-validation gate: a
/// non-manager confirm of an over-threshold PO is parked in `to_approve`, and a non-manager
/// approve of one refuses with `not_approvable`. Reference wiring only — a real deployment derives
/// the claim from verified role claims at the composing service (this module has no role model).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfirmBody {
    order_id: Uuid,
    #[serde(default)] is_manager: bool,
}
async fn confirm_po(State(svc): State<Arc<BuyingWriteService>>, Json(b): Json<ConfirmBody>) -> axum::response::Response {
    match svc.confirm_purchase_order(b.order_id, b.is_manager).await {
        Ok(()) => ok_id(b.order_id),
        Err(e) => err(e),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrderBody { order_id: Uuid }
async fn approve_po(State(svc): State<Arc<BuyingWriteService>>, Json(b): Json<ConfirmBody>) -> axum::response::Response {
    match svc.approve_purchase_order(b.order_id, b.is_manager).await { Ok(()) => ok_id(b.order_id), Err(e) => err(e) }
}
async fn reset_po(State(svc): State<Arc<BuyingWriteService>>, Json(b): Json<OrderBody>) -> axum::response::Response {
    match svc.reset_purchase_order(b.order_id).await { Ok(()) => ok_id(b.order_id), Err(e) => err(e) }
}
async fn cancel_po(State(svc): State<Arc<BuyingWriteService>>, Json(b): Json<OrderBody>) -> axum::response::Response {
    match svc.cancel_purchase_order(b.order_id).await { Ok(()) => ok_id(b.order_id), Err(e) => err(e) }
}
async fn send_po(State(svc): State<Arc<BuyingWriteService>>, Json(b): Json<OrderBody>) -> axum::response::Response {
    match svc.send_purchase_order(b.order_id).await { Ok(()) => ok_id(b.order_id), Err(e) => err(e) }
}
async fn lock_po(State(svc): State<Arc<BuyingWriteService>>, Json(b): Json<OrderBody>) -> axum::response::Response {
    match svc.lock_purchase_order(b.order_id).await { Ok(()) => ok_id(b.order_id), Err(e) => err(e) }
}
async fn unlock_po(State(svc): State<Arc<BuyingWriteService>>, Json(b): Json<OrderBody>) -> axum::response::Response {
    match svc.unlock_purchase_order(b.order_id).await { Ok(()) => ok_id(b.order_id), Err(e) => err(e) }
}
async fn acknowledge_po(State(svc): State<Arc<BuyingWriteService>>, Json(b): Json<OrderBody>) -> axum::response::Response {
    match svc.acknowledge_purchase_order(b.order_id).await { Ok(()) => ok_id(b.order_id), Err(e) => err(e) }
}
async fn delete_po(State(svc): State<Arc<BuyingWriteService>>, Json(b): Json<OrderBody>) -> axum::response::Response {
    match svc.delete_purchase_order(b.order_id).await { Ok(()) => ok_id(b.order_id), Err(e) => err(e) }
}

/// The manual-receipt tier: set a `manual` line's received quantity directly.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManualReceiptBody { order_id: Uuid, line_id: Uuid, quantity: Decimal }
async fn set_manual_receipt(
    State(svc): State<Arc<BuyingWriteService>>,
    tenant: CompanyContext,
    Json(b): Json<ManualReceiptBody>,
) -> axum::response::Response {
    match svc.set_manual_line_receipt(b.order_id, tenant.company_id, b.line_id, b.quantity).await {
        Ok(()) => ok_id(b.order_id),
        Err(e) => err(e),
    }
}

/// Delete one PO line (G9: only while the parent order is `draft`/`sent`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LineRefBody { order_id: Uuid, line_id: Uuid }
async fn delete_po_line(State(svc): State<Arc<BuyingWriteService>>, Json(b): Json<LineRefBody>) -> axum::response::Response {
    match svc.delete_purchase_order_line(b.order_id, b.line_id).await {
        Ok(()) => ok_id(b.line_id),
        Err(e) => err(e),
    }
}

// ---- blanket agreements ------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgreementLineBody { item_id: Uuid, quantity: Decimal, rate: Decimal }
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateAgreementBody {
    agreement_number: String,
    supplier_id: Uuid,
    #[serde(default)] currency: Option<String>,
    #[serde(default)] date_start: Option<chrono::NaiveDate>,
    #[serde(default)] date_end: Option<chrono::NaiveDate>,
    #[serde(default)] notes: Option<String>,
    lines: Vec<AgreementLineBody>,
}
async fn create_agreement(
    State(svc): State<Arc<BuyingWriteService>>,
    tenant: CompanyContext,
    Json(b): Json<CreateAgreementBody>,
) -> axum::response::Response {
    let a = NewPurchaseAgreement {
        agreement_number: b.agreement_number,
        company_id: tenant.company_id,
        supplier_id: b.supplier_id,
        currency: b.currency,
        date_start: b.date_start,
        date_end: b.date_end,
        notes: b.notes,
        lines: b.lines.into_iter()
            .map(|l| NewAgreementLine { item_id: l.item_id, quantity: l.quantity, rate: l.rate })
            .collect(),
    };
    match svc.create_purchase_agreement(a).await {
        Ok(id) => (StatusCode::CREATED, Json(IdResponse { id })).into_response(),
        Err(e) => err(e),
    }
}

async fn confirm_agreement(State(svc): State<Arc<BuyingWriteService>>, Json(b): Json<OrderBody>) -> axum::response::Response {
    match svc.confirm_purchase_agreement(b.order_id).await { Ok(()) => ok_id(b.order_id), Err(e) => err(e) }
}
async fn close_agreement(State(svc): State<Arc<BuyingWriteService>>, Json(b): Json<OrderBody>) -> axum::response::Response {
    match svc.close_purchase_agreement(b.order_id).await { Ok(()) => ok_id(b.order_id), Err(e) => err(e) }
}
async fn cancel_agreement(State(svc): State<Arc<BuyingWriteService>>, Json(b): Json<OrderBody>) -> axum::response::Response {
    match svc.cancel_purchase_agreement(b.order_id).await { Ok(()) => ok_id(b.order_id), Err(e) => err(e) }
}

/// Re-price one line of an OPEN agreement (the line + its minted supplier price move together).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinePriceBody { line_id: Uuid, rate: Decimal }
async fn update_agreement_line_price(State(svc): State<Arc<BuyingWriteService>>, Json(b): Json<LinePriceBody>) -> axum::response::Response {
    match svc.update_agreement_line_price(b.line_id, b.rate).await { Ok(()) => ok_id(b.line_id), Err(e) => err(e) }
}

/// Replace a DRAFT agreement's line set (the resequence).
async fn resequence_agreement(
    State(svc): State<Arc<BuyingWriteService>>,
    Json(b): Json<CreateAgreementBodyResequence>,
) -> axum::response::Response {
    let lines = b.lines.into_iter()
        .map(|l| NewAgreementLine { item_id: l.item_id, quantity: l.quantity, rate: l.rate })
        .collect();
    match svc.resequence_agreement_lines(b.agreement_id, lines).await {
        Ok(()) => ok_id(b.agreement_id),
        Err(e) => err(e),
    }
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateAgreementBodyResequence {
    agreement_id: Uuid,
    lines: Vec<AgreementLineBody>,
}

/// Create a call-off PO against an open blanket (prices from the agreement lines).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CallOffLineBody { agreement_line_id: Uuid, quantity: Decimal, #[serde(default)] warehouse_id: Option<Uuid> }
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CallOffBody {
    po_number: String,
    agreement_id: Uuid,
    order_date: chrono::NaiveDate,
    #[serde(default)] schedule_date: Option<chrono::NaiveDate>,
    #[serde(default)] tax_rate: Decimal,
    #[serde(default)] notes: Option<String>,
    lines: Vec<CallOffLineBody>,
}
async fn create_call_off(
    State(svc): State<Arc<BuyingWriteService>>,
    tenant: CompanyContext,
    Json(b): Json<CallOffBody>,
) -> axum::response::Response {
    let o = NewCallOffOrder {
        po_number: b.po_number,
        company_id: tenant.company_id,
        branch_id: tenant.branch_id,
        agreement_id: b.agreement_id,
        order_date: b.order_date,
        schedule_date: b.schedule_date,
        tax_rate: b.tax_rate,
        notes: b.notes,
        lines: b.lines.into_iter()
            .map(|l| crate::application::service::buying_agreement::CallOffLine {
                agreement_line_id: l.agreement_line_id,
                quantity: l.quantity,
                warehouse_id: l.warehouse_id,
            })
            .collect(),
    };
    match svc.create_call_off_po(o).await {
        Ok(id) => (StatusCode::CREATED, Json(IdResponse { id })).into_response(),
        Err(e) => err(e),
    }
}

// ---- manual bill matching + settings ------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BillMatchLineBody { bill_line_ref: String, po_item_id: Uuid, quantity: Decimal }
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MatchBillsBody { order_id: Uuid, matches: Vec<BillMatchLineBody> }
async fn match_bills(
    State(svc): State<Arc<BuyingWriteService>>,
    tenant: CompanyContext,
    Json(b): Json<MatchBillsBody>,
) -> axum::response::Response {
    let proposals: Vec<BillLineProposal> = b.matches.into_iter()
        .map(|m| BillLineProposal { bill_line_ref: m.bill_line_ref, po_item_id: m.po_item_id, quantity: m.quantity })
        .collect();
    match svc.match_bill_lines(b.order_id, tenant.company_id, &proposals).await {
        Ok(()) => ok_id(b.order_id),
        Err(e) => err(e),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompanySettingsBody {
    double_validation: String,
    double_validation_amount: Decimal,
    company_currency: String,
    #[serde(default = "default_send_reminder")] send_reminder: bool,
}
fn default_send_reminder() -> bool { true }
async fn upsert_company_settings(
    State(svc): State<Arc<BuyingWriteService>>,
    tenant: CompanyContext,
    Json(b): Json<CompanySettingsBody>,
) -> axum::response::Response {
    match svc.upsert_purchase_company_settings(
        tenant.company_id, b.double_validation, b.double_validation_amount,
        b.company_currency, b.send_reminder,
    ).await {
        Ok(()) => ok_id(tenant.company_id),
        Err(e) => err(e),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SupplierSettingsBody {
    supplier_id: Uuid,
    #[serde(default = "default_send_reminder")] receipt_reminder_email: bool,
    #[serde(default = "default_days_before")] reminder_days_before: i32,
}
fn default_days_before() -> i32 { 1 }
async fn upsert_supplier_settings(
    State(svc): State<Arc<BuyingWriteService>>,
    tenant: CompanyContext,
    Json(b): Json<SupplierSettingsBody>,
) -> axum::response::Response {
    match svc.upsert_supplier_reminder_settings(
        tenant.company_id, b.supplier_id, b.receipt_reminder_email, b.reminder_days_before,
    ).await {
        Ok(()) => ok_id(b.supplier_id),
        Err(e) => err(e),
    }
}

fn write_routes(svc: Arc<BuyingWriteService>, verifier: CompanyVerifier) -> Router {
    Router::new()
        .route("/purchase-orders", post(create_po))
        .route("/purchase-orders/confirm", post(confirm_po))
        .route("/purchase-orders/approve", post(approve_po))
        .route("/purchase-orders/reset", post(reset_po))
        .route("/purchase-orders/cancel", post(cancel_po))
        .route("/purchase-orders/send", post(send_po))
        .route("/purchase-orders/lock", post(lock_po))
        .route("/purchase-orders/unlock", post(unlock_po))
        .route("/purchase-orders/acknowledge", post(acknowledge_po))
        .route("/purchase-orders/delete", post(delete_po))
        .route("/purchase-orders/delete-line", post(delete_po_line))
        .route("/purchase-orders/manual-receipt", post(set_manual_receipt))
        .route("/purchase-orders/match-bills", post(match_bills))
        .route("/agreements", post(create_agreement))
        .route("/agreements/confirm", post(confirm_agreement))
        .route("/agreements/close", post(close_agreement))
        .route("/agreements/cancel", post(cancel_agreement))
        .route("/agreements/reprice-line", post(update_agreement_line_price))
        .route("/agreements/resequence-lines", post(resequence_agreement))
        .route("/agreements/call-off", post(create_call_off))
        .route("/settings/company", post(upsert_company_settings))
        .route("/settings/supplier-reminder", post(upsert_supplier_settings))
        // Every write above is tenant-scoped: `company_auth` rejects a request whose token is absent,
        // invalid, or carries no `company_id`, so a handler only ever runs with a proven tenant.
        //
        // `route_layer`, not `layer`: `layer` would also wrap this router's fallback, so once merged
        // every *unmatched* path (e.g. the generic CRUD paths this surface deliberately does not
        // mount) would answer 401 instead of 404 — leaking "auth required" for routes that do not
        // exist, and masking the CRUD-bypass probes.
        .route_layer(from_fn_with_state(verifier, company_auth))
        .with_state(svc)
}

/// Mount the buying module: read documents + validated, tenant-scoped creates + the lifecycle /
/// agreement / matching / settings verbs. Generic mutation is not mounted — supplier prices in
/// particular have NO route: only the agreement verbs write them. **Prefer this over
/// `BuyingModule::all_crud_routes()` for any real deployment.**
///
/// The composing service builds one [`CompanyVerifier`] from its JWT secret and passes it here; the
/// write surface derives `company_id` from the token, so no tenant crosses the wire in a body.
pub fn create_guarded_buying_routes(
    m: &BuyingModule,
    pool: PgPool,
    verifier: CompanyVerifier,
) -> Router {
    let write = Arc::new(BuyingWriteService::new(pool));
    Router::new()
        .merge(create_material_request_read_routes(m.material_request_service.clone()))
        .merge(create_supplier_quotation_read_routes(m.supplier_quotation_service.clone()))
        .merge(create_purchase_order_read_routes(m.purchase_order_service.clone()))
        .merge(write_routes(write, verifier))
}
