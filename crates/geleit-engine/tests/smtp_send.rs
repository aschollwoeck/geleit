//! S4.1 — SMTP transport verified end-to-end against a **self-contained in-process SMTP sink**.
//! Unlike the live IMAP tests, this needs no external server and no TLS, so it runs in CI: a tokio
//! listener speaks minimal plaintext SMTP and captures what `smtp::send` actually delivered.

use std::sync::{Arc, Mutex};

use geleit_engine::smtp::{self, SmtpSecurity, SmtpSettings};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

#[derive(Default, Debug)]
struct Captured {
    auth: String,
    mail_from: String,
    rcpt_to: Vec<String>,
    data: String,
}

/// Accept one connection and speak just enough SMTP to receive a message, capturing the envelope,
/// auth, and body.
async fn run_sink(listener: TcpListener, cap: Arc<Mutex<Captured>>) {
    let (sock, _) = listener.accept().await.unwrap();
    let (rd, mut wr) = sock.into_split();
    let mut rd = BufReader::new(rd);
    wr.write_all(b"220 sink ESMTP\r\n").await.unwrap();
    let mut line = String::new();
    loop {
        line.clear();
        if rd.read_line(&mut line).await.unwrap() == 0 {
            break;
        }
        let cmd = line.trim_end().to_owned();
        let upper = cmd.to_ascii_uppercase();
        if upper.starts_with("EHLO") || upper.starts_with("HELO") {
            wr.write_all(b"250-sink\r\n250 AUTH PLAIN\r\n")
                .await
                .unwrap();
        } else if upper.starts_with("AUTH") {
            cap.lock().unwrap().auth = cmd;
            wr.write_all(b"235 2.7.0 OK\r\n").await.unwrap();
        } else if upper.starts_with("MAIL FROM") {
            cap.lock().unwrap().mail_from = cmd;
            wr.write_all(b"250 OK\r\n").await.unwrap();
        } else if upper.starts_with("RCPT TO") {
            cap.lock().unwrap().rcpt_to.push(cmd);
            wr.write_all(b"250 OK\r\n").await.unwrap();
        } else if upper.starts_with("DATA") {
            wr.write_all(b"354 send data\r\n").await.unwrap();
            let mut body = String::new();
            let mut l = String::new();
            loop {
                l.clear();
                if rd.read_line(&mut l).await.unwrap() == 0 || l == ".\r\n" || l == ".\n" {
                    break;
                }
                body.push_str(&l);
            }
            cap.lock().unwrap().data = body;
            wr.write_all(b"250 OK queued\r\n").await.unwrap();
        } else if upper.starts_with("QUIT") {
            wr.write_all(b"221 Bye\r\n").await.unwrap();
            break;
        } else {
            wr.write_all(b"250 OK\r\n").await.unwrap();
        }
    }
}

#[tokio::test]
async fn delivers_message_with_envelope_and_auth() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let cap = Arc::new(Mutex::new(Captured::default()));
    let server = tokio::spawn(run_sink(listener, cap.clone()));

    let settings = SmtpSettings {
        host: "127.0.0.1".into(),
        port,
        username: "alice".into(),
        security: SmtpSecurity::Plaintext,
        allow_invalid_certs: false,
    };
    let env = smtp::envelope("alice@test.local", &["bob@test.local".into()]).unwrap();
    let msg = b"From: alice@test.local\r\nTo: bob@test.local\r\nSubject: Hello\r\n\r\nHi Bob.\r\n";
    smtp::send(&settings, "s3cr3t", &env, msg).await.unwrap();

    server.await.unwrap();
    let c = cap.lock().unwrap();
    assert!(
        c.mail_from.contains("alice@test.local"),
        "MAIL FROM: {}",
        c.mail_from
    );
    assert!(
        c.rcpt_to.iter().any(|r| r.contains("bob@test.local")),
        "RCPT TO: {:?}",
        c.rcpt_to
    );
    assert!(c.data.contains("Subject: Hello"), "DATA: {}", c.data);
    assert!(c.data.contains("Hi Bob."), "DATA: {}", c.data);
    assert!(!c.auth.is_empty(), "AUTH should have been presented");
}

