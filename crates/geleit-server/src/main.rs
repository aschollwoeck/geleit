//! GeleitMail — the localhost web host (ADR-0014).
//!
//! Serves the *same* Leptos/WASM UI as the desktop shell and exposes [`geleit_host`]'s command logic
//! over HTTP instead of the Tauri IPC bridge. Four routes carry everything the UI needs:
//!
//! - `POST /invoke/<cmd>` — a command call; JSON args in, JSON reply out (see [`dispatch`]).
//! - `GET  /mail/<id>`    — a message's sanitized HTML, on its own opaque origin + strict CSP (S9.2).
//! - `GET  /events`       — the one Server-Sent-Events stream that carries every backend push.
//! - everything else      — the static UI bundle (`geleit-app/dist`), with a web `early.js` shim.
//!
//! **Binds `127.0.0.1` only**, so there is no auth: nothing else on the machine can reach it. LAN
//! access + TLS + a login token are a later, opt-in slice. It is otherwise the desktop host's twin —
//! same `AppState`, same store, same keychain — so a self-hosted GeleitMail keeps every byte of mail
//! on the operator's own hardware.
mod auth;
mod dispatch;
mod shell;

use axum::body::Bytes;
use axum::extract::{Path, Query, RawQuery, State};
use axum::http::{header, HeaderValue, StatusCode, Uri};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{middleware, Router};
use geleit_host::{AppState, Shell};
use geleit_platform::os_secret::OsSecretStore;
use geleit_platform::secret::SecretStore;
use shell::{ServerShell, SseEvent};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

/// The CSP for the app document. `wasm-unsafe-eval` is what lets the browser instantiate the WASM UI
/// under an otherwise strict policy; `connect-src 'self'` permits the `/invoke` fetches + `/events`
/// stream; `frame-src 'self'` permits the sandboxed `/mail/<id>` reading-pane iframe.
// `style-src 'unsafe-inline'` is deliberate: unlike WebKitGTK (the desktop shell's engine), real
// browsers enforce style-src for the inline `style=` attributes the Leptos views use, so the same
// `dist/` needs it to render here. Reconciling the rest with the desktop Tauri CSP is tracked polish
// (ADR-0014). `object-src 'none'` matches the desktop's extra hardening.
const APP_CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; \
     style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; \
     connect-src 'self'; frame-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'";

/// Shared across every request. All fields are cheap to clone (the state's heavy bits are `Arc`s).
#[derive(Clone)]
struct AppCtx {
    state: AppState,
    shell: Arc<dyn Shell>,
    events: broadcast::Sender<SseEvent>,
    dist: PathBuf,
    web: PathBuf,
}

