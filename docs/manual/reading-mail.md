# Reading your mail

This page covers signing in and reading. See also: [writing](writing-mail.md),
[organizing](organizing-mail.md), [searching](searching-mail.md),
[multiple accounts](accounts.md), [keyboard & appearance](shortcuts-and-appearance.md), and
[privacy](privacy.md).

## Adding your account

The first time you open GeleitMail, it asks you to add an account. Fill in:

- **Email** — your address (e.g. `you@example.com`).
- **Display name** — optional, how your name should read on messages you send.
- **IMAP server** and **Port** — your provider's incoming-mail server (often `imap.<provider>`
  on port `993`). Your provider's help pages list these.
- **Username** — usually your full email address.
- **Password** — your mail password (some providers ask you to create an *app password* for other
  mail apps).
- **Outgoing (SMTP) server**, **Port**, and **STARTTLS** — your provider's sending server (often
  `smtp.<provider>` on port `465`, or `587` with STARTTLS). Used when you send.
- **Signature** — optional; appended to messages you write.

![The Add-account screen, with fields for email, IMAP and SMTP servers, and password.](images/setup.png)

Choose **Connect**. GeleitMail signs in, downloads your inbox, and shows your mail. Your details
stay on your own device — your password is saved securely in your system keychain, so it's
remembered the next time you open GeleitMail.

> Sign-in is manual IMAP/SMTP for now. One-click Gmail/Outlook (OAuth) is planned.

> If your keychain is locked (some setups lock it until you sign in), the first Refresh may ask you
> to re-enter your password via the same form.

When you open GeleitMail you see three areas, left to right:

- **Your folders** on the left (Inbox, Sent, and so on). Click one to switch to it.
- **Your messages** in the middle — newest first. Each row shows who it's from, the subject, a
  short preview, and the date. A small dot marks messages you haven't read yet, and a paperclip
  shows when a message has an attachment.
- **The reading area** on the right, where the message you pick opens.

![The main window: folder rail on the left, the inbox list with unread dots, stars, and attachment markers in the middle, and the reading area on the right.](images/inbox-light.png)

Drag the divider between the message list and the reading area to make either wider; your choice is
remembered next time you open GeleitMail.

When several messages are part of the same back-and-forth, a small **conversation · N** marker shows
how many messages are in that thread.

## Opening a message

Click a message and it opens on the right, showing the subject, who it's from, the date, and the
text of the message. The row you're reading is marked with a soft highlight and a coloured edge.

![An open message in the reading area, with Reply, Reply all, Forward, Star, Archive, Delete, Move, Spam, and Mark-as-unread actions across the top.](images/reading.png)

Opening a message marks it as read, and its unread dot disappears. If you'd like to come back to it
later, choose **Mark as unread** at the top of the reading area to bring the dot back.

If the message has files attached, an **Attachments** list appears under the message showing each
file's name and size. (Saving attachments to your computer is coming soon.)

Messages written in rich (HTML) formatting are shown **formatted** — with their colors, fonts,
layout, images, and links intact. GeleitMail renders them itself, entirely on your device: there's
no embedded browser, **scripts can't run, and trackers/remote images aren't loaded** until you ask,
so opening a message can't quietly fetch anything from the internet or execute code. Links stay
clickable — choosing one opens it in your normal web browser. Plain-text messages show as before.

When a message *did* contain remote content, you'll see a small **"Remote content blocked"** note
with a **Load images** button. Nothing remote loads until you choose to: click it and GeleitMail
fetches that one message's images and re-shows it with them in place. (Scripts are never run, even
then, and only that message's images are fetched.)

## Saving and opening message files

You can save any message to a standard **`.eml`** file and open `.eml` files back into GeleitMail —
handy for archiving a message, moving one between computers, or sharing it.

- **Save:** with a message open, choose **Save** at the top of the reading area, pick where to put it,
  and GeleitMail writes the message (subject, sender/recipients, date, and the text + formatted
  bodies) as a `.eml` file that any mail program can also open.
- **Open:** choose **Open mail file…** at the bottom-left, pick a `.eml`, and it appears in a local
  **Saved** folder and opens in the reading area — formatted exactly like your other mail. The Saved
  folder stays on your device and is never uploaded to your provider.

## Writing, replying, and organizing

Choose **New message** on the left to compose, or use **Reply**, **Reply all**, and **Forward** at
the top of an open message. See [writing your mail](writing-mail.md) for the full details (Cc,
attachments, formatting, signatures, and drafts), and [organizing your mail](organizing-mail.md) for
starring, archiving, moving, folders, and acting on several messages at once.

## Getting new mail

Choose **Refresh** at the top of the message list to fetch new mail from your provider. While it
works, the button reads *Refreshing…* and a quiet line shows what's happening — *Checking for new
mail…*, then *Catching up…* with a count as older messages download in the background. The app
stays responsive the whole time, so you can keep reading. When it finishes, the list updates with
anything new. If it can't reach your provider, a short message explains what to try; your existing
mail stays put.

Everything you see is kept on your own device, so the list stays fast and works offline — refresh is
the moment GeleitMail talks to your provider to catch up.

## Your data is encrypted on this device

The local copy of your mail is **encrypted at rest** — on disk it's unreadable ciphertext. The key
is kept in your operating system's keychain and applied automatically when GeleitMail opens, so you
never type a separate passphrase. (If you wipe the keychain, the local copy can't be opened; just
add the account again to re-download it.)

## Reading offline

Because your mail lives on your own device, you can read everything you've synced with no internet
connection. Refresh is the only thing that needs the network; the rest keeps working offline.

## More than one account

GeleitMail can hold several accounts at once — see [accounts](accounts.md) for adding more, switching
between them, and removing one.
