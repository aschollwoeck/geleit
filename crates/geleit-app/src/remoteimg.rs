//! Opt-in remote-image loading (PRIV-2). When the user clicks "Load images", we fetch the message's
//! remote images and inline them as `data:` URIs so the offline CPU HTML renderer can show them —
//! the renderer itself never touches the network. This is the ONLY place the app makes an outbound
//! HTTP request for mail content, and it runs only on an explicit click, on a worker thread.
use std::io::Read;
use std::time::Duration;

use base64::Engine;

const MAX_IMAGES: usize = 80; // cap fetches per message
const MAX_BYTES: usize = 8 * 1024 * 1024; // skip images larger than 8 MB
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const READ_TIMEOUT: Duration = Duration::from_secs(12);

/// Fetch the `http(s)` images referenced by `src="..."` in `html` and return `html` with each
/// replaced by a `data:` URI. Best-effort: anything that fails to fetch (or isn't an image, or is
/// too big) is left untouched. Blocking network — call on a worker thread, never the UI thread.
pub fn inline_remote_images(html: &str) -> String {
    let urls = extract_img_urls(html);
    if urls.is_empty() {
        return html.to_owned();
    }
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(READ_TIMEOUT)
        .redirects(4)
        .build();
    let mut out = html.to_owned();
    for url in urls.into_iter().take(MAX_IMAGES) {
        if let Some(data_uri) = fetch_as_data_uri(&agent, &url) {
            out = out.replace(&format!("src=\"{url}\""), &format!("src=\"{data_uri}\""));
        }
    }
    out
}

/// Unique `http(s)` URLs appearing in `src="..."` (the sanitizer emits double-quoted attributes).
fn extract_img_urls(html: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("src=\"") {
        rest = &rest[i + 5..];
        let Some(end) = rest.find('"') else { break };
        let url = &rest[..end];
        if (url.starts_with("http://") || url.starts_with("https://"))
            && !urls.iter().any(|u: &String| u == url)
        {
            urls.push(url.to_owned());
        }
        rest = &rest[end + 1..];
    }
    urls
}

fn fetch_as_data_uri(agent: &ureq::Agent, url: &str) -> Option<String> {
    let resp = agent.get(url).call().ok()?;
    let ct = resp
        .header("Content-Type")
        .unwrap_or("image/png")
        .split(';')
        .next()
        .unwrap_or("image/png")
        .trim()
        .to_owned();
    if !ct.starts_with("image/") {
        return None; // only images — never inline html/scripts/etc.
    }
    let mut buf = Vec::new();
    resp.into_reader()
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut buf)
        .ok()?;
    if buf.is_empty() || buf.len() > MAX_BYTES {
        return None;
    }
    let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);
    Some(format!("data:{ct};base64,{b64}"))
}

#[cfg(test)]
mod tests {
    use super::extract_img_urls;

    #[test]
    fn extracts_unique_http_image_urls() {
        let html = r#"<img src="https://a.test/1.png"><img src="data:image/png;base64,xx">
            <img src="http://b.test/2.gif"><img src="https://a.test/1.png">"#;
        let urls = extract_img_urls(html);
        assert_eq!(urls, vec!["https://a.test/1.png", "http://b.test/2.gif"]);
    }
}
