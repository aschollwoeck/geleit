# One Drafts — the server's folder and this device's, in one list

**Constitution:** P2 (privacy), P3 (calm), P1 (the UI never waits), P8 (spec-driven).
**Story:** *As someone who writes mail in more than one place, I want one Drafts — whatever I started,
wherever I started it.*

## The bug this fixes

The folder rail shows **"Drafts" twice**: GeleitMail's own drafts view (the local `draft` table — what
gives you save/resume with attachments) *and* the provider's real `Drafts` folder, which arrives with
every other folder at sync. Nearly every provider has one, so nearly every user sees the duplicate.

## The rule

> **If the server defines a Drafts folder we use it; if it doesn't, the drafts live on this device.**

One **Drafts** entry in the rail, always. It lists:

- every draft saved on this device (unchanged: instant, offline, attachments, resume), **and**
- every draft sitting in the provider's `Drafts` folder — one you started in webmail or on your phone.

Neither is hidden, and nothing is uploaded that wasn't already going to be: mirroring *this device's*
drafts to the server stays **opt-in and off by default** (P2). This slice only *reads* the folder the
provider already keeps.

## The de-duplication that makes it work

With "sync drafts" **on**, each local draft already has a server copy — so a naive merge would list
every one of them **twice**, which is the bug we're fixing, one level down.

The copies are ours and we can prove it: every draft carries its own stored `Message-ID`, and that is
what its copy on the server is appended under. A server row whose `Message-ID` belongs to a draft **we
still hold** is that draft, so it folds into the local row rather than adding one.

Deliberately matched against the drafts that *exist*, not against the id **pattern**: a server copy
whose local draft is gone (deleted while offline, so the expunge never landed) is then shown as what it
is — a draft still on the provider — and can be deleted from here. Pattern-matching would hide it
forever.

And the id is **stored**, not derived from the row id — because SQLite reuses the ids of deleted rows,
so a new draft could otherwise inherit a dead one's identity, swallow its stranded copy from the list,
and then expunge that copy's content off the server on its next save. (Store migration 15; it was a
live data-loss bug in the drafts-sync feature, found in review of this slice.)

## Continuing a draft that's on the server

Clicking it opens it in the composer, prefilled — recipients, subject, text, **and its attachments**
(fetched on demand; that is the one part of *opening* it that needs the network — the pane's sync and the
expunge on save need it too).

**Save** then writes it as a draft on this device and **removes the server copy**, because the draft
you edited is the draft you now have — leaving the original would put it straight back in the list as a
second row. With "sync drafts" on it is re-uploaded as our own copy, so the list still shows one.
The server copy is removed only *after* the local save succeeds — so a failed expunge costs a duplicate
(an orphan row the user can delete, and is told about), never the draft. Discard it instead and the
server copy is untouched.

**Formatting is lost, so we ask first.** GeleitMail's composer writes plain text, and webmail composes
HTML — so continuing a formatted draft keeps the words and drops the styling. That is fine for reply
and forward (the original still exists), but here the original is *replaced*, so a draft written with
formatting asks before it opens.

## Keeping the list current

The scheduler syncs `INBOX` only, so opening **Drafts** syncs the provider's Drafts folder — through
`ipc::sync_folder_once`, like every other sync, so it takes the folder lock. The list renders from the
local store **first** (instant, offline) and re-lists when the sync lands (P1). **Refresh** is available
in the drafts pane and does the same thing on demand.

## Out of scope

Rich-text composing (the reason formatting is lost at all) — its own milestone. Creating a `Drafts`
folder *on the server* when the provider has none: the user's rule says the fallback is this device,
and that is also the private default.
