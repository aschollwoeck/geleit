//! Interactive CPU HTML rendering for the reading pane (READ-12). A live Blitz `HtmlDocument` is kept
//! per open message; we render only the visible **viewport** (no GPU, no native window, no giant
//! bitmap) and forward Slint's mouse / scroll events into Blitz so text is **selectable**, scrollable,
//! and links clickable — like a browser. Proven feasible by the spikes (docs/technical/).
use anyrender::{render_to_buffer, PaintScene as _};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use base64::Engine;
use blitz_dom::{local_name, Document, DocumentConfig};
use blitz_html::HtmlDocument;
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, MouseEventButton, PointerCoords, UiEvent,
};
use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request};
use blitz_traits::shell::{ColorScheme, Viewport};
use peniko::kurbo::Rect;
use peniko::{Color, Fill};
use std::sync::Arc;

/// Blitz resolves every image resource (including `data:`) through a `NetProvider`. Ours is fully
/// offline: it decodes `data:` URIs locally and serves nothing else — so inline images and any the
/// user explicitly loaded (turned into `data:` URIs by [`crate::remoteimg`]) render, while raw
/// `http(s)` references are never fetched here. (Without any provider, Blitz draws no images at all.)
struct DataUriProvider;

impl NetProvider for DataUriProvider {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        let url = request.url.as_str();
        let Some(comma) = url.strip_prefix("data:").and(url.find(',')) else {
            return; // not a data: URI → not served (http/cid/etc. don't load here)
        };
        let (meta, data) = (&url[..comma], &url[comma + 1..]);
        let bytes = if meta.contains(";base64") {
            base64::engine::general_purpose::STANDARD.decode(data).ok()
        } else {
            Some(data.as_bytes().to_vec())
        };
        if let Some(b) = bytes {
            handler.bytes(url.to_string(), Bytes::from(b));
        }
    }
}

/// A live, interactive HTML message in the reading pane. Holds the laid-out Blitz document; renders
/// the visible viewport on demand and is driven by Slint mouse/scroll events.
pub struct HtmlView {
    doc: HtmlDocument,
    html: String,
    width: u32,
    height: u32,
    dark: bool,
    dragging: bool,
}

/// Build + lay out a document for a `width`×`height` px viewport.
fn build_doc(html: &str, width: u32, height: u32, dark: bool) -> HtmlDocument {
    let scheme = if dark {
        ColorScheme::Dark
    } else {
        ColorScheme::Light
    };
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(width, height, 1.0, scheme)),
            net_provider: Some(Arc::new(DataUriProvider)),
            ..Default::default()
        },
    );
    // Resolve a few times so images served during the first pass get laid out.
    for _ in 0..3 {
        doc.as_mut().resolve(0.0);
    }
    doc
}

impl HtmlView {
    /// Open a message in a `width`×`height` px viewport. `dark` picks the page background.
    pub fn open(html: &str, width: u32, height: u32, dark: bool) -> Self {
        let (width, height) = (width.max(1), height.max(1));
        Self {
            doc: build_doc(html, width, height, dark),
            html: html.to_owned(),
            width,
            height,
            dark,
            dragging: false,
        }
    }

    /// Re-lay-out for a new viewport size (the body area was resized / first sized), keeping the
    /// scroll position. No-op if the size is unchanged.
    pub fn resize(&mut self, width: u32, height: u32) {
        let (width, height) = (width.max(1), height.max(1));
        if width == self.width && height == self.height {
            return;
        }
        let scroll = self.scroll_y();
        self.width = width;
        self.height = height;
        self.doc = build_doc(&self.html, width, height, self.dark);
        self.scroll_to(scroll);
    }

    /// Paint the current viewport (at the current scroll offset) into a viewport-sized bitmap.
    pub fn render(&mut self) -> slint::Image {
        let (w, h) = (self.width, self.height);
        let bg = if self.dark {
            Color::from_rgb8(0x16, 0x21, 0x1f)
        } else {
            Color::from_rgb8(0xfb, 0xfa, 0xf7)
        };
        let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
            |scene| {
                scene.fill(
                    Fill::NonZero,
                    Default::default(),
                    bg,
                    None,
                    &Rect::new(0.0, 0.0, w as f64, h as f64),
                );
                // scroll is stored in the document; paint_scene applies it.
                blitz_paint::paint_scene(scene, self.doc.as_mut(), 1.0, w, h, 0, 0);
            },
            w,
            h,
        );
        let pb = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&buffer, w, h);
        slint::Image::from_rgba8(pb)
    }

    /// Total laid-out content height (px) — for sizing the scrollbar.
    pub fn content_height(&self) -> f32 {
        self.doc.as_ref().root_element().final_layout.size.height
    }

    pub fn viewport_height(&self) -> f32 {
        self.height as f32
    }

    pub fn scroll_y(&self) -> f32 {
        self.doc.as_ref().viewport_scroll().y as f32
    }

    /// Scroll by `dy` px (positive = down); auto-clamped to the content. Returns whether it moved.
    pub fn scroll_by(&mut self, dy: f32) -> bool {
        // `scroll_viewport_by` subtracts its argument from the scroll offset, so negate for "down".
        self.doc
            .as_mut()
            .scroll_viewport_by_has_changed(0.0, -dy as f64)
    }

    /// Scroll so the top of the viewport is at `y` px (clamped). Returns whether it moved.
    pub fn scroll_to(&mut self, y: f32) -> bool {
        self.scroll_by(y - self.scroll_y())
    }

    fn pointer_event(
        &self,
        x: f32,
        y: f32,
        button: MouseEventButton,
        held: bool,
    ) -> BlitzPointerEvent {
        let scroll = self.scroll_y();
        BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords: PointerCoords {
                // page coords = viewport coords + scroll; client coords = viewport-relative
                page_x: x,
                page_y: y + scroll,
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

    /// Begin a selection (or a potential click) at a viewport point.
    pub fn pointer_down(&mut self, x: f32, y: f32) {
        self.dragging = true;
        let ev = self.pointer_event(x, y, MouseEventButton::Main, true);
        self.doc.handle_ui_event(UiEvent::PointerDown(ev));
    }

    /// Extend the selection while dragging. Returns true if a drag is in progress (caller re-renders).
    pub fn pointer_move(&mut self, x: f32, y: f32) -> bool {
        if !self.dragging {
            return false;
        }
        let ev = self.pointer_event(x, y, MouseEventButton::default(), true);
        self.doc.handle_ui_event(UiEvent::PointerMove(ev));
        true
    }

    pub fn pointer_up(&mut self, x: f32, y: f32) {
        self.dragging = false;
        let ev = self.pointer_event(x, y, MouseEventButton::Main, false);
        self.doc.handle_ui_event(UiEvent::PointerUp(ev));
    }

    pub fn selected_text(&self) -> Option<String> {
        self.doc
            .as_ref()
            .get_selected_text()
            .filter(|s| !s.is_empty())
    }

    pub fn clear_selection(&mut self) {
        self.doc.as_mut().clear_text_selection();
    }

    /// The `href` of the link at a viewport point, if any — walks up to the nearest `<a href>`.
    pub fn link_at(&self, x: f32, y: f32) -> Option<String> {
        let base = self.doc.as_ref();
        let mut id = Some(base.hit(x, y + self.scroll_y())?.node_id);
        while let Some(nid) = id {
            let node = base.get_node(nid)?;
            if let Some(href) = node.attr(local_name!("href")) {
                if !href.is_empty() {
                    return Some(href.to_owned());
                }
            }
            id = node.parent;
        }
        None
    }
}