#[tokio::main]
async fn main() {
    let db_path = std::env::var("GELEIT_DB").unwrap_or_else(|_| "geleit.db".to_owned());
    let port: u16 = std::env::var("GELEIT_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8080);
    // Bind loopback by default (localhost-only, no auth). Reaching the app across a LAN is opt-in via
    // GELEIT_BIND=0.0.0.0 + GELEIT_PASSWORD (ADR-0014, topology A: put HTTPS in front with a proxy).
    let bind_ip: IpAddr = std::env::var("GELEIT_BIND")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let password: auth::AuthState = std::env::var("GELEIT_PASSWORD")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| Arc::from(s.as_str()));
    // Fail-safe: never expose the mailbox to the network with no auth. A non-loopback bind without a
    // password refuses to start rather than silently serving everyone's mail to the whole subnet.
    if !bind_ip.is_loopback() && password.is_none() {
        eprintln!(
            "geleit-server: refusing to bind {bind_ip} with no GELEIT_PASSWORD — that would serve \
             your mailbox to the network with no auth. Set GELEIT_PASSWORD (and terminate TLS with a \
             reverse proxy, see the README), or leave GELEIT_BIND unset for localhost-only."
        );
        std::process::exit(1);
    }
    let manifest = env!("CARGO_MANIFEST_DIR");
    let dist = std::env::var("GELEIT_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(manifest).join("../geleit-app/dist"));
    let web = std::env::var("GELEIT_WEB")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(manifest).join("web"));

    // Clear any files a previous run left in the staging dir (abandoned uploads / built exports).
    geleit_host::commands::clear_staging();
    // The at-rest key + credentials live in the OS keychain, exactly as the desktop host (SEC-1/2).
    let state = AppState::new(db_path, secret_store());
    let (events, _) = broadcast::channel::<SseEvent>(256);
    let shell: Arc<dyn Shell> = Arc::new(ServerShell::new(events.clone()));

    // Background auto-sync, same as the desktop host (ADR-0014): the scheduler (periodic sweep +
    // notifications + badge), the per-account IMAP IDLE watchers, and the full-mailbox backfill — all
    // host-agnostic now, spawned here on axum's own tokio runtime.
    let (scheduler, idle, backfill) = geleit_host::worker::futures(state.clone(), shell.clone());
    tokio::spawn(scheduler);
    tokio::spawn(idle);
    tokio::spawn(backfill);

    let ctx = AppCtx {
        state,
        shell,
        events,
        dist,
        web,
    };

    let app = Router::new()
        .route("/invoke/{cmd}", post(invoke))
        .route("/mail/{id}", get(mail))
        .route("/events", get(events_stream))
        // Web-native file I/O (ADR-0014): the browser downloads generated files and uploads compose
        // attachments, instead of the native zenity/kdialog dialogs the desktop host uses.
        .route("/download/eml/{id}", get(download_eml))
        .route(
            "/download/attachment/{message_id}/{index}",
            get(download_attachment),
        )
        .route("/download/staged/{token}", get(download_staged))
        .route("/upload", post(upload))
        .fallback(static_file)
        // The auth gate wraps every route (static, downloads, SSE, invoke). No-op when no password is
        // set; otherwise a browser must present a matching HTTP Basic credential.
        .layer(middleware::from_fn_with_state(
            password.clone(),
            auth::require_auth,
        ))
        .with_state(ctx);

    let addr = SocketAddr::from((bind_ip, port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("GeleitMail web host could not bind {addr}: {e}"));
    let auth_note = if password.is_some() {
        "auth: HTTP Basic"
    } else {
        "auth: off (localhost)"
    };
    println!("GeleitMail web host on http://{addr}  ({auth_note}, Ctrl-C to stop)");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .expect("server error");
}

/// The secret store. Release builds always use the OS keychain. Debug builds honour
/// `GELEIT_DEV_MEMORY_SECRETS` to run against a throwaway in-memory store (hermetic live tests against
/// local Dovecot, no keychain/DBus) — the switch does not exist in a release binary.
fn secret_store() -> Arc<dyn SecretStore> {
    #[cfg(debug_assertions)]
    if std::env::var_os("GELEIT_DEV_MEMORY_SECRETS").is_some() {
        eprintln!("geleit-server: DEV in-memory secret store (credentials are not persisted)");
        return Arc::new(geleit_platform::secret::InMemorySecretStore::new());
    }
    Arc::new(OsSecretStore::new())
}

/// `POST /invoke/<cmd>` — run a command. Ok → 200 JSON; Err → 400 with the calm error string as the
/// body (the web shim throws it, so it reaches the UI's error text just like a rejected Tauri call).
async fn invoke(State(ctx): State<AppCtx>, Path(cmd): Path<String>, body: Bytes) -> Response {
    let args: serde_json::Value = if body.is_empty() {
        serde_json::Value::Null
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(_) => return (StatusCode::BAD_REQUEST, "Malformed request.").into_response(),
        }
    };
    match dispatch::dispatch(&ctx.state, &ctx.shell, &cmd, args).await {
        Ok(value) => axum::Json(value).into_response(),
        Err(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
    }
}

/// `GET /mail/<id>[?images=1]` — a message's sanitized HTML, carrying the same locked-down CSP the
/// document embeds (defense in depth), served so the reading pane's sandboxed iframe gets an opaque
/// origin it can't reach back from.
async fn mail(State(ctx): State<AppCtx>, Path(id): Path<i64>, RawQuery(q): RawQuery) -> Response {
    let allow_remote = q.as_deref().is_some_and(|q| {
        q.split('&')
            .any(|kv| matches!(kv, "images=1" | "images=true"))
    });
    let (body, allow) = match geleit_host::commands::message_html(&ctx.state, id, allow_remote) {
        Some(doc) => (doc, allow_remote),
        None => (placeholder("This message has no formatted content."), false),
    };
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8".to_owned()),
            (
                "Content-Security-Policy".to_owned().parse().unwrap(),
                mail_csp(allow),
            ),
        ],
        body,
    )
        .into_response()
}

/// `GET /download/eml/<id>` — a message rebuilt as a `.eml`, served as a browser download.
async fn download_eml(State(ctx): State<AppCtx>, Path(id): Path<i64>) -> Response {
    match geleit_host::commands::eml_bytes(&ctx.state, id).await {
        Ok((bytes, name)) => file_download(bytes, &name, "message/rfc822"),
        Err(msg) => (StatusCode::NOT_FOUND, msg).into_response(),
    }
}

