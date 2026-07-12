# GeleitMail — Visual Design Language

How GeleitMail looks and feels — the canonical "what it looks like," governed by `constitution.md`.
Established in slice S1.1 and refined through M3 (rich content) and the **design overhaul** (the
settled "Soft daylight" reskin). This doc now reflects the **shipped** design; the full reference is
the handoff bundle in `design/design_handoff_geleitmail/` (README + interactive `.dc.html`
prototypes), and the implementation lives in `crates/geleit-app/dist/style.css` (tokens as CSS
custom properties) + `crates/geleit-ui/src/app.rs` (the `view!` markup). The app is **Tauri (OS
webview) + Leptos** (Rust→WASM, ADR-0012) — the old Slint stack was removed in M9.

**Direction: "Soft daylight."** Gentle, rounded, approachable — the least "techy," most
reassuring reading of the brand for **regular people** (constitution P5). Calm and quiet by
default (P3), lean and unobtrusive (P4). The escort/safe-passage idea shows up not as imagery
but as *reassurance*: privacy moments feel kind and calm, never alarming.

> Token values below are the source of truth and match `dist/style.css`. Exact WCAG contrast ratios
> should be re-verified with a checker; values were chosen to meet the stated targets.

---

## 1. Principles (the look, derived from the constitution)
- **Quiet by default.** Content is the only loud thing; chrome recedes. No badges, gradients, or
  decoration that doesn't carry meaning.
- **Soft, not sharp.** Rounded surfaces, gentle dividers, low soft shadows — approachable, never
  clinical or "developer-tool."
- **Roomy, not wasteful.** Comfortable spacing and a larger base text size; calm, not dense.
- **Reassuring privacy.** The "remote content blocked" / tracker moments are calm, informative,
  and in the user's control — a quiet confidence, not a warning.
- **Honest motion.** Subtle and fast; nothing animates in a way that delays content (P1).

## 2. Layout & navigation
A three-region desktop window:

