use tokio::sync::broadcast;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", content = "data")]
pub enum WsEvent {
    NewEmail {
        account_id: String,
        message_id: String,
    },
    SyncStatus {
        account_id: String,
        status: String,
        progress: Option<f32>,
    },
    SyncComplete {
        account_id: String,
    },
    AiProcessed {
        message_id: String,
    },
    JobCompleted {
        message_id: Option<String>,
        job_type: String,
    },
}

#[derive(Clone)]
pub struct WsHub {
    sender: broadcast::Sender<String>,
}

impl WsHub {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(256);
        Self { sender }
    }

    pub fn broadcast(&self, event: WsEvent) {
        let json = serde_json::to_string(&event).unwrap_or_default();
        let _ = self.sender.send(json);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.sender.subscribe()
    }
}
