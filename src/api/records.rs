use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::state::{check_rest_response, AppError, AppState};
use crate::ws_protocol::WsCommand;

// ---------------------------------------------------------------------------
// GET /records/:table
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ListParams {
    #[allow(dead_code)] pub instance: String,
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
    let sn_query = match (p.q.is_empty(), p.order_by.is_empty()) {
        (true, true)   => String::new(),
        (false, true)  => p.q.clone(),
        (true, false)  => p.order_by.clone(),
        (false, false) => format!("{}^{}", p.q, p.order_by),
    };

    let mut query_string = format!("sysparm_fields={}&sysparm_limit={}", p.fields, p.limit);
    if !sn_query.is_empty() {
        query_string.push_str(&format!("&sysparm_query={sn_query}"));
    }

    let instance = s.get_sn_instance().await?;
    let resp = s.call(WsCommand::QueryRecords {
        instance,
        table_name: table.clone(),
        query_string,
    }).await?;

    let records = resp.get("records").cloned().unwrap_or(json!([]));
    let count = resp
        .get("count")
        .and_then(|c| c.as_u64())
        .unwrap_or_else(|| records.as_array().map(|a| a.len() as u64).unwrap_or(0));

    Ok(Json(json!({
        "table":   table,
        "count":   count,
        "records": records,
    })))
}

// ---------------------------------------------------------------------------
// GET /records/:table/:sys_id
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct GetParams {
    #[allow(dead_code)] pub instance: String,
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

    let instance = s.get_sn_instance().await?;
    let resp = s.call(WsCommand::RestApi {
        instance,
        method: "GET".into(),
        endpoint: format!("/api/now/table/{table}/{sys_id}"),
        body: None,
        query_params: Some(query_params),
    }).await?;

    check_rest_response(&resp)?;

    Ok(Json(json!({
        "table":  table,
        "sys_id": sys_id,
        "record": resp["data"]["result"].clone(),
    })))
}

// ---------------------------------------------------------------------------
// POST /records/:table
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateBody {
    #[allow(dead_code)] pub instance: String,
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

    let instance = s.get_sn_instance().await?;
    let resp = s.call(WsCommand::RestApi {
        instance,
        method: "POST".into(),
        endpoint: format!("/api/now/table/{table}"),
        body: Some(Value::Object(body.fields)),
        query_params: None,
    }).await?;

    check_rest_response(&resp)?;

    let result = resp["data"]["result"].clone();
    Ok(Json(json!({
        "sys_id": result["sys_id"],
        "table":  table,
        "record": result,
    })))
}

// ---------------------------------------------------------------------------
// PATCH /records/:table/:sys_id
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct UpdateBody {
    #[allow(dead_code)] pub instance: String,
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

    let instance = s.get_sn_instance().await?;
    let resp = s.call(WsCommand::RestApi {
        instance,
        method: "PATCH".into(),
        endpoint: format!("/api/now/table/{table}/{sys_id}"),
        body: Some(Value::Object(body.fields)),
        query_params: None,
    }).await?;

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
    #[allow(dead_code)] pub instance: String,
}

pub async fn delete(
    State(s): State<AppState>,
    Path((table, sys_id)): Path<(String, String)>,
    Query(_p): Query<DeleteParams>,
) -> Result<Json<Value>, AppError> {
    let instance = s.get_sn_instance().await?;
    let resp = s.call(WsCommand::RestApi {
        instance,
        method: "DELETE".into(),
        endpoint: format!("/api/now/table/{table}/{sys_id}"),
        body: None,
        query_params: None,
    }).await?;

    check_rest_response(&resp)?;

    Ok(Json(json!({
        "table":   table,
        "sys_id":  sys_id,
        "deleted": true,
    })))
}

// ---------------------------------------------------------------------------
// GET /records/:table/schema
// ---------------------------------------------------------------------------

pub async fn schema(
    State(s): State<AppState>,
    Path(table): Path<String>,
    Query(_p): Query<GetParams>,
) -> Result<Json<Value>, AppError> {
    let instance = s.get_sn_instance().await?;
    let resp = s.call(WsCommand::TableStructure {
        instance,
        table_name: table.clone(),
    }).await?;

    // The extension fetches /api/now/ui/meta/:table and wraps the response in
    // resp.result.  Fields are an object keyed by field name, not an array.
    let fields = resp
        .get("result")
        .and_then(|r| r.get("fields"))
        .cloned()
        .unwrap_or(json!({}));

    Ok(Json(json!({
        "table":  table,
        "fields": fields,
    })))
}
