//! `geleit-platform` — abstractions over OS / external capabilities implemented differently
//! per platform (OS keychain, OAuth loopback) or per UI host (HTML rendering).
//!
//! UI-agnostic by construction: every trait uses only std/primitive types. Real per-OS
//! implementations land in later milestones; this crate defines the seams (ADR-0004) and ships
//! in-memory / no-op doubles so the rest of the workspace can be written and tested now. Must
//! never depend on UI code (constitution P4, ADR-0003).

pub mod html;
pub mod notify;
pub mod oauth;
pub mod os_notify;
pub mod os_secret;
pub mod secret;
