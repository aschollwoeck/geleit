// The web host's shim (ADR-0014) — the exact four globals the WASM UI expects, backed by HTTP + SSE
// instead of the Tauri bridge. Same npm-free approach as the desktop shim: plain JS, no bundler, so
// `cargo` + `deny.toml` still cover the whole tree. The UI itself is byte-for-byte the desktop build.

// Theme before first paint, so there's no light-then-dark flash. Mirrors the last choice; the app
// reconciles it against the store on mount.
try {
  const t = localStorage.getItem('geleit-theme');
  if (t === 'dark' || (!t && matchMedia('(prefers-color-scheme: dark)').matches)) {
    document.documentElement.dataset.theme = 'dark';
  }
} catch (_) {
  /* private mode / storage disabled: fall through to light */
}

// invoke → POST /invoke/<cmd>. A command's Ok(T) comes back as JSON (204 for unit); its Err(String)
// comes back as a non-2xx whose body IS that string — thrown so the UI's calm error text picks it up
// (api.rs `js_error_text` reads `.as_string()` off the thrown value).
window.geleitInvoke = async function (cmd, args) {
  const res = await fetch('/invoke/' + encodeURIComponent(cmd), {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(args ?? {}),
  });
  if (!res.ok) {
    throw await res.text();
  }
  if (res.status === 204) {
    return null;
  }
  return await res.json();
};

// One shared Server-Sent-Events stream carries every backend push; each subscriber filters by name.
let eventStream = null;
function events() {
  if (!eventStream) {
    eventStream = new EventSource('/events');
  }
  return eventStream;
}
window.geleitOnSyncProgress = function (cb) {
  events().addEventListener('sync-progress', function (e) { cb(JSON.parse(e.data)); });
};
window.geleitOnMailArrived = function (cb) {
  events().addEventListener('mail-arrived', function (e) { cb(JSON.parse(e.data)); });
};
window.geleitOnUpdateAvailable = function (cb) {
  events().addEventListener('update-available', function (e) { cb(JSON.parse(e.data)); });
};

// A message's HTML is served from this host at /mail/<id>, on its own opaque origin (the reading
// pane sandboxes the iframe without allow-same-origin) with its own strict CSP.
window.geleitMailUrl = function (id, images) {
  return '/mail/' + id + (images ? '?images=1' : '');
};
