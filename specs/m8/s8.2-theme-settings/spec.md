# S8.2 — Light/dark theme (APP-3) + settings (APP-4) · Spec

## In scope
- Palette gains `dark` with "Soft dusk" colors; every color is a `dark ? … : …` ternary. `export`ed so
  Rust can set it.
- Store: migration #12 `setting(key, value)` k/v table (app-wide, not account-scoped) + get/set.
- App: a **Settings…** rail link → a settings overlay with a Light/Dark toggle. The choice is
  persisted (`theme` setting) and applied at startup before first paint; Esc closes the overlay.

## Out of scope
- "System" (follow-OS) theme; per-account settings; more preferences (font size, density) — the panel
  is scaffolded for them.

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. `get_setting`/`set_setting` (incl. upsert) tested + store mutants 0-missed.
3. Toggling theme recolors the app + survives restart (maintainer eyeballs the colors).

## Deliverables
- Palette dark variant; migration #12 + get/set + test; settings overlay + rail link + toggle;
  startup apply.
