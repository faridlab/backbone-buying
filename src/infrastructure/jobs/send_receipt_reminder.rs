//! `send_receipt_reminder` — the module's ONE scheduled job (hand-authored, user-owned).
//!
//! The declaration of record is the `scheduled_jobs.send_receipt_reminder` block in
//! `schema/hooks/index.hook.yaml`; this file is the handler it names. Its ADR-0020 posture:
//!
//! - **`posture: pull`** — a plain daily scan (`0 7 * * *`); no domain event re-arms it.
//! - **`commit_policy: single_transaction`** — the per-company run is ONE transaction: claim,
//!   stamp, commit. A crash mid-run replays the whole run next time — safe because the claim is
//!   idempotent by the `last_reminder_on` stamp (a PO already reminded today is not claimable
//!   again), which is the at-least-once-delivery vocabulary of ADR-0017: delivery may repeat
//!   across runs, the reminder effect happens once per day.
//! - **`pickup_lock: true`** — the claim reads `FOR UPDATE SKIP LOCKED`, so two concurrent
//!   replicas (or a manual overlap with the cron) take disjoint sets instead of double-firing.
//!
//! What fires a reminder: a PO in the operational state (`purchase`), NOT yet acknowledged, not
//! yet fully received, whose `schedule_date` minus the supplier's reminder lead (`1` day when the
//! supplier has no override row) lands on TODAY — the reminder goes out on the morning of the
//! due window, exactly once (the stamp), and never again until the window next matches.
//!
//! G7 (silent skip): a company with NO settings row or `send_reminder=false` is skipped without
//! an error — reminders are opt-out at the company level. A supplier with no override row gets
//! the enabled default (remind, 1 day); a supplier override with `receipt_reminder_email=false`
//! is excluded from the claim.
//!
//! Buying has NO mail stack: the job publishes `PurchaseReceiptReminderDue` per PO — the event IS
//! the notification port; a composing service wires the actual email send.
//!
//! **Per-company handler**: under FORCE RLS a job cannot enumerate companies, so the host
//! enumerates its companies and calls [`send_receipt_reminder`] once per company
//! (ADR-0008) — `app.company_id` is bound on the claim connection and the RLS fence stays
//! meaningful inside the job. [`send_receipt_reminder_for_companies`] is the thin wrapper for
//! hosts that just want the fan-out.

use sqlx::{PgPool, Row};
use tracing::warn;
use uuid::Uuid;

use backbone_orm::company_scope;

use crate::application::service::buying_events::{BuyingEvent, PurchaseReceiptReminderDue};
use crate::application::service::BuyingWriteService;

/// Upper bound on POs reminded in one per-company run. The `single_transaction` posture means one
/// commit; a pathological tenant cannot wedge the job into an unbounded transaction. The overflow
/// is NOT lost — unclaimed rows are still unstamped, so the next run (manual or cron) picks them
/// up while their window still matches.
pub const REMINDER_CLAIM_CAP: i64 = 500;

/// One per-company run's counters.
#[derive(Debug, Clone, Default)]
pub struct ReminderReport {
    /// POs claimed + stamped + published this run.
    pub reminded: usize,
    /// True when the company gate (G7) skipped the run: no settings row, or `send_reminder=false`.
    pub company_skipped: bool,
    /// Claimed rows that could not be stamped (unexpected) — logged, not fatal.
    pub stamp_failures: usize,
}

/// One due PO, as claimed.
struct DueOrder {
    order_id: Uuid,
    supplier_id: Uuid,
    schedule_date: chrono::NaiveDate,
}

