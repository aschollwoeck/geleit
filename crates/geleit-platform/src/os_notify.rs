//! The desktop's notification service — `org.freedesktop.Notifications` over D-Bus.
//!
//! Transport glue, and excluded from mutation testing for the same reason `os_secret.rs` is: there is
//! nothing here but a call to something outside the process, and no mutant of it can be caught without
//! a live desktop. The **decisions** — what a notification says, and whether to raise one at all —
//! are pure and live in `geleit-app/src/notify.rs`, where they stay mutation-tested.
//!
//! Spoken through **zbus**, which is already in the tree (the secret-service keyring backend is built
//! on it), so notifications cost no new dependency and no C bindings.
use crate::notify::{Notification, Notifier, NotifyError};

/// The real one: `org.freedesktop.Notifications` over the session bus.
pub struct DesktopNotifier {
    app_name: String,
}

impl Default for DesktopNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl DesktopNotifier {
    #[must_use]
    pub fn new() -> Self {
        Self {
            app_name: "GeleitMail".to_owned(),
        }
    }
}

impl Notifier for DesktopNotifier {
    fn notify(&self, n: &Notification) -> Result<(), NotifyError> {
        // A fresh connection per notification. Mail arrives every few minutes at most, so the cost is
        // irrelevant — and holding a bus connection open for the life of the app, for something used
        // this rarely, is a socket and a background task we'd have to keep healthy for nothing.
        let conn = zbus::blocking::Connection::session().map_err(|_| NotifyError::Unavailable)?;
        conn.call_method(
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
                Vec::<&str>::new(), // no actions: a notification you can click into is a later slice
                // `desktop-entry` is what lets the desktop match this notification to our .desktop
                // file — and therefore what makes the user's OWN per-app controls ("show on the lock
                // screen", "show message content") apply to GeleitMail. Without it, their settings
                // silently don't reach us.
                std::collections::HashMap::from([(
                    "desktop-entry",
                    zbus::zvariant::Value::from("GeleitMail"),
                )]),
                -1i32, // let the desktop decide how long it stays
            ),
        )
        .map_err(|_| NotifyError::Unavailable)?;
        Ok(())
    }
}
