//! Throwaway dev tool: seed a demo encrypted DB with a fake account + a few messages, so the app can
//! be launched for documentation screenshots WITHOUT a real account or network. NOT shipped/tested.
//! It reads the EXISTING db key from the keychain (never overwrites it). Usage:
//!   GELEIT_DB=/tmp/geleit-demo.db cargo run -p geleit-app --example seed_demo -- [--dark]
use geleit_platform::os_secret::OsSecretStore;
use geleit_platform::secret::SecretStore;
use geleit_store::{NewMessage, Store};

fn main() {
    let path = std::env::var("GELEIT_DB").expect("set GELEIT_DB");
    let dark = std::env::args().any(|a| a == "--dark");
    let secrets = OsSecretStore::new();
    let key = secrets
        .get("geleit-db", "key")
        .expect("keychain read")
        .expect("no geleit-db key yet — launch the app once to create it");
    let s = Store::open_encrypted(&path, &key).expect("open encrypted demo db");
    let acc = s.add_account("you@example.com", Some("You")).unwrap();
    // The store orders Inbox first, so a realistic rail still opens to the Inbox. "Work" is a
    // user-created folder (not a protected one), so it shows the rename/delete options (ORG-6).
    for f in ["INBOX", "Sent", "Archive", "Junk", "Trash", "Work"] {
        s.upsert_folder(acc, f).unwrap();
    }
    let inbox = s.upsert_folder(acc, "INBOX").unwrap();
    // (subject, from_name, from_addr, body, seen, flagged, attach, day)
    let demo = [
        ("Welcome to GeleitMail", "The GeleitMail Team", "hello@geleit.app",
         "Thanks for trying GeleitMail — a calm, private, local-first mail client. Everything you see here lives on your own device.", false, true, false, 20i64),
        ("Lunch on Thursday?", "Alice Baker", "alice@example.com",
         "Are we still on for lunch Thursday at the usual place? Let me know what time works.", false, false, false, 19),
        ("Q3 report — draft attached", "Bob Carter", "bob@work.example",
         "Here's the draft of the quarterly report for your review. The numbers are looking good.", true, false, true, 18),
        ("Your invoice for June", "Vendor Billing", "billing@vendor.example",
         "Your invoice #4821 is ready. No action needed — this is just your receipt.", true, false, true, 17),
        ("Weekend hike photos", "Carol Diaz", "carol@example.com",
         "Sharing the photos from Saturday's hike — what a view from the top! Attached a few favourites.", true, true, true, 16),
        ("Re: project kickoff", "Dan Evans", "dan@work.example",
         "Sounds great. I'll set up the repo and send round an invite for the kickoff call.", true, false, false, 15),
    ];
    for (i, (subj, fname, faddr, body, seen, flagged, attach, day)) in demo.iter().enumerate() {
        let id = s
            .upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid: Some(i as i64 + 1),
                    subject: Some((*subj).to_owned()),
                    from_name: Some((*fname).to_owned()),
                    from_addr: Some((*faddr).to_owned()),
                    date: Some(1_718_000_000 + day * 86_400),
                    seen: *seen,
                    flagged: *flagged,
                    ..Default::default()
                },
            )
            .unwrap();
        let snippet: String = body.chars().take(80).collect();
        s.store_body(id, Some(body), None, Some(&snippet), *attach)
            .unwrap();
        // Give the attachment-flagged demo messages some attachment metadata so the reading pane's
        // attachment list has something to show (bytes aren't stored — the real save fetches them).
        if *attach {
            s.store_attachments(
                id,
                &[geleit_store::Attachment {
                    filename: Some(format!("attachment-{}.pdf", i + 1)),
                    content_type: "application/pdf".to_owned(),
                    size: 320_500 + (i as i64) * 12_000,
                }],
            )
            .unwrap();
        }
    }
    // An HTML newsletter with a LONG (wrapping) subject and a remote image — exercises the CPU HTML
    // renderer + the "remote content blocked" cue. Newest, so `GELEIT_SHOT=read` opens it.
    let hid = s
        .upsert_message(
            acc,
            inbox,
            &NewMessage {
                uid: Some(100),
                subject: Some(
                    "A rather long newsletter subject line that wraps onto a second line to test \
                     the reading pane and webview placement"
                        .to_owned(),
                ),
                from_name: Some("GeleitMail Newsletter".to_owned()),
                from_addr: Some("news@example.com".to_owned()),
                date: Some(1_718_000_000 + 21 * 86_400),
                ..Default::default()
            },
        )
        .unwrap();
    let html = r##"<!doctype html><html><body style="margin:0;background:#eef2f4;">
      <table width="100%" cellpadding="0" cellspacing="0" bgcolor="#eef2f4"><tr><td align="center">
        <table width="600" cellpadding="0" cellspacing="0" bgcolor="#ffffff" style="margin:24px auto;border-radius:8px;overflow:hidden;font-family:Arial,sans-serif;">
          <tr><td bgcolor="#1c7e7b" style="padding:24px;color:#ffffff;font-size:24px;font-weight:bold;">Acme Weekly</td></tr>
          <tr><td style="padding:24px;color:#222;font-size:15px;line-height:1.6;">
            <h2 style="color:#1c7e7b;margin:0 0 8px;">Your week in review</h2>
            <p>Hi there — here are this week's <b>highlights</b>. Thanks for being a subscriber!</p>
            <table width="100%" cellpadding="8"><tr>
              <td bgcolor="#f4f7f8" align="center" style="border-radius:6px;">Opens<br><b style="font-size:20px;">1,204</b></td>
              <td width="12"></td>
              <td bgcolor="#f4f7f8" align="center" style="border-radius:6px;">Clicks<br><b style="font-size:20px;">317</b></td>
            </tr></table>
            <p><img src="https://example.com/banner.png" alt="banner" width="520"></p>
            <p style="margin-top:16px;">
              <a href="https://example.com/read" style="background:#1c7e7b;color:#fff;padding:12px 22px;border-radius:6px;text-decoration:none;font-weight:bold;">Read more &rarr;</a>
            </p>
          </td></tr>
          <tr><td bgcolor="#222" style="padding:16px 24px;color:#9aa;font-size:12px;">&copy; Acme &middot; <a href="https://example.com/unsub" style="color:#9cc;">Unsubscribe</a></td></tr>
        </table>
      </td></tr></table>
    </body></html>"##;
    s.store_body(
        hid,
        Some("This month in GeleitMail — a few highlights from the latest release."),
        Some(html),
        Some("A few highlights from the latest release."),
        false,
    )
    .unwrap();
    // A couple of saved drafts so the Drafts overlay has content to show. The first carries a saved
    // attachment (bytes) so resuming it exercises the attachments-in-drafts round-trip.
    use geleit_store::{DraftAttachment, DraftContent};
    for (to, subject, body, attach) in [
        (
            "team@work.example",
            "Sprint notes",
            "Quick recap of what we shipped this sprint…",
            false,
        ),
        // Saved last → newest → what GELEIT_RESUME reopens; carries the attachment.
        (
            "alice@example.com",
            "Re: Lunch on Thursday?",
            "Thursday at noon works for me — see you then!",
            true,
        ),
    ] {
        let did = s
            .save_draft(
                acc,
                None,
                &DraftContent {
                    to: to.to_owned(),
                    subject: subject.to_owned(),
                    body: body.to_owned(),
                    ..Default::default()
                },
            )
            .unwrap();
        if attach {
            s.replace_draft_attachments(
                did,
                &[DraftAttachment {
                    filename: Some("agenda.pdf".to_owned()),
                    content_type: "application/pdf".to_owned(),
                    data: b"%PDF-1.4 demo agenda".to_vec(),
                }],
            )
            .unwrap();
        }
    }
    s.set_setting("theme", if dark { "dark" } else { "light" })
        .unwrap();
    if let Ok(w) = std::env::var("LISTW") {
        s.set_setting("list_width", &w).unwrap(); // test the persisted splitter width
    }
    println!(
        "seeded {} messages into {path} (theme={})",
        demo.len(),
        if dark { "dark" } else { "light" }
    );
}