/// `GET /download/attachment/<message_id>/<index>` — an attachment fetched from the server, served as
/// a browser download.
async fn download_attachment(
    State(ctx): State<AppCtx>,
    Path((message_id, index)): Path<(i64, usize)>,
) -> Response {
    match geleit_host::commands::attachment_bytes(&ctx.state, message_id, index).await {
        Ok((bytes, name)) => file_download(bytes, &name, "application/octet-stream"),
        Err(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
    }
}

/// `GET /download/staged/<token>?name=<f>` — a folder export the invoke step built and staged, served
/// once as a browser download (the file is deleted on read).
async fn download_staged(
    Path(token): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let name = params.get("name").map_or("export.mbox", String::as_str);
    match geleit_host::commands::take_staged(&token) {
        Ok(bytes) => file_download(bytes, name, "application/mbox"),
        Err(msg) => (StatusCode::NOT_FOUND, msg).into_response(),
    }
}

/// `POST /upload?name=<filename>` — stage a compose attachment the browser uploaded (the raw file bytes
/// are the body), returning `{ "path": "…" }` for `send_message` to read. The desktop host uses the
/// native picker instead.
async fn upload(Query(params): Query<HashMap<String, String>>, body: Bytes) -> Response {
    let name = params.get("name").map_or("upload", String::as_str);
    match geleit_host::commands::stage_upload(name, &body) {
        Ok(path) => axum::Json(serde_json::json!({ "path": path })).into_response(),
        Err(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
    }
}

/// A file download: the bytes plus a `Content-Disposition: attachment` with a header-safe filename (any
/// quote/backslash/newline stripped so the name can't break out of the header).
fn file_download(bytes: Vec<u8>, filename: &str, content_type: &str) -> Response {
    let safe: String = filename
        .chars()
        .filter(|c| !matches!(c, '"' | '\\' | '\r' | '\n'))
        .collect();
    (
        [
            (header::CONTENT_TYPE, content_type.to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{safe}\""),
            ),
        ],
        bytes,
    )
        .into_response()
}

/// `GET /events` — subscribe to the backend push stream (sync progress, new mail, badge). Lagged
/// frames (a slow tab) are skipped rather than closing the stream.
async fn events_stream(
    State(ctx): State<AppCtx>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let stream = BroadcastStream::new(ctx.events.subscribe()).filter_map(|frame| {
        frame
            .ok()
            .map(|ev| Ok(Event::default().event(ev.name).data(ev.data)))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// The static UI bundle. `/early.js` is served from the web host's own shim; everything else comes
/// from the shared `geleit-app/dist`. Path traversal is refused.
async fn static_file(State(ctx): State<AppCtx>, uri: Uri) -> Response {
    let path = uri.path();
    let rel = if path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };
    if rel.split('/').any(|seg| seg == ".." || seg.is_empty()) {
        return StatusCode::NOT_FOUND.into_response();
    }

    let file = if rel == "early.js" {
        ctx.web.join("early.js")
    } else {
        ctx.dist.join(rel)
    };

    let Ok(bytes) = tokio::fs::read(&file).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let ctype = content_type(rel);
    let mut resp = ([(header::CONTENT_TYPE, ctype)], bytes).into_response();
    // The app document carries the CSP the Tauri build gets via config; the web host owns the header.
    if rel == "index.html" {
        resp.headers_mut()
            .insert("Content-Security-Policy", HeaderValue::from_static(APP_CSP));
    }
    resp
}

/// The CSP for the mail origin — identical to what `webview_document` embeds (a test in the engine
/// enforces the wording), sent as a header too so it holds even if the markup weren't parsed.
fn mail_csp(allow_remote_images: bool) -> String {
    let img_src = if allow_remote_images {
        "data: cid: https:"
    } else {
        "data: cid:"
    };
    format!(
        "default-src 'none'; img-src {img_src}; style-src 'unsafe-inline'; font-src data:; \
         form-action 'none'; base-uri 'none'"
    )
}

/// A calm, inert page for the "nothing to show" cases — still rendered inside the mail origin, so it
/// must be unable to fetch or run anything.
fn placeholder(text: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; \
style-src 'unsafe-inline'\">\
         <style>html{{font-family:system-ui,sans-serif;color:#5e7177;background:#fff;\
margin:0;padding:28px;text-align:center}}</style></head><body>{text}</body></html>"
    )
}

/// Content-Type by extension. `.wasm` → `application/wasm` is load-bearing: the browser refuses to
/// stream-instantiate the module otherwise.
fn content_type(rel: &str) -> &'static str {
    match rel.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("json") => "application/json",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}
