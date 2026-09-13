//! Route-level probes: the guarded surface validates creates and does NOT expose generic mutation
//! (create/update/delete/bulk) on buying documents — closing the CRUD-bypass — while the request
//! body never names a tenant (the module is tenant-agnostic, ADR-0029). Requires DATABASE_URL
//! (:5433/backbone_buying).
//!
//! BIP-1..BIP-4  the CRUD-bypass and validated-write invariants.
//! BIT           the tenancy posture: the surface mounts no guard of its own (the composing
//!               service authenticates), so the leg proves a smuggled tenant field in the body
//!               is tolerated and cannot name a tenant.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use backbone_auth::org::OrgContext;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

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
fn app(pool: &PgPool, m: &BuyingModule) -> axum::Router {
    // The module ships no guard, so the test stands in for the composing service's outer org
    // guard: it inserts the OrgContext the write handlers extract.
    create_guarded_buying_routes(m, pool.clone()).layer(axum::middleware::from_fn(
        |mut req: axum::http::Request<axum::body::Body>, next: axum::middleware::Next| async move {
            req.extensions_mut().insert(OrgContext {
                acting_unit_id: Uuid::nil(),
                entitled_units: vec![],
                legacy_company_id: None,
                user_id: "probe".to_string(),
            });
            next.run(req).await
        },
    ))
}

/// Send a request.
async fn req(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<String>,
) -> (StatusCode, String) {
    let b = body.map(Body::from).unwrap_or(Body::empty());
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    let resp = app.oneshot(builder.body(b).unwrap()).await.unwrap();
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
    let (s, _) = req(app(&pool, &m), "POST", "/purchase-orders", Some(body)).await;
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
    let (s, b) = req(app(&pool, &m), "POST", "/purchase-orders", Some(body)).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(b.contains("empty_document"), "got: {b}");
}

// BIT: the surface mounts no guard of its own — authentication is the composing service's duty,
// so the unauthenticated/no-tenant refusals belong to its authn probes, not here. The tenancy
// posture that stays at module level: a `companyId` smuggled into the body is ignored.
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
    let (s, _) = req(app(&pool, &m), "POST", "/purchase-orders", Some(body)).await;
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
