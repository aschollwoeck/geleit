# spikes/ — throwaway feasibility experiments

Code in here is **throwaway** (guidelines §11): it exists to produce evidence for a decision,
not to ship. It is excluded from the Cargo workspace and CI, and is **not** held to the project
guidelines. Nothing here may enter a real milestone without being rewritten from scratch.

- `s0.2-html-render/` — M0 slice S0.2: can a sandboxed webview render real HTML email with
  zero network leakage and no script execution? See `specs/m0/s0.2-html-spike/`.
