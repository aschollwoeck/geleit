//! The desktop's notification service — `org.freedesktop.Notifications` over D-Bus.
//!
//! Transport glue, and excluded from mutation testing for the same reason `os_secret.rs` is: there is
//! nothing here but a call to something outside the process, and no mutant of it can be caught without
//! a live desktop. The **decisions** — what a notification says, and whether to raise one at all —
//! are pure and live in `geleit-app/src/notify.rs`, where they stay mutation-tested.
//!
//! Spoken through **zbus**, which is already in the tree (the secret-service keyring backend is built
//! on it), so notifications cost no new dependency and no C bindings.
//!
//! Notifications are **clickable**: each is sent with the `default` action, and a background thread
//! holds the same connection open to receive the daemon's `ActionInvoked` signal — so a click brings
//! the app forward (`set_on_activate`). That's why the connection is now persistent (one socket + one
//! thread for the app's life) rather than opened per notification.
use crate::notify::{Notification, Notifier, NotifyError};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// What to run when the user clicks a notification (bring the app forward). Shared between the notifier
/// and its listener thread.
type OnActivate = Arc<Mutex<Option<Box<dyn Fn() + Send + Sync>>>>;

/// The real one: `org.freedesktop.Notifications` over the session bus.
pub struct DesktopNotifier {
    app_name: String,
    /// Held open for the app's life so the action listener below can receive `ActionInvoked`. `None`
    /// when there's no session bus (a headless run) — then notifications simply report `Unavailable`.
    conn: Option<zbus::blocking::Connection>,
    /// The ids of notifications **we** raised, so the listener only acts on *our* clicks, not every
    /// app's. Capped so a long session of un-clicked notifications can't grow it without bound.
    ours: Arc<Mutex<HashSet<u32>>>,
    /// What to do when one of ours is clicked (set by the app: bring the window forward).
    on_activate: OnActivate,
}

impl Default for DesktopNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl DesktopNotifier {
    #[must_use]
    pub fn new() -> Self {
        let conn = zbus::blocking::Connection::session().ok();
        let ours = Arc::new(Mutex::new(HashSet::new()));
        let on_activate: OnActivate = Arc::new(Mutex::new(None));
        if let Some(c) = conn.clone() {
            spawn_action_listener(c, ours.clone(), on_activate.clone());
        }
        Self {
            app_name: "GeleitMail".to_owned(),
            conn,
            ours,
            on_activate,
        }
    }
}

impl Notifier for DesktopNotifier {
    fn notify(&self, n: &Notification) -> Result<(), NotifyError> {
        let conn = self.conn.as_ref().ok_or(NotifyError::Unavailable)?;
        let reply = conn
            .call_method(
                Some("org.freedesktop.Notifications"),
                "/org/freedesktop/Notifications",
                Some("org.freedesktop.Notifications"),
                "Notify",
                // (app_name, replaces_id, app_icon, summary, body, actions, hints, expire_timeout)
                &(
                    self.app_name.as_str(),
                    0u32, // replaces_id: 0 = a new notification, never overwrite another app's
                    "mail-unread", // a stock icon name; desktops that don't have it show none
                    n.summary.as_str(),
                    n.body.as_str(),
                    // The `default` action makes the whole notification clickable. Its label is shown by
                    // daemons that render actions as buttons and ignored by click-to-activate ones. A
                    // `Vec` (not a `[_; N]` array — that serialises as a D-Bus struct, not the `as` the
                    // Notify signature wants).
                    vec!["default", "Open GeleitMail"],
                    // `desktop-entry` lets the desktop match this notification to our .desktop file — so
                    // the user's OWN per-app controls ("show on the lock screen", "show message content")
                    // apply to GeleitMail. Without it, their settings silently don't reach us.
                    std::collections::HashMap::from([(
                        "desktop-entry",
                        zbus::zvariant::Value::from("GeleitMail"),
                    )]),
                    -1i32, // let the desktop decide how long it stays
                ),
            )
            .map_err(|_| NotifyError::Unavailable)?;
        // Remember the id the daemon assigned, so the listener knows this click is ours to handle.
        if let Ok(id) = reply.body().deserialize::<u32>() {
            let mut set = self.ours.lock().expect("notify id set");
            if set.len() >= 256 {
                set.clear(); // ancient un-clicked notifications are gone from screen anyway
            }
            set.insert(id);
        }
        Ok(())
    }

    fn set_on_activate(&self, on_activate: Box<dyn Fn() + Send + Sync>) {
        *self.on_activate.lock().expect("on_activate") = Some(on_activate);
    }
}

/// One thread for the app's life, blocking on the daemon's `ActionInvoked(id, action_key)` signal. When
/// a notification **we** sent is activated, run the registered callback (bring the app forward). Any
/// other app's notification clicks are ignored (their ids aren't in `ours`).
fn spawn_action_listener(
    conn: zbus::blocking::Connection,
    ours: Arc<Mutex<HashSet<u32>>>,
    on_activate: OnActivate,
) {
    std::thread::spawn(move || {
        let Ok(proxy) = zbus::blocking::Proxy::new(
            &conn,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        ) else {
            return;
        };
        let Ok(signals) = proxy.receive_signal("ActionInvoked") else {
            return;
        };
        for msg in signals {
            let Ok((id, _action)) = msg.body().deserialize::<(u32, String)>() else {
                continue;
            };
            // Remove-and-test: only fire for a notification we raised, and only once.
            let is_ours = ours.lock().expect("notify id set").remove(&id);
            if is_ours {
                if let Some(cb) = on_activate.lock().expect("on_activate").as_ref() {
                    cb();
                }
            }
        }
    });
}
