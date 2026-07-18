//! Live check for the background full-mailbox backfill (SYNC-3) against the local Dovecot — never part of
//! the test suite.
//!
//! ```text
//! cargo run -p geleit-app --example live_backfill_all_folders --features dangerous-tls
//! ```
//!
//! Mirrors what `backfill.rs`'s worker does per account — enumerate the server folders (skipping the
//! local-only `Saved` folder) and `run_backfill` each — and proves it against a real server: every folder
//! backfills without error, and a second pass over INBOX reports **0** (it's complete and resumable, so a
//! finished folder is a cheap no-op — exactly what makes the hourly re-scan cheap).
//!
//! Requires the local Dovecot (`geleittest` / `testpass123`, self-signed → `dangerous-tls`). In-memory
//! secret store, no keychain. Never point it at a real account.
use geleit_engine::sync_actions::{build_settings, build_smtp_settings, run_backfill, run_setup};
use geleit_platform::secret::InMemorySecretStore;

fn main() {
    let path = format!("/tmp/geleit-live-backfill-{}.db", std::process::id());
    let _ = std::fs::remove_file(&path);
    let secrets = InMemorySecretStore::new();

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

    // Enumerate the server folders exactly as the worker does (skip the local-only Saved folder).
    let store = geleit_engine::localstore::open_store(&path, &secrets).expect("open store");
    let folders: Vec<String> = store
        .folders_for_account(acc)
        .unwrap()
        .into_iter()
        .filter(|f| !f.name.eq_ignore_ascii_case(geleit_store::SAVED_FOLDER))
        .map(|f| f.name)
        .collect();
    assert!(
        !folders.is_empty(),
        "the account should have server folders after setup"
    );
    println!("backfilling {} folders: {:?}", folders.len(), folders);

    for folder in &folders {
        run_backfill(&path, &secrets, acc, folder, 200, &mut |_| {})
            .unwrap_or_else(|e| panic!("backfill of {folder} failed: {e}"));
    }
    println!("✓ every server folder backfilled without error");

    // Resumable: a second pass over a completed folder pulls nothing more (this is what keeps the hourly
    // re-scan cheap).
    let again = run_backfill(&path, &secrets, acc, "INBOX", 200, &mut |_| {})
        .expect("second INBOX backfill");
    assert_eq!(
        again, 0,
        "a fully-backfilled folder fetches nothing on the next pass"
    );
    println!("✓ resumable: a completed folder is a cheap no-op on the next round");
    println!("\nbackground-backfill live check passed.");
    let _ = std::fs::remove_file(&path);
}
