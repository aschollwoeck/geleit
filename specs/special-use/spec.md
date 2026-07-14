# Special folders, found by the server's own word (RFC 6154 SPECIAL-USE)

**Constitution:** P3 (calm + correct), P8 (spec-driven).
**Story:** *As someone whose provider is not in English, I want GeleitMail to find my Drafts, Sent,
Trash, Archive and Junk folders — because it currently finds none of them.*

## The bug

Every special folder in GeleitMail was found by **matching the English word**:

```rust
.find(|n| n.eq_ignore_ascii_case("sent") || n.to_ascii_lowercase().contains("sent"))
```

On a provider that localizes its folders — GMX's `Entwürfe` / `Gesendet` / `Papierkorb`, and most
European providers — that finds **nothing**, silently:

- **Sent mail is saved nowhere.** `run_send` looks for a Sent folder, doesn't find one, and skips the
  save. The mail goes out; no copy is kept. The user finds out weeks later.
- **Archive, Junk and Trash decline to act** ("This account has no Trash folder") — or, worse, a
  substring match lands on the wrong folder and moves mail into it.
- **The merged Drafts list is inert**: the drafts folder is never recognised, so drafts started in
  webmail never appear, and the folder shows up twice in the rail again.
- The rail draws the folders with a generic icon, sorts them among the user's own, and lets the user
  **rename or delete their own Trash** — because nothing knows what it is.

## The rule

> **The server already tells us. Believe it.**

RFC 6154 has servers mark folders on the `LIST` response with `\Drafts`, `\Sent`, `\Trash`,
`\Archive`, `\Junk`. That is the authority, and it is language-independent. The English-name match
stays as a **fallback**, for servers that don't advertise it (and for every account already synced,
until its next folder listing).

`\All` (Gmail's "All Mail") is deliberately **not** treated as an archive: archiving into it is a
no-op, since every message is already there. `\Flagged` is a saved search, not a destination.

## Where it lives

`geleit_core::FolderRole` — the one definition, because the **engine** needs it (the Sent folder a
message is saved into), the **store** persists it, and the **app** resolves every action against it.
`geleit_core::pick_folder(folders, role)` is the single answer to "which folder is the X?": the
server's flag first, the name second, `None` third — and `None` means the caller *declines the action*
rather than inventing a destination. Moving someone's mail into a folder we guessed at is worse than
telling them we can't.

The role is stored on the folder row (migration 16) and **refreshed on every listing**, because the
server owns it: mark a different folder as Drafts in webmail and the role must move with it.

## Not in scope

Creating the missing folder when a provider has none (Archive, say) — declining is still the right
answer; offering to create one is a separate decision. Localized *display* names for GeleitMail's own
local folders (`Saved`, the drafts view) — the app is English for now.