```
,──────────────┬──────────────────────┬─────────────────────────.
│  A  ACCOUNT ▾│  Inbox         3 · Q ⟳│▎Reply  Reply all  Fwd … │
│  + Compose   ├──────────────────────┤  A  Anna · Today 09:14  │
│ ▎Inbox     3 │ TODAY                │  Trip details           │
│  Sent        │▎• Anna        09:14  │  ─────────────────────  │
│  Archive     │   Trip details       │  Hi — here are the …    │
│  Trash     2 │   snippet…  •anna    │                         │
│  Junk        │   Bob          1h 📎 │  (warm reading paper,   │
│              │   Re: invoice        │   guide edge on left)   │
│  ☾ Theme     │   …                  │                         │
│  ⚙ Settings  │                      │                         │
`──────────────┴──────────────────────┴─────────────────────────'
```

- **Left rail (224px, collapsible to a 64px icon rail):** **account switcher** at top (avatar +
  current scope + chevron → a menu of **All inboxes** / each account / Add account); the **Compose**
  primary action just below it (indigo, near the top so it's always reachable); system + user
  folders with **unread counts** and the guide edge on the active one; the rail fills, then **Theme
  toggle** and **Settings** pinned at the bottom. Collapsed rail shows icons only.
- **Center (380px, user-resizable):** the message list. Drag the splitter between it and the reading
  pane to resize (persisted); reading always keeps a comfortable minimum.
- **Right (flex, largest):** the reading pane on `surface-reading` (warm daylight paper) with the
  full-height `accent` guide edge.
- **Slim list header:** folder title · unread count · **search** + **refresh** icon buttons.
- **Merged view:** picking **All inboxes** shows every account's inbox in one date-sorted list with a
  per-row account dot; the folder list hides (folders are per-account) and one "All inboxes" marker
  shows instead.
- **Responsive:** below a threshold collapse to two regions (list + reading), then to one with
  back-navigation. Never a cramped three-pane on a small window. (Desktop is the shipped target;
  the handoff also has iOS/Android screens for later.)

## 3. Typography
- **UI + body:** **Hanken Grotesk** — a soft humanist grotesque: friendly, highly legible, not
  the default "Inter." Fallback: `system-ui, sans-serif`.
- **Monospace (rare — raw headers/code):** **IBM Plex Mono**, fallback `ui-monospace, monospace`.
- **Bundled locally** (`dist/fonts/`, subset woff2) — never fetched at runtime, for local-first +
  the strict CSP (`font-src 'self'`).
- **Numerals:** tabular figures for times/counts (steady alignment).
- **Base size 15px** (comfortable for regular people). Scale:

| Role | Size / weight / line-height |
|---|---|
| Display (onboarding) | 28 / 600 / 1.25 |
| **Reading-pane title (subject)** | **22 / 600 / 1.3** |
| List pane title ("Inbox") | 20 / 600 |
| List subject | 15 / 600 (unread) · 15 / 400 (read) |
| Sender (list) | 14 / 700 (unread) · 14 / 500 (read) |
| Body (message) | 15 / 400 / **1.6** |
| Snippet / secondary | 13 / 400 (muted) |
| Meta (time, day labels, counts) | 11.5–12 / 600 (muted, tabular) |

## 4. Color & theme tokens
Semantic tokens (use these names, not raw hex, in the UI). Accent at full saturation is for
**fills/icons/indicators**; for accent-colored **text/links** use `accent-strong`. The **primary
action** (Compose / Send) is a separate **deep indigo**, deliberately *not* the teal accent, so the
one main action carries distinct weight.

| Token | Light | Dark | Use |
|---|---|---|---|
| `bg` | `#F5F7F8` | `#14191B` | app background |
| `surface` | `#FFFFFF` | `#1C2326` | panes, cards, list |
| `surface-reading` | `#FBFAF7` | `#222B2E` | reading pane — a faintly warm "daylight" paper |
| `surface-raised` | `#EEF2F3` | `#232C2F` | hover, secondary surface |
| `text` | `#1F2A2E` | `#E7EEF0` | primary text |
| `text-muted` | `#5E7177` | `#9FB0B5` | secondary text, meta (AA ≈4.8:1) |
| `text-faint` | `#90A0A5` | `#6A7E84` | decoration / disabled only — never meaningful text |
| `accent` | `#2E9E9B` | `#56C4C0` | fills, unread dot, icons, the guide edge |
| `accent-strong` | `#1C7E7B` | `#7FD8D4` | accent-colored text/links |
| `accent-quiet` | `#E2F1F0` | `#1E3A39` | selected-row / chip tint bg |
| **`primary`** | **`#232C54`** (hover `#1A2142`) | **`#5C6FD1`** (hover `#6E80DE`) | **Compose / Send fill** (white text light; dark-text on the lighter dark fill) |
| `danger` | `#CF5B45` | `#E08066` | destructive/error **fills & icons** |
| `danger-strong` | `#B3472E` | `#ECA893` | destructive/error **text** (AA) |
| `success` | `#2E9E63` | `#5CC98C` | success **fills & icons** (toast check) |
| `warning` | `#D99A3C` | `#E0B05A` | warning **fills & icons** |
| `warning-strong` | `#8F5E16` | `#E6C079` | warning **text** (AA) |
| `divider` | `#E3EAEC` | `#2A3438` | hairline separators |
| `focus` | `#1C7E7B` | `#7FD8D4` | focus ring |
| `avatar-bg` | `#EEF2F3` | `#2E393C` | reading-pane / settings avatar |
| `account-1` | `#5B8DBE` | `#7AA6D6` | per-account indicator dot (by account order) |
| `account-2` | `#C98A4B` | `#D9A56B` | per-account indicator dot |
| `account-3` | `#7A9E5B` | `#9FC080` | per-account indicator dot |

- Full `accent`/`danger`/`warning` are **fills, not text** (they fail AA as text on light surfaces);
  use the matching `*-strong` for colored text. `accent-strong` text must not sit on `accent-quiet`
  (≈4.2:1) — keep `*-strong` text on `surface`/`bg`. **Info** state reuses `accent`/`accent-strong`
  (no separate token).
- **Account dots** cycle `account-1/2/3` by the account's position, so each mailbox reads as a
  consistent colour in the merged view and on list rows.
- The app follows the OS light/dark setting by default (APP-3), with a manual override, persisted in
  the store (survives restart; painted before first paint to avoid a flash).

## 5. Spacing, density, shape
- **Spacing scale (hand-tuned, not a strict ×4 multiplier):** `1`=4 · `2`=8 · `3`=12 · `4`=16 ·
  `5`=24 · `6`=32 · `7`=40 · `8`=48 (use `7`=40 for minimum hit targets).
