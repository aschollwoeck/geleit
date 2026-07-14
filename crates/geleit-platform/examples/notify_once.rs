//! Dev seam: raise one notification, to check the desktop actually shows it — the one thing the unit
//! tests structurally cannot see (they use the fake).
//!   cargo run -p geleit-platform --example notify_once
use geleit_platform::notify::{Notification, Notifier};
use geleit_platform::os_notify::DesktopNotifier;

fn main() {
    let n = Notification {
        summary: "Alice Baker".to_owned(),
        body: "Lunch on Thursday?".to_owned(),
    };
    match DesktopNotifier::new().notify(&n) {
        Ok(()) => println!("raised"),
        Err(e) => println!("not raised: {e}"),
    }
}
