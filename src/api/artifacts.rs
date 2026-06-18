use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::state::{AppError, AppState};
use crate::ws_protocol::WsCommand;

// ---------------------------------------------------------------------------
// POST /artifacts  — create a SN development artifact via createRecord
//
// Unlike POST /records/:table (which uses agentRestApi and works for any
// table), this action is specifically for development artifacts — Script
// Includes, Business Rules, UI Scripts, etc.  createRecord adds the new
// record to the active update set and opens it in the browser editor.
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateArtifactBody {
    #[allow(dead_code)] pub instance: String,
    pub table: String,
    #[serde(default = "default_scope")]
    pub scope: String,
    /// Field values; `name` is required (all SN artifact tables have a name field)
    pub fields: Map<String, Value>,
}

fn default_scope() -> String {
    "global".to_string()
}

pub async fn create(
    State(s): State<AppState>,
    Json(r): Json<CreateArtifactBody>,
) -> Result<Json<Value>, AppError> {
    if r.table.trim().is_empty() {
        return Err(AppError::BadRequest("table cannot be empty".into()));
    }
    if !r.fields.contains_key("name") {
        return Err(AppError::BadRequest("fields.name is required".into()));
    }

    let instance = s.get_sn_instance().await?;
    let resp = s.call(WsCommand::CreateArtifact {
        instance,
        table_name: r.table.clone(),
        scope:      r.scope,
        payload:    r.fields,
    }).await?;

    if resp.get("success").and_then(|v| v.as_bool()) == Some(false) {
        let msg = resp["error"]
            .as_str()
            .unwrap_or("createRecord failed")
            .to_string();
        return Err(AppError::Remote(msg));
    }

    let rec = resp.get("newRecord").cloned().unwrap_or(json!({}));
    Ok(Json(json!({
        "sys_id": rec["sys_id"],
        "name":   rec["name"],
        "table":  rec.get("tableName").cloned().unwrap_or(json!(r.table)),
        "scope":  rec["scope"],
        "url":    rec["url"],
    })))
}
