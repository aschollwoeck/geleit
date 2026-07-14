# Folder roles — what a folder is *for*

Spec: `specs/special-use/spec.md`. How GeleitMail knows which folder is the Drafts folder, the Sent
folder, the bin — on a provider whose folders are not named in English.

## The bug this fixes

Every special folder was found by matching the **English word**. On GMX (`Entwürfe`, `Gesendet`,
`Papierkorb`) — and on most non-English providers — that matched nothing, and the failures were all
silent:

| What the user does | What happened before |
| --- | --- |
| Sends a message | No Sent folder found → **no copy kept**, no error |
| Archives / deletes / marks spam | "This account has no Trash folder" |
| Opens Drafts | The provider's drafts never appear; the folder shows up twice in the rail |
| Looks at the rail | Generic icons, special folders sorted among their own — and their **Trash is renamable** |

## The rule

**The server already tells us.** RFC 6154 (SPECIAL-USE) has the server mark folders on the `LIST`
response: `\Drafts`, `\Sent`, `\Trash`, `\Archive`, `\Junk`. That flag is the authority, and it doesn't
care what language the folder is named in.

```
* LIST (\NoInferiors \UnMarked \Drafts) "/" Entwürfe
```

The English-name match survives as a **fallback** — for servers that don't advertise SPECIAL-USE, and
for every account already on disk until its next folder listing. So nothing regresses; the roles simply
start being right.

## One definition, three crates

`geleit_core::FolderRole` — in **core**, because all three need it: the **engine** picks the Sent folder
to save a copy into, the **store** persists the role, the **app** resolves every folder action against
it. And one resolver:

```rust
geleit_core::pick_folder(folders, FolderRole::Drafts)
//  1. a folder the server flagged with that role   ← the point of the exercise
//  2. else a folder whose name says so, in English ← the fallback
//  3. else None                                    ← decline; never invent a destination
```

`None` is a real answer. The caller declines the action rather than guessing: moving someone's mail into
a folder we picked by resemblance is worse than telling them we can't. Which is also why the name
fallback matches the path's **last segment** exactly (`INBOX.Drafts` ✓, `INBOX.Alte Drafts` ✗) and never
a substring.

Two roles are deliberately **not** derived from their flags:

- **`\All`** (Gmail's "All Mail") is not an Archive. Archiving into it is a no-op — every message is
  already there — so treating it as one would make Archive silently do nothing, which is precisely the
  class of bug this document exists to close.
- **`\Flagged`** is a saved search, not a folder we move mail into.

`INBOX` gets the `Inbox` role from its name, because IMAP reserves that one name itself (RFC 3501) and
servers rarely bother to flag it.

## The role belongs to the server

`folder.role` (migration 16) is **rewritten on every listing**, because the server owns it: a user who
marks a different folder as their Drafts folder in webmail must see that here on the next sync — which
means the role has to move *off* the old folder too.

That is why there are two upserts, and the distinction matters:

- `upsert_folder_with_role` — used **only** by the folder listing, the one place that knows the roles.
  Writes the role, clearing it when the server no longer sends one.
- `upsert_folder` — used by every *message* sync, which knows a folder name and nothing else. It
  deliberately **leaves the role alone**. If it wrote `NULL`, the first message sync after a listing
  would quietly wipe every role and the app would silently fall back to English names.

`syncing_a_folders_mail_never_blanks_the_role_the_listing_gave_it` pins exactly that.

## What the role reaches

- **Sent** — `run_send` saves the copy (`geleit-engine`, the one role the engine resolves itself).
- **Drafts** — the merged Drafts list, the drafts-sync target, and the folder hidden from the rail
  (see `drafts.md`).
- **Trash / Archive / Junk** — the toolbar's Archive / Delete / Spam (`move_to_role`), Empty Trash, and
  whether **Delete** means *delete forever* (`view::is_trash_folder` — in a `Papierkorb` it used to mean
  neither, so Empty Trash never appeared and Delete told the user they had no Trash folder).
- **The rail** — the icon, the sort order, and whether the folder is protected from renaming and
  deleting. A folder the server called `\Trash` is the bin whatever its name; without the role, a German
  user could rename their own bin and the app would then find neither it nor the mail in it.

### The Move… menu does *not* use roles — and that was a bug

`Move…` names the folder the user picked, so there is nothing to resolve: it goes through
**`move_to_folder`**, added here. It used to map every folder it listed onto one of four roles by
matching English words — and anything matching none of them fell through to `inbox`. **Picking an
ordinary folder filed the message in the Inbox**, in any language. Roles are for the actions that must
*find* a folder; when the user has already named one, guessing is the bug.

## One list of names, not three

The name fallback lives in exactly one place — `FolderRole::matches_name` — and `FolderRole::of(name,
role)` answers "does this folder hold a role at all". `dto::is_protected_folder` is that call. The
frontend keeps a **mirror** (it cannot depend on our crates — P4), and the mirror must accept the same
names: this list decides what the rail *offers* to rename, that one decides what the app *files mail
into*. When they drifted, `Archives` resolved as the Archive folder and stayed deletable — the user
could delete the folder the app was archiving into.

The names are matched on the path's **last segment**, exactly — never a substring. `Presentations`
contains "sent"; the old code filed sent mail there. And only the plural `Drafts` counts: a folder
called `Draft` is somebody's own, and the drafts folder is *hidden from the rail* with its whole
contents listed as deletable drafts, so claiming one on a guess makes a user's mail unreachable and one
click from gone.

## Known limit

**Everything in the drafts folder is treated as a draft.** `Store::drafts_in_folder` lists every message
in it — there is no `\Draft`-flag filter, because the store has no column for the flag. That is fine for
a folder that is genuinely the provider's Drafts (its contents *are* drafts), but it means a
**misconfigured** server that flags an ordinary mailbox `\Drafts` would have that mailbox hidden from
the rail and its mail listed as deletable drafts. The defence today is that we never *guess* the folder:
only an exact name or the server's own flag can claim it.

## Verified against a real server

The pure tests build the `NameAttribute`s themselves, so they cannot tell whether a server actually
sends them or whether `async-imap` surfaces them. `the_server_tells_us_which_folder_is_the_drafts_folder`
(live, Dovecot) does: it lists, asserts `\Drafts` came back as `FolderRole::Drafts`, and then asserts the
role survived into the store — which is where every caller reads it from.
