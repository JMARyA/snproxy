use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::{AppError, AppState};

// ---------------------------------------------------------------------------
// POST /raw  — send any WebSocket message to the browser
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct RawReq {
    /// When true, send the message and return immediately without waiting for
    /// a response.  Use this for actions that don't send a correlated reply
    /// (e.g. bannerMessage, runSlashCommand).  Defaults to false.
    #[serde(default)]
    pub fire_and_forget: bool,
    #[serde(flatten)]
    pub payload: Value,
}

pub async fn handler(
    State(s): State<AppState>,
    Json(r): Json<RawReq>,
) -> Result<Json<Value>, AppError> {
    if r.payload.get("action").and_then(|v| v.as_str()).is_none() {
        return Err(AppError::BadRequest(
            "payload must contain an 'action' field".into(),
        ));
    }

    if r.fire_and_forget {
        s.fire(r.payload).await?;
        return Ok(Json(json!({ "sent": true })));
    }

    let resp = s.call(r.payload).await?;
    Ok(Json(json!({ "response": resp })))
}
