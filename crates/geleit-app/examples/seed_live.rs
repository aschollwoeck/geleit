//! Dev seam: point a throwaway mailbox at the **local Dovecot** so the app can be driven against a
//! real IMAP server by hand (screenshots, live checks). Never used by the app or the test suite.
//!
//! ```text
//! GELEIT_DB=/tmp/geleit-live.db cargo run -p geleit-app --example seed_live --features dangerous-tls
//! GELEIT_DB=/tmp/geleit-live.db cargo run -p geleit-app --features dangerous-tls
//! ```
//!
//! Requires the local Dovecot from `docs/technical/…` (user `geleittest` / `testpass123`, self-signed
//! certificate — hence `dangerous-tls`). Never point this at a real account: see the testing memo.
use geleit_engine::sync_actions::{build_settings, build_smtp_settings, run_setup};
use geleit_platform::os_secret::OsSecretStore;
use geleit_platform::secret::SecretStore;

fn main() {
    let path = std::env::var("GELEIT_DB").expect("set GELEIT_DB");
    let _ = std::fs::remove_file(&path);
    let secrets = OsSecretStore::new();
    // The app's own key, so the seeded mailbox is the one it opens (it makes one on first launch).
    secrets
        .get("geleit-db", "key")
        .expect("keychain read")
        .expect("no geleit-db key yet — launch the app once to create it");

    let (email, imap) = build_settings(
        "geleittest@localhost",
        "127.0.0.1",
        "993",
        "geleittest",
        true,
    )
    .expect("valid imap settings");
    let smtp = build_smtp_settings("127.0.0.1", "465", false).expect("valid smtp settings");
    let acc = run_setup(
        &path,
        &secrets,
        &email,
        Some("Geleit Test"),
        imap,
        smtp,
        "",
        "testpass123",
    )
    .expect("setup + first sync against local Dovecot");
    println!("seeded account {acc} into {path}");
}
