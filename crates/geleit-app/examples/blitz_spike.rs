//! Spike: render an HTML string to a PNG on the CPU (no GL) via Blitz + anyrender_vello_cpu, to
//! evaluate replacing the webkit webview. Run: cargo run -p geleit-app --example blitz_spike
use anyrender::{render_to_buffer, PaintScene as _};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use blitz_traits::shell::{ColorScheme, Viewport};
use peniko::kurbo::Rect;
use peniko::{Color, Fill};

fn main() {
    let html = r#"<html><head><style>
      body { font-family: sans-serif; color: #1f2a2e; margin: 16px; }
      h1 { color: #1c7e7b; } a { color: #1c7e7b; }
      .box { background: #e2f1f0; padding: 12px; border-radius: 8px; }
      table { border-collapse: collapse; } td { border: 1px solid #ccc; padding: 6px; }
    </style></head><body>
      <h1>This month in GeleitMail</h1>
      <p>A few <b>highlights</b> from the latest release — rendered by <i>Blitz</i> on the CPU.</p>
      <div class="box">A styled box with a <a href="https://example.com">clickable link</a>.</div>
      <table><tr><td>Cell A</td><td>Cell B</td></tr><tr><td>1</td><td>2</td></tr></table>
    </body></html>"#;

    let (w, h, scale) = (800u32, 600u32, 1.0f32);
    let mut document = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(w, h, scale, ColorScheme::Light)),
            ..Default::default()
        },
    );
    document.as_mut().resolve(0.0);

    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| {
            scene.fill(
                Fill::NonZero,
                Default::default(),
                Color::WHITE,
                None,
                &Rect::new(0.0, 0.0, w as f64, h as f64),
            );
            blitz_paint::paint_scene(scene, document.as_mut(), scale as f64, w, h, 0, 0);
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
    println!("wrote /tmp/blitz-out.png ({} bytes)", buffer.len());
}
