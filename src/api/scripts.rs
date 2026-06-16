use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::{AppError, AppState};

// ---------------------------------------------------------------------------
// POST /scripts/bg  — run a server-side Glide script, block for output
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct BgReq {
    pub instance: String,
    /// Full server-side JavaScript to execute (gs.*, GlideRecord, etc.)
    pub script: String,
}

/// Executes via `agentRunBackgroundScript` — a fully correlated call that
/// returns the captured script output synchronously.  Unlike the old fire-and-
/// forget `/bg`, this blocks until ServiceNow finishes running the script.
pub async fn bg(
    State(s): State<AppState>,
    Json(r): Json<BgReq>,
) -> Result<Json<Value>, AppError> {
    if r.script.trim().is_empty() {
        return Err(AppError::BadRequest("script cannot be empty".into()));
    }

    let resp = s
        .call(json!({
            "action": "agentRunBackgroundScript",
            "instance": r.instance,
            "script": r.script,
            "appName": "snproxy",
        }))
        .await?;

    if resp.get("success").and_then(|v| v.as_bool()) == Some(false) {
        let msg = resp["error"].as_str().unwrap_or("script failed").to_string();
        return Err(AppError::Remote(msg));
    }

    Ok(Json(json!({
        "executed": true,
        "output": resp.get("output").and_then(|o| o.as_str()).unwrap_or(""),
    })))
}

// ---------------------------------------------------------------------------
// POST /scripts/slash  — run an SN Utils slash command
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SlashReq {
    pub instance: String,
    /// Full slash command including the leading slash, e.g. "/token" or "/tn"
    pub command: String,
    /// URL pattern to match the target tab (default: any SN tab)
    pub url: Option<String>,
    pub tab_id: Option<Value>,
    #[serde(default = "default_true")]
    pub auto_run: bool,
}

fn default_true() -> bool {
    true
}

pub async fn slash(
    State(s): State<AppState>,
    Json(r): Json<SlashReq>,
) -> Result<Json<Value>, AppError> {
    if r.command.trim().is_empty() {
        return Err(AppError::BadRequest("command cannot be empty".into()));
    }

    let mut payload = json!({
        "action": "runSlashCommand",
        "instance": r.instance,
        "command": r.command,
        "autoRun": r.auto_run,
    });
    if let Some(url) = r.url {
        payload["url"] = json!(url);
    }
    if let Some(tab_id) = r.tab_id {
        payload["tabId"] = tab_id;
    }

    let resp = s.call(payload).await?;

    Ok(Json(json!({
        "executed":  true,
        "command":   resp["command"],
        "tab_id":    resp["tabId"],
        "auto_run":  resp["autoRun"],
    })))
}
