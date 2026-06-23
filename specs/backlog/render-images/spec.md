# Backlog — Render images in HTML mail + opt-in remote-image loading (PRIV-2)

A real image-based newsletter rendered as empty boxes. Root cause: Blitz draws NO images unless a
`NetProvider` resolves them — even `data:` URIs — and the initial Blitz integration set none. So
images never rendered. Separately, the per-message "load remote images" opt-in (PRIV-2) was dropped
in the webview→Blitz move.

## Fix
1. `htmlrender::DataUriProvider` — an offline `NetProvider` that decodes `data:` URIs locally and
   serves nothing else; set on every render + resolve a few passes so images lay out. This makes
   inline `data:` images render and is the prerequisite for any image to show.
2. Opt-in remote images (PRIV-2): `remoteimg::inline_remote_images` fetches a message's `http(s)`
   `<img>` URLs with `ureq` (the only allowed HTTP client; deny.toml relaxed for it), inlines them as
   `data:` URIs, on a worker thread, only when the user clicks **Load images**; the UI then re-renders
   (apply-loaded-html) so the provider serves them. Caps: ≤80 images, ≤8 MB each, image/* only.

## Acceptance criteria
1. Inline + loaded images render in-app (verified by screenshot — banner shows after Load images).
2. Default: remote images blocked, cue shown; nothing fetched until the explicit click.
3. The fetch runs off the UI thread; never on open.
4. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny` green (ureq allow passes).

## Follow-ups
cid: (inline-attachment) images; "loading…" affordance polish; per-image error placeholder.
