# S8.5 — Security review + no-telemetry (PRIV-5) · Spec

## In scope
- A documented security/privacy review (`docs/security-review.md`): crypto-at-rest, secrets, HTML
  sandbox, network egress, dependency hygiene.
- **Enforceable no-telemetry:** verified the dep tree has no HTTP-client/telemetry crates, then added
  a `deny.toml` `[bans]` deny-list (reqwest/hyper/ureq/isahc/surf/sentry/opentelemetry) so CI blocks
  any future egress dep.

## Acceptance criteria
1. `cargo deny check` green with the new bans; the audited crates confirmed absent.
2. The review documents the actual code paths (one TCP connect to the user's IMAP host; lettre to
   their SMTP; CSP-sandboxed webview; SQLCipher + keychain).

## Deliverables
- `docs/security-review.md`; `deny.toml` no-egress bans.
