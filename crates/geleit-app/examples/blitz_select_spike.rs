//! SPIKE (throwaway): prove the core of in-app interactive Blitz (option B) — drive a TEXT SELECTION
//! by feeding pointer events to a blitz-dom document (no window, no blitz-shell), render it with the
//! selection highlighted, and read the selected text back. If this works, the in-app version is just
//! wiring Slint's mouse/scroll events to these calls + re-rendering the viewport.
//!   cargo run -p geleit-app --example blitz_select_spike
use anyrender::{render_to_buffer, PaintScene as _};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{Document, DocumentConfig};
use blitz_html::HtmlDocument;
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, MouseEventButton, PointerCoords, UiEvent,
};
use blitz_traits::shell::{ColorScheme, Viewport};
use peniko::kurbo::Rect;
use peniko::{Color, Fill};

fn pointer(x: f32, y: f32, button: MouseEventButton, held: bool) -> BlitzPointerEvent {
    BlitzPointerEvent {
        id: BlitzPointerId::Mouse,
        is_primary: true,
        coords: PointerCoords {
            page_x: x,
            page_y: y,
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y,
        },
        button,
        buttons: if held {
            MouseEventButton::Main.into()
        } else {
            Default::default()
        },
        mods: Default::default(),
        details: Default::default(),
    }
}

fn main() {
    let body = "<h1>Selectable heading</h1>\
        <p>Drag-select this paragraph of text. The quick brown fox jumps over the lazy dog. \
        Numbers 1234567890 and a <a href='https://example.com'>link</a>.</p>";
    let html = geleit_engine::safehtml::document(body, true);
    let (w, h) = (700u32, 300u32);
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(w, h, 1.0, ColorScheme::Light)),
            ..Default::default()
        },
    );
    for _ in 0..3 {
        doc.as_mut().resolve(0.0);
    }

    // Simulate a drag from (16,70) to (520,90) across the paragraph line(s).
    doc.handle_ui_event(UiEvent::PointerDown(pointer(
        22.0,
        114.0,
        MouseEventButton::Main,
        true,
    )));
    doc.handle_ui_event(UiEvent::PointerMove(pointer(
        300.0,
        137.0,
        MouseEventButton::Main,
        true,
    )));
    doc.handle_ui_event(UiEvent::PointerUp(pointer(
        300.0,
        137.0,
        MouseEventButton::Main,
        false,
    )));
    for _ in 0..2 {
        doc.as_mut().resolve(0.0);
    }

    println!("SELECTED TEXT: {:?}", doc.as_ref().get_selected_text());

    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| {
            scene.fill(
                Fill::NonZero,
                Default::default(),
                Color::WHITE,
                None,
                &Rect::new(0.0, 0.0, w as f64, h as f64),
            );
            blitz_paint::paint_scene(scene, doc.as_mut(), 1.0, w, h, 0, 0);
        },
        w,
        h,
    );
    let f = std::fs::File::create("/tmp/sel.png").unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&buffer)
        .unwrap();
    println!("wrote /tmp/sel.png");
}
