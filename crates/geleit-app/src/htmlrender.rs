//! Pure-CPU HTML rendering for the reading pane (replaces the webkit GL webview, which caused the
//! GL-on-X11 crashes). Renders sanitized mail HTML to a bitmap with Blitz + `anyrender_vello_cpu`
//! (no GPU, no native child window), and keeps the DOM so link clicks can be hit-tested.
use anyrender::{render_to_buffer, PaintScene as _};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{local_name, DocumentConfig};
use blitz_html::HtmlDocument;
use blitz_traits::shell::{ColorScheme, Viewport};
use peniko::kurbo::Rect;
use peniko::{Color, Fill};

/// Max rendered height (px). Bounds memory for pathological emails; the content scrolls in the pane.
const MAX_H: u32 = 16000;

/// A rendered HTML message: the bitmap to show + the laid-out DOM (for hit-testing link clicks).
pub struct Rendered {
    pub image: slint::Image,
    pub doc: HtmlDocument,
}

/// Render sanitized `html` at `width` px into a bitmap sized to the content height. `dark` picks the
/// page background + color scheme to match the app theme. No network provider is set, so remote
/// content never loads (privacy).
pub fn render(html: &str, width: u32, dark: bool) -> Rendered {
    let width = width.max(1);
    let scheme = if dark {
        ColorScheme::Dark
    } else {
        ColorScheme::Light
    };
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(width, MAX_H, 1.0, scheme)),
            ..Default::default()
        },
    );
    doc.as_mut().resolve(0.0);
    let content_h = doc.as_ref().root_element().final_layout.size.height;
    let h = (content_h.ceil() as u32).clamp(1, MAX_H);

    // page background behind the message (matches the reading pane surface)
    let bg = if dark {
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
                &Rect::new(0.0, 0.0, width as f64, h as f64),
            );
            blitz_paint::paint_scene(scene, doc.as_mut(), 1.0, width, h, 0, 0);
        },
        width,
        h,
    );
    let pixbuf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&buffer, width, h);
    Rendered {
        image: slint::Image::from_rgba8(pixbuf),
        doc,
    }
}

/// The `href` of the link at (`x`, `y`) in the rendered document, if any — walks up from the hit
/// node to the nearest `<a href>`. Used to make rendered links clickable.
pub fn link_at(doc: &HtmlDocument, x: f32, y: f32) -> Option<String> {
    let base = doc.as_ref();
    let mut id = Some(base.hit(x, y)?.node_id);
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
