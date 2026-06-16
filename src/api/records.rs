use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::state::{check_rest_response, AppError, AppState};

// ---------------------------------------------------------------------------
// GET /records/:table
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ListParams {
    pub instance: String,
    /// ServiceNow encoded query, e.g. "active=true^category=software"
    #[serde(default)]
    pub q: String,
    /// Comma-separated field list
    #[serde(default = "default_fields")]
    pub fields: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Appended to the query as an ORDER BY clause, e.g. "ORDERBYname"
    #[serde(default)]
    pub order_by: String,
}

fn default_fields() -> String {
    "sys_id,name,sys_created_on,sys_updated_on".to_string()
}
fn default_limit() -> u32 {
    20
}

pub async fn list(
    State(s): State<AppState>,
    Path(table): Path<String>,
    Query(p): Query<ListParams>,
) -> Result<Json<Value>, AppError> {
    // Build the sysparm_query value (encoded query + optional ORDER BY)
    let sn_query = match (p.q.is_empty(), p.order_by.is_empty()) {
        (true, true) => String::new(),
        (false, true) => p.q.clone(),
        (true, false) => p.order_by.clone(),
        (false, false) => format!("{}^{}", p.q, p.order_by),
    };

    let mut query_string = format!("sysparm_fields={}&sysparm_limit={}", p.fields, p.limit);
    if !sn_query.is_empty() {
        query_string.push_str(&format!("&sysparm_query={sn_query}"));
    }

    let resp = s
        .call(json!({
            "action": "agentQueryRecords",
            "instance": p.instance,
            "tableName": table,
            "queryString": query_string,
        }))
        .await?;

    let records = resp.get("records").cloned().unwrap_or(json!([]));
    let count = resp
        .get("count")
        .and_then(|c| c.as_u64())
        .unwrap_or_else(|| records.as_array().map(|a| a.len() as u64).unwrap_or(0));

    Ok(Json(json!({
        "table": table,
        "count": count,
        "records": records,
    })))
}

// ---------------------------------------------------------------------------
// GET /records/:table/:sys_id
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct GetParams {
    pub instance: String,
    /// Comma-separated field list; omit for all fields
    #[serde(default)]
    pub fields: String,
}

pub async fn get(
    State(s): State<AppState>,
    Path((table, sys_id)): Path<(String, String)>,
    Query(p): Query<GetParams>,
) -> Result<Json<Value>, AppError> {
    let mut query_params = json!({"sysparm_display_value": "false"});
    if !p.fields.is_empty() {
        query_params["sysparm_fields"] = json!(p.fields);
    }

    let resp = s
        .call(json!({
            "action": "agentRestApi",
            "instance": p.instance,
            "method": "GET",
            "endpoint": format!("/api/now/table/{table}/{sys_id}"),
            "queryParams": query_params,
            "appName": "snproxy",
        }))
        .await?;

    check_rest_response(&resp)?;

    Ok(Json(json!({
        "table": table,
        "sys_id": sys_id,
        "record": resp["data"]["result"].clone(),
    })))
}

// ---------------------------------------------------------------------------
// POST /records/:table  — create via agentRestApi (works for any table)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateBody {
    pub instance: String,
    pub fields: Map<String, Value>,
}

pub async fn create(
    State(s): State<AppState>,
    Path(table): Path<String>,
    Json(body): Json<CreateBody>,
) -> Result<Json<Value>, AppError> {
    if body.fields.is_empty() {
        return Err(AppError::BadRequest("fields cannot be empty".into()));
    }

    let resp = s
        .call(json!({
            "action":   "agentRestApi",
            "instance": body.instance,
            "method":   "POST",
            "endpoint": format!("/api/now/table/{table}"),
            "body":     body.fields,
            "appName":  "snproxy",
        }))
        .await?;

    check_rest_response(&resp)?;

    let result = resp["data"]["result"].clone();
    Ok(Json(json!({
        "sys_id": result["sys_id"],
        "table":  table,
        "record": result,
    })))
}

// ---------------------------------------------------------------------------
// PATCH /records/:table/:sys_id  — update fields via agentRestApi PATCH
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct UpdateBody {
    pub instance: String,
    pub fields: Map<String, Value>,
}

pub async fn update(
    State(s): State<AppState>,
    Path((table, sys_id)): Path<(String, String)>,
    Json(body): Json<UpdateBody>,
) -> Result<Json<Value>, AppError> {
    if body.fields.is_empty() {
        return Err(AppError::BadRequest("fields cannot be empty".into()));
    }

    let resp = s
        .call(json!({
            "action": "agentRestApi",
            "instance": body.instance,
            "method": "PATCH",
            "endpoint": format!("/api/now/table/{table}/{sys_id}"),
            "body": body.fields,
            "appName": "snproxy",
        }))
        .await?;

    check_rest_response(&resp)?;

    Ok(Json(json!({
        "table":   table,
        "sys_id":  sys_id,
        "updated": true,
        "record":  resp["data"]["result"].clone(),
    })))
}

// ---------------------------------------------------------------------------
// DELETE /records/:table/:sys_id
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct DeleteParams {
    pub instance: String,
}

pub async fn delete(
    State(s): State<AppState>,
    Path((table, sys_id)): Path<(String, String)>,
    Query(p): Query<DeleteParams>,
) -> Result<Json<Value>, AppError> {
    let resp = s
        .call(json!({
            "action": "agentRestApi",
            "instance": p.instance,
            "method": "DELETE",
            "endpoint": format!("/api/now/table/{table}/{sys_id}"),
            "appName": "snproxy",
        }))
        .await?;

    check_rest_response(&resp)?;

    Ok(Json(json!({
        "table":   table,
        "sys_id":  sys_id,
        "deleted": true,
    })))
}

// ---------------------------------------------------------------------------
// GET /records/:table/schema  — inspect a table's field metadata
// ---------------------------------------------------------------------------

pub async fn schema(
    State(s): State<AppState>,
    Path(table): Path<String>,
    Query(p): Query<GetParams>,
) -> Result<Json<Value>, AppError> {
    let resp = s
        .call(json!({
            "action":    "requestTableStructure",
            "instance":  p.instance,
            "tableName": table,
        }))
        .await?;

    Ok(Json(json!({
        "table":  table,
        "fields": resp.get("fields").cloned().unwrap_or(json!([])),
    })))
}
