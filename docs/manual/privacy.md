# Privacy & security

GeleitMail is **local-first** and **privacy-first**: no middleman, no telemetry, no tracking. (We
don't claim "your mail never leaves your device" — it comes from, and goes to, your provider's
servers. What we promise is that *we* add nothing in between.)

## No telemetry, no tracking

GeleitMail never phones home. It connects to exactly two things: your provider's **incoming (IMAP)**
and **outgoing (SMTP)** servers, which you configure. There's no analytics, no usage reporting, and
no third-party services. (This is even enforced in the build, so it can't creep in.)

## Encrypted on this device

Your local copy of mail — messages, attachments, and the search index — is **encrypted at
rest**: on disk it's unreadable ciphertext. The key lives in your operating system's keychain and is
applied automatically when GeleitMail opens, so you never type a separate passphrase. Your account
password is kept in the keychain too, never in plain text.

## Drafts stay on your device

An unsent draft is often the most private thing in your mailbox, so GeleitMail keeps drafts **on this
device only**, encrypted like the rest of your mail. Nothing you haven't sent is uploaded to your
provider.

If you want your drafts on your other devices, you can choose to share them: turn on **Sync drafts to
your provider** in **Settings → Privacy**. It's **off by default**, and while it's off no draft ever
leaves your machine.

Be clear about the trade you're making when you turn it on: a synced draft is stored on your
provider's server like any other mail there, so your provider — and anyone who can get into your
account — can read it, even though you never sent it. Turning the setting back off removes those
copies from the server again.

## Remote content is blocked by default

Messages written in rich (HTML) formatting are shown formatted — colors, fonts, layout, links — in a
**protected view**: scripts can't run, and **remote images and trackers don't load**. So opening a
message can't quietly fetch anything or run code.

When a message contained remote content, you'll see a **"Remote content blocked"** note with a
**Load images** button. Nothing remote loads until you choose it — and only for that one
message, that one time. (Scripts are never run, even then.) This keeps "read receipts" and tracking
pixels from firing without your say-so.

![A message in the sandboxed HTML view with a "Remote content blocked — Load images" bar above it.](images/reading.png)

## Reading offline

Because your mail lives on your own device, you can read everything you've synced with no internet
connection. Talking to your provider is the only part that needs the network — that means the
automatic check for new mail (every few minutes, in the background) and **Refresh** when you ask for
it. Nothing else about your mail leaves this machine.

For the developer-facing security review, see [`../security-review.md`](../security-review.md).