/// Run the receipt-reminder sweep for ONE company. Publishes one
/// `PurchaseReceiptReminderDue` per reminded PO — strictly AFTER the claim transaction commits,
/// never ahead of the durable `last_reminder_on` stamp. The caller passes the write service (its
/// sink is the publication port, its settings read is the G7 gate).
pub async fn send_receipt_reminder(
    pool: &PgPool,
    service: &BuyingWriteService,
    company_id: Uuid,
) -> Result<ReminderReport, sqlx::Error> {
    company_scope::with_company_scope(Some(company_id), async move {
        let mut report = ReminderReport::default();

        // G7: the company gate. No settings row, or reminders off → silent skip (Ok, zero emitted).
        let settings = service.company_purchase_settings().await?;
        match settings {
            Some(s) if !s.send_reminder => {
                report.company_skipped = true;
                return Ok(report);
            }
            _ => {}
        }

        // The claim + stamp: ONE transaction. SKIP LOCKED keeps concurrent replicas disjoint; the
        // `last_reminder_on` predicate keeps a sequential double-run (manual overlap + cron) from
        // re-reminding the same PO on the same day.
        let mut due: Vec<DueOrder> = Vec::new();
        let mut tx = pool.begin().await?;
        company_scope::bind_company_on(&mut tx, company_id).await?;
        let claimed = sqlx::query(
            r#"SELECT po.id, po.supplier_id, po.schedule_date
                 FROM buying.purchase_orders po
                 LEFT JOIN buying.supplier_reminder_settings srs
                        ON srs.company_id = po.company_id
                       AND srs.supplier_id = po.supplier_id
                       AND (srs.metadata->>'deleted_at') IS NULL
                WHERE po.status='purchase'::purchase_order_status
                  AND po.acknowledged = false
                  AND po.receipt_status <> 'full'::purchase_receipt_status
                  AND po.schedule_date IS NOT NULL
                  AND po.schedule_date - COALESCE(srs.reminder_days_before, 1) = CURRENT_DATE
                  AND COALESCE(srs.receipt_reminder_email, true)
                  AND COALESCE(po.metadata->>'last_reminder_on', '') <> CURRENT_DATE::text
                  AND (po.metadata->>'deleted_at') IS NULL
                LIMIT $1
                FOR UPDATE OF po SKIP LOCKED"#,
        )
        .bind(REMINDER_CLAIM_CAP)
        .fetch_all(&mut *tx)
        .await?;

        for row in &claimed {
            let order_id: Uuid = row.get("id");
            let stamped = sqlx::query(
                r#"UPDATE buying.purchase_orders
                      SET metadata = jsonb_set(metadata, '{last_reminder_on}', to_jsonb(CURRENT_DATE::text))
                    WHERE id=$1"#,
            )
            .bind(order_id)
            .execute(&mut *tx)
            .await?;
            if stamped.rows_affected() == 1 {
                due.push(DueOrder {
                    order_id,
                    supplier_id: row.get("supplier_id"),
                    schedule_date: row.get("schedule_date"),
                });
            } else {
                report.stamp_failures += 1;
                warn!(target: "buying.reminder", order_id = %order_id, "reminder stamp affected no rows; skipped");
            }
        }
        tx.commit().await?;
        report.reminded = due.len();

        // Events strictly after the commit — never ahead of the durable stamp.
        let sink = service.event_sink();
        for d in &due {
            sink.publish(BuyingEvent::PurchaseReceiptReminderDue(PurchaseReceiptReminderDue {
                order_id: d.order_id,
                company_id,
                supplier_id: d.supplier_id,
                schedule_date: d.schedule_date,
            }));
        }
        Ok(report)
    }).await
}

/// The host-driven fan-out: run the sweep for each company in turn. Companies are named by the
/// HOST (the job cannot self-enumerate under FORCE RLS); a failure for one company is reported,
/// not fatal to the rest.
pub async fn send_receipt_reminder_for_companies(
    pool: &PgPool,
    service: &BuyingWriteService,
    companies: &[Uuid],
) -> Vec<(Uuid, Result<ReminderReport, sqlx::Error>)> {
    let mut out = Vec::with_capacity(companies.len());
    for company_id in companies {
        let r = send_receipt_reminder(pool, service, *company_id).await;
        out.push((*company_id, r));
    }
    out
}
