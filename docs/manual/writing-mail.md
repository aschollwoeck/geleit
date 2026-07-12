# Writing your mail

See also: [reading](reading-mail.md) · [organizing](organizing-mail.md) ·
[accounts](accounts.md).

## A new message

Choose **Compose** on the left. Fill in:

- **To** — type an address and press **Enter** (or a comma) to turn it into a **chip**; add as many as
  you like, and remove one with its **✕**. You can also paste several comma-separated addresses at
  once. A repeated address is kept just once. As you type, GeleitMail suggests people you've had mail
  from — choose one to add it as a chip, or press **Esc** to dismiss the list.
- **Cc** — optional; the same chips and suggestions.
- **Subject** and the **message body**.

Then choose **Send**. GeleitMail sends through your account's outgoing (SMTP) server (set up when you
added the account) and saves a copy to your **Sent** folder. Sending happens in the background, so
the app stays responsive; if something goes wrong you get a short, plain explanation and the message
stays open to try again. To throw a message away, choose **Discard**.

![The compose window: From, To and Cc recipient chips, Subject, body, and a footer with Send, Attach, Discard, and a Markdown toggle.](images/compose.png)

## Replying and forwarding

Open a message and use the action buttons at the top of the reading area:

- **Reply** — writes back to the sender, quoting the original.
- **Reply all** — also includes everyone who was on the original (minus your own address).
- **Forward** — sends the message on to someone new.

Replies keep the conversation threaded, and the **from** address is automatically the account you're
reading in — including in the merged "All inboxes" view, where it's the account that received the
message.

## Attachments

In the compose window, choose **Attach** to pick one or more files with your system's file chooser.
Each attached file shows as a chip with its name; remove one with its **✕**. Attachments are included
when you send. (There's a size limit of 25 MB in total, so an over-large file is turned away with a
clear message rather than failing mid-send.)

## Markdown formatting

Turn on **Markdown** in the compose footer to write with light formatting — `**bold**`, `*italic*`,
lists, `> quotes`, links, and tables. GeleitMail sends both a formatted version and a plain-text
version, so people whose mail apps don't show formatting still get a perfectly readable message. The
toggle is off by default and resets for each new message.

## Your signature

Set a **Signature** per account in **Settings → Accounts** (see [accounts](accounts.md)); it's
appended to messages you compose, reply to, and forward from that account.
