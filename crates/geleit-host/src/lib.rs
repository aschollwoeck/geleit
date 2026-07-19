//! GeleitMail's host-agnostic core (ADR-0014).
//!
//! This crate holds [`AppState`] and the *logic* of every command behind the IPC seam. A host —
//! [`geleit-app`](../geleit_app/index.html) (Tauri desktop) or `geleit-server` (localhost web) — is a
//! thin adapter over it: it constructs an `AppState`, implements [`Shell`] for its environment, and
//! forwards each incoming request to the matching function in [`commands`]. The two hosts therefore
//! run byte-for-byte the same mail logic and cannot drift apart.
//!
//! It depends on the engine crates but on no host and no UI — the boundary that `check-boundary.sh`
//! guards. The host-specific bits (emit an event, set the unread badge) are injected through
//! [`Shell`]; native dialogs and the auto-updater stay in the hosts (see [`shell`]).
pub mod commands;
pub mod dto;
pub mod shell;
pub mod snooze;
pub mod worker;

pub use commands::AppState;
pub use shell::{NullShell, Shell};
