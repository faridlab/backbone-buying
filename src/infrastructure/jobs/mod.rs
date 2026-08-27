//! Scheduled jobs (hand-authored, user-owned).
//!
//! The module's single scheduled job lives here: `send_receipt_reminder` — the daily supplier
//! receipt-reminder sweep declared in `schema/hooks/index.hook.yaml` under `scheduled_jobs`.

pub mod send_receipt_reminder;

pub use send_receipt_reminder::{
    send_receipt_reminder, send_receipt_reminder_for_companies, ReminderReport,
};
