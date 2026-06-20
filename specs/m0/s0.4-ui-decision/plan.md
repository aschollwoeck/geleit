# S0.4 — Finalize the UI-framework decision · Plan (the HOW)

Implements `spec.md`. Documentation-only; no code, no build artifacts.

## Steps
1. **ADR-0001 → Accepted.** Update the Status line to *Accepted* with a one-line rationale
   ("both M0 gates passed: S0.2 HTML, S0.3 list"). Leave the gate results and consequences.
2. **Finalize `guidelines.md` §13.** Drop the *(provisional — pending M0)* marker and write the
   committed Slint conventions:
   - Framework = Slint; UI crate holds no business logic; engine is the source of truth; the UI
     observes engine state and sends intents; nothing blocks the UI thread (P1).
   - Large collections use Slint's **virtualized `ListView`** bound to a model (S0.3).
   - HTML email renders **only** in the sandboxed webview component (ADR-0001), with
     **pre-render sanitization + JavaScript disabled** and remote content blocked by default
     (S0.2, PRIV-1); a **CSS-aware sanitizer** preserves safe inline CSS (S0.2 carry-forward).
   - Prefer the **GPU backend**; software fallback is acceptable but slower (S0.3).
   - Calm/fast feedback (P3); accessibility + keyboard nav first-class; light/dark theming
     from a single set of theme tokens.
3. **Consistency pass.** Verify ADR-0001 ↔ S0.2/S0.3 findings ↔ guidelines §13 agree.

## Verification
- `grep` shows no "provisional" marker left on §13; ADR-0001 Status = Accepted.
- Code review of the doc diff (guidelines §11) — content review (no tests; no user stories).
