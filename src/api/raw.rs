use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::{AppError, AppState};

// ---------------------------------------------------------------------------
// POST /raw  — send any WebSocket message to the browser, get the response
// ---------------------------------------------------------------------------

/// Pass the exact JSON payload.  If it contains an `agentRequestId` field snproxy
/// uses it as-is; otherwise a fresh ID is injected so the response is correlated.
///
/// Use this when none of the higher-level endpoints cover what you need.
#[derive(Deserialize)]
pub struct RawReq {
    #[serde(flatten)]
    pub payload: Value,
}

pub async fn handler(
    State(s): State<AppState>,
    Json(r): Json<RawReq>,
) -> Result<Json<Value>, AppError> {
    let payload = r.payload;

    if payload.get("action").and_then(|v| v.as_str()).is_none() {
        return Err(AppError::BadRequest("payload must contain an 'action' field".into()));
    }

    // If the caller already embedded agentRequestId, honour it; otherwise
    // call() injects one automatically.
    let resp = s.call(payload).await?;
    Ok(Json(json!({ "response": resp })))
}
