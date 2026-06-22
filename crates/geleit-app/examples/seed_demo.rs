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
    // The store orders Inbox first, so a realistic rail still opens to the Inbox.
    for f in ["INBOX", "Sent", "Archive", "Junk", "Trash"] {
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
    }
    s.set_setting("theme", if dark { "dark" } else { "light" })
        .unwrap();
    println!(
        "seeded {} messages into {path} (theme={})",
        demo.len(),
        if dark { "dark" } else { "light" }
    );
}
