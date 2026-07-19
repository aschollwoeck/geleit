//! The background workers — how mail keeps arriving, syncing, and backfilling without anyone pressing
//! anything. Each is host-agnostic: a top-level `run(state, …)` async loop (the host spawns it on its
//! own runtime) that drives the same `commands`/engine machinery a user-pressed Refresh does.
//!
//! - [`scheduler`] — the periodic sweep of every account's INBOX (recent mail, flag reconcile, outbox
//!   drain, notifications, badge). Needs a [`Shell`](crate::Shell) to emit `mail-arrived` + set the badge.
//! - [`idle`] — one IMAP IDLE connection per account; on a server push it wakes the scheduler. No Shell.
//! - [`backfill`] — the slow, low-priority catch-up of *every* folder of *every* account. No Shell.
//!
//! `notify` + `schedule` are the pure decision logic the scheduler acts on (kept private to this
//! module, and mutation-tested in place).
pub mod backfill;
pub mod idle;
pub mod scheduler;

mod notify;
mod schedule;

use crate::{AppState, Shell};
use std::sync::Arc;

/// Start every background worker for a host: the scheduler (needs a `Shell`), the IDLE watchers, and
/// the backfill. Each host calls this once at startup, spawning the returned futures on its runtime —
/// the desktop shell via `tauri::async_runtime::spawn`, the web host via `tokio::spawn`.
pub fn futures(
    state: AppState,
    shell: Arc<dyn Shell>,
) -> (
    impl std::future::Future<Output = ()>,
    impl std::future::Future<Output = ()>,
    impl std::future::Future<Output = ()>,
) {
    (
        scheduler::run(state.clone(), shell),
        idle::run(state.clone()),
        backfill::run(state),
    )
}
