# S9.6 — Search, settings, and adding an account

**Milestone:** M9. **Constitution:** P1 (local search is instant, off the network), P5 (effortless
setup), P3 (calm), P2 (credentials to the keychain, never logged).

## What it delivers

The three things that make the new UI *self-sufficient* — you can add your account, find mail, and
choose your theme — none of which the shell could do yet.

| | Story | Acceptance |
|---|---|---|
| **S9.6-1** | I can add my account. | A setup form (email, IMAP host/port/user, SMTP host/port/STARTTLS, password, signature) creates the account, stores the password in the keychain, and does a first sync — reusing `run_setup`. On success the app shows the new account's mail. |
| **S9.6-2** | I see a calm empty state that leads me to setup. | With no account, the empty state offers **Add account** (not a dead end). |
| **S9.6-3** | I can search my mail. | A search box over the list runs the store's FTS5 search (instant, local, operators like `from:`/`subject:`/`has:attachment`); results replace the list; clearing returns to the folder. |
| **S9.6-4** | I can switch light/dark. | A theme toggle flips the theme and **persists it to the store** (the same `setting` row S9.1 reads on boot). |

## How

- **Reuse:** `run_setup` + the pure validators `build_imap` / `build_smtp` move from the Slint
  `refresh.rs` into the engine (re-exported — the pattern). `store::search_messages` (FTS5, M6) and
  `store::{get,set}_setting` already exist.
- **Shell commands:** `add_account(form)` (worker — network + keychain), `search(account_id, query)`,
  `set_theme(theme)`.
- **Frontend:** a setup overlay (our own document, a plain form); a search box in the list header
  that swaps the list for results; a theme toggle in the rail.

## Out of scope (named follow-ups)

Multi-account switching + remove-account UI (the store/engine support it — `run_remove_account`
already moved in S9.4 — but the switcher UI is a follow-up); save/open `.eml`; OAuth (still M7,
blocked on provider credentials); cross-account "all accounts" search. Setup is **manual IMAP/SMTP**,
as today.
