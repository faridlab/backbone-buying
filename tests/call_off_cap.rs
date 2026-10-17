//! A call-off never draws past its blanket line's quantity, however the draws are split.
//!
//! The cap is held in the write that advances `qty_ordered`, not by a check that reads the line
//! first: a call-off that names one line twice, each draw within the cap and the two together
//! past it, is refused, and nothing it would have written stays behind.

use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use backbone_buying::application::service::buying_agreement::{CallOffLine, NewAgreementLine, NewCallOffOrder, NewPurchaseAgreement};
use backbone_buying::application::service::buying_write_service::{BuyingError, BuyingWriteService};

fn d(s: &str) -> Decimal {
    Decimal::from_str_exact(s).unwrap()
}
fn uq(p: &str) -> String {
    format!("{p}-{}", &Uuid::new_v4().simple().to_string()[..8])
}
async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/backbone_buying".to_string());
    PgPool::connect(&url).await.expect("connect DB")
}

#[tokio::test]
async fn a_call_off_naming_a_line_twice_cannot_draw_past_the_blanket() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let agreement = w
        .create_purchase_agreement(NewPurchaseAgreement {
            agreement_number: uq("BPA"),
            supplier_id: Uuid::new_v4(),
            currency: None,
            date_start: None,
            date_end: None,
            notes: None,
            lines: vec![NewAgreementLine { item_id: Uuid::new_v4(), quantity: d("10"), rate: d("5") }],
        })
        .await
        .unwrap();
    w.confirm_purchase_agreement(agreement).await.unwrap();
    let line: Uuid = sqlx::query_scalar("SELECT id FROM buying.purchase_agreement_lines WHERE agreement_id = $1")
        .bind(agreement)
        .fetch_one(&pool)
        .await
        .unwrap();
    let draw = |q: &str| CallOffLine { agreement_line_id: line, quantity: d(q), warehouse_id: None };
    let po_number = uq("PO");

    let refused = w
        .create_call_off_po(NewCallOffOrder {
            po_number: po_number.clone(),
            branch_id: None,
            agreement_id: agreement,
            order_date: chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap(),
            schedule_date: None,
            tax_rate: Decimal::ZERO,
            notes: None,
            lines: vec![draw("6"), draw("6")],
        })
        .await;
    assert!(matches!(refused, Err(BuyingError::AgreementExceeded(id)) if id == line), "{refused:?}");

    let drawn: Decimal = sqlx::query_scalar("SELECT qty_ordered FROM buying.purchase_agreement_lines WHERE id = $1")
        .bind(line)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(drawn, Decimal::ZERO, "a refused call-off draws nothing");
    let orders: i64 = sqlx::query_scalar("SELECT count(*) FROM buying.purchase_orders WHERE po_number = $1")
        .bind(&po_number)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(orders, 0, "and leaves no order behind");
}

/// Supplier prices change only through their agreement: confirming mints them through the
/// agreement's own write, and a write from anywhere else is refused with the rule's code.
#[tokio::test]
async fn supplier_prices_change_only_through_their_agreement() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let agreement = w
        .create_purchase_agreement(NewPurchaseAgreement {
            agreement_number: uq("BPA"),
            supplier_id: Uuid::new_v4(),
            currency: None,
            date_start: None,
            date_end: None,
            notes: None,
            lines: vec![NewAgreementLine { item_id: Uuid::new_v4(), quantity: d("10"), rate: d("5") }],
        })
        .await
        .unwrap();
    w.confirm_purchase_agreement(agreement).await.expect("the agreement mints its prices");
    let minted: Uuid = sqlx::query_scalar("SELECT id FROM buying.supplier_prices WHERE agreement_id = $1")
        .bind(agreement).fetch_one(&pool).await.expect("one minted price");

    let refused = sqlx::query("UPDATE buying.supplier_prices SET price = 1 WHERE id = $1")
        .bind(minted).execute(&pool).await.expect_err("a write outside the agreement is refused");
    let db = refused.as_database_error().expect("a database refusal");
    assert_eq!(db.constraint(), Some("ck_supplier_prices_agreement_owned__supplier_price_agreement_owned"), "{db:?}");

    let refused = sqlx::query("DELETE FROM buying.supplier_prices WHERE id = $1").bind(minted).execute(&pool).await;
    assert!(refused.is_err(), "a delete outside the agreement is refused");
}

