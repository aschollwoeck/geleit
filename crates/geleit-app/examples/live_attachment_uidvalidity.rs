//! Live check for the UIDVALIDITY guard on the single-attachment save path (READ-8) against the local
//! Dovecot — never part of the test suite.
//!
//! ```text
//! cargo run -p geleit-app --example live_attachment_uidvalidity --features dangerous-tls
//! ```
//!
//! `run_fetch_attachment` pulls one attachment by uid on demand. If the server reset its UIDVALIDITY
//! since we synced, the stored uid names a different message — so the fetch must refuse rather than save
//! the wrong message's attachment. This appends a message with a known attachment, then:
//!   1. fetches it with the **synced** validity → the right attachment comes back,
//!   2. corrupts the stored validity (as a reset would) → the fetch is refused.
//!
//! Requires the local Dovecot (`geleittest` / `testpass123`, self-signed → `dangerous-tls`). In-memory
//! secret store, no keychain. Never point it at a real account.
use geleit_engine::sync_actions::{
    account_imap, build_settings, build_smtp_settings, run_fetch_attachment, run_refresh,
    run_setup, runtime,
};
use geleit_platform::secret::InMemorySecretStore;

fn main() {
    let path = format!("/tmp/geleit-live-attuidv-{}.db", std::process::id());
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

    const FILENAME: &str = "att-uidv-proof.bin";
    const PAYLOAD: &str = "R0VMRUlUQVRUUFJPT0Y="; // base64("GELEITATTPROOF")
    let raw = format!(
        "From: tester@localhost\r\nTo: geleittest@localhost\r\nSubject: att uidvalidity test\r\n\
         MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"GB\"\r\n\r\n\
         --GB\r\nContent-Type: text/plain\r\n\r\nbody\r\n\
         --GB\r\nContent-Type: application/octet-stream\r\n\
         Content-Disposition: attachment; filename=\"{FILENAME}\"\r\n\
         Content-Transfer-Encoding: base64\r\n\r\n{PAYLOAD}\r\n--GB--\r\n"
    );
    let config = account_imap(&path, &secrets, acc).expect("imap config");
    let rt = runtime().expect("runtime");
    rt.block_on(geleit_engine::imap::append_message(
        &config,
        &secrets,
        "INBOX",
        "(\\Seen)",
        raw.as_bytes(),
    ))
    .expect("APPEND");

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
        .find(|m| m.subject.as_deref() == Some("att uidvalidity test"))
        .and_then(|m| m.uid)
        .expect("appended message synced with a uid") as u32;

    // 1. Correct (synced) validity → the right attachment.
    let (name, _data) = run_fetch_attachment(&path, &secrets, acc, "INBOX", uid, 0)
        .expect("fetch with the synced validity should succeed");
    assert_eq!(name.as_deref(), Some(FILENAME), "got the right attachment");
    println!("✓ synced validity: fetched the attachment ({FILENAME})");

    // 2. Corrupt the stored validity (as a server UID reset would leave it) → the fetch must refuse
    //    rather than hand back whatever message now wears that uid.
    store.set_folder_uidvalidity(inbox.id, -1).unwrap();
    let refused = run_fetch_attachment(&path, &secrets, acc, "INBOX", uid, 0);
    assert!(
        refused.is_err(),
        "a UIDVALIDITY mismatch must refuse the fetch, not save the wrong message's attachment"
    );
    println!("✓ validity mismatch: the attachment fetch was refused (no wrong-message save)");
    println!("\nattachment UIDVALIDITY-guard live check passed.");
    let _ = std::fs::remove_file(&path);
}
