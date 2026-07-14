# Drafts — one list, two homes

Spec: `specs/drafts-merged/spec.md`. How GeleitMail's own drafts and the provider's Drafts folder end
up as **one** Drafts.

## The rule

> **If the server defines a Drafts folder we use it; if it doesn't, the drafts live on this device.**

The rail used to show **"Drafts" twice** — GeleitMail's drafts view (the local `draft` table, which is
what gives save/resume/attachments) and the provider's real `Drafts` folder, which arrives with every
other folder at sync. Nearly every provider keeps one, so nearly every user saw the duplicate.

Now there is one **Drafts** entry, and `ipc::list_folders` **hides** the provider's Drafts folder from
the rail — because that entry *is* it. Which folder that is comes from `geleit_core::pick_folder(…,
FolderRole::Drafts)`: the server's own `\Drafts` flag first, then the name (see `folder-roles.md`).
That is the **single** answer to "which folder holds the drafts": it is what `list_drafts` reads, what
drafts are mirrored to, and what the rail hides. Two different answers would put the folder back in the
rail beside its own contents.

## De-duplication is the whole trick

With "sync drafts" **on** (opt-in, off by default — P2), every local draft *already has* a copy in the
server's Drafts folder. A naive merge would list each of them **twice** — the same bug, one level down.

The copies are ours and we can prove it: every draft carries its own `Message-ID` (`DraftRow::msgid`),
and that is what its copy on the server is appended under. So `dto::merged_drafts` drops a server row
whose `Message-ID` belongs to a draft **we still hold**, and keeps everything else:

```rust
// The drafts we hold, by the id their copies are stamped with.
let ours: HashSet<&str> = local.iter().map(|d| d.msgid.as_str()).collect();
server.iter().filter(|s| !s.message_id.as_deref().is_some_and(|m| ours.contains(m)))
```

Two decisions in there, and both are load-bearing.

**Matched against the drafts that *exist*, not the id pattern.** A copy whose local draft is **gone** —
deleted while offline, so the expunge never landed — shows up as what it really is: a draft still
sitting on the provider, which the user can now delete from here. Pattern-matching `geleit-draft-*`
would have hidden it forever, which is exactly the kind of silent orphan the drafts-sync slice exists
to prevent.

