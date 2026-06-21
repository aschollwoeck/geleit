//! S3.1 throwaway spike — embed a sandboxed `wry` webview as a child of the Slint window (X11) and
//! render sanitized HTML. Decides whether the real reading-pane HTML viewer is feasible.
//!
//! Oracle: ammonia strips `<script>`/remote refs (PRIV-1/PRIV-4); we additionally inject a page
//! script into the wrapper to check whether the webview itself executes page JS. If `SCRIPT-RAN`
//! ever reaches the IPC handler, the webview runs page JS and we must rely on sanitization.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use slint::winit_030::WinitWindowAccessor;
use slint::ComponentHandle;

slint::slint! {
    export component Demo inherits Window {
        preferred-width: 960px;
        preferred-height: 640px;
        title: "S3.1 embed spike";
        background: #20303a;
        Text { x: 16px; y: 8px; text: "Slint chrome — webview should render to the right →"; color: white; }
    }
}

fn main() {
    gtk::init().expect("gtk init"); // webkit2gtk needs GTK initialised + pumped

    let ui = Demo::new().unwrap();

    // Sanitize like the engine will: drop scripts + remote schemes (only mailto/cid survive).
    let sanitized = ammonia::Builder::default()
        .url_schemes(std::collections::HashSet::from(["mailto", "cid"]))
        .clean(
            "<h1 style='color:#2e9e9b'>Hello from HTML</h1>\
             <p>This is a <b>formatted</b> email with a <a href='http://example.com'>link</a>.</p>\
             <img src='http://example.com/tracker.gif' width=1 height=1>\
             <script>window.ipc.postMessage('SANITIZED-SCRIPT-RAN')</script>",
        )
        .to_string();
    eprintln!("SANITIZED-HTML: {sanitized}");

    let webview = Rc::new(RefCell::new(None));
    let wv = webview.clone();
    let weak = ui.as_weak();
    let init = slint::Timer::default();
    init.start(slint::TimerMode::SingleShot, Duration::from_millis(500), move || {
        let Some(ui) = weak.upgrade() else { return };
        // Wrap with a page script AFTER sanitization to test the webview's own JS execution.
        let html = format!(
            "<!doctype html><html><body style='font-family:sans-serif'>{sanitized}\
             <script>if(window.ipc)window.ipc.postMessage('PAGE-SCRIPT-RAN')</script></body></html>"
        );
        let built = ui.window().with_winit_window(|win| {
            wry::WebViewBuilder::new()
                .with_html(&html)
                .with_bounds(wry::Rect {
                    position: wry::dpi::LogicalPosition::new(360, 0).into(),
                    size: wry::dpi::LogicalSize::new(600, 640).into(),
                })
                .with_ipc_handler(|req| eprintln!("IPC-FROM-PAGE: {}", req.body()))
                .build_as_child(win)
        });
        match built {
            Some(Ok(w)) => {
                eprintln!("WEBVIEW-BUILT-OK");
                *wv.borrow_mut() = Some(w);
            }
            Some(Err(e)) => eprintln!("WEBVIEW-ERR: {e}"),
            None => eprintln!("NO-WINIT-WINDOW"),
        }
    });

    // Pump GTK so webkit2gtk processes/render the embedded webview under Slint's event loop.
    let pump = slint::Timer::default();
    pump.start(slint::TimerMode::Repeated, Duration::from_millis(16), || {
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
    });

    ui.run().unwrap();
}
