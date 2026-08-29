//! Probes for the PO grouping domain's project partition (the never-merge-across-projects rule).
//!
//! The grouping domain resolves merge/group candidates through exactly one named lookup —
//! `BuyingWriteService::find_open_po_for_demand` — whose key is (company_id, supplier_id,
//! project_id) with `project_id` matched exactly (NULL matches NULL only). These probes lock:
//!
//!   * two demands differing ONLY in project_id resolve to DISTINCT candidates — a PO bought
//!     for one project is never in another project's (or a project-less) candidate set;
//!   * the same demand resolves to the same candidate (same project → same candidate);
//!   * NULL matches NULL only;
//!   * the candidate band is the still-editable one (draft/sent) — parked or confirmed
//!     commitments never silently absorb new lines, cancelled orders are gone from the domain;
//!   * `project_id` round-trips through the validated create path (service and guarded HTTP).
//!
//! Requires DATABASE_URL (:5433, migrated with this module's migrations).

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use backbone_auth::company::CompanyVerifier;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use backbone_buying::application::service::buying_po_grouping::PoDemand;
use backbone_buying::application::service::buying_write_service::{
    BuyingWriteService, NewLine, NewPurchaseOrder,
};
use backbone_buying::presentation::http::create_guarded_buying_routes;
use backbone_buying::BuyingModule;

fn d(s: &str) -> Decimal { Decimal::from_str_exact(s).unwrap() }
fn day() -> chrono::NaiveDate { chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap() }
fn uq(p: &str) -> String { format!("{p}-{}", &Uuid::new_v4().simple().to_string()[..8]) }
async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/backbone_buying".to_string());
    PgPool::connect(&url).await.expect("connect DB")
}
fn line(item: Uuid, qty: &str, rate: &str) -> NewLine {
    NewLine { item_id: item, warehouse_id: None, description: None, quantity: d(qty), rate: d(rate), qty_received_method: None, purchase_method: None }
}

/// A draft PO for `supplier`, bought for `project` (None = unassigned).
async fn project_po(
    w: &BuyingWriteService,
    company: Uuid,
    supplier: Uuid,
    project: Option<Uuid>,
) -> Uuid {
    w.create_purchase_order(NewPurchaseOrder {
        po_number: uq("PO"), supplier_quotation_id: None, order_kind: None, company_id: company,
        branch_id: None, supplier_id: supplier, order_date: day(), schedule_date: None,
        currency: None, currency_rate: None, agreement_id: None, project_id: project,
        tax_rate: Decimal::ZERO, notes: None,
        lines: vec![line(Uuid::new_v4(), "1", "100")],
    }).await.unwrap()
}

// The named probe: two demands differing only in project_id NEVER resolve to the same candidate;
// same project → same candidate; NULL matches NULL only.
#[tokio::test]
async fn prj4_merge_domain_partitions_by_project() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, supplier) = (Uuid::new_v4(), Uuid::new_v4());
    let (project_a, project_b, project_c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());

    // One supplier, three open orders: for project A, for project B, and unassigned.
    let po_a = project_po(&w, company, supplier, Some(project_a)).await;
    let po_b = project_po(&w, company, supplier, Some(project_b)).await;
    let po_n = project_po(&w, company, supplier, None).await;

    // Two demands differing ONLY in project_id → DISTINCT candidates (never coalesce).
    let cand_a = w.find_open_po_for_demand(&PoDemand::new(company, supplier).for_project(project_a)).await.unwrap().expect("project A demand finds its PO");
    let cand_b = w.find_open_po_for_demand(&PoDemand::new(company, supplier).for_project(project_b)).await.unwrap().expect("project B demand finds its PO");
    assert_eq!(cand_a.id, po_a, "project A's demand resolves to project A's order");
    assert_eq!(cand_b.id, po_b, "project B's demand resolves to project B's order");
    assert_ne!(cand_a.id, cand_b.id, "demands differing only in project_id NEVER share a candidate");
    assert_eq!(cand_a.project_id, Some(project_a), "the candidate echoes the partition it matched on");
    assert_eq!(cand_b.project_id, Some(project_b), "the candidate echoes the partition it matched on");

    // Same project (repeated demand) → the SAME candidate.
    let cand_a_again = w.find_open_po_for_demand(&PoDemand::new(company, supplier).for_project(project_a)).await.unwrap().expect("same demand still finds its PO");
    assert_eq!(cand_a_again.id, po_a, "same project → same candidate");

    // NULL matches NULL only: an unassigned demand sees the unassigned order, never A's or B's.
    let cand_n = w.find_open_po_for_demand(&PoDemand::new(company, supplier).without_project()).await.unwrap().expect("unassigned demand finds the unassigned PO");
    assert_eq!(cand_n.id, po_n, "a project-less demand resolves to the project-less order");
    assert_ne!(cand_n.id, po_a);
    assert_ne!(cand_n.id, po_b);
    assert_eq!(cand_n.project_id, None, "the NULL partition matches only NULL");

    // A demand for a project with no open order finds nothing — a grouping engine then creates a
    // fresh PO rather than borrowing another project's.
    let none = w.find_open_po_for_demand(&PoDemand::new(company, supplier).for_project(project_c)).await.unwrap();
    assert!(none.is_none(), "no open order for project C");

    // A different supplier's demand never sees this supplier's orders, whatever the project.
    let other_supplier = w.find_open_po_for_demand(&PoDemand::new(company, Uuid::new_v4()).for_project(project_a)).await.unwrap();
    assert!(other_supplier.is_none(), "supplier is (and stays) part of the grouping key");

    // A different company's demand never sees them either (the read rides the caller's scope).
    let other_company = w.find_open_po_for_demand(&PoDemand::new(Uuid::new_v4(), supplier).for_project(project_a)).await.unwrap();
    assert!(other_company.is_none(), "company fence holds in the grouping domain");
}

