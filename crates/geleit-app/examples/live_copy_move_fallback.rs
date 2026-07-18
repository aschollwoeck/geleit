//! Live check for the non-`MOVE`-server fallback against the local Dovecot — never part of the suite.
//!
//! ```text
//! cargo run -p geleit-app --example live_copy_move_fallback --features dangerous-tls
//! ```
//!
//! Servers without the `MOVE` extension (RFC 6851) need the portable equivalent — COPY the message to the
//! target, mark the source copy `\Deleted`, `UID EXPUNGE` it. Dovecot *does* support `MOVE`, so this uses
//! the `move_message_via_copy` seam to force the fallback path and proves it lands a message just like
//! `UID MOVE` would: after the move the message is **gone from INBOX** and **present in the target**.
//!
//! Requires the local Dovecot (`geleittest` / `testpass123`, self-signed → `dangerous-tls`). In-memory
//! secret store, no keychain. Never point it at a real account.
use geleit_engine::sync_actions::{
    account_imap, build_settings, build_smtp_settings, run_refresh, run_setup, runtime,
};
use geleit_platform::secret::InMemorySecretStore;

fn main() {
    let path = format!("/tmp/geleit-live-copymove-{}.db", std::process::id());
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

    // A uniquely-subjected message so we can find it before and after the move.
    let marker = format!("copy-move-fallback-{}", std::process::id());
    let raw = format!(
        "From: tester@localhost\r\nTo: geleittest@localhost\r\nSubject: {marker}\r\n\r\nmove me\r\n"
    );
    rt.block_on(geleit_engine::imap::append_message(
        &config,
        &secrets,
        "INBOX",
        "(\\Seen)",
        raw.as_bytes(),
    ))
    .expect("APPEND the test message");

    run_refresh(&path, &secrets, acc, "INBOX").expect("sync INBOX");
    let store = geleit_engine::localstore::open_store(&path, &secrets).expect("open store");
    let inbox = folder_named(&store, acc, "INBOX");
    let target_name = store
        .folders_for_account(acc)
        .unwrap()
        .into_iter()
        .find(|f| !f.name.eq_ignore_ascii_case("INBOX"))
        .map(|f| f.name)
        .expect("account has a non-INBOX folder to move into");

    let msg = store
        .messages_in_folder(inbox.id, 200)
        .unwrap()
        .into_iter()
        .find(|m| m.subject.as_deref() == Some(marker.as_str()))
        .expect("the appended message should be in INBOX");
    let uid = msg.uid.expect("a synced message has a uid") as u32;
    println!("moving uid {uid} ({marker}) via the COPY fallback → {target_name}");

    // Force the non-MOVE path against real Dovecot.
    rt.block_on(geleit_engine::imap::move_message_via_copy(
        &config,
        &secrets,
        "INBOX",
        uid,
        &target_name,
    ))
    .expect("COPY+delete+expunge fallback");

    // The server should now agree: expunged from INBOX, present in the target.
    run_refresh(&path, &secrets, acc, "INBOX").expect("re-sync INBOX");
    run_refresh(&path, &secrets, acc, &target_name).expect("re-sync target");
    let store = geleit_engine::localstore::open_store(&path, &secrets).expect("reopen store");
    let inbox = folder_named(&store, acc, "INBOX");
    let target = store
        .folders_for_account(acc)
        .unwrap()
        .into_iter()
        .find(|f| f.name == target_name)
        .unwrap();
    let still_in_inbox = store
        .messages_in_folder(inbox.id, 200)
        .unwrap()
        .into_iter()
        .any(|m| m.subject.as_deref() == Some(marker.as_str()));
    let in_target = store
        .messages_in_folder(target.id, 200)
        .unwrap()
        .into_iter()
        .any(|m| m.subject.as_deref() == Some(marker.as_str()));
    assert!(
        !still_in_inbox,
        "the fallback must remove the message from INBOX (UID EXPUNGE)"
    );
    assert!(
        in_target,
        "the fallback must land the message in {target_name} (UID COPY)"
    );
    println!("✓ non-MOVE fallback: COPY+delete+expunge landed the message — gone from INBOX, in {target_name}");
    println!("\nnon-MOVE fallback live check passed.");
    let _ = std::fs::remove_file(&path);
}

fn folder_named(store: &geleit_store::Store, acc: i64, name: &str) -> geleit_store::Folder {
    store
        .folders_for_account(acc)
        .unwrap()
        .into_iter()
        .find(|f| f.name.eq_ignore_ascii_case(name))
        .expect("folder exists")
}
