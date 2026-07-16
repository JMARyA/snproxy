use axum::{
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use tracing::{debug, warn};

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
    /// Cached ServiceNow instance object {url, name, g_ck} received when user runs /token.
    /// g_ck is the CSRF token required for all SN API calls — treat as a secret.
    pub sn_instance: Arc<Mutex<Option<Value>>>,
}

impl AppState {
    pub fn new(timeout_secs: u64) -> Self {
        let (event_tx, _) = broadcast::channel(512);
        Self {
            ws_tx: Arc::new(Mutex::new(None)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            event_tx,
            timeout_secs,
            sn_instance: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn connected(&self) -> bool {
        self.ws_tx.lock().await.is_some()
    }

    /// Returns the cached ServiceNow instance object, or an error if /token hasn't been run yet.
    pub async fn get_sn_instance(&self) -> Result<Value, AppError> {
        self.sn_instance
            .lock()
            .await
            .clone()
            .ok_or(AppError::NoInstance)
    }

    /// Returns the cached instance only if it matches `requested`, rejecting
    /// mismatches so callers can't accidentally fire at the wrong environment.
    pub async fn check_instance(&self, requested: &str) -> Result<Value, AppError> {
        let inst = self.get_sn_instance().await?;
        let connected = inst
            .get("name")
            .and_then(|v| v.as_str())
            .map(sncore::normalize_instance)
            .unwrap_or_default();
        let normalized = sncore::normalize_instance(requested);
        if normalized != connected {
            return Err(AppError::InstanceMismatch { requested: normalized, connected });
        }
        Ok(inst)
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

    /// Serialize `cmd`, inject `agentRequestId` + `appName`, send to the Helper
    /// Tab, and block until the correlated reply arrives.  Fully concurrent —
    /// each in-flight call waits on its own oneshot channel.
    ///
    /// Accepts any `T: Serialize`: pass a [`WsCommand`] variant for typed
    /// calls, or a raw [`serde_json::Value`] for the `/raw` passthrough.
    pub async fn call<T: serde::Serialize>(&self, cmd: T) -> Result<Value, AppError> {
        let mut payload = serde_json::to_value(cmd)
            .map_err(|e| AppError::BadRequest(format!("command serialization error: {e}")))?;

        let req_id = new_req_id();
        payload["agentRequestId"] = json!(&req_id);
        payload["appName"] = json!("snproxy");

        let action = payload
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_owned();
        // instance is an object {url,name,g_ck}; fall back to bare string for
        // commands that pass it as a hostname (legacy / raw calls).
        let instance = payload
            .get("instance")
            .and_then(|v| v.get("name").or(Some(v)))
            .and_then(|v| v.as_str())
            .unwrap_or("-")
            .to_owned();

        debug!(%req_id, %action, %instance, "→ sending to Helper Tab");

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(req_id.clone(), tx);

        if let Err(e) = self.fire(payload).await {
            self.pending.lock().await.remove(&req_id);
            warn!(%req_id, %action, "send failed: {e:?}");
            return Err(e);
        }

        match tokio::time::timeout(Duration::from_secs(self.timeout_secs), rx).await {
            Ok(Ok(val)) => {
                debug!(%req_id, %action, "← response received");
                Ok(val)
            }
            Ok(Err(_)) => {
                warn!(%req_id, %action, "response channel closed unexpectedly");
                Err(AppError::ChannelClosed)
            }
            Err(_) => {
                self.pending.lock().await.remove(&req_id);
                warn!(%req_id, %action, timeout_secs = self.timeout_secs, "timed out waiting for Helper Tab");
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
    NoInstance,
    InstanceMismatch { requested: String, connected: String },
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
            AppError::NoInstance => (
                StatusCode::PRECONDITION_FAILED,
                "no ServiceNow session — run /token from your ServiceNow instance first".to_string(),
            ),
            AppError::InstanceMismatch { requested, connected } => (
                StatusCode::CONFLICT,
                format!("instance mismatch: requested '{requested}' but '{connected}' is connected — run /token on the correct instance"),
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
