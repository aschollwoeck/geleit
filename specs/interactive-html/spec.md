# Slice: Interactive (selectable) HTML reading pane — option B  (READ-12)

## Goal
Replace the static full-email bitmap (tiled) reading pane with a **live, interactive Blitz view**
driven from Slint: render only the visible **viewport**, and forward Slint's mouse/scroll/keyboard
to Blitz so the user can **select text, copy it, scroll, and click links** — like a browser — all on
the CPU (no GPU, no second window). Proven feasible by the spikes (docs/technical/blitz-shell-spike.md).

## Behaviour
- The reading pane shows the **current viewport** of the message (a viewport-sized bitmap).
- **Scroll** (wheel / scrollbar) moves the Blitz viewport and re-renders.
- **Select** (mouse drag) highlights text; **Ctrl+C** copies the selection to the clipboard.
- **Click** a link → opens in the system browser (a click, not a drag).
- Remote images still opt-in (Load images); border/font/digit fixes still apply.

## Design
- `htmlrender::HtmlView` holds the live `HtmlDocument` (+ viewport size, dark). Methods:
  `open()`, `render() -> Image` (paints the viewport at the current scroll), `content_height()`,
  `scroll_by()/scroll_y()`, `pointer_down/move/up()` (→ `handle_ui_event`), `selected_text()`,
  `clear_selection()`, `link_at()`.
- App keeps `Rc<RefCell<Option<HtmlView>>>`; re-renders the viewport image on each interaction.
- Slint: a viewport `Image` + a `TouchArea` (pointer + scroll) + a simple scrollbar; Ctrl+C in the
  key handler copies via the clipboard.
- Clipboard: best-effort `wl-copy`/`xclip` subprocess (matches the zenity pattern; no new dep).

## Acceptance criteria
1. The email renders in the pane; wheel/scrollbar scrolls smoothly (viewport-sized re-render).
2. Dragging selects text (visible highlight); Ctrl+C puts it on the clipboard.
3. Clicking a link opens the browser; clicking text doesn't.
4. Heavy emails no longer lag the way the full-bitmap render did (viewport-only).
5. build/test/clippy -D warnings (+dangerous-tls)/fmt/cargo deny green.

## Non-goals (this slice)
Find-in-page, IME, rich keyboard nav inside the page, pixel-perfect Thunderbird fidelity, resize
re-layout polish (re-open on resize is acceptable).