#[tokio::test]
async fn builds_and_sends_a_drafted_message_end_to_end() {
    use geleit_engine::message::{self, Draft};

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let cap = Arc::new(Mutex::new(Captured::default()));
    let server = tokio::spawn(run_sink(listener, cap.clone()));

    let draft = Draft {
        from_name: Some("Alice".into()),
        from_addr: "alice@test.local".into(),
        to: vec!["bob@test.local".into()],
        cc: vec!["carol@test.local".into()],
        subject: "Lunch?".into(),
        body_text: "Are you free at noon?".into(),
        ..Default::default()
    };
    let bytes = message::build(&draft).unwrap();
    let env = smtp::envelope(&draft.from_addr, &message::recipients(&draft)).unwrap();
    let settings = SmtpSettings {
        host: "127.0.0.1".into(),
        port,
        username: "alice".into(),
        security: SmtpSecurity::Plaintext,
        allow_invalid_certs: false,
    };
    smtp::send(&settings, "pw", &env, &bytes).await.unwrap();

    server.await.unwrap();
    let c = cap.lock().unwrap();
    // both To and Cc become envelope recipients
    assert!(
        c.rcpt_to.iter().any(|r| r.contains("bob@test.local")),
        "{:?}",
        c.rcpt_to
    );
    assert!(
        c.rcpt_to.iter().any(|r| r.contains("carol@test.local")),
        "{:?}",
        c.rcpt_to
    );
    assert!(c.data.contains("Subject: Lunch?"), "DATA: {}", c.data);
    assert!(c.data.contains("Are you free at noon?"), "DATA: {}", c.data);
}

#[tokio::test]
async fn unreachable_server_yields_a_calm_error() {
    let settings = SmtpSettings {
        host: "127.0.0.1".into(),
        port: 1, // nothing listening
        username: "x".into(),
        security: SmtpSecurity::Plaintext,
        allow_invalid_certs: false,
    };
    let env = smtp::envelope("a@test.local", &["b@test.local".into()]).unwrap();
    let err = smtp::send(
        &settings,
        "secretpw123",
        &env,
        b"From: a@test.local\r\n\r\nx\r\n",
    )
    .await
    .unwrap_err();
    assert!(!err.message.is_empty());
    // An unreachable server is the ordinary offline case — RETRYABLE, so the outbox will queue it.
    assert!(
        !err.permanent,
        "can't-connect must not be treated as a rejection"
    );
    // calm + PII-free: neither the address nor the password leaks into the message (P2)
    assert!(
        !err.message.contains("test.local") && !err.message.contains("secretpw123"),
        "err leaks PII: {}",
        err.message
    );
}

// ---- Outbox (SEND-10): queue-on-failure + the permanent/retryable split ----

use geleit_engine::localstore::open_store;
use geleit_engine::sync_actions::{run_flush_outbox, run_send, SendStatus};
use geleit_platform::secret::{InMemorySecretStore, SecretStore};
use geleit_store::{ImapSettings, SmtpConfig, SmtpSecurityKind};

/// A sink that accepts the conversation but **rejects the recipient** with a 550 — a permanent error.
async fn run_rejecting_sink(listener: TcpListener) {
    let (sock, _) = listener.accept().await.unwrap();
    let (rd, mut wr) = sock.into_split();
    let mut rd = BufReader::new(rd);
    wr.write_all(b"220 sink ESMTP\r\n").await.unwrap();
    let mut line = String::new();
    loop {
        line.clear();
        if rd.read_line(&mut line).await.unwrap() == 0 {
            break;
        }
        let upper = line.trim_end().to_ascii_uppercase();
        if upper.starts_with("EHLO") || upper.starts_with("HELO") {
            wr.write_all(b"250-sink\r\n250 AUTH PLAIN\r\n")
                .await
                .unwrap();
        } else if upper.starts_with("AUTH") {
            wr.write_all(b"235 2.7.0 OK\r\n").await.unwrap();
        } else if upper.starts_with("MAIL FROM") {
            wr.write_all(b"250 OK\r\n").await.unwrap();
        } else if upper.starts_with("RCPT TO") {
            wr.write_all(b"550 5.1.1 no such user\r\n").await.unwrap(); // permanent rejection
        } else if upper.starts_with("QUIT") {
            wr.write_all(b"221 Bye\r\n").await.unwrap();
            break;
        } else {
            wr.write_all(b"250 OK\r\n").await.unwrap();
        }
    }
}

