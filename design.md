# GeleitMail — Visual Design Language

How GeleitMail looks and feels. The canonical "what it looks like," governed by
`constitution.md`; `guidelines.md` §13 is *how* this is implemented in Slint. UI slice specs
cite both. Established in slice S1.1; rich-content styling (HTML bodies, threads) is refined in
M3.

**Direction: "Soft daylight."** Gentle, rounded, approachable — the least "techy," most
reassuring reading of the brand for **regular people** (constitution P5). Calm and quiet by
default (P3), native and unobtrusive (P4). The escort/safe-passage idea shows up not as imagery
but as *reassurance*: privacy moments feel kind and calm, never alarming.

> Token values below are the source of truth. Exact WCAG contrast ratios should be re-verified
> with a checker at implementation; values were chosen to meet the stated targets.

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
│  ACCOUNT ▾   │  Inbox            ⟳ Q │  Trip details           │
│              ├──────────────────────┤  Anna · Today 09:14     │
│  Inbox    3  │ • Anna        09:14  │  ─────────────────────  │
│  Sent        │   Trip details       │  Hi — here are the …    │
│  Archive     │   Bob          1h 📎 │  (calm reading column,  │
│  Trash       │   Re: invoice        │   max ~680px wide)      │
│  Junk        │   …                  │                         │
│              │                      │  [Reply] [Forward] …    │
│  ✏ Compose   │                      │                         │
`──────────────┴──────────────────────┴─────────────────────────'
```

- **Left rail (~240px, collapsible):** account **switcher** at top (MULTI-1; unified inbox
  later), then folders with unread counts, then the **Compose** primary action pinned at the
  bottom-left.
- **Center (~380px):** the message list.
- **Right (flex, largest):** the reading pane on `surface-reading` (warm daylight paper) with the
  `accent` guide edge; body text in a centered column **max ~680px** for readability.
- **Slim header** over the list: folder name · refresh (⟳) · search.
- **Responsive:** below a threshold collapse to two regions (list + reading), then to one with
  back-navigation. Never a cramped three-pane on a small window.

## 3. Typography
- **UI + body:** **Hanken Grotesk** — a soft humanist grotesque: friendly, highly legible, not
  the default "Inter." Fallback: `system-ui, sans-serif`.
- **Monospace (rare — raw headers/code):** **IBM Plex Mono**, fallback `ui-monospace, monospace`.
- **Numerals:** tabular figures for times/counts (steady alignment).
- **Base size 15px** (comfortable for regular people). Scale:

| Role | Size / weight / line-height |
|---|---|
| Display (onboarding) | 28 / 600 / 1.25 |
| Pane title ("Inbox") | 18 / 600 / 1.3 |
| Subject / reading title | 15 / 600 (unread) · 15 / 500 (read) / 1.4 |
| Sender (list) | 14 / 600 (unread) · 14 / 500 (read) |
| Body (message) | 15 / 400 / **1.6** |
| Snippet / secondary | 13 / 400 / 1.45 (muted) |
| Meta (time, labels) | 12 / 500 (muted, tabular) |

## 4. Color & theme tokens
Semantic tokens (use these names, not raw hex, in the UI). Accent at full saturation is for
**fills/icons/indicators**; for accent-colored **text/links** use `accent-strong`.

