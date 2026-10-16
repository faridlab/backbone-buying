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
