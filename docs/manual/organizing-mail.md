# Organizing your mail

See also: [reading](reading-mail.md) · [searching](searching-mail.md).

Actions apply **instantly** to what you see, then sync to your provider in the background. If a sync
hiccup happens, the change simply reconciles on the next refresh — nothing is lost or duplicated.

## On an open message

The action buttons sit **across the top of the reading area**, above the sender and subject, so they
stay in the same place no matter how long the subject is:

- **Star** — flag a message you want to find again; the button fills in gold and the message shows a
  gold **★** on its list row. Choose it again to unstar.
- **Archive** — move it out of the inbox into your Archive folder.
- **Snooze** — hide the message until later. Pick a time — *later today, this evening, tomorrow, this
  weekend, next week* — and it leaves your inbox (and stops counting as unread) until then, when it comes
  back and notifies you as if it had just arrived. See **Snoozing** below.
- **Delete** — move it to Trash. When the message is **already in Trash**, this button reads **Delete
  forever** and permanently removes it (see below).
- **Move…** — pick a folder to move the message into. The message goes to the folder you picked,
  whatever it's called and whatever it's for.
- **Spam** — from the **Move…** menu, send a message to your Junk folder (or move it back to Inbox).
- **Mark as unread** — bring back the unread dot.

## Managing folders

You can make your own folders to sort your mail. At the bottom of the folder list:

- **New folder** — choose it, type a name, and choose **Create**. GeleitMail makes the folder on your
  provider and shows it in the list. Move messages into it with **Move…**.
- **Rename** or **Delete** — hover a folder you created and choose the **⋯** button. **Rename** lets
  you give it a new name (its messages come along). **Delete** removes the folder — and, after you
  confirm, **all the messages in it** — from your provider and this device; this can't be undone.

Your standard folders — Inbox, Sent, Drafts, Archive, Trash, Junk, and Saved, **under whatever names
your provider gives them** — are kept as they are, so they don't show the rename or delete options.

**Whatever your provider calls them.** GeleitMail asks your provider which folder is which, so if your
mail is in German, French, or anything else — *Entwürfe*, *Gesendet*, *Papierkorb* — they get the right
icons, sit in the right place in the folder list, and are protected from being renamed or deleted by
accident. They're also the folders GeleitMail actually **uses**: the copy of every message you send goes
to your real Sent folder, **Archive** and **Delete** move mail to your real Archive and Trash, **Empty
Trash** and **Delete forever** appear when you're in your real Trash, and **Drafts** shows the drafts
your provider is keeping for you. (For a provider that doesn't say, GeleitMail falls back to recognising
the usual English names.)

If your provider doesn't keep one of these folders at all — some have no Archive — GeleitMail tells you
so rather than putting your mail somewhere it doesn't belong. You can always use **Move…** to pick a
folder yourself.

## Emptying the Trash

Mail you delete goes to **Trash**, where it stays until you clear it.

- **Empty Trash** — open the Trash folder and choose the **trash-can button** in the message-list
  header. GeleitMail asks you to confirm, then permanently deletes every message in Trash from both
  your provider and this device.
- **Delete forever** — open a single message that's in Trash and choose **Delete forever** (or press
  **`#`**). After you confirm, just that one message is permanently removed.

Both are **irreversible** — there's no undo, which is why GeleitMail always asks first. Everywhere
else, deleting only moves mail to Trash, so you have a safety net until you choose to empty it.

The one exception is a **draft**, which isn't mail you've received: deleting a draft removes it straight
away rather than moving it to Trash. Deleting one that lives on your provider asks first, because that
copy is the only one there is — see [writing your mail](writing-mail.md).

## Acting on several messages at once

To handle a batch of mail together, **hover a message row** and a checkbox appears on the left —
choose it to select that message. To pick a whole run at once, select one message, then **Shift-click**
another: everything between them is selected. Once anything is selected, a small bar appears above the
list showing how many you've picked, with actions for all of them:

- **Archive** — move them all to Archive.
- **Delete** — move them all to Trash.
- **Mark read** / **Mark unread** — clear or bring back their unread dots.
- **Snooze** — hide them all until a time you pick (see below).
- **Clear** (the ✕) — deselect everything.

The checkbox at the far left of that bar **selects (or clears) every message in the list** at once.
Archive and Delete work just like single messages: the rows slide out with one **Undo** bar
(*3 archived · Undo*), and nothing is sent to your provider until the moment passes — so a bulk Undo
can never lose mail. Switching folders or accounts clears your selection.

## Snoozing

Some mail you can't act on yet — you're waiting on someone, or it's for the weekend. **Snooze** it and
it leaves your inbox until the time you choose, then comes back to the top as if it had just arrived
(with a notification), instead of scrolling away half-handled.

Choose **Snooze** on an open message, or from the bulk bar for a whole selection, and pick when it should
return: **Later today**, **This evening**, **Tomorrow**, **This weekend**, or **Next week**. The times
are in your own timezone, and only the ones still ahead of you are offered.

Everything you've snoozed waits in the **Snoozed** view — choose it in the folder list to see each
message and when it's due back. To pull one back early, choose **Un-snooze**.

Snoozing is local to this device: a message you snooze here still shows normally in webmail or on your
phone, and it comes back here even if you were offline when its time arrived (on the next check, or the
next time you open GeleitMail).

## Rules that sort your mail for you

An inbox you triage the same way every morning can do some of that work itself. In **Settings → Rules**,
a rule is *when this, do that*:

- **When** a message's **From**, **Subject**, or **To** contains some text (case-insensitive) —
- **do** one or more of: **move it to a folder**, **mark it read**, **star it**.

Add a rule with the little sentence-builder — pick the field, type the text to match, choose a folder
and/or tick *mark read* / *star*, and **Add rule**. Your rules show as plain sentences (*"If From
contains **newsletter** → move to **Reading**, mark read"*), and you can **Delete** any of them.

New mail is sorted **as it arrives**, while GeleitMail is checking your mail — so a rule fires on the next
sync, not the instant you write it, and (since rules run here on your device) not while GeleitMail is
closed. If two rules could match the same message, the **first** one wins.

To tidy the mail already sitting in your inbox, choose **Run on inbox now** — it applies your rules to
everything that's there.

## Undo

When you **Archive** or **Delete** a message, it slides out of the list and a short bar appears at the
bottom — *Archived · Undo* (or *Deleted · Undo*). Choose **Undo** within a few seconds and the message
comes straight back, exactly where it was. Nothing is sent to your provider until that moment passes,
so an undo can never lose mail. You can also press **`z`** to undo.

> Tip: keyboard shortcuts make this quick — `e` archives the open message, `#` deletes it, and `z`
> undoes. See [keyboard & settings](shortcuts-and-appearance.md).
