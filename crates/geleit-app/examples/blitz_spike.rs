//! Spike: render an HTML string to a PNG on the CPU (no GL) via Blitz + anyrender_vello_cpu, through
//! the EXACT path the app uses (sanitize → document() wrapper → content-height render), so we can see
//! what the reading pane actually produces. Run: cargo run -p geleit-app --example blitz_spike
use anyrender::{render_to_buffer, PaintScene as _};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use base64::Engine;
use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request};
use blitz_traits::shell::{ColorScheme, Viewport};
use peniko::kurbo::Rect;
use peniko::{Color, Fill};
use std::sync::Arc;

/// Offline provider: serves `data:` URIs only (decoded locally), ignores everything else.
struct DataUriProvider;
impl NetProvider for DataUriProvider {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        let url = request.url.as_str();
        if let Some(comma) = url.strip_prefix("data:").and_then(|_| url.find(',')) {
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
}

fn main() {
    // A realistic, legacy newsletter: table layout + presentational attrs (bgcolor/width/align/
    // cellpadding) + inline styles + a CTA "button" — the kind of HTML real newsletters use.
    let raw = r##"<!doctype html><html><body style="margin:0;background:#eef2f4;">
      <table width="100%" cellpadding="0" cellspacing="0" bgcolor="#eef2f4"><tr><td align="center">
        <table width="600" cellpadding="0" cellspacing="0" bgcolor="#ffffff" style="margin:24px auto;border-radius:8px;overflow:hidden;font-family:Arial,sans-serif;">
          <tr><td bgcolor="#1c7e7b" style="padding:24px;color:#ffffff;font-size:24px;font-weight:bold;">Acme Weekly</td></tr>
          <tr><td style="padding:24px;color:#222;font-size:15px;line-height:1.6;">
            <h2 style="color:#1c7e7b;margin:0 0 8px;">Your week in review</h2>
            <p>Hi there — here are this week's <b>highlights</b>. Thanks for being a subscriber!</p>
            <table width="100%" cellpadding="8"><tr>
              <td bgcolor="#f4f7f8" align="center" style="border-radius:6px;">Opens<br><b style="font-size:20px;">1,204</b></td>
              <td width="12"></td>
              <td bgcolor="#f4f7f8" align="center" style="border-radius:6px;">Clicks<br><b style="font-size:20px;">317</b></td>
            </tr></table>
            <p style="margin-top:16px;">
              <a href="https://example.com/read" style="background:#1c7e7b;color:#fff;padding:12px 22px;border-radius:6px;text-decoration:none;font-weight:bold;">Read more &rarr;</a>
            </p>
          </td></tr>
          <tr><td bgcolor="#222" style="padding:16px 24px;color:#9aa;font-size:12px;">&copy; Acme &middot; <a href="https://example.com/unsub" style="color:#9cc;">Unsubscribe</a></td></tr>
        </table>
      </td></tr></table>
    </body></html>"##;

    // exactly what the app feeds Blitz (or a raw file via GELEIT_SPIKE_HTML, to isolate rendering)
    let doc_html = if let Ok(path) = std::env::var("GELEIT_SPIKE_EML") {
        // parse a real .eml exactly as the app does, dump the decoded HTML for inspection
        let bytes = std::fs::read(path).unwrap();
        let eml = geleit_engine::message::parse_eml(&bytes);
        let html = eml.html.unwrap_or_default();
        std::fs::write("/tmp/decoded-html.html", &html).unwrap();
        eprintln!(
            "decoded HTML → /tmp/decoded-html.html ({} bytes)",
            html.len()
        );
        let sanitized = geleit_engine::safehtml::sanitize_html(&html);
        geleit_engine::safehtml::document(&sanitized, false)
    } else if let Ok(path) = std::env::var("GELEIT_SPIKE_HTML") {
        std::fs::read_to_string(path).unwrap()
    } else {
        let sanitized = geleit_engine::safehtml::sanitize_html(raw);
        geleit_engine::safehtml::document(&sanitized, false)
    };
    std::fs::write("/tmp/blitz-in.html", &doc_html).unwrap();
    eprintln!(
        "sanitized doc written to /tmp/blitz-in.html ({} bytes)",
        doc_html.len()
    );

    let w = 760u32;
    let mut document = HtmlDocument::from_html(
        &doc_html,
        DocumentConfig {
            viewport: Some(Viewport::new(w, 16000, 1.0, ColorScheme::Light)),
            net_provider: Some(Arc::new(DataUriProvider)),
            ..Default::default()
        },
    );
    // resolve a few times so data: images fetched during the first pass are laid out
    for _ in 0..3 {
        document.as_mut().resolve(0.0);
    }
    let content_h = document.as_ref().root_element().final_layout.size.height;
    let h = (content_h.ceil() as u32).clamp(1, 16000);
    eprintln!("content height = {content_h} → image {w}x{h}");

    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| {
            scene.fill(
                Fill::NonZero,
                Default::default(),
                Color::from_rgb8(0xfb, 0xfa, 0xf7),
                None,
                &Rect::new(0.0, 0.0, w as f64, h as f64),
            );
            blitz_paint::paint_scene(scene, document.as_mut(), 1.0, w, h, 0, 0);
        },
        w,
        h,
    );

    let f = std::fs::File::create("/tmp/blitz-out.png").unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&buffer)
        .unwrap();
    println!("wrote /tmp/blitz-out.png ({w}x{h}, {} bytes)", buffer.len());
}
