//! Desktop notifications — telling the user that mail arrived, once, quietly.
//!
//! A trait, so the app never talks to D-Bus directly and the tests never need a desktop: exactly the
//! shape of [`crate::secret::SecretStore`], for the same reason.
//!
//! On Linux the real implementation speaks `org.freedesktop.Notifications` over D-Bus. That protocol
//! is what every desktop notification on the system already uses; it is spoken through **zbus**, which
//! is already in the dependency tree (the secret-service keyring backend is built on it), so notifying
//! costs no new dependency and no C bindings.
use std::sync::Mutex;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NotifyError {
    #[error("the desktop's notification service can't be reached")]
    Unavailable,
}

/// One notification: what the user sees on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// The bold line — a sender, or "3 new messages".
    pub summary: String,
    /// The quieter second line — a subject, or a list of senders. May be empty.
    pub body: String,
}

/// Somewhere to send a notification.
pub trait Notifier: Send + Sync {
    /// Raise it. Best-effort by nature: a desktop with no notification service is not an error the
    /// user needs to hear about while reading their mail.
    ///
    /// # Errors
    /// [`NotifyError::Unavailable`] when the desktop's notification service can't be reached.
    fn notify(&self, n: &Notification) -> Result<(), NotifyError>;
}

/// An in-memory notifier for tests: records what would have been shown.
#[derive(Debug, Default)]
pub struct FakeNotifier {
    sent: Mutex<Vec<Notification>>,
    /// Refuse every notification, as a desktop whose notification service isn't up yet does. Without
    /// this the "the mail is still owed if the desktop wouldn't show it" guarantee can't be tested at
    /// all — and that guarantee is the difference between a lost notification and a lost message.
    fails: bool,
}

impl FakeNotifier {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A notifier that refuses everything — a session whose notification service hasn't started.
    #[must_use]
    pub fn failing() -> Self {
        Self {
            fails: true,
            ..Self::default()
        }
    }

    /// Everything raised so far, in order.
    ///
    /// # Panics
    /// If the lock is poisoned (a test thread panicked while holding it).
    #[must_use]
    pub fn sent(&self) -> Vec<Notification> {
        self.sent.lock().expect("lock").clone()
    }
}

impl Notifier for FakeNotifier {
    fn notify(&self, n: &Notification) -> Result<(), NotifyError> {
        if self.fails {
            return Err(NotifyError::Unavailable);
        }
        self.sent.lock().expect("lock").push(n.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{FakeNotifier, Notification, Notifier};

    #[test]
    fn a_notifier_that_refuses_reports_it_and_shows_nothing() {
        // The desktop's notification service isn't always up (a session 30 seconds after login isn't).
        // The caller must be able to tell — because a notification that was never shown is a message
        // the user still hasn't been told about, and settling that debt would lose it for good.
        let f = super::FakeNotifier::failing();
        let n = Notification {
            summary: "Alice".to_owned(),
            body: "Lunch?".to_owned(),
        };
        assert!(f.notify(&n).is_err());
        assert!(f.sent().is_empty(), "nothing was shown");
    }

    #[test]
    fn the_fake_notifier_records_what_would_have_been_shown() {
        let f = FakeNotifier::new();
        assert!(f.sent().is_empty());
        let n = Notification {
            summary: "Alice Baker".to_owned(),
            body: "Lunch on Thursday?".to_owned(),
        };
        f.notify(&n).unwrap();
        assert_eq!(f.sent(), vec![n]);
    }
}
