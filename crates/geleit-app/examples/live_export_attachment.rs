//! Live check for SEC-4 export completeness against the local Dovecot — never part of the test suite.
//!
//! ```text
//! cargo run -p geleit-app --example live_export_attachment --features dangerous-tls
//! ```
//!
//! Proves the one new network path — pulling raw originals so an export keeps **attachments**. The store
//! never holds attachment bytes (only parsed body text), so a faithful backup must fetch them. This:
//!   1. APPENDs a multipart message with a recognisable attachment to INBOX,
//!   2. syncs (so the message + its uid land in the store — the store still has no attachment bytes),
//!   3. runs `run_fetch_folder_raws` (exactly what the export uses) and asserts the returned raw carries
//!      the attachment filename + payload that the store lacked.
//!
//! Requires the local Dovecot (`geleittest` / `testpass123`, self-signed → `dangerous-tls`). In-memory
//! secret store, no keychain. Never point it at a real account.
use geleit_engine::sync_actions::{
    account_imap, build_settings, build_smtp_settings, run_fetch_folder_raws, run_refresh,
    run_setup, runtime,
};
use geleit_platform::secret::InMemorySecretStore;

fn main() {
    let path = format!("/tmp/geleit-live-export-{}.db", std::process::id());
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

    // A multipart/mixed message with a plain part and an attachment. FILENAME + PAYLOAD are unique
    // tokens the store never keeps (it only parses body text) — so finding them proves the raw fetch.
    const FILENAME: &str = "geleit-export-proof.bin";
    const PAYLOAD: &str = "R0VMRUlURVhQT1JUUFJPT0Y="; // base64("GELEITEXPORTPROOF")
    let raw = format!(
        "From: tester@localhost\r\nTo: geleittest@localhost\r\n\
         Subject: SEC-4 export attachment test\r\nMIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"GBND\"\r\n\r\n\
         --GBND\r\nContent-Type: text/plain\r\n\r\nbody the store keeps\r\n\
         --GBND\r\nContent-Type: application/octet-stream\r\n\
         Content-Disposition: attachment; filename=\"{FILENAME}\"\r\n\
         Content-Transfer-Encoding: base64\r\n\r\n{PAYLOAD}\r\n--GBND--\r\n"
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
    .expect("APPEND the attachment message");

    // Sync so the envelope + uid are stored (the store still has NO attachment bytes).
    run_refresh(&path, &secrets, acc, "INBOX").expect("sync INBOX");
    let store = geleit_engine::localstore::open_store(&path, &secrets).expect("open store");
    let inbox = store
        .folders_for_account(acc)
        .unwrap()
        .into_iter()
        .find(|f| f.name.eq_ignore_ascii_case("INBOX"))
        .expect("INBOX");
    let uids = store.folder_uids(inbox.id).expect("folder uids");
    assert!(
        !uids.is_empty(),
        "the appended message should have synced with a uid"
    );

    // The exact call the export makes: pull the raw originals from the server.
    let raws = run_fetch_folder_raws(&path, &secrets, acc, "INBOX", &uids)
        .expect("server reachable — fetch should return a map, not None");
    let found = raws.values().any(|bytes| {
        let text = String::from_utf8_lossy(bytes);
        text.contains(FILENAME) && text.contains(PAYLOAD)
    });
    assert!(
        found,
        "the fetched raw must carry the attachment (filename + payload) the store never stored"
    );
    println!(
        "✓ export completeness: the raw pulled from the server carries the attachment ({FILENAME})"
    );
    println!("\nSEC-4 export-attachment live check passed.");
    let _ = std::fs::remove_file(&path);
}
