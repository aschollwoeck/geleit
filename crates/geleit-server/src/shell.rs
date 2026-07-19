//! The web host's [`Shell`]: every backend push becomes a Server-Sent Event on one broadcast channel
//! that the browser subscribes to at `/events`.
use geleit_host::Shell;
use tokio::sync::broadcast;

/// A single named SSE frame — `event: <name>\ndata: <data>`.
#[derive(Clone, Debug)]
pub struct SseEvent {
    pub name: String,
    pub data: String,
}

/// Fans host events out to every connected browser tab. Cloning shares the same channel.
#[derive(Clone)]
pub struct ServerShell {
    tx: broadcast::Sender<SseEvent>,
}

impl ServerShell {
    #[must_use]
    pub fn new(tx: broadcast::Sender<SseEvent>) -> Self {
        Self { tx }
    }
}

impl Shell for ServerShell {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        // A send fails only when no tab is currently listening — which is fine, the event is simply
        // dropped (the UI re-reads state on its next interaction).
        let _ = self.tx.send(SseEvent {
            name: event.to_owned(),
            data: payload.to_string(),
        });
    }

    fn set_badge(&self, title: &str) {
        // A browser tab has no OS window to title, so the badge is just another event; a later slice
        // can drive an in-page unread indicator (or the tab title) from it.
        let _ = self.tx.send(SseEvent {
            name: "badge".to_owned(),
            data: serde_json::Value::String(title.to_owned()).to_string(),
        });
    }
}
