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
    assert!(!err.is_empty());
    // calm + PII-free: neither the address nor the password leaks into the message (P2)
    assert!(
        !err.contains("test.local") && !err.contains("secretpw123"),
        "err leaks PII: {err}"
    );
}
