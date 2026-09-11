//! Route-level probes: the guarded surface validates creates and does NOT expose generic mutation
//! (create/update/delete/bulk) on buying documents — closing the CRUD-bypass — and every write
//! demands a signed token while the request body never names a tenant (the module is
//! tenant-agnostic, ADR-0029). Requires DATABASE_URL (:5433/backbone_buying).
//!
//! BIP-1..BIP-4  the CRUD-bypass and validated-write invariants.
//! BIT-1..BIT-3  the auth-gate invariants (mirrors the TG-* cases backbone-pos proved).

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use backbone_auth::company::CompanyVerifier;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::Serialize;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use backbone_buying::presentation::http::create_guarded_buying_routes;
use backbone_buying::BuyingModule;

const SECRET: &[u8] = b"buying-integrity-probe-secret";

#[derive(Serialize)]
struct TestClaims {
    sub: String,
    exp: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    company_id: Option<Uuid>,
}

/// Mint an HS256 access token. `company_id = None` models a token that authenticates a user but
/// carries no tenant claim — the auth gate must not let it write.
fn token(company_id: Option<Uuid>) -> String {
    let claims = TestClaims { sub: "probe-user".into(), exp: 9_999_999_999, company_id };
    encode(&Header::new(Algorithm::HS256), &claims, &EncodingKey::from_secret(SECRET)).unwrap()
}

async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/backbone_buying".to_string());
    PgPool::connect(&url).await.unwrap()
}
async fn module(pool: &PgPool) -> BuyingModule {
    BuyingModule::builder().with_database(pool.clone()).build().unwrap()
}
fn app(pool: &PgPool, m: &BuyingModule) -> axum::Router {
    create_guarded_buying_routes(m, pool.clone(), CompanyVerifier::hs256(SECRET))
}

/// Send a request with an optional bearer token.
async fn req_with(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<String>,
    bearer: Option<String>,
) -> (StatusCode, String) {
    let b = body.map(Body::from).unwrap_or(Body::empty());
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(t) = bearer {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let resp = app.oneshot(builder.body(b).unwrap()).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

/// Unauthenticated request.
async fn req(app: axum::Router, method: &str, uri: &str, body: Option<String>) -> (StatusCode, String) {
    req_with(app, method, uri, body, None).await
}

/// Request authenticated as a principal of `company`.
async fn req_as(
    app: axum::Router,
    company: Uuid,
    method: &str,
    uri: &str,
    body: Option<String>,
) -> (StatusCode, String) {
    req_with(app, method, uri, body, Some(token(Some(company)))).await
}

fn uq(p: &str) -> String { format!("{p}-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]) }

// BIP-1: generic bulk create on POs is not exposed.
#[tokio::test]
async fn guarded_locks_generic_po_bulk() {
    let pool = pool().await;
    let m = module(&pool).await;
    let (s, _) = req(app(&pool, &m), "POST", "/purchase-orders/bulk", Some("[]".into())).await;
    assert!(s == StatusCode::METHOD_NOT_ALLOWED || s == StatusCode::NOT_FOUND, "got {s}");
}

// BIP-2: generic delete on a PO is not exposed.
#[tokio::test]
async fn guarded_locks_generic_po_delete() {
    let pool = pool().await;
    let m = module(&pool).await;
    let id = uuid::Uuid::new_v4();
    let (s, _) = req(app(&pool, &m), "DELETE", &format!("/purchase-orders/{id}"), None).await;
    assert!(s == StatusCode::METHOD_NOT_ALLOWED || s == StatusCode::NOT_FOUND, "got {s}");
}

// BIP-3: validated PO create works (201). No tenant anywhere in the body — the module is
// tenant-agnostic (ADR-0029).
#[tokio::test]
async fn guarded_create_po_ok() {
    let pool = pool().await;
    let m = module(&pool).await;
    let body = format!(
        r#"{{"poNumber":"{}","supplierId":"{}","orderDate":"2026-07-05","taxRate":"11",
             "lines":[{{"itemId":"{}","quantity":"10","rate":"100000"}}]}}"#,
        uq("PO"), uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let (s, _) = req_as(app(&pool, &m), uuid::Uuid::new_v4(), "POST", "/purchase-orders", Some(body)).await;
    assert_eq!(s, StatusCode::CREATED);
}

// BIP-4: validated PO create rejects an empty document (422 empty_document).
#[tokio::test]
async fn guarded_create_po_rejects_empty() {
    let pool = pool().await;
    let m = module(&pool).await;
    let body = format!(
        r#"{{"poNumber":"{}","supplierId":"{}","orderDate":"2026-07-05","lines":[]}}"#,
        uq("PO"), uuid::Uuid::new_v4());
    let (s, b) = req_as(
        app(&pool, &m), uuid::Uuid::new_v4(), "POST", "/purchase-orders", Some(body),
    ).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(b.contains("empty_document"), "got: {b}");
}

// BIT-1: an unauthenticated write is rejected — the auth gate refuses before any handler runs.
#[tokio::test]
async fn guarded_write_rejects_unauthenticated() {
    let pool = pool().await;
    let m = module(&pool).await;
    let body = format!(
        r#"{{"poNumber":"{}","supplierId":"{}","orderDate":"2026-07-05","lines":[]}}"#,
        uq("PO"), uuid::Uuid::new_v4());
    let (s, _) = req(app(&pool, &m), "POST", "/purchase-orders", Some(body)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "an unauthenticated write must not reach the service");
}

// BIT-2: a token that authenticates a user but carries no `company_id` claim is rejected — the
// auth gate's claim shape is the composing deployment's contract, and a claim-less token fails it.
#[tokio::test]
async fn guarded_write_rejects_token_without_company_id() {
    let pool = pool().await;
    let m = module(&pool).await;
    let body = format!(
        r#"{{"poNumber":"{}","supplierId":"{}","orderDate":"2026-07-05","lines":[]}}"#,
        uq("PO"), uuid::Uuid::new_v4());
    let (s, _) = req_with(
        app(&pool, &m), "POST", "/purchase-orders", Some(body), Some(token(None)),
    ).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "a token with no tenant must not write");
}

// BIT-3: a `companyId` smuggled in the body is ignored — the body must never be able to name a
// tenant. The module is tenant-agnostic (ADR-0029): the create succeeds and the smuggled id lands
// nowhere, because the legacy tenant column is gone from the module's schema entirely.
#[tokio::test]
async fn body_company_id_is_ignored() {
    let pool = pool().await;
    let m = module(&pool).await;
    let attacker_company = uuid::Uuid::new_v4();
    let number = uq("PO");
    let body = format!(
        r#"{{"poNumber":"{}","companyId":"{}","supplierId":"{}","orderDate":"2026-07-05","taxRate":"11",
             "lines":[{{"itemId":"{}","quantity":"10","rate":"100000"}}]}}"#,
        number, attacker_company, uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let (s, _) = req_as(app(&pool, &m), uuid::Uuid::new_v4(), "POST", "/purchase-orders", Some(body)).await;
    assert_eq!(s, StatusCode::CREATED, "the create itself succeeds — the unknown field is ignored");

    // The row was persisted, and no tenant column is left to stamp on it.
    let row: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM buying.purchase_orders WHERE po_number = $1")
            .bind(&number)
            .fetch_optional(&pool)
            .await
            .expect("purchase order row query");
    assert!(row.is_some(), "the validated create persisted the order");
    let tenant_columns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.columns \
         WHERE table_schema = 'buying' AND table_name = 'purchase_orders' AND column_name = 'company_id'",
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(tenant_columns, 0, "the module's schema carries no tenant column the body could reach");
}