// The candidate band is the still-editable one: draft and sent qualify; once parked at the
// approval gate, confirmed, or cancelled, an order leaves the grouping domain even for its own
// project — a confirmed commitment never silently absorbs new lines.
#[tokio::test]
async fn prj4_open_band_is_the_editable_one() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, supplier) = (Uuid::new_v4(), Uuid::new_v4());
    let project = Uuid::new_v4();

    // draft qualifies.
    let po1 = project_po(&w, company, supplier, Some(project)).await;
    let cand = w.find_open_po_for_demand(&PoDemand::new(company, supplier).for_project(project)).await.unwrap().unwrap();
    assert_eq!(cand.id, po1);
    assert_eq!(cand.status, "draft");

    // sent still qualifies.
    w.send_purchase_order(po1).await.unwrap();
    let cand = w.find_open_po_for_demand(&PoDemand::new(company, supplier).for_project(project)).await.unwrap().unwrap();
    assert_eq!(cand.id, po1);
    assert_eq!(cand.status, "sent");

    // confirmed (purchase) does not: the demand must find nothing (oldest-first moves to the
    // next open order only if one exists — here none does).
    w.confirm_purchase_order(po1, false).await.unwrap();
    let none = w.find_open_po_for_demand(&PoDemand::new(company, supplier).for_project(project)).await.unwrap();
    assert!(none.is_none(), "a confirmed commitment is out of the grouping domain");

    // cancelled does not either.
    let po2 = project_po(&w, company, supplier, Some(project)).await;
    w.cancel_purchase_order(po2).await.unwrap();
    let none = w.find_open_po_for_demand(&PoDemand::new(company, supplier).for_project(project)).await.unwrap();
    assert!(none.is_none(), "a cancelled order is out of the grouping domain");
}

// Deterministic oldest-first: with two open orders in the same partition, the earlier-created one
// is the candidate (a merge engine built on this finder is reproducible).
#[tokio::test]
async fn prj4_candidate_is_oldest_first() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, supplier, project) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());

    let po1 = project_po(&w, company, supplier, Some(project)).await;
    // Give the first order a head start on the audit clock, then create the second.
    sqlx::query(r#"UPDATE buying.purchase_orders
                      SET metadata = jsonb_set(metadata, '{created_at}', to_jsonb(NOW() - INTERVAL '1 hour'))
                    WHERE id=$1"#)
        .bind(po1).execute(&pool).await.unwrap();
    let po2 = project_po(&w, company, supplier, Some(project)).await;

    let cand = w.find_open_po_for_demand(&PoDemand::new(company, supplier).for_project(project)).await.unwrap().unwrap();
    assert_eq!(cand.id, po1, "the earlier-created order wins the partition");
    assert_ne!(cand.id, po2);
}

// The column round-trips through the validated create path: a project-anchored PO stores its
// project, an unanchored one stores NULL (and only NULL).
#[tokio::test]
async fn prj4_project_id_roundtrips_on_create() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let (company, supplier, project) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());

    let anchored = project_po(&w, company, supplier, Some(project)).await;
    let stored: Option<Uuid> = sqlx::query_scalar("SELECT project_id FROM buying.purchase_orders WHERE id=$1")
        .bind(anchored).fetch_one(&pool).await.unwrap();
    assert_eq!(stored, Some(project), "the project anchor persists through the validated write path");

    let bare = project_po(&w, company, supplier, None).await;
    let stored: Option<Uuid> = sqlx::query_scalar("SELECT project_id FROM buying.purchase_orders WHERE id=$1")
        .bind(bare).fetch_one(&pool).await.unwrap();
    assert_eq!(stored, None, "an unanchored PO stores NULL, not a default");
}

// ---- guarded HTTP surface ----------------------------------------------------

const SECRET: &[u8] = b"buying-prj4-probe-secret";

#[derive(Serialize)]
struct TestClaims {
    sub: String,
    exp: usize,
    company_id: Option<Uuid>,
}
fn token(company_id: Uuid) -> String {
    let claims = TestClaims { sub: "probe-user".into(), exp: 9_999_999_999, company_id: Some(company_id) };
    encode(&Header::new(Algorithm::HS256), &claims, &EncodingKey::from_secret(SECRET)).unwrap()
}
async fn module(pool: &PgPool) -> BuyingModule {
    BuyingModule::builder().with_database(pool.clone()).build().unwrap()
}

// The guarded create accepts the project anchor and threads it into the validated write path
// (the tenant itself still comes from the signed token, never the body).
#[tokio::test]
async fn prj4_guarded_create_accepts_project() {
    let pool = pool().await;
    let m = module(&pool).await;
    let app = create_guarded_buying_routes(&m, pool.clone(), CompanyVerifier::hs256(SECRET));
    let (company, project, item) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());

    let body = serde_json::json!({
        "poNumber": uq("PO-HTTP"),
        "supplierId": Uuid::new_v4().to_string(),
        "orderDate": "2026-07-05",
        "projectId": project.to_string(),
        "lines": [{ "itemId": item.to_string(), "quantity": 1, "rate": 100 }],
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/purchase-orders")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {}", token(company)))
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED, "the guarded create accepts the project anchor");
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    let id: Uuid = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["id"]
        .as_str().unwrap().parse().unwrap();

    let stored: Option<Uuid> = sqlx::query_scalar("SELECT project_id FROM buying.purchase_orders WHERE id=$1")
        .bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(stored, Some(project), "the HTTP-threaded project anchor lands in the row");
}
