//! Live check for background folder reconciliation (`run_reconcile_folder`) against the local Dovecot —
//! never part of the test suite.
//!
//! ```text
//! cargo run -p geleit-app --example live_pull_flags --features dangerous-tls
//! ```
//!
//! The scheduler only reconciles INBOX; the background backfill worker now calls `run_reconcile_folder`
//! per folder so *every* folder stays in step with changes made on **another device**. This proves both
//! halves: (1) a `\Flagged` set on the server after a sync reaches the local star, and (2) delete-first —
//! a message deleted on the server is *pruned* locally, never flipped to an unread ghost.
//!
//! Requires the local Dovecot (`geleittest` / `testpass123`, self-signed → `dangerous-tls`). In-memory
//! secret store, no keychain. Never point it at a real account.
use geleit_engine::sync_actions::{
    account_imap, build_settings, build_smtp_settings, run_reconcile_folder, run_refresh,
    run_setup, runtime,
};
use geleit_platform::secret::InMemorySecretStore;

fn local_flagged(path: &str, secrets: &InMemorySecretStore, acc: i64, uid: u32) -> bool {
    let store = geleit_engine::localstore::open_store(path, secrets).expect("open store");
    let inbox = store
        .folders_for_account(acc)
        .unwrap()
        .into_iter()
        .find(|f| f.name.eq_ignore_ascii_case("INBOX"))
        .expect("INBOX");
    store
        .messages_in_folder(inbox.id, 200)
        .unwrap()
        .into_iter()
        .find(|m| m.uid == Some(i64::from(uid)))
        .map(|m| m.flagged)
        .expect("message present")
}

fn main() {
    let path = format!("/tmp/geleit-live-pullflags-{}.db", std::process::id());
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

    let config = account_imap(&path, &secrets, acc).expect("imap config");
    let rt = runtime().expect("runtime");

    // Pick a message and establish a known baseline: unflagged on the server, then synced.
    run_refresh(&path, &secrets, acc, "INBOX").expect("sync INBOX");
    let store = geleit_engine::localstore::open_store(&path, &secrets).expect("open store");
    let inbox = store
        .folders_for_account(acc)
        .unwrap()
        .into_iter()
        .find(|f| f.name.eq_ignore_ascii_case("INBOX"))
        .expect("INBOX");
    let uid = store
        .messages_in_folder(inbox.id, 200)
        .unwrap()
        .into_iter()
        .find_map(|m| m.uid)
        .expect("INBOX has a message with a uid") as u32;
    drop(store);

    rt.block_on(geleit_engine::imap::set_flag(
        &config, &secrets, "INBOX", uid, false,
    ))
    .expect("clear \\Flagged on server");
    run_refresh(&path, &secrets, acc, "INBOX").expect("re-sync to baseline");
    assert!(
        !local_flagged(&path, &secrets, acc, uid),
        "baseline: unflagged locally"
    );

    // A star made on ANOTHER device: set it on the server *after* the sync, so the local copy is stale.
    rt.block_on(geleit_engine::imap::set_flag(
        &config, &secrets, "INBOX", uid, true,
    ))
    .expect("set \\Flagged on server");
    assert!(
        !local_flagged(&path, &secrets, acc, uid),
        "still stale locally until we pull — proves the pull is what does it"
    );

    // The new background path.
    let changed = run_reconcile_folder(&path, &secrets, acc, "INBOX").expect("reconcile folder");
    assert!(
        changed >= 1,
        "the reconcile reported at least one changed flag row"
    );
    assert!(
        local_flagged(&path, &secrets, acc, uid),
        "after run_reconcile_folder the star made elsewhere is reflected locally"
    );
    println!("✓ background flag reconcile: a server-side star reached the local store");

    // Clean up: clear the flag on the server again so the mailbox is left as we found it.
    rt.block_on(geleit_engine::imap::set_flag(
        &config, &secrets, "INBOX", uid, false,
    ))
    .expect("cleanup: clear the flag");

    // Delete-first: a message deleted on the server (as in webmail) must be *pruned* by the reconcile,
    // not flipped to unread. Append a throwaway, sync it in, delete it on the server, reconcile, and
    // confirm it's gone locally.
    let marker = format!("reconcile-prune-{}", std::process::id());
    let raw =
        format!("From: t@localhost\r\nTo: geleittest@localhost\r\nSubject: {marker}\r\n\r\nx\r\n");
    rt.block_on(geleit_engine::imap::append_message(
        &config,
        &secrets,
        "INBOX",
        "(\\Seen)",
        raw.as_bytes(),
    ))
    .expect("APPEND throwaway");
    run_refresh(&path, &secrets, acc, "INBOX").expect("sync the throwaway in");
    let store = geleit_engine::localstore::open_store(&path, &secrets).expect("open");
    let inbox = store
        .folders_for_account(acc)
        .unwrap()
        .into_iter()
        .find(|f| f.name.eq_ignore_ascii_case("INBOX"))
        .unwrap();
    let throwaway_uid = store
        .messages_in_folder(inbox.id, 200)
        .unwrap()
        .into_iter()
        .find(|m| m.subject.as_deref() == Some(marker.as_str()))
        .and_then(|m| m.uid)
        .expect("throwaway synced with a uid") as u32;
    drop(store);
    rt.block_on(geleit_engine::imap::delete_permanently(
        &config,
        &secrets,
        "INBOX",
        throwaway_uid,
    ))
    .expect("delete the throwaway on the server");
    run_reconcile_folder(&path, &secrets, acc, "INBOX").expect("reconcile after server delete");
    let store = geleit_engine::localstore::open_store(&path, &secrets).expect("reopen");
    let still_there = store
        .messages_in_folder(inbox.id, 200)
        .unwrap()
        .into_iter()
        .any(|m| m.subject.as_deref() == Some(marker.as_str()));
    assert!(
        !still_there,
        "a server-deleted message must be pruned by the reconcile, not left as a ghost"
    );
    println!(
        "✓ delete-first: a message deleted on the server was pruned locally (no unread ghost)"
    );

    println!("\nbackground folder-reconcile live check passed.");
    let _ = std::fs::remove_file(&path);
}
