//! Route-level probes: the guarded surface validates creates and does NOT expose generic mutation.
//! Requires DATABASE_URL (:5433/backbone_buying).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;

use backbone_buying::presentation::http::create_guarded_buying_routes;
use backbone_buying::BuyingModule;

async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/backbone_buying".to_string());
    PgPool::connect(&url).await.unwrap()
}
async fn module(pool: &PgPool) -> BuyingModule {
    BuyingModule::builder().with_database(pool.clone()).build().unwrap()
}
fn app(pool: &PgPool, m: &BuyingModule) -> axum::Router { create_guarded_buying_routes(m, pool.clone()) }
async fn req(app: axum::Router, method: &str, uri: &str, body: Option<String>) -> (StatusCode, String) {
    let b = body.map(Body::from).unwrap_or(Body::empty());
    let resp = app.oneshot(Request::builder().method(method).uri(uri).header("content-type", "application/json").body(b).unwrap()).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
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

// BIP-3: validated PO create works (201).
#[tokio::test]
async fn guarded_create_po_ok() {
    let pool = pool().await;
    let m = module(&pool).await;
    let body = format!(
        r#"{{"poNumber":"{}","companyId":"{}","supplierId":"{}","orderDate":"2026-07-05","taxRate":"11",
             "lines":[{{"itemId":"{}","quantity":"10","rate":"100000"}}]}}"#,
        uq("PO"), uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let (s, _) = req(app(&pool, &m), "POST", "/purchase-orders", Some(body)).await;
    assert_eq!(s, StatusCode::CREATED);
}

// BIP-4: validated PO create rejects an empty document (422 empty_document).
#[tokio::test]
async fn guarded_create_po_rejects_empty() {
    let pool = pool().await;
    let m = module(&pool).await;
    let body = format!(
        r#"{{"poNumber":"{}","companyId":"{}","supplierId":"{}","orderDate":"2026-07-05","lines":[]}}"#,
        uq("PO"), uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let (s, b) = req(app(&pool, &m), "POST", "/purchase-orders", Some(body)).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(b.contains("empty_document"), "got: {b}");
}
