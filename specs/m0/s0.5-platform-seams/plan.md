# S0.5 — Platform-abstraction seams · Plan (the HOW)

Implements `spec.md`. References ADR-0001/0003; produces ADR-0004. Dependency-free.

## New crate: `crates/geleit-platform`
UI-agnostic library (no UI types; traits use std/primitives only). Modules:

- `secret` —
  ```rust
  pub trait SecretStore: Send + Sync {
      fn set(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), SecretError>;
      fn get(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>, SecretError>;
      fn delete(&self, service: &str, account: &str) -> Result<(), SecretError>;
  }
  pub struct InMemorySecretStore { /* Mutex<HashMap<(String,String),Vec<u8>>> */ }
  ```
  `InMemorySecretStore` is a real, testable double (explicitly *not* secure — dev/tests only).
- `oauth` —
  ```rust
  pub trait OAuthRedirect: Send + Sync {
      fn obtain_code(&self, auth_url: &str, redirect_port: u16) -> Result<String, OAuthError>;
  }
  pub struct FakeOAuthRedirect { pub code: String } // test double
  ```
- `html` —
  ```rust
  pub trait HtmlRenderHost {
      /// Caller guarantees `sanitized_html` has scripts + remote refs removed (guidelines §13).
      fn render_sanitized(&self, sanitized_html: &str) -> Result<(), RenderError>;
  }
  pub struct NullHtmlRenderHost; // no-op test double
  ```

Each error type (`SecretError`, `OAuthError`, `RenderError`) is a small enum implementing
`Display` + `std::error::Error` **by hand** (no `thiserror` yet — see spec out-of-scope).
Doc comments name the real per-OS backends and the milestone that implements them.

## Wiring
- Add `geleit-platform` to the workspace `members`.
- `geleit-engine` depends on `geleit-platform`; add a small, tested placeholder that uses the
  `SecretStore` seam (e.g. `store_account_marker(&dyn SecretStore, address)`), proving the
  engine consumes the trait. (Real key/credential handling: M1+.)

## Boundary & CI
- Extend `scripts/check-boundary.sh` `ENGINE_CRATES` to include `geleit-platform` (it must not
  depend on `geleit-app`). Direction stays `app → engine → {core, platform}`; Cargo's no-cycle
  rule still enforces it.
- Extend CI `mutants-diff` package list to include `geleit-platform`; nightly mutants stays on
  `geleit-core` (cheap) — optionally add platform.

## Tests (acceptance, infra — measurable, no user stories)
- `InMemorySecretStore`: set→get round-trip, get-missing → None, delete removes, overwrite.
- `FakeOAuthRedirect`: returns the preset code.
- `NullHtmlRenderHost`: accepts sanitized HTML, returns Ok.
- `geleit-engine` placeholder: stores+reads a marker via `InMemorySecretStore`.

## Docs
- `docs/adr/0004-platform-abstraction-seams.md` — the three seams, dependency-free now, doubles
  for testing, real impls deferred (which milestone each).
- Update `docs/technical/workspace.md` crate list with `geleit-platform`.

## Verify
`cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace`, `./scripts/check-boundary.sh`,
`cargo mutants --package geleit-platform` — all green before PR.