**The id is *stored*, not derived** (store migration 15). It used to be
`draft_message_id(account, draft.id)` — a pure function of the row id. But `draft.id` is a bare SQLite
rowid, and **SQLite reuses the id of the highest deleted row**, so a new, unrelated draft could inherit
a dead draft's identity. Then: the dead draft's stranded copy folds into the new draft and vanishes from
the list for good — and, far worse, the new draft's next save expunges **by Message-ID**, destroying
that stranded draft's content on the server. So each draft now mints its own id once
(`<geleit-draft-{account}-{id}-{random}@geleit.local>`, via SQLite's `randomblob` — no RNG dependency)
and keeps it. `a_new_draft_never_inherits_a_deleted_drafts_message_id` pins it, and it fails on the old
scheme. Existing rows are backfilled with the derived form, so copies already on a server stay findable.

The one thing the pure tests *cannot* see is whether a real server hands that id back unchanged — they
build the server row by calling the same function the dedup calls. So the round-trip has its own live
test against Dovecot (`a_draft_we_uploaded_comes_back_carrying_the_message_id_we_stamped`): if an
`ENVELOPE` came back unbracketed or folded, the dedup would silently stop matching and every synced
draft would list twice, with every unit test still green.

`DraftSummary.on_server` says which table a row's `id` points into (**draft** id vs **message** id).
Every use site branches on it — including the optimistic `retain`s in the UI, where `d.id != id` alone
would drop the wrong row whenever a draft id and a message id happen to be the same number.

## Continuing a draft that's on the server

`ipc::resume_server_draft` loads it back into the compose form. Text comes from the local store (the
folder is synced). For an HTML-only draft — which is what webmail writes — there *is* no authored text
part: `plain` is mail-parser's `html_to_text` down-conversion, made at sync time in `mime::parse_body`
(`body_text(0)`). That is why `body.plain.unwrap_or_default()` is not the empty-composer hole it looks
like, and it is the mechanical reason "keeps every word, drops the styling" is true. **Attachments are fetched now** — they're never stored on this device until needed
(P2) — and written to temp files, so send/re-save go down the same path-based flow as a resumed local
draft's.

That fetch is **not** best-effort, unlike everywhere else we touch attachments: if a file can't be
fetched, the resume **fails**. Dropping one silently and then expunging the original on save would
destroy it. Same reason a draft with no body in the store refuses to open: an empty compose form saved
back over the real draft is data loss wearing a smile.

**Save replaces the original**, and only *after* the local save succeeds — so a failed expunge can
never lose the draft. The price is a **duplicate**: the old copy stays on the provider and comes back as
an *On your provider* row, which the user can delete. Nothing retries it. That is the same trade the
dedup makes — a visible orphan beats a silent loss — and the UI says so rather than claiming the save
was clean ("Draft saved here, but the copy on your provider couldn't be removed"). Abandon the compose
window instead and the server copy is untouched. Sending expunges the copy too: a draft that's been sent
is not a draft (and if *that* expunge fails, the user is told the draft is still in the provider's
Drafts).

With "sync drafts" **on**, `save_draft` uploads the new copy *before* `delete_forever` removes the old
one, so both exist server-side for an instant. They have different `Message-ID`s, so a list drawn in
that window would show one row (ours) and one orphan; the next one shows one.

**Formatting is lost, so the UI asks first.** Our composer writes plain text; webmail composes HTML.
For a reply or a forward that's harmless (the formatted original still exists) — but here the original
is *replaced*, so `DraftSummary.formatted` (the message has an HTML body) drives a confirmation before
the composer opens. The real fix is rich-text composing, which is its own milestone.

## Keeping the list current

The scheduler sweeps `INBOX` only, and the Drafts folder is no longer selectable in the rail, so
nothing else would ever fetch it. Opening **Drafts** therefore calls `ipc::refresh_drafts`, which goes
through `sync_folder_once` like every other sync — so it takes the per-`(account, folder)` lock and
can't race the scheduler or a Refresh. **Refresh** in the drafts pane does the same on demand.

The list renders from the store **first** (instant, offline — P1) and re-lists when the sync lands,
guarded on the pane still being open and the account still being the same one. While that first look is
in flight the pane says *"Checking your provider for drafts…"* rather than *"No drafts."*, which would
be contradicted a second later.

`SERVER_DRAFTS_CAP = 200` bounds the read: a Drafts folder is a handful of messages, and the cap only
stops a pathological one (a broken client that appended thousands) from stalling the pane. Above it, the
newest 200 are shown.

The whole folder is read in **one** store query (`Store::drafts_in_folder`). The obvious shape —
`messages_in_folder` + `header_by_id` + `body_for` per row — is three reads per draft, and the last pulls
the entire body through SQLCipher just to ask whether it has an HTML part. The query asks SQLite that
instead (`EXISTS(… html IS NOT NULL)`) and never reads a body. `list_drafts` runs twice per open (before
and after the sync), so the difference is not academic (P3: latency is a defect).

## Known limits

- **A resumed server draft carries only `In-Reply-To`, not the full `References` chain** — the message
  table has no column for it. A half-written reply, continued here and sent, can break threading in
  clients that thread on `References`.

## Two consequences of hiding the folder

Both follow from `list_folders` being the single source of the folder list, and neither is a bug — but
neither is obvious:

- **Drafts is no longer a Move… destination** — the move menu iterates the same `folders`. Correct
  (moving mail *into* drafts is not a thing a user asks for), but it changed silently.
- **Search still finds the provider's drafts**, as ordinary read-only messages, because
  `store::search_messages` is account-wide across folders and doesn't know a folder is hidden.
