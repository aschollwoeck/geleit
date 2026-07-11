// Boots the Leptos frontend, replacing the skeleton that index.html painted while WebKit was still
// spawning its web process (~630 ms — docs/technical/tauri-webkit-spike.md).
//
// External, not inline: Tauri's CSP nonce injection does not reach inline module scripts, so an
// inline version silently never runs — the app sits on its skeleton forever, which looks exactly
// like a hang. Loading from a file keeps `script-src 'self'` sufficient.
import init from '/pkg/geleit_ui.js';

try {
  await init(); // instantiates the wasm; #[wasm_bindgen(start)] mounts the app into #app
} catch (e) {
  // Never leave the user staring at a frozen skeleton — a hang is the worst failure to report.
  const app = document.getElementById('app');
  app.textContent = '';
  const p = document.createElement('p');
  p.className = 'empty';
  p.textContent = "GeleitMail couldn't start its interface.";
  const code = document.createElement('code');
  code.textContent = String((e && e.stack) || e);
  p.append(document.createElement('br'), code);
  app.append(p);
}
