//! S0.2 throwaway spike — render an HTML email in a sandboxed `wry` webview.
//!
//! Usage: `s0_2_html_render <fixture.html> [--raw|--sanitize]`
//! - `--sanitize` runs the HTML through `ammonia` first (strips scripts, on* handlers, and
//!   restricts URL schemes so remote refs are dropped) — the shipped-path simulation.
//! - `--raw` loads the HTML untouched — demonstrates the threat.
//!
//! The window auto-closes after a short delay so runs are scriptable and capturable under
//! `strace` (the zero-network evidence). THROWAWAY code — not held to project guidelines.

use std::collections::HashSet;
use std::time::{Duration, Instant};
use std::{env, fs};

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
#[cfg(target_os = "linux")]
use tao::platform::unix::WindowExtUnix;
use tao::window::WindowBuilder;
use wry::WebViewBuilder;
#[cfg(target_os = "linux")]
use wry::WebViewBuilderExtUnix;

/// Strict email sanitizer for the spike. `ammonia` already removes `<script>` and all `on*`
/// handlers; we additionally restrict URL schemes to inline-only so any remote-loading
/// reference (img src, etc.) is dropped, and remove tags that pull remote resources.
fn sanitize(html: &str) -> String {
    let schemes: HashSet<&str> = ["cid", "data", "mailto"].into_iter().collect();
    ammonia::Builder::default()
        .url_schemes(schemes)
        .rm_tags(["link", "style", "iframe", "object", "embed", "base"])
        .clean(html)
        .to_string()
}

fn main() -> wry::Result<()> {
    let args: Vec<String> = env::args().collect();
    let path = args
        .get(1)
        .expect("usage: s0_2_html_render <fixture.html> [--raw|--sanitize]");
    let do_sanitize = args.iter().any(|a| a == "--sanitize");

    let raw = fs::read_to_string(path).expect("read fixture");
    let content = if do_sanitize {
        sanitize(&raw)
    } else {
        raw.clone()
    };

    eprintln!(
        "[spike] file={path} mode={} in_bytes={} out_bytes={} contains_script={} contains_http={}",
        if do_sanitize { "sanitize" } else { "raw" },
        raw.len(),
        content.len(),
        content.contains("<script"),
        content.contains("http://") || content.contains("https://"),
    );

    // `--dump`: print the (possibly sanitized) HTML and exit without opening a window.
    // Used to inspect fidelity impact of sanitization without a GUI/screenshot.
    if args.iter().any(|a| a == "--dump") {
        println!("{content}");
        return Ok(());
    }

    let event_loop = EventLoopBuilder::new().build();
    let window = WindowBuilder::new()
        .with_title("S0.2 HTML spike")
        .build(&event_loop)
        .expect("window");

    // IPC oracle: if the page's JavaScript runs, it calls window.ipc.postMessage(...) and we
    // print it here. This is a network-independent proof of whether JS executes in this config.
    let builder = WebViewBuilder::new()
        .with_ipc_handler(|req| eprintln!("[spike] IPC-FROM-PAGE: {}", req.body()))
        // Diagnostic: a wry-injected init script. If JS executes at all in this config, this
        // fires at document-start regardless of page content.
        .with_initialization_script("window.ipc.postMessage('init-script-ran')")
        .with_html(content);
    #[cfg(target_os = "linux")]
    let _webview = {
        let vbox = window.default_vbox().expect("gtk vbox");
        builder.build_gtk(vbox)?
    };
    #[cfg(not(target_os = "linux"))]
    let _webview = builder.build(&window)?;

    let start = Instant::now();
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(100));
        if let Event::NewEvents(StartCause::Init) = event {
            eprintln!("[spike] window initialised, rendering…");
        }
        if start.elapsed() > Duration::from_millis(2500) {
            eprintln!("[spike] auto-exit");
            *control_flow = ControlFlow::Exit;
        }
    });
}
