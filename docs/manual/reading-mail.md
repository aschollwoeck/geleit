# Reading your mail

> Early days: GeleitMail can add an account, show and open your mail, and refresh. Writing
> messages arrives shortly.

## Adding your account

The first time you open GeleitMail, it asks you to add an account. Fill in:

- **Email** — your address (e.g. `you@example.com`).
- **Display name** — optional, how your name should read.
- **IMAP server** and **Port** — your provider's incoming-mail server (often `imap.<provider>`
  on port `993`). Your provider's help pages list these.
- **Username** — usually your full email address.
- **Password** — your mail password (some providers ask you to create an *app password* for other
  mail apps).

Choose **Connect**. GeleitMail signs in, downloads your inbox, and shows your mail. Your details
stay on your own device — your password is saved securely in your system keychain, so it's
remembered the next time you open GeleitMail.

> If your keychain is locked (some setups lock it until you sign in), the first Refresh may ask you
> to re-enter your password via the same form.

When you open GeleitMail you see three areas, left to right:

- **Your folders** on the left (Inbox, Sent, and so on). Click one to switch to it.
- **Your messages** in the middle — newest first. Each row shows who it's from, the subject, a
  short preview, and the date. A small dot marks messages you haven't read yet, and a paperclip
  shows when a message has an attachment.
- **The reading area** on the right, where the message you pick opens.

When several messages are part of the same back-and-forth, a small **conversation · N** marker shows
how many messages are in that thread.

## Opening a message

Click a message and it opens on the right, showing the subject, who it's from, the date, and the
text of the message. The row you're reading is marked with a soft highlight and a coloured edge.

Opening a message marks it as read, and its unread dot disappears. If you'd like to come back to it
later, choose **Mark as unread** at the top of the reading area to bring the dot back.

If the message has files attached, an **Attachments** list appears under the message showing each
file's name and size. (Saving attachments to your computer is coming soon.)

Messages written in rich (HTML) formatting are shown **formatted**, in a protected view that has
**remote images, trackers, and scripts already stripped out** — so opening a message can't quietly
load anything from the internet or run code. Plain-text messages show as before.

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

## Removing an account

To make GeleitMail forget an account on this device, choose **Remove account** at the bottom of the
folder list and confirm. This deletes the local copy of that account's mail and its saved password
from your system keychain. **Your mail stays safe on the server** — removing the account here only
clears the copy on this device, and you can add the account again any time to re-download it.
