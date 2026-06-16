use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::state::{AppError, AppState};

// ---------------------------------------------------------------------------
// POST /artifacts  — create a SN record and open it in the browser
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateArtifactBody {
    pub instance: String,
    pub table: String,
    #[serde(default = "default_scope")]
    pub scope: String,
    /// Field values to set on the new record
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

    let resp = s
        .call(json!({
            "action":    "createRecord",
            "instance":  r.instance,
            "tableName": r.table,
            "scope":     r.scope,
            "payload":   r.fields,
        }))
        .await?;

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

// ---------------------------------------------------------------------------
// GET /artifacts/metadata  — inspect a table's field schema
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct MetadataParams {
    pub instance: String,
    pub table: String,
}

pub async fn metadata(
    State(s): State<AppState>,
    Query(p): Query<MetadataParams>,
) -> Result<Json<Value>, AppError> {
    if p.table.trim().is_empty() {
        return Err(AppError::BadRequest("table cannot be empty".into()));
    }

    let resp = s
        .call(json!({
            "action":    "requestTableStructure",
            "instance":  p.instance,
            "tableName": p.table,
        }))
        .await?;

    Ok(Json(json!({
        "table":  p.table,
        "fields": resp.get("fields").cloned().unwrap_or(json!([])),
    })))
}
