use axum::{
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};

static REQ_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn new_req_id() -> String {
    format!("sp_{}", REQ_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

#[derive(Clone)]
pub struct AppState {
    /// Outbound channel to the active Helper Tab WebSocket connection.
    pub ws_tx: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
    /// In-flight requests keyed by agentRequestId, waiting for a correlated reply.
    pub pending: Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>,
    /// Every inbound WS message is broadcast here for the SSE stream.
    pub event_tx: broadcast::Sender<Value>,
    pub timeout_secs: u64,
}

impl AppState {
    pub fn new(timeout_secs: u64) -> Self {
        let (event_tx, _) = broadcast::channel(512);
        Self {
            ws_tx: Arc::new(Mutex::new(None)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            event_tx,
            timeout_secs,
        }
    }

    pub async fn connected(&self) -> bool {
        self.ws_tx.lock().await.is_some()
    }

    /// Send a message to the Helper Tab without waiting for a reply.
    pub async fn fire(&self, payload: Value) -> Result<(), AppError> {
        let s = payload.to_string();
        self.ws_tx
            .lock()
            .await
            .as_ref()
            .ok_or(AppError::NoClient)?
            .send(s)
            .map_err(|_| AppError::SendFailed)
    }

    /// Inject a unique `agentRequestId`, send the message, and block until the
    /// Helper Tab echoes that ID back in a response.  Fully concurrent — each
    /// in-flight call waits on its own oneshot channel.
    pub async fn call(&self, mut payload: Value) -> Result<Value, AppError> {
        let req_id = new_req_id();
        payload["agentRequestId"] = json!(&req_id);

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(req_id.clone(), tx);

        if let Err(e) = self.fire(payload).await {
            self.pending.lock().await.remove(&req_id);
            return Err(e);
        }

        match tokio::time::timeout(Duration::from_secs(self.timeout_secs), rx).await {
            Ok(Ok(val)) => Ok(val),
            Ok(Err(_)) => Err(AppError::ChannelClosed),
            Err(_) => {
                self.pending.lock().await.remove(&req_id);
                Err(AppError::Timeout)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum AppError {
    NoClient,
    SendFailed,
    Timeout,
    ChannelClosed,
    BadRequest(String),
    Remote(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, msg) = match self {
            AppError::NoClient => (
                StatusCode::SERVICE_UNAVAILABLE,
                "no Helper Tab connected".to_string(),
            ),
            AppError::SendFailed => (
                StatusCode::SERVICE_UNAVAILABLE,
                "WebSocket send failed".to_string(),
            ),
            AppError::Timeout => (
                StatusCode::GATEWAY_TIMEOUT,
                "timeout waiting for response from Helper Tab".to_string(),
            ),
            AppError::ChannelClosed => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "response channel closed".to_string(),
            ),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            AppError::Remote(msg) => (StatusCode::BAD_GATEWAY, format!("ServiceNow: {msg}")),
        };
        (status, Json(json!({"error": msg}))).into_response()
    }
}

// ---------------------------------------------------------------------------
// Shared helper: check a browser REST-passthrough response for errors
// ---------------------------------------------------------------------------

pub fn check_rest_response(resp: &Value) -> Result<(), AppError> {
    if resp.get("success").and_then(|v| v.as_bool()) == Some(false) {
        let msg = resp
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("REST request failed")
            .to_string();
        return Err(AppError::Remote(msg));
    }
    Ok(())
}
