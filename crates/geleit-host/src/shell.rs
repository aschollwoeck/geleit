//! The one seam between the host-agnostic command logic and the host it runs in (ADR-0014).
//!
//! Everything a command needs from its host that *isn't* data — pushing a progress/new-mail event to
//! the frontend, and reflecting the unread count in the window chrome — goes through this trait. The
//! Tauri desktop shell implements it against an `AppHandle` (emit over the webview bridge, set the
//! window title + tray tooltip); the web server implements it against an SSE broadcast (and, having no
//! OS window, treats the badge as just another event). This is the same dependency-injection the
//! store already uses for `SecretStore`/`Notifier`, so the logic stays testable with a no-op fake.
//!
//! Native file dialogs are deliberately *not* here: they already shell out to `zenity`/`kdialog`
//! (a subprocess, not a Tauri API), so they are host-agnostic as-is. The auto-updater is *also* not
//! here — it is inherently Tauri, so it stays in the desktop host and the web host stubs it.

/// The host-specific side-effects a command may need. `Send + Sync` so it can be shared across the
/// blocking threads and detached workers the commands spawn.
pub trait Shell: Send + Sync {
    /// Push a named event to the frontend. Payload is JSON so one method covers every event
    /// (`sync-progress`/`mail-arrived` carry an integer; `update-available` an object).
    fn emit(&self, event: &str, payload: serde_json::Value);

    /// Reflect the total unread count in the host's chrome. `title` is the fully-formatted window
    /// title (e.g. `"GeleitMail — 3 unread"`); a windowless host may surface it however it likes.
    fn set_badge(&self, title: &str);
}

/// A `Shell` that drops everything on the floor — for tests and for command paths that legitimately
/// have no host to talk to.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullShell;

impl Shell for NullShell {
    fn emit(&self, _event: &str, _payload: serde_json::Value) {}
    fn set_badge(&self, _title: &str) {}
}
