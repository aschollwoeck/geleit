// Runs before first paint, from <head>.
//
// Deliberately an external file, not an inline <script>: that keeps the app's CSP at a strict
// `script-src 'self'` with no 'unsafe-inline' and no nonce games. (It also has to be external —
// Tauri's nonce injection does not reach inline module scripts, so they silently never run.)

// Theme before first paint, so there is no light-then-dark flash. The store is the source of truth;
// this mirrors the last choice so we don't have to await IPC before painting. The app reconciles it
// on mount.
try {
  const t = localStorage.getItem('geleit-theme');
  if (t === 'dark' || (!t && matchMedia('(prefers-color-scheme: dark)').matches)) {
    document.documentElement.dataset.theme = 'dark';
  }
} catch (_) {
  /* private mode / storage disabled: fall through to light */
}

// The IPC seam. Keeping this shim here, rather than pulling Tauri's npm package, is what lets the
// project stay npm-free — cargo and deny.toml keep covering the entire dependency tree (ADR-0012).
window.geleitInvoke = function (cmd, args) {
  return window.__TAURI__.core.invoke(cmd, args);
};

// Subscribe to backend sync-progress events (S9.4). `cb` gets the batch count, or -1 when the
// backfill has finished. Kept in this shim for the same npm-free reason as invoke.
window.geleitOnSyncProgress = function (cb) {
  window.__TAURI__.event.listen('sync-progress', function (e) { cb(e.payload); });
};

// Subscribe to new-mail events from the background scheduler (NOTIF-1). `cb` gets how many messages
// arrived that are worth announcing. Same npm-free shim as above.
window.geleitOnMailArrived = function (cb) {
  window.__TAURI__.event.listen('mail-arrived', function (e) { cb(e.payload); });
};

// Subscribe to the on-launch auto-update check (APP-7). `cb` gets {version, notes} when a newer signed
// release is available. Same npm-free shim as above.
window.geleitOnUpdateAvailable = function (cb) {
  window.__TAURI__.event.listen('update-available', function (e) { cb(e.payload); });
};
