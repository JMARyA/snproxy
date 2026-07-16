use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::{AppError, AppState};
use crate::ws_protocol::WsCommand;

// ---------------------------------------------------------------------------
// POST /scripts/bg  — run a server-side Glide script, block for output
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct BgReq {
    pub instance: String,
    /// Full server-side JavaScript to execute (gs.*, GlideRecord, etc.)
    pub script: String,
}

pub async fn bg(
    State(s): State<AppState>,
    Json(r): Json<BgReq>,
) -> Result<Json<Value>, AppError> {
    if r.script.trim().is_empty() {
        return Err(AppError::BadRequest("script cannot be empty".into()));
    }

    let instance = s.check_instance(&r.instance).await?;
    let resp = s.call(WsCommand::BackgroundScript {
        instance,
        script: r.script,
    }).await?;

    if resp.get("success").and_then(|v| v.as_bool()) == Some(false) {
        let msg = resp["error"].as_str().unwrap_or("script failed").to_string();
        return Err(AppError::Remote(msg));
    }

    let raw = resp.get("output").and_then(|o| o.as_str()).unwrap_or("");

    // Extract content from <PRE> block, split on <BR/>, strip prefixes
    let lines: Vec<String> = raw
        .split("<PRE>")
        .nth(1)
        .unwrap_or("")
        .split("</PRE>")
        .next()
        .unwrap_or("")
        .split("<BR/>")
        .flat_map(|s| s.split("<br/>"))
        .map(|line| {
            let s = line
                .strip_prefix("*** Script: ")
                .unwrap_or(line)
                .trim();
            s.replace("&quot;", "\"")
                .replace("&#39;", "'")
                .replace("&amp;", "&")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
        })
        .filter(|s| !s.is_empty())
        .collect();

    let output = lines.join("\n");

    Ok(Json(json!({
        "executed": true,
        "output":   output,
        "lines":    lines,
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

    s.check_instance(&r.instance).await?;
    let resp = s.call(WsCommand::SlashCommand {
        command:  r.command,
        auto_run: r.auto_run,
        url:      r.url,
        tab_id:   r.tab_id,
    }).await?;

    Ok(Json(json!({
        "executed": true,
        "command":  resp["command"],
        "tab_id":   resp["tabId"],
        "auto_run": resp["autoRun"],
    })))
}
