//! Cold-start harness. `t0` is stamped as the first statement in `main()`, so every reported number
//! includes process exec, GTK init, webview spawn, page load, and runtime boot — i.e. what the user
//! actually waits for after double-clicking the icon.
//!
//!   GELEIT_MODE=wasm cargo run --release   → Leptos/wasm UI
//!   GELEIT_MODE=js   cargo run --release   → identical vanilla-JS UI
use std::borrow::Cow;
use std::path::PathBuf;
use std::time::Instant;
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use wry::http::{header::CONTENT_TYPE, Response};
use wry::WebViewBuilder;

fn mime_for(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html",
        "js" => "text/javascript",
        "css" => "text/css",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

fn main() -> wry::Result<()> {
    let t0 = Instant::now();

    let mode = std::env::var("GELEIT_MODE").unwrap_or_else(|_| "wasm".into());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let index = if mode == "js" {
        "index-js.html"
    } else {
        "index-wasm.html"
    };
    println!("mode={mode}  index={index}");

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("cold start")
        .with_inner_size(tao::dpi::LogicalSize::new(1100.0, 800.0))
        .build(&event_loop)
        .unwrap();

    let serve_root = root.clone();
    let builder = WebViewBuilder::new()
        .with_url(&format!("app://localhost/{index}"))
        .with_custom_protocol("app".into(), move |_id, req| {
            // "/pkg/x.wasm" and "/style.css" both resolve under the spike dir; "www" holds the page,
            // "ui/pkg" the wasm-bindgen output.
            let path = req.uri().path().trim_start_matches('/').to_string();
            let file = if path.starts_with("pkg/") {
                serve_root.join("ui").join(&path)
            } else {
                serve_root.join("www").join(&path)
            };
            match std::fs::read(&file) {
                Ok(bytes) => Response::builder()
                    .header(CONTENT_TYPE, mime_for(&path))
                    .body(Cow::Owned(bytes))
                    .unwrap(),
                Err(e) => Response::builder()
                    .status(404)
                    .body(Cow::Owned(format!("{}: {e}", file.display()).into_bytes()))
                    .unwrap(),
            }
        })
        .with_ipc_handler(move |req| {
            let body = req.body();
            if let Some(name) = body.strip_prefix("MARK ") {
                println!("{:>7.1} ms  {name}", t0.elapsed().as_secs_f64() * 1000.0);
                if name == "done" {
                    println!("--- cold start complete ---");
                    std::process::exit(0);
                }
            }
        });

    #[cfg(not(target_os = "linux"))]
    let _webview = builder.build(&window)?;
    #[cfg(target_os = "linux")]
    let _webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.default_vbox().unwrap())?
    };

    println!("{:>7.1} ms  webview_created", t0.elapsed().as_secs_f64() * 1000.0);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
    });
}
