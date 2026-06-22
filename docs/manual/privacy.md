# Privacy & security

GeleitMail is **local-first** and **privacy-first**: no middleman, no telemetry, no tracking. (We
don't claim "your mail never leaves your device" — it comes from, and goes to, your provider's
servers. What we promise is that *we* add nothing in between.)

## No telemetry, no tracking

GeleitMail never phones home. It connects to exactly two things: your provider's **incoming (IMAP)**
and **outgoing (SMTP)** servers, which you configure. There's no analytics, no usage reporting, and
no third-party services. (This is even enforced in the build, so it can't creep in.)

## Encrypted on this device

Your local copy of mail — messages, attachments, drafts, and the search index — is **encrypted at
rest**: on disk it's unreadable ciphertext. The key lives in your operating system's keychain and is
applied automatically when GeleitMail opens, so you never type a separate passphrase. Your account
password is kept in the keychain too, never in plain text.

## Remote content is blocked by default

Messages written in rich (HTML) formatting are shown formatted — colors, fonts, layout, links — in a
**protected view**: scripts can't run, and **remote images and trackers don't load**. So opening a
message can't quietly fetch anything or run code.

When a message contained remote content, you'll see a **"Remote content blocked"** note with a
**Load remote images** button. Nothing remote loads until you choose it — and only for that one
message, that one time. (Scripts are never run, even then.) This keeps "read receipts" and tracking
pixels from firing without your say-so.

## Reading offline

Because your mail lives on your own device, you can read everything you've synced with no internet
connection. **Refresh** is the only thing that needs the network.

For the developer-facing security review, see [`../security-review.md`](../security-review.md).
