//! Route-level probes: the guarded surface validates creates and does NOT expose generic mutation
//! (create/update/delete/bulk) on buying documents — closing the CRUD-bypass — and every validated
//! write derives its tenant from a signed token rather than the request body. Requires
//! DATABASE_URL (:5433/backbone_buying).
//!
//! BIP-1..BIP-4  the CRUD-bypass and validated-write invariants.
//! BIT-1..BIT-3  the tenancy invariants (mirrors the TG-* cases backbone-pos proved).

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use backbone_auth::tenant::TenantVerifier;
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
/// carries no tenant — it must not be allowed to write.
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
    create_guarded_buying_routes(m, pool.clone(), TenantVerifier::hs256(SECRET))
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

// BIP-3: validated PO create works (201). No `companyId` in the body — the tenant rides on the token.
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

// BIT-1: an unauthenticated write is rejected. Before the tenant guard this create succeeded and
// stamped whatever `companyId` the caller put in the body.
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

// BIT-2: a token that authenticates a user but carries no `company_id` claim is rejected — a writer
// that cannot name its tenant must never run.
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

// BIT-3: a `companyId` smuggled in the body is ignored — the persisted tenant is the token's. This is
// the regression that motivated the change: the body must not be able to name the tenant.
#[tokio::test]
async fn body_company_id_cannot_override_the_token_tenant() {
    let pool = pool().await;
    let m = module(&pool).await;
    let token_company = uuid::Uuid::new_v4();
    let attacker_company = uuid::Uuid::new_v4();
    let number = uq("PO");
    let body = format!(
        r#"{{"poNumber":"{}","companyId":"{}","supplierId":"{}","orderDate":"2026-07-05","taxRate":"11",
             "lines":[{{"itemId":"{}","quantity":"10","rate":"100000"}}]}}"#,
        number, attacker_company, uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let (s, _) = req_as(app(&pool, &m), token_company, "POST", "/purchase-orders", Some(body)).await;
    assert_eq!(s, StatusCode::CREATED);

    let persisted: Uuid =
        sqlx::query_scalar("SELECT company_id FROM buying.purchase_orders WHERE po_number = $1")
            .bind(&number)
            .fetch_one(&pool)
            .await
            .expect("purchase order row");
    assert_eq!(persisted, token_company, "tenant must come from the token, not the body");
    assert_ne!(persisted, attacker_company, "the body's companyId must be ignored");
}
