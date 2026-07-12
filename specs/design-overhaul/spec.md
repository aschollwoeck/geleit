# Design overhaul — "Soft daylight" desktop reskin

**Constitution:** P3 (calm and fast *is* the feature), P4 (lean), P5 (effortless setup), P2 (privacy).
**Reference:** the "Soft daylight" design handoff in `design/design_handoff_geleitmail/` (README +
the interactive desktop/mobile `.dc.html` prototypes), and `design.md` (the canonical token set,
kept in sync with the handoff). Implemented in-repo (`crates/geleit-app/dist/style.css` +
`crates/geleit-ui/src/app.rs`). High-fidelity goal: reproduce the settled look pixel-closely.

## What it delivers

A full visual + interaction reskin of the existing Tauri + Leptos desktop app to the settled
"Soft daylight" design language — mapped onto the current backend. The functional plumbing (IPC,
store, actions) is unchanged; this replaces the *presentation* and adds the interaction polish the
design specifies.

### In scope (wired to the current backend)

- **Design tokens** → CSS custom properties (light + dark): the full set including the new **indigo
  `primary`** (Compose/Send), **account-dot** colors, warm reading surfaces, radii, spacing, shadows.
  **Hanken Grotesk + IBM Plex Mono bundled locally** (`dist/fonts/`, ~200 KB) — never fetched at
  runtime (local-first + CSP).
- **Left rail:** expandable (224px) ⇄ collapsed (64px); account avatar + switcher menu (real accounts
  from `list_accounts`, switch scope, Add account); **Compose** button (indigo); system + user
  folders with **line icons, unread counts, and the 3px accent guide edge** on the active one; Theme
  toggle + Settings at the bottom.
- **Message list:** slim header (folder title · unread count · search + refresh icon buttons);
  **day-grouped** rows (Today / Yesterday / Earlier); row = unread dot + sender + time · subject ·
  snippet · account dot · paperclip; selected row = `accent-quiet` + guide edge + 12px radius.
- **Reading pane:** warm surface + full-height guide edge; header (avatar · sender · `<address>` ·
  account dot · date); **bordered action row** (Reply / Reply all / Forward / Archive / Move… /
  Delete, Delete in danger) — actions live **only** here; **privacy banner** in the warn style with
  its own guide edge and a "Load images" action.
- **Compose:** centered modal over a dimmed app — From row, To recipient chips + input, Subject, body
  textarea, footer with indigo **Send** + attach + "Aa" + draft status + discard (danger).
- **Settings window:** sidebar (Accounts / General / Appearance / Privacy / Notifications) + panel.
  Wired: theme, per-account signature, remove-account (→ danger confirm), block-remote-images, and
  the toggles (persisted via the store `setting` k/v table).
- **Add-account:** the tiered wizard — provider grid (pre-fills known IMAP/SMTP servers), a manual
  IMAP tier (fully wired to `add_account`), and the OAuth tier presented honestly.
- **Feedback:** archive/delete act optimistically with a **toast + Undo** (dark slate, ~5 s);
  Move… menu; keyboard shortcuts (`c`, `j`/`k`, `e`, `r`, `f`, `/`, `z`, `Esc`).
- **Icons:** the inline-SVG line set (1.75px stroke) as a small Leptos helper.

### Out of scope (named follow-ups, honest about the backend)

- **True unified "All accounts" inbox** (one merged cross-account list) — needs a backend cross-account
  folder query. The switcher offers per-account scope now; "All accounts" is deferred.
- **Real OAuth** (Continue with Google/Microsoft) — M7, blocked on provider credentials. The tier is
  shown; picking it routes to the manual/provider path with an honest note.
- **Mobile** (iOS/Android screens) — this app is desktop; mobile is a later milestone.
- **Notifications delivery**, **mark-as-read toggle** enforcement in the open path — settings persist,
  behavior wiring is a follow-up.

## Acceptance

The desktop app matches the prototype's look in light and dark: the rail (collapsed + expanded), the
day-grouped list with the new row style, the reading pane with the guide edge and bordered actions,
the compose modal, the settings window, and the add-account wizard — all screenshot-verified. Real
mail still renders correctly (the `.eml` re-check). All gates green; reviewed.
