//! HTML render-host seam — the UI host that displays *already-sanitized* email HTML in the
//! sandboxed webview component (ADR-0001, guidelines §13). The engine sanitizes; the host
//! renders. The real implementation (wry/WebKitGTK et al.) lands in M3/M4 in the UI crate.

/// Error rendering HTML in the host.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    /// The underlying render host failed.
    #[error("html render host error: {0}")]
    Host(String),
}

/// Displays pre-sanitized email HTML in the sandboxed webview.
///
/// The caller guarantees `sanitized_html` has had scripts, event handlers, and remote references
/// removed (guidelines §13); the host must additionally render with JavaScript disabled and
/// remote content blocked.
///
/// Intentionally **not** `Send + Sync` (unlike the other seams): the real implementation is a
/// thread-affine webview in the UI crate (ADR-0001), so it must not be forced cross-thread.
pub trait HtmlRenderHost {
    /// Render the given pre-sanitized HTML.
    fn render_sanitized(&self, sanitized_html: &str) -> Result<(), RenderError>;
}

/// No-op [`HtmlRenderHost`] for tests — renders nothing, always succeeds.
#[derive(Default)]
pub struct NullHtmlRenderHost;

impl HtmlRenderHost for NullHtmlRenderHost {
    fn render_sanitized(&self, _sanitized_html: &str) -> Result<(), RenderError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{HtmlRenderHost, NullHtmlRenderHost, RenderError};

    #[test]
    fn error_displays_message() {
        assert!(RenderError::Host("boom".to_owned())
            .to_string()
            .contains("boom"));
    }

    #[test]
    fn null_host_accepts_sanitized_html() {
        assert!(NullHtmlRenderHost.render_sanitized("<p>hi</p>").is_ok());
    }
}
