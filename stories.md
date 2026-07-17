# GeleitMail — User Story Catalog (the complete "what")

The full set of things the app does, across the whole `vision.md`. This is the canonical
"what": milestones in `roadmap.md` are derived from these stories, and per `constitution.md`
P10 each story is covered by **executable acceptance tests** named after its ID
(`acceptance_<id>_...`).

- **IDs are stable** — never reused or renumbered; new stories get the next free number.
- **`(later)`** = part of the vision but after the first release; not in the early milestones.
- Stories describe outcomes for the user (a *private person*, not a power user), not mechanism.

---

## ACC — Accounts
- **ACC-1** Add a Gmail account via one-click sign-in (OAuth).
- **ACC-2** Add an Outlook/Microsoft account via one-click sign-in (OAuth).
- **ACC-3** Add any other account via manual IMAP/SMTP setup.
- **ACC-4** See my mail start appearing within seconds of adding an account.
- **ACC-5** Have several accounts at once.
- **ACC-6** Edit or remove an account.
- **ACC-7** Set my display name and signature per account.
- **ACC-8** Re-authenticate an expired login without losing local data.

## SYNC — Sync
- **SYNC-1** Mail syncs automatically in the background.
- **SYNC-2** Manually check for new mail now (refresh).
- **SYNC-3** Have my full mailbox become available locally over time (progressive backfill,
  newest first). *Default: we sync everything eventually so search/offline are complete —
  not recent-only.*
- **SYNC-4** See non-blocking sync status / progress.
- **SYNC-5** Have the changes I make (read state, flags, moves, deletes) sync back to the server.

## READ — Reading
- **READ-1** See a message list, newest first.
- **READ-2** See sender, subject, snippet, date, unread state, and an attachment indicator in the list.
- **READ-3** Read a message in plain text.
- **READ-4** Read an HTML message, safely rendered.
- **READ-5** See conversations grouped into threads.
- **READ-6** Navigate folders / labels.
- **READ-7** Mark messages read/unread (automatically on open, and manually).
- **READ-8** View and save attachments.
- **READ-9** Navigate the list and messages by keyboard.

## SEND — Composing & sending
- **SEND-1** Compose a new message.
- **SEND-2** Reply / reply-all.
- **SEND-3** Forward.
- **SEND-4** Add attachments.
- **SEND-5** Save and resume drafts.
- **SEND-6** Use basic formatting (bold, lists, links).
- **SEND-7** Have my signature included automatically.
- **SEND-8** See sent mail appear in Sent.
- **SEND-9** Get address autocomplete from history (no separate address book).
- **SEND-10** Compose offline and send on reconnect. `(later)`

## ORG — Organizing
- **ORG-1** Archive a message.
- **ORG-2** Delete to trash; empty trash / delete permanently.
- **ORG-3** Move a message to a folder.
- **ORG-4** Star / flag a message.
- **ORG-5** See and manage the provider's Junk/Spam folder (we rely on server-side filtering;
  we never classify mail ourselves).
- **ORG-6** Create / rename / delete folders.
- **ORG-7** Select multiple messages and act in bulk.
- **ORG-8** Auto-sort with rules / filters. `(later)`
- **ORG-9** Snooze a message. ✅

## SEARCH — Search
- **SEARCH-1** Search my mail by sender / subject / body.
- **SEARCH-2** Search offline (against the local index).
- **SEARCH-3** Get fast, near-instant results.
- **SEARCH-4** Use operators (from:, has:attachment, date ranges). `(later)`
- **SEARCH-5** Search across all accounts at once. `(later)`

## PRIV — Privacy & safety
- **PRIV-1** Have remote content blocked by default.
- **PRIV-2** Choose to load remote content per message / for a trusted sender.
- **PRIV-3** See that trackers were blocked.
- **PRIV-4** Be sure scripts in mail never execute.
- **PRIV-5** Trust there is no telemetry. *(product property, not a UI feature)*

## OFF — Offline
- **OFF-1** Read already-synced mail offline.
- **OFF-2** Search offline.
- **OFF-3** Compose offline, queue, and send on reconnect. `(later)` *(= SEND-10)*
- **OFF-4** Organize offline and reconcile on reconnect. `(later)`

## MULTI — Multi-account experience
- **MULTI-1** Switch between accounts (per-account view).
- **MULTI-2** Have the correct from-address chosen automatically when I reply.
- **MULTI-3** See all accounts together in a unified inbox. `(later)`

## SEC — Security & data
- **SEC-1** Have my mail encrypted at rest, transparently (no master password).
- **SEC-2** Have credentials / tokens stored in the OS keychain.
- **SEC-3** Have local data wiped when I remove an account.
- **SEC-4** Export / back up my mail. ✅ *(per-folder → mbox; attachment bytes + whole-account export are follow-ups)*

## NOTIF — Notifications `(later)`
- **NOTIF-1** Get notified of new mail.
- **NOTIF-2** Control notifications per account / set quiet hours.
- **NOTIF-3** See an unread count / badge.

## APP — App & cross-cutting
- **APP-1** Be guided through an effortless first-run onboarding.
- **APP-2** Use a calm, fast, uncluttered interface.
- **APP-3** Switch between light and dark themes.
- **APP-4** Adjust settings / preferences.
- **APP-5** Run the app on Windows, macOS, and Linux.
- **APP-6** Use keyboard shortcuts.
- **APP-7** Have the app update itself. `(later)`
