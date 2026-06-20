# ADR-0004: Platform-abstraction seams

## Status
Accepted (slice S0.5).

## Context
The product is cross-platform desktop (constitution), but only Linux is the active dev target
(roadmap). Several capabilities are implemented differently per OS or per UI host: OS keychain,
desktop OAuth loopback, and the HTML render host. If engine/UI code called these directly,
cross-platform support and testability would become deferred risk.

## Decision
Introduce a UI-agnostic crate **`geleit-platform`** holding **trait seams** for these
capabilities, so the rest of the workspace depends on interfaces, not platforms:

- **`secret::SecretStore`** — `(service, account)` → secret bytes. Real backends later: Secret
  Service/libsecret (Linux), Keychain (macOS), Credential Manager (Windows). Used for the
  at-rest key (M1) and OAuth tokens (M7).
- **`oauth::OAuthRedirect`** — desktop OAuth loopback (open auth URL, capture localhost
  redirect, return the code). Real impl M7.
- **`html::HtmlRenderHost`** — display *already-sanitized* email HTML in the sandboxed webview
  (ADR-0001). Real impl M3/M4 in the UI crate.

Each trait ships a **testable double** now — `InMemorySecretStore` (real, in-memory; explicitly
not secure), `FakeOAuthRedirect`, `NullHtmlRenderHost` — so dependent code can be written and
tested before the OS implementations exist.

Dependency direction is `app → engine → {core, platform}`; `geleit-platform` is UI-agnostic and
Cargo's no-cycle rule plus the boundary check (`scripts/check-boundary.sh`) keep it from
depending on the UI crate.

`geleit-platform` was dependency-free in M0 (hand-rolled errors) to preserve the "zero
third-party deps" state. **Updated (M1/S1.2):** `thiserror` is now adopted for the error types
(guidelines §4), and the supply-chain gate (`cargo-deny`: advisories + licenses + bans +
sources, guidelines §6) is wired into CI — so every dependency from here on is checked.

## Provisional API surfaces (revisit when the real impls land)
These trait shapes are deliberately minimal for S0.5 and **will change** when the real
implementations arrive — flagged now so callers written against them expect churn:
- **`SecretStore` secret bytes** are plain `&[u8]` / `Vec<u8>`. Guidelines §9 wants in-memory
  secrets zeroized; when `zeroize` lands with the first real dependency (M1), the secret type
  becomes a zeroizing wrapper (a breaking change to the trait).
- **`OAuthRedirect::obtain_code`** returns only the code and takes a caller-chosen
  `redirect_port`. The real desktop loopback (M7, RFC 8252) is a security-critical path (P6) that
  must carry a CSRF `state` and PKCE `code_verifier` and typically binds an ephemeral port; the
  signature will widen to pass/verify `state` + PKCE and likely to report the bound port.

## Consequences
- Engine/UI code targets the traits; swapping in a real OS backend is additive and local.
- Tests use the doubles — no OS keychain / browser / webview needed in unit tests.
- The `HtmlRenderHost` trait is pure (takes `&str`), so it lives in the engine-facing platform
  crate without pulling UI types across the boundary; the UI crate provides the real impl.
- When the real keychain/loopback/webview impls land, this ADR is updated, not replaced.