- **Density:** comfortable. List rows ~**64px** (3 lines: sender+time / subject / snippet), pane
  padding `4`–`5` (16–24px).
- **Radius (the "soft" signature):** `sm`=6 (inputs, chips) · `md`=10 (cards, buttons) · `row`=12
  (list rows) · `lg`=14 (panels, windows) · `full` (dots, avatars).
- **Shadows (soft, low):** `1` = `0 1px 2px rgba(0,0,0,.04)` · `2` = `0 4px 12px rgba(0,0,0,.06)`.
  In dark mode prefer surface contrast over shadow.

## 6. Components
- **Message-list row (day-grouped under Today / Yesterday / Earlier):** unread dot (`accent`) ·
  sender (bold if unread) · time (right, tabular) on line 1; subject on line 2; snippet (`text-muted`)
  on line 3; a small **account dot + address** line; paperclip if attachments. Selected =
  `accent-quiet` bg + a **3px `accent` guide edge on the left**, `row`(12) radius; hover =
  `surface-raised`. Unread is shown by **dot + weight**, never colour alone. Rows carry **no action
  buttons** — actions live only in the reading pane.
- **Reading pane:** sits on `surface-reading` (warm daylight paper) with a full-height **3px `accent`
  guide edge on the left**. Header order, top to bottom: **① the action row · ② sender · ③ subject**
  — the actions are pinned at the top so they never shift when the subject wraps to more lines. The
  action row is **bordered secondary buttons** (Reply · Reply all · Forward · Archive · Move… ·
  Delete · Unread); Delete uses the danger token. HTML bodies render in the sandboxed `mail://`
  iframe (ADR-0012); plain text in a themed column.
- **Privacy banner (PRIV-3):** shown only when a message has remote content, "Block remote images"
  is on, and images haven't been loaded for it. A calm **warning-tinted** strip (`warning-quiet`
  with its own `warning` guide edge): `[ ⚠ Remote content blocked   Load images ]`. "Load images"
  reveals for that message only. Reassuring and in-control, never alarming.
- **Compose (centred window over a dimmed app):** From (identity + account dot) · **To / Cc as
  removable recipient chips** + an input (Enter / comma / blur commits; addresses de-duplicated) ·
  Subject · body + signature; footer = **Send** (indigo `primary`) · **Attach** (native file picker →
  attachment chips) · Discard (danger) · draft status.
- **Buttons:** **primary = `primary` (indigo) fill** / white text (light), dark text on the lighter
  dark fill / `md` radius — the Compose and Send actions; **secondary** = `surface` + `divider`
  border (the reading action row, ghost buttons); **ghost** = `text-muted`, no border. Destructive =
  `danger-strong` fill / white text (irreversible confirms) or danger-tinted text/hover (Delete).
- **Inputs:** `surface`, `divider` border, `sm` radius; focus = 2px `focus` ring.
- **Empty & loading:** empty states are kind and actionable ("No account yet." → Add account);
  loading uses **non-blocking skeleton rows** and a calm "Checking for new mail…" strip, never a
  blocking spinner (P1).

## 7. Iconography
Line icons, **1.75px stroke, rounded caps & joins** (matches the soft language), 20–24px on a
24px grid, single color (`text-muted`/`accent`). A consistent rounded set (Lucide-style).
Define the style now; produce/choose assets when building the UI.

## 8. Motion
Subtle and quick: **120–200ms, ease-out**. Use for hover/selection, gentle pane cross-fades, and
the skeleton shimmer. No bouncing, no long sequences (that reads as gimmicky / AI-generated).
**Respect `prefers-reduced-motion`** (disable non-essential motion). Motion never gates content.

## 9. Accessibility
- **Contrast:** meaningful text ≥ **4.5:1** (AA). Use `accent-strong` for accent-colored text and
  `danger-strong` for error/destructive text (full `accent`/`danger` fail as text). The indigo
  `primary` fill carries white text (light) / dark text on the lighter dark fill so Compose/Send
  labels stay AA. Large/secondary ≥ 3:1.
- **Focus:** always visible — 2px `focus` ring with offset. Keyboard navigation is first-class
  (READ-9, APP-6). Shipped desktop shortcuts: `c` compose · `j`/`k` (or ↑/↓) move selection · `e`
  archive · `#` trash · `r` reply · `f` forward · `/` search · `z` undo · `Esc` closes overlays.
