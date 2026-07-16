use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::{AppError, AppState};
use crate::ws_protocol::WsCommand;

// ---------------------------------------------------------------------------
// PUT /context  — switch update set, application scope, or domain
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SwitchReq {
    pub instance: String,
    /// "updateset" | "application" | "domain"
    #[serde(rename = "type")]
    pub switch_type: String,
    /// sys_id or name of the target update set / scope / domain
    pub value: String,
    #[serde(default = "default_true")]
    pub reload_tab: bool,
}

fn default_true() -> bool {
    true
}

pub async fn switch(
    State(s): State<AppState>,
    Json(r): Json<SwitchReq>,
) -> Result<Json<Value>, AppError> {
    const VALID: &[&str] = &["updateset", "application", "domain"];
    let switch_type = r.switch_type.to_lowercase();
    if !VALID.contains(&switch_type.as_str()) {
        return Err(AppError::BadRequest(format!(
            "type must be one of: {}",
            VALID.join(", ")
        )));
    }
    if r.value.trim().is_empty() {
        return Err(AppError::BadRequest("value cannot be empty".into()));
    }

    let instance = s.check_instance(&r.instance).await?;
    let resp = s.call(WsCommand::SwitchContext {
        instance,
        switch_type,
        value:      r.value,
        reload_tab: r.reload_tab,
    }).await?;

    Ok(Json(json!({
        "switched": true,
        "type":     resp["switchType"],
        "value":    resp["value"],
        "reloaded": resp["reloaded"],
    })))
}