/// A close and a call-off racing on one agreement never leave a retired agreement with an open
/// call-off: either the call-off lands first and the close is refused, or the close lands first
/// and the call-off is refused.
#[tokio::test]
async fn a_close_racing_a_call_off_never_retires_an_agreement_with_an_open_call_off() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    for _ in 0..20 {
        let agreement = w
            .create_purchase_agreement(NewPurchaseAgreement {
                agreement_number: uq("BPA"),
                supplier_id: Uuid::new_v4(),
                currency: None,
                date_start: None,
                date_end: None,
                notes: None,
                lines: vec![NewAgreementLine { item_id: Uuid::new_v4(), quantity: d("10"), rate: d("5") }],
            })
            .await
            .unwrap();
        w.confirm_purchase_agreement(agreement).await.unwrap();
        let line: Uuid = sqlx::query_scalar("SELECT id FROM buying.purchase_agreement_lines WHERE agreement_id = $1")
            .bind(agreement)
            .fetch_one(&pool)
            .await
            .unwrap();
        let call_off = w.create_call_off_po(NewCallOffOrder {
            po_number: uq("PO"),
            branch_id: None,
            agreement_id: agreement,
            order_date: chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap(),
            schedule_date: None,
            tax_rate: Decimal::ZERO,
            notes: None,
            lines: vec![CallOffLine { agreement_line_id: line, quantity: d("1"), warehouse_id: None }],
        });
        let (placed, closed) = tokio::join!(call_off, w.close_purchase_agreement(agreement));
        assert!(placed.is_ok() != closed.is_ok(), "exactly one wins: {placed:?} / {closed:?}");

        let stranded: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM buying.purchase_orders po JOIN buying.purchase_agreements a ON a.id = po.agreement_id
              WHERE a.id = $1 AND a.status::text = 'done' AND po.status IN ('draft','sent','to_approve')",
        )
        .bind(agreement)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stranded, 0, "a closed agreement keeps no open call-off");
    }
}

/// The database holds the close rule for any writer: a raw status write that retires an
/// agreement with an open call-off is refused with the rule's code.
#[tokio::test]
async fn a_raw_close_with_an_open_call_off_is_refused_by_the_database() {
    let pool = pool().await;
    let w = BuyingWriteService::new(pool.clone());
    let agreement = w
        .create_purchase_agreement(NewPurchaseAgreement {
            agreement_number: uq("BPA"),
            supplier_id: Uuid::new_v4(),
            currency: None,
            date_start: None,
            date_end: None,
            notes: None,
            lines: vec![NewAgreementLine { item_id: Uuid::new_v4(), quantity: d("10"), rate: d("5") }],
        })
        .await
        .unwrap();
    w.confirm_purchase_agreement(agreement).await.unwrap();
    let line: Uuid = sqlx::query_scalar("SELECT id FROM buying.purchase_agreement_lines WHERE agreement_id = $1")
        .bind(agreement)
        .fetch_one(&pool)
        .await
        .unwrap();
    w.create_call_off_po(NewCallOffOrder {
        po_number: uq("PO"),
        branch_id: None,
        agreement_id: agreement,
        order_date: chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap(),
        schedule_date: None,
        tax_rate: Decimal::ZERO,
        notes: None,
        lines: vec![CallOffLine { agreement_line_id: line, quantity: d("1"), warehouse_id: None }],
    })
    .await
    .unwrap();

    let refused = sqlx::query("UPDATE buying.purchase_agreements SET status = 'done' WHERE id = $1")
        .bind(agreement)
        .execute(&pool)
        .await
        .expect_err("the database refuses the close");
    let db = refused.as_database_error().expect("a database refusal");
    assert_eq!(db.constraint(), Some("ck_purchase_agreements_agreement_close_no_draft_calloffs__agreement_has_draft_orders"), "{db:?}");
}
