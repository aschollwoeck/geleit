# Handoff: GeleitMail UI (desktop + mobile)

## Overview
GeleitMail is a native, local-first, privacy-first email client for regular people
(2–3 accounts, calm + fast, no tracking). This package contains the first full design draft:
a desktop three-pane client (light + dark), an interactive desktop prototype, and iOS/Android
mobile screens + an interactive mobile prototype. It covers reading, composing, folders,
unified inbox + account switching, add-account, settings, and the privacy/feedback moments.

## About the design files
The files in this bundle are **design references authored in HTML** (as streaming "Design
Components" — `.dc.html`). They show the intended **look and behavior**, not production code to
ship as-is. The task is to **recreate them in the target codebase**.

**Target: Rust + Leptos** (switched from Slint). This is good news for reuse: Leptos renders real
HTML + CSS to the DOM via the `view!` macro, so these designs map closely:
- DOM structure in the templates → Leptos `view!` markup (near 1:1).
- Inline `style="…"` → extract into CSS classes/variables keyed off the design tokens below
  (don't ship inline styles; centralize the tokens as CSS custom properties or a stylesheet).
- Interactive prototype state + handlers → Leptos **signals** (`create_signal`) and event
  handlers (`on:click`, `on:input`). The prototype logic (below) is a behavioral spec.
- Line icons are inline SVG (1.75px stroke, rounded caps) — reuse the SVG paths directly, or
  wrap them as small Leptos components / an icon set.

Do **not** treat the `.dc.html` runtime (`support.js`, `<x-import>`, `sc-if`/`sc-for`) as part
of the target — it's only the preview harness. Read the markup and logic, reimplement in Leptos.

## Fidelity
**High-fidelity.** Final colors, typography, spacing, radii, and interactions are settled and
should be reproduced pixel-accurately using the tokens below. The device bezels in the mobile
screens (`ios-frame.jsx` / `android-frame.jsx`) are **preview chrome only** — do not implement
the phone frame; implement the screen content inside it.

## Design language (settled decisions)
- **Direction "Soft daylight":** quiet by default, soft/rounded, roomy, reassuring privacy.
- **Signature:** a 3px **accent guide edge** down the left of whatever has attention (selected
  row, reading pane, privacy chip). Paired with the faintly warm reading surface.
- **Type:** Hanken Grotesk (UI + body), IBM Plex Mono (rare, raw headers). Base 15px, tabular
  figures for times/counts.
- **Primary action color:** **deep indigo `#232C54`** (hover `#1A2142`) for Compose/Send —
  chosen over the teal accent to give the main action distinct weight. In dark it lifts to
  `#5C6FD1` (hover `#6E80DE`) for contrast.
- **Reading actions:** bordered secondary buttons (white surface, hairline border, 8px radius);
  Delete uses the danger token. Actions live **only** in the reading-pane header — never on list
  rows.
- **Icons:** line icons, 1.75px stroke, rounded caps/joins, single color, ~16–24px.

## Screens / views

### Desktop — three-pane (light + dark)
Grid: **expandable left rail (224px) · message list (380px, user-resizable) · reading pane (flex)**.
Below a width threshold, collapse to list+reading, then to one column with back-nav.

- **Left rail (expandable ⇄ 64px icon rail):** account switcher at top (avatar + current
  scope + chevron opens All/per-account menu); **Compose** button (indigo, 10px radius, line
  "+" icon, top of rail); system folders (Inbox/Sent/Archive/Trash/Junk) with unread counts and
  guide edge on the active one; a "Folders" section with user folders + "new folder" (+); at the
  very bottom, **Theme toggle** and **Settings**. Collapsed rail shows icons only + a settings gear.
- **Message list:** slim header (folder title · unread count · search + refresh icon buttons);
  **day-grouped** rows (Today / Yesterday / Monday …). Row = unread dot (accent) + sender (bold
  if unread) + time (right, tabular) on line 1; subject line 2; snippet line 3 (muted); a small
  account dot + label; paperclip if attachments. Selected row = `accent-quiet` bg + guide edge +
  12px radius. Hover = `surface-raised`.
- **Reading pane:** on the warm reading surface with the guide edge. Header: subject (21–22/600);
  sender name + `<address>` + account dot + date; then the single **action row** (Reply, Reply
  all, Forward, Archive, Move…, Delete) as bordered buttons. Below: the **privacy warning banner**
  (see below), then the body in a column; structured data (e.g. a booking) renders as a bordered
  card grid.

### Desktop — compose window
Separate centered window over a dimmed app (native-window feel): title bar (New message / Reply /
Forward) + close; **From** (identity per account, with account dot, click to switch); **To**
(recipient chips + typeahead autocomplete against contacts; Enter/comma commits; ↑/↓ navigates) +
Cc link; **Subject**; body textarea with signature; footer = **Send** (indigo) + attach + "Aa"
formatting + "Saved · just now" draft status + discard (danger). Attachments show as chips.

### Desktop — add account (effortless setup, 3 tiers)
1. **One-click OAuth:** "Continue with Google" / "Continue with Microsoft" → OAuth hand-off screen
   ("Finish in your browser… GeleitMail never sees your password", spinner, Cancel / I've approved).
2. **Known providers grid:** GMX, Web.de, Yahoo, iCloud — servers pre-filled, password only.
3. **Manual IMAP fallback:** email, password, IMAP/SMTP server+port; inline field errors; a
   keychain note. Provider names are plain text buttons (no branded logos).

### Desktop — settings (sidebar window)
Left categories (Accounts / General / Appearance / Privacy / Notifications) + right panel.
Accounts: list with per-account signature + Remove (→ danger confirm dialog); Add account.
General: default view (All inboxes / Per account), mark-as-read toggle. Appearance: theme
Light/Dark. Privacy: **one** real toggle "Block remote images" + the stated fact "No telemetry
and no tracking — always." Notifications: new-mail toggle.

### Mobile — iOS & Android (same language, native conventions)
Single column with a persistent **bottom bar: Mail · Folders · Search · Settings** (Settings always
one tap away, not hidden). Compose is a floating action above the bar (iOS round pill / Android
rounded-square FAB). Accounts live behind the avatar (top-right) → account sheet (iOS) / could also
be a nav drawer (Android). Reading: back nav; **iOS** puts actions in a bottom toolbar, **Android**
in the top app bar + a pill Reply/Forward row.
- **iOS conventions:** large title; active tab-bar item in **iOS system blue `#007AFF`**; notch/island;
  home indicator; grouped inset lists for settings/add-account.
- **Android conventions:** Material 3; active nav item gets the **pill indicator** tinted with the app
  accent (`accent-quiet` bg + accent icon); top app bar; FAB.

## Interactions & behavior
- **Reading:** opening a message marks it read (dot + weight clear). Archive / Delete act
  optimistically and show a **toast with Undo** (dark slate `#263238`, ~5s). Move… opens a folder
  menu. Reply/Reply-all/Forward open compose prefilled.
- **Privacy banner:** shown only when a message has remote content AND "Block remote images" is on
  AND images haven't been loaded for that message. "Load" reveals for that message only.
- **List splitter (desktop):** drag the divider between list and reading to resize the list
  (min 240px; always leave ≥360px for reading; clamp on window resize; persist width).
- **Account scope:** All inboxes / per-account filters the unified list and row dots.
- **Search:** filters against sender + subject + full body; empty state "No messages match…".
- **Compose:** From cycles identities; To typeahead; Send closes + "Sent" toast.
- **Add account:** any choice simulates success → "Account added" toast (real impl: OAuth flow /
  IMAP validation with inline errors).
- **Keyboard (desktop prototype):** `c` compose, `j`/`k` or ↑/↓ move selection, `e` archive,
  `#`/Delete trash, `r` reply, `f` forward, `/` search, `z` undo, `Esc` closes overlays.
- **Motion:** subtle, 120–200ms ease-out; respect `prefers-reduced-motion`.

## State management (map to Leptos signals)
- `theme` (light|dark; follow OS by default, manual override; persist)
- `scope` (all | gmail | gmx), `folder`, `selId` (selected message), `railCollapsed`, `listW`
- `searchOpen`/`query`, `composeOpen` (+ from/to/subject/body), `addOpen`, `settingsOpen`/tab
- `blockRemote`, per-message `loaded` set (images revealed)
- `toast` (+ undo op: {id, prevFolder}) for optimistic actions
- Messages are the data model: {id, folder, acct, unread, sender, email, subject, snippet,
  group, time, body[], attach?, blocked?}. Real impl: IMAP sync + encrypted local store.

## Design tokens
Semantic tokens (use names, not raw hex, in the implementation). These match the repo's `design.md`.

| Token | Light | Dark | Use |
|---|---|---|---|
| `bg` | `#F5F7F8` | `#14191B` | app background |
| `surface` | `#FFFFFF` | `#1C2326` | panes, cards, list |
| `surface-reading` | `#FBFAF7` | `#222B2E` | reading pane (warm daylight paper) |
| `surface-raised` | `#EEF2F3` | `#232C2F` | hover / secondary surface |
| `text` | `#1F2A2E` | `#E7EEF0` | primary text |
| `text-muted` | `#5E7177` | `#9FB0B5` | secondary text, meta |
| `text-faint` | `#90A0A5` | `#6A7E84` | decoration / disabled only |
| `accent` | `#2E9E9B` | `#56C4C0` | guide edge, unread dot, icons, fills |
| `accent-strong` | `#1C7E7B` | `#7FD8D4` | accent text/links |
| `accent-quiet` | `#E2F1F0` | `#1E3A39` | selected-row / chip tint |
| `primary` (Compose/Send) | `#232C54` (hover `#1A2142`) | `#5C6FD1` (hover `#6E80DE`) | primary button fill; white text |
| `danger` / `danger-strong` | `#CF5B45` / `#B3472E` | `#E08066` / `#ECA893` | destructive fill / text |
| `success` | `#2E9E63` | `#5CC98C` | success icon (toast check) |
| `warning` / `warning-strong` / `warning-quiet` | `#D99A3C` / `#8F5E16` / `#FBF0DC` | `#E0B05A` / `#E6C079` / `#2A2412` | remote-content banner |
| `divider` | `#E3EAEC` | `#2A3438` | hairlines |
| `toast-surface` / `on-toast` | `#263238` / `#F2F5F6` | (same) | toast |
| account dot · Gmail | `#5B8DBE` | `#7AA6D6` | per-account indicator |
| account dot · GMX | `#C98A4B` | `#D9A56B` | per-account indicator |
| iOS active tab | `#007AFF` | — | iOS system blue (mobile only) |

- **Radius:** sm 6 (inputs/chips) · md 10 (buttons/cards) · lg 14 (panels/windows) · list rows 12 · full (dots/avatars).
- **Spacing:** 4 · 8 · 12 · 16 · 24 · 32 · 40 · 48. Hit targets ≥ 40px (desktop), ≥ 44px (mobile).
- **Shadows (light):** `0 1px 2px rgba(0,0,0,.04)` · `0 4px 12px rgba(0,0,0,.06)`. Dark: prefer surface contrast over shadow.
- **Accessibility:** meaningful text ≥ 4.5:1; use `*-strong` for colored text; never color alone (unread = dot + weight); visible 2px focus ring.

## Assets
No external image assets. All icons are inline SVG (line style, 1.75px stroke) — reuse the paths.
Fonts: Hanken Grotesk via Google Fonts (or bundle locally for offline/local-first). No brand logos
were drawn for providers (plain text buttons) — swap in real provider marks at implementation if desired.

## Files in this bundle
- `Email Client Explorations.dc.html` — the full static design doc: every desktop decision
  (layout, folders, button system, feedback surfaces, settings, add-account) in light + dark,
  plus iOS/Android screens, mobile navigation (drawer/sheet + bottom bar). Organized as numbered
  "turns" (1–20) with option ids (e.g. 10a = settled desktop, 14a = dark, 20a/20b = mobile bars).
- `GeleitMail Prototype.dc.html` — interactive **desktop** app (folders, accounts, search,
  read/archive/delete + undo, compose w/ autocomplete, add-account wizard, settings, resizable
  splitter, keyboard shortcuts, light/dark).
- `GeleitMail Mobile Prototype.dc.html` — interactive **mobile** app with an iOS/Android toggle
  (bottom-bar nav, tap-to-read, compose, search, folders, settings, account sheet, add-account).
- `ios-frame.jsx` / `android-frame.jsx` — **preview-only** device bezels. Not part of the target.
- `support.js` — **preview-only** Design Component runtime. Not part of the target.

## How to use this with a coding agent
Point the agent at this README first, then the three `.dc.html` files. The README is the spec;
the HTML is the visual + behavioral reference. Recreate in Leptos: tokens → CSS variables, markup
→ `view!`, prototype logic → signals. Cross-check against the repo's `design.md` (tokens) and any
`guidelines.md` implementation notes.
