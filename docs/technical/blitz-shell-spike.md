# Spike: Blitz as a native renderer (blitz-shell) — findings

Question: would using Blitz as the *primary* renderer (its own window/event loop, via `blitz-shell`)
give us native, **selectable** HTML mail — instead of compositing a bitmap into Slint?

## What was tried
A throwaway example launched a `blitz-shell` window showing a real mail `.eml` (run through the same
`safehtml::document()` we use in-app), first with the default GPU renderer, then with the CPU one.

## Results
1. **GPU (default) is a non-starter here.** `blitz::launch_static_html` renders via Vello on `wgpu`.
   It **panicked**: the GPU/driver lacks `SHADER_FLOAT16_IN_FLOAT32`. So the out-of-the-box "Blitz owns
   the window" path crashes on exactly the kind of varied hardware we must support — the same risk
   class as the GL webview we abandoned.
2. **CPU works.** `blitz-shell` is generic over the renderer (`BlitzApplication<Rend: WindowRenderer>`);
   with `anyrender_vello_cpu::VelloCpuWindowRenderer` (softbuffer, no GPU) it runs: a native window,
   correct rendering (our border / font-fallback / digit fixes all apply — clean, no phantom borders),
   and it is **interactive** — Blitz handles text selection, scrolling and link clicks itself. So
   *selectable, native HTML without a GPU is achievable.*

## Caveats (both real)
- **Performance.** Small content paints instantly; a 5 MB image-heavy email took **~25 s to first
  paint** on the CPU. `blitz-shell` re-renders the whole document, so heavy mail is slow — and
  re-rendering per frame would make selection/scroll laggy. GPU would fix it, but GPU crashes here.
  Mitigation = viewport-only rendering (render just the visible region), image downscaling, caching.
- **Integration.** `blitz-shell` owns its own winit window + event loop. Embedding it *inside* the
  Slint app means coexisting two event loops / a child surface — non-trivial. Its natural mode is a
  standalone window.

## Options
- **A. Separate Blitz reading window (CPU)** — "Open in reader window" pops the message into its own
  blitz-shell-CPU window: native selection/scroll/links, no Slint embedding. Perf still needs work for
  heavy mail. Lowest-cost path to native+selectable.
- **B. Embed a Blitz-CPU surface in Slint** — the true in-app version. Big integration + perf project.
- **C. Keep the bitmap pane, add selection by driving Blitz input in Slint** — no 2nd window, but
  selection-on-bitmap + render-on-interaction perf is its own work.
- **D. Keep the bitmap pane + an "Open in browser" / "Open in reader window" escape hatch** — cheapest.

## Lean
Native in-app selection is a real project on a pre-alpha stack with an unsolved CPU-perf problem.
Best value today: keep the bitmap reading pane and add an escape hatch (browser, or a Blitz-CPU reader
window) for when selection/copy/print is needed — revisit full embedding only if rich HTML reading
becomes a defining feature.

## Follow-up spike: option B core proven (driving Blitz from our own events)

Rather than embed blitz-shell's window, option B drives `blitz-dom` directly. Proven in a throwaway
example (`blitz_select_spike`): feeding `doc.handle_ui_event(UiEvent::PointerDown/Move/Up(
BlitzPointerEvent{ coords … }))` at document coordinates makes Blitz **select text**; `paint_scene`
renders the **selection highlight**; and `doc.get_selected_text()` returns the selected string. No
window, no blitz-shell, no GPU. So in-app interactive HTML = wiring Slint's input to these calls.

### Implementation plan (in-app B)
1. **Keep the blitz `HtmlDocument` alive** per open message (already do for hit-testing).
2. **Viewport rendering** — render only the visible region (`paint_scene` with the current
   `viewport_scroll` offset + viewport-sized buffer) into one Slint `Image`, instead of the whole
   email + tiles. Fast per-frame (images stay decoded in the doc) — fixes the heavy-email perf too.
3. **Custom scroll** — drop the Slint `ScrollView`; a scrollbar/scroll-wheel drives
   `doc.scroll_viewport_by(dx,dy)` (or `UiEvent::Wheel`) → re-render. Track `viewport_scroll()`.
4. **Selection** — Slint `TouchArea` pointer down/move/up → `handle_ui_event(PointerDown/Move/Up)` at
   `(local + scroll)` doc coords → re-render the viewport (now highlighted).
5. **Copy** — Ctrl+C → `get_selected_text()` → clipboard.
6. **Links** — keep the current `hit()` → open in browser (click without drag).
7. **Coordinate mapping** — Slint logical px ↔ Blitz page coords (+ scroll, + scale factor).

Effort: real (re-architects the reading-pane render + input), but no new unknowns — every Blitz piece
is confirmed working. Risk: pre-alpha; per-frame CPU paint must stay smooth at viewport size.
