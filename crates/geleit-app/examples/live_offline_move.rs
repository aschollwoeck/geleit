//! Live check for OFF-4 (offline moves) against the local Dovecot — never part of the test suite.
//!
//! ```text
//! cargo run -p geleit-app --example live_offline_move --features dangerous-tls
//! ```
//!
//! It drives the real engine path end to end against a real IMAP server:
//!   1. sets up the throwaway `geleittest` account and syncs INBOX,
//!   2. queues a move to a **bogus** folder and flushes — the server is reachable but rejects the move,
//!      so it must be un-hidden (the message comes back rather than hiding forever behind an impossible
//!      move); nothing is lost,
//!   3. re-queues the same message to a **real** folder and flushes — now it must leave INBOX on the
//!      server and appear in the target (the reconnect case).
//!
//! Requires the local Dovecot (`geleittest` / `testpass123`, self-signed → `dangerous-tls`). Uses an
//! in-memory secret store, so it needs no keychain. Never point it at a real account.
use geleit_engine::sync_actions::{
    build_settings, build_smtp_settings, run_flush_moves, run_setup,
};
use geleit_platform::secret::InMemorySecretStore;

fn main() {
    let path = format!("/tmp/geleit-live-move-{}.db", std::process::id());
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
    println!("seeded account {acc}");

    // Sync INBOX so we have a real message with a server uid to move.
    geleit_engine::sync_actions::run_refresh(&path, &secrets, acc, "INBOX").expect("sync INBOX");

    let store = geleit_engine::localstore::open_store(&path, &secrets).expect("open store");
    let inbox = store
        .folders_for_account(acc)
        .unwrap()
        .into_iter()
        .find(|f| f.name.eq_ignore_ascii_case("INBOX"))
        .expect("INBOX folder");
    let target = store
        .folders_for_account(acc)
        .unwrap()
        .into_iter()
        .find(|f| !f.name.eq_ignore_ascii_case("INBOX"))
        .map(|f| f.name)
        .expect("account has some non-INBOX folder to move into");

    let before = store.messages_in_folder(inbox.id, 50).unwrap();
    let Some(msg) = before.first() else {
        eprintln!(
            "INBOX is empty — send a test message to geleittest first (the move needs a real message)."
        );
        std::process::exit(1);
    };
    let id = msg.id;
    let uid = msg.uid.expect("a synced INBOX message has a uid");
    println!(
        "moving message id={id} uid={uid} (subject {:?})",
        msg.subject
    );
    println!("target folder: {target}");

    // ---- 1. refusal case: move to a folder the server does NOT have ------------------------------
    store
        .queue_move(id, "No Such Folder \u{2014} offline test")
        .unwrap();
    assert!(
        store
            .messages_in_folder(inbox.id, 50)
            .unwrap()
            .iter()
            .all(|m| m.id != id),
        "a queued move must hide the message from INBOX immediately"
    );
    let pushed = run_flush_moves(&path, &secrets, acc).expect("flush (bogus target)");
    assert_eq!(
        pushed, 0,
        "a move the server rejects must not count as pushed"
    );
    assert!(
        store.pending_moves(acc).unwrap().is_empty(),
        "a refused move must be un-hidden, not left queued forever"
    );
    assert!(
        store
            .messages_in_folder(inbox.id, 50)
            .unwrap()
            .iter()
            .any(|m| m.id == id),
        "the refused message must reappear in INBOX — never lost, never hidden forever"
    );
    println!(
        "✓ refusal case: server refused the move, message came back to INBOX (not stuck hidden)"
    );

    // ---- 2. reconnect case: re-aim at a real folder and flush ------------------------------------
    store.queue_move(id, &target).unwrap();
    let pushed = run_flush_moves(&path, &secrets, acc).expect("flush (real target)");
    assert_eq!(pushed, 1, "the move to a real folder must reach the server");
    assert!(
        store.pending_moves(acc).unwrap().is_empty(),
        "a landed move must clear from the queue"
    );

    // Re-sync both folders from the server and confirm the server agrees: gone from INBOX, in target.
    geleit_engine::sync_actions::run_refresh(&path, &secrets, acc, "INBOX").expect("re-sync INBOX");
    geleit_engine::sync_actions::run_refresh(&path, &secrets, acc, &target)
        .expect("re-sync target");
    let store = geleit_engine::localstore::open_store(&path, &secrets).expect("reopen store");
    let inbox_now = store.messages_in_folder(inbox.id, 50).unwrap();
    assert!(
        inbox_now.iter().all(|m| m.uid != Some(uid)),
        "the moved message must be gone from INBOX on the server"
    );
    let tgt = store
        .folders_for_account(acc)
        .unwrap()
        .into_iter()
        .find(|f| f.name == target)
        .unwrap();
    let in_target = store.messages_in_folder(tgt.id, 50).unwrap();
    assert!(
        in_target.iter().any(|m| m.subject == msg.subject),
        "the moved message must now be in {target} on the server"
    );
    println!("✓ reconnect case: move reached the server — gone from INBOX, present in {target}");
    println!("\nOFF-4 live check passed.");
    let _ = std::fs::remove_file(&path);
}