- **Hit targets ≥ 40×40px** (comfortable for regular people).
- **Never color alone** to convey state (unread = dot + weight; errors = icon + text).
- Honor OS light/dark (APP-3) and reduced-motion.

## 10. Feedback & messages
How the app tells you something happened — quiet and reassuring (P3), never blocking (P1). Four
surfaces, each with one job.

**Surfaces**
- **Toast** — bottom-centre, on `toast-surface` (a dark slate, same in both themes) with
  `on-toast` text and an `accent` **Undo** link; `md` radius, soft shadow. One at a time
  (queue), auto-dismiss ~5s, pause on hover/focus, dismissible. Confirms reversible actions:
  *Archived · Undo*, *Deleted · Undo*, *Moved · Undo*, *Sent*.
- **Inline** — directly under the field/control that's wrong: `danger-strong` text + a small
  danger icon. For input/validation errors (e.g. account setup). Never colour alone.
- **Banner** — a slim strip at the top of the relevant pane, tinted by severity, with an icon
  and optional action; dismissible or self-resolving; non-blocking. For connection/sync state
  and contextual notices.
- **Dialog** — centred, on `surface`; **only** for irreversible confirmations (empty trash,
  delete forever, remove account): a plain question, a confirm button (`danger-strong` for
  destructive), a cancel. Used rarely.

**When to use which**
- Reversible destructive (archive / delete→trash / move): act immediately (optimistic, P1) →
  **toast with Undo**; no dialog.
- Irreversible (empty trash / delete forever / remove account): **dialog**.
- Success: a **toast only when the result isn't already visible** (Sent; a row that disappears).
  No toast for in-place changes (read/unread, star).
- Connection/sync problems: **banner**, calm; reads keep working and sync retries in the
  background — never a popup.
- Input errors: **inline**, at the field.

**Connectivity & sync (local-first)**
A quiet status in the list header or a slim banner — *"You're offline — showing your saved mail"*
(info), *"Couldn't sync — will retry"* (warning). It never blocks reading (P1) and resolves
itself when the connection returns; no user action needed.

**Tone** (extends Voice & copy below)
- Success = the action's own verb, past tense: Send → "Sent", Archive → "Archived".
- Error = what happened + how to fix, in the app's voice, no apology, not techy:
  *"That password didn't work — check it and try again,"* not *"Auth failed (401)."*
- Offline = reassuring, not alarming.
- An action keeps its name through the flow (the **Send** button produces a "Sent" toast).

**Accessibility**
- Messages post to an ARIA live region — toasts `polite`, errors `assertive`.
- Errors and irreversible dialogs **never auto-dismiss**; toasts stay long enough to use Undo and
  pause on hover/focus; Undo is keyboard-reachable.
- Never colour alone — always icon + text.

**Feedback tokens** (banner tints + toast; core state colours are in §4)

| Token | Light | Dark | Use |
|---|---|---|---|
| `toast-surface` | `#263238` | `#263238` | toast background (both themes) |
| `on-toast` | `#F2F5F6` | `#F2F5F6` | toast text |
| `info-quiet` | `#E2F1F0` | `#1E3A39` | info banner bg (= `accent-quiet`) |
| `warning-quiet` | `#FBF0DC` | `#2A2412` | warning banner bg |
| `danger-quiet` | `#FBE9E4` | `#33211C` | error banner bg |

Banner **message text uses `text`** (always AA on the light tint); the severity colour
(`*-strong`) is used only for the **icon and any action link**. Don't set body text in a
`*-strong` colour on its own `*-quiet` tint — e.g. `accent-strong` on `info-quiet` ≈ 4.2:1,
below AA for normal text.

## Voice & copy (interface words are design material)
Plain, kind, sentence case, active voice — **never techy**. Name things by what the user controls
("Block remote images", not "disable remote content policy"). Actions keep their name through the
flow (a **Send** button → a "Sent" toast). Errors say what happened and how to fix it, in the
app's voice, without apologising. Empty screens invite action.

## Signature
The memorable tell is the **guiding edge** — a soft `accent` line down the left of whatever has
your attention (the selected message, the reading pane, the privacy chip), echoing *Geleit*
(safe escort / safe passage): the app quietly guides you through your mail. Paired with the
faintly warm **"daylight" reading surface**, that's where the boldness is spent; softness
(rounded surfaces, gentle motion, kind empty states) is the ambient quality, kept disciplined
everywhere else.