#[tokio::test]
async fn a_rejected_recipient_is_a_permanent_error_so_the_outbox_will_not_queue_it() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(run_rejecting_sink(listener));

    let settings = SmtpSettings {
        host: "127.0.0.1".into(),
        port,
        username: "alice".into(),
        security: SmtpSecurity::Plaintext,
        allow_invalid_certs: false,
    };
    let env = smtp::envelope("alice@test.local", &["ghost@test.local".into()]).unwrap();
    let err = smtp::send(&settings, "pw", &env, b"From: a\r\n\r\nx\r\n")
        .await
        .unwrap_err();
    let _ = server.await;
    assert!(
        err.permanent,
        "a 5xx rejection must be permanent, so send never queues it"
    );
}

/// Build an encrypted store + a fully-configured account whose SMTP points at `smtp_port`.
fn account_at(smtp_port: u16) -> (String, InMemorySecretStore, i64) {
    let dir = std::env::temp_dir().join(format!(
        "geleit-outbox-{}-{}",
        std::process::id(),
        smtp_port
    ));
    let _ = std::fs::create_dir_all(&dir);
    let db = dir.join("mail.db").to_string_lossy().into_owned();
    let _ = std::fs::remove_file(&db);
    let secrets = InMemorySecretStore::new();
    let store = open_store(&db, &secrets).unwrap();
    let imap = ImapSettings {
        host: "127.0.0.1".into(),
        port: 993,
        username: "user@test.local".into(),
        allow_invalid_certs: true,
    };
    let acc = store
        .add_imap_account("user@test.local", Some("User"), &imap)
        .unwrap();
    store
        .update_smtp_settings(
            acc,
            &SmtpConfig {
                host: "127.0.0.1".into(),
                port: smtp_port,
                security: SmtpSecurityKind::Implicit,
            },
        )
        .unwrap();
    secrets
        .set("geleit-imap", "user@test.local", b"pw")
        .unwrap();
    (db, secrets, acc)
}

#[test]
fn a_send_that_cannot_reach_the_server_is_queued_and_stays_queued_until_it_can_go_out() {
    // Nothing listening on this port → the send can't connect → the message is queued, not lost, not
    // errored. (The connection fails before any TLS, so it doesn't matter that the account is TLS.)
    let (db, secrets, acc) = account_at(1); // port 1: closed
    let status = run_send(
        &db,
        &secrets,
        acc,
        "bob@test.local",
        "",
        "Offline hello",
        "Sent from a train.",
        None,
        Vec::new(),
        Vec::new(),
        false,
        None,
    )
    .expect("send should queue, not error");
    assert_eq!(status, SendStatus::Queued);

    let store = open_store(&db, &secrets).unwrap();
    assert_eq!(
        store.outbox_counts().unwrap(),
        (1, 0),
        "one message waiting"
    );
    let queued = store.pending_outbox(acc).unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].subject, "Offline hello");
    assert!(queued[0].recipients.iter().any(|r| r == "bob@test.local"));
    assert!(!queued[0].raw.is_empty());
    drop(store);

    // Draining while still offline leaves it queued — retried, never dropped.
    assert_eq!(run_flush_outbox(&db, &secrets, acc).unwrap(), 0);
    let store = open_store(&db, &secrets).unwrap();
    assert_eq!(
        store.outbox_counts().unwrap(),
        (1, 0),
        "still waiting after a failed drain"
    );

    let _ = std::fs::remove_file(&db);
}
