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
- **Delete** — move it to Trash. When the message is **already in Trash**, this button reads **Delete
  forever** and permanently removes it (see below).
- **Move…** — pick a folder to move the message into.
- **Spam** — from the **Move…** menu, send a message to your Junk folder (or move it back to Inbox).
- **Mark as unread** — bring back the unread dot.

## Managing folders

You can make your own folders to sort your mail. At the bottom of the folder list:

- **New folder** — choose it, type a name, and choose **Create**. GeleitMail makes the folder on your
  provider and shows it in the list. Move messages into it with **Move…**.
- **Rename** or **Delete** — hover a folder you created and choose the **⋯** button. **Rename** lets
  you give it a new name (its messages come along). **Delete** removes the folder — and, after you
  confirm, **all the messages in it** — from your provider and this device; this can't be undone.

Your standard folders — Inbox, Sent, Drafts, Archive, Trash, Junk, and Saved — are kept as they are,
so they don't show the rename or delete options.

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
- **Clear** (the ✕) — deselect everything.

The checkbox at the far left of that bar **selects (or clears) every message in the list** at once.
Archive and Delete work just like single messages: the rows slide out with one **Undo** bar
(*3 archived · Undo*), and nothing is sent to your provider until the moment passes — so a bulk Undo
can never lose mail. Switching folders or accounts clears your selection.

## Undo

When you **Archive** or **Delete** a message, it slides out of the list and a short bar appears at the
bottom — *Archived · Undo* (or *Deleted · Undo*). Choose **Undo** within a few seconds and the message
comes straight back, exactly where it was. Nothing is sent to your provider until that moment passes,
so an undo can never lose mail. You can also press **`z`** to undo.

> Tip: keyboard shortcuts make this quick — `e` archives the open message, `#` deletes it, and `z`
> undoes. See [keyboard & settings](shortcuts-and-appearance.md).
