//! Whether the desktop has a **system-tray host** — a `StatusNotifierWatcher` on the session bus.
//!
//! This is the one fact the tray needs before it's safe to make *closing the window hide it* rather
//! than quit: with no host, an app-indicator icon registers on D-Bus but nothing paints it, so a
//! hidden window would have no icon to bring it back — a trap. Spoken through the same blocking `zbus`
//! the notifier uses (no async runtime pulled in — see the crate's zbus feature note).

/// `true` if a `StatusNotifierWatcher` currently owns its bus name — i.e. something on this desktop
/// will actually show a tray icon (KDE, XFCE, MATE, Cinnamon, GNOME + AppIndicator extension). `false`
/// on a bare session bus (vanilla GNOME) or when there's no session bus at all. Conservative: any
/// error answers `false`, so an uncertain environment keeps the safe close-quits behaviour.
///
/// A single synchronous `NameHasOwner` round-trip, called once at startup. It's un-timed, so in theory
/// a present-but-hung session bus could stall the caller — but `NameHasOwner` is a sub-millisecond
/// bus-daemon local lookup (no third party in the loop), so in practice it returns instantly.
#[must_use]
pub fn host_present() -> bool {
    let Ok(conn) = zbus::blocking::Connection::session() else {
        return false;
    };
    conn.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        "NameHasOwner",
        &"org.kde.StatusNotifierWatcher",
    )
    .ok()
    .and_then(|reply| reply.body().deserialize::<bool>().ok())
    .unwrap_or(false)
}