| Token | Light | Dark | Use |
|---|---|---|---|
| `bg` | `#F5F7F8` | `#14191B` | app background |
| `surface` | `#FFFFFF` | `#1C2326` | panes, cards, list |
| `surface-reading` | `#FBFAF7` | `#1A2123` | reading pane — a faintly warm "daylight" paper |
| `surface-raised` | `#EEF2F3` | `#232C2F` | hover, secondary surface |
| `text` | `#1F2A2E` | `#E7EEF0` | primary text |
| `text-muted` | `#5E7177` | `#9FB0B5` | secondary text, meta (AA ≈4.8:1) |
| `text-faint` | `#90A0A5` | `#6A7E84` | decoration / disabled only — never meaningful text |
| `accent` | `#2E9E9B` | `#56C4C0` | fills, unread dot, icons, the guide edge |
| `accent-strong` | `#1C7E7B` | `#7FD8D4` | accent-colored text/links; primary-button fill (light) |
| `accent-quiet` | `#E2F1F0` | `#1E3A39` | selected-row / chip tint bg |
| `danger` | `#CF5B45` | `#E08066` | destructive/error **fills & icons** |
| `danger-strong` | `#B3472E` | `#ECA893` | destructive/error **text** (AA) |
| `success` | `#2E9E63` | `#5CC98C` | success **fills & icons** |
| `success-strong` | `#1E7A47` | `#7FD8A6` | success **text** (AA) |
| `warning` | `#D99A3C` | `#E0B05A` | warning **fills & icons** |
| `warning-strong` | `#8F5E16` | `#E6C079` | warning **text** (AA) |
| `divider` | `#E3EAEC` | `#2A3438` | hairline separators |
| `focus` | `#1C7E7B` | `#7FD8D4` | focus ring |

- Full `accent`/`danger`/`success`/`warning` are **fills, not text** (they fail AA as text on
  light surfaces); use the matching `*-strong` for colored text. `accent-strong` text must not
  sit on `accent-quiet` (≈4.2:1) — keep `*-strong` text on `surface`/`bg`. **Info** state reuses
  `accent`/`accent-strong` (no separate token).
- The app follows the OS light/dark setting by default (APP-3), with a manual override.

## 5. Spacing, density, shape
- **Spacing scale (hand-tuned, not a strict ×4 multiplier):** `1`=4 · `2`=8 · `3`=12 · `4`=16 ·
  `5`=24 · `6`=32 · `7`=40 · `8`=48 (use `7`=40 for minimum hit targets).
- **Density:** comfortable. List rows ~**64px** (3 lines: sender+time / subject / snippet), pane
  padding `4`–`5` (16–24px).
- **Radius (the "soft" signature):** `sm`=6 (inputs, chips) · `md`=10 (cards, buttons) · `lg`=14
  (panels) · `full` (dots, avatars).
- **Shadows (soft, low):** `1` = `0 1px 2px rgba(0,0,0,.04)` · `2` = `0 4px 12px rgba(0,0,0,.06)`.
  In dark mode prefer surface contrast over shadow.

## 6. Components
- **Message-list row:** unread dot (`accent`) · sender (bold if unread) · time (right, `text-muted`)
  on line 1; subject on line 2; snippet (`text-muted`) on line 3; paperclip icon if attachments.
  Selected = `accent-quiet` bg + a **3px `accent` guide edge on the left**, `md` radius; hover =
  `surface-raised`. Unread is shown by **dot + weight**, never color alone.
- **Reading pane:** sits on `surface-reading` (warm daylight paper) with a **3px `accent` guide
  edge on the left**. Header: subject as title; sender name + address; date; a quiet row of icon
  actions (Reply, Reply all, Forward, Archive, Delete).
- **Privacy chip (PRIV-3):** in the reading header, calm and in-control —
  `[ Remote content blocked · Load ]` on `surface` with an `accent-strong` "Load" link and the
  same left guide edge. Reassuring, not a warning.
- **Buttons:** primary = **`accent-strong` fill / white text** (light) · **`accent` fill /
  `#14191B` text** (dark) / `md` radius — full `accent` with white text fails AA (Compose, Send);
  secondary = `surface` + `divider` border; ghost = `text-muted`, no border.
- **Inputs:** `surface`, `divider` border, `sm` radius; focus = 2px `focus` ring.
- **Empty & loading:** empty states are kind and actionable ("You're all caught up."); loading
  uses **non-blocking skeleton rows**, never a blocking spinner (P1).

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
  `danger-strong` for error/destructive text (full `accent`/`danger` fail as text); primary
  button fills use `accent-strong` (light) or `accent` with dark `#14191B` text (dark) so labels
  stay AA. Large/secondary ≥ 3:1.
- **Focus:** always visible — 2px `focus` ring with offset. Keyboard navigation is first-class
  (READ-9, APP-6).
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
