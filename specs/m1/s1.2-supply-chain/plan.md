# S1.2 — First-dependency setup · Plan (the HOW)

Implements `spec.md`. References guidelines §4/§6, ADR-0004.

## cargo-deny
- Add **`deny.toml`** at repo root:
  - `[advisories]` version 2, `yanked = "deny"` (RustSec advisory DB).
  - `[licenses]` version 2, allow a permissive SPDX set: MIT, Apache-2.0 (+ LLVM-exception),
    BSD-2/3-Clause, ISC, Unicode-3.0, Zlib, MPL-2.0, CC0-1.0 — all MIT-compatible for an MIT
    project. (Tighten later if needed.)
  - `[bans]` `multiple-versions = "warn"`, `wildcards = "deny"`.
  - `[sources]` `unknown-registry = "deny"`, `unknown-git = "deny"`.
- **CI:** new `supply-chain` job in `.github/workflows/ci.yml` (PR + push), installs cargo-deny
  via `taiki-e/install-action` and runs `cargo deny check`. (cargo-deny's advisories check is the
  same RustSec DB as `cargo audit`, so a separate audit step isn't needed.)

## thiserror migration (`geleit-platform`)
- `thiserror = "2"` already added to the crate. For each error enum, replace the hand-rolled
  `Display` + `std::error::Error` impls with derives, keeping the **exact same messages**:
  ```rust
  #[derive(Debug, thiserror::Error)]
  pub enum SecretError {
      #[error("secret store backend error: {0}")]
      Backend(String),
  }
  ```
  Same for `OAuthError` (`Backend(String)` + unit `Denied`) and `RenderError` (`Host(String)`).
  Drop the now-unused `use std::fmt;`.
- The existing Display tests assert the message substrings — they must still pass unchanged.

## Docs
- ADR-0004: change the "dependency-free … thiserror arrives in M1" note to record that thiserror
  + supply-chain CI landed in this slice; geleit-platform Cargo.toml comment updated likewise.

## Verify
- `cargo deny check` clean · `cargo build/test --workspace` · `clippy -D warnings` · `fmt --check`
  · `cargo mutants --package geleit-platform` — all green. Confirm CI's supply-chain job passes.
