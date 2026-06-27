# Slice: Font fallback so digits render in real mail — READ-11

## Problem (found via a real saved .eml)
A real loan newsletter rendered with **every number missing** — "Bis zu 15.000 € · Laufzeit bis 10
Jahre" came out "Bis zu . € · Laufzeit bis Jahre", "Datum: 19. Juni 2026" → "…202", repayment
figures gone. The numbers ARE in the source. Cause: Blitz/parley drops **digit** glyphs (only) for a
`font-family` that names a font which isn't installed (Helvetica/Roboto/Verdana/Arial — none present;
only DejaVu/Liberation are). It falls back for letters but maps digits to `.notdef`. The list (Slint's
own text) showed the digits fine, so it was render-side, font-specific.

## Fix
In `safehtml::document` (applied to all rendered HTML), `add_font_fallbacks` appends `, sans-serif`
to every `font-family` value (inline `style=` or in `<style>`) that doesn't already name a generic
family. A missing named font then falls through to an installed generic, which has digits. Values
that already list a generic are untouched, preserving the email's chosen fonts where available.

## Acceptance criteria
1. The real test email renders all its numbers (verified by re-render: 15.000 €, 10 Jahre, 100 %,
   80 Monate, 8.604,33 €, the dates).
2. `add_font_fallbacks`: bare named font → fallback appended; value with a generic → unchanged;
   `<style>` block + quoted `'PT Serif'` handled; no-font HTML untouched (unit test).
3. build/test/clippy -D warnings (+dangerous-tls)/fmt/cargo deny green.

## Notes
- Spike got a `GELEIT_SPIKE_EML=<path>` mode to render a real .eml through the exact app path and
  dump the decoded HTML — used to diagnose this.
- Remaining empty bordered boxes in such emails are blocked remote images (load via "Load images").
