use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::{check_rest_response, AppError, AppState};
use crate::ws_protocol::WsCommand;

// ---------------------------------------------------------------------------
// POST /rest  — proxy any ServiceNow REST call through the browser session
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct RestReq {
    #[allow(dead_code)] pub instance: String,
    /// HTTP method: GET POST PUT PATCH DELETE
    #[serde(default = "default_get")]
    pub method: String,
    /// ServiceNow API path, e.g. "/api/now/table/incident"
    pub endpoint: String,
    /// Request body (POST/PUT/PATCH)
    pub body: Option<Value>,
    /// Query parameters as a JSON object, e.g. {"sysparm_limit": "10"}
    pub query_params: Option<Value>,
}

fn default_get() -> String {
    "GET".to_string()
}

/// The browser executes the request with its authenticated session cookies —
/// no tokens or credentials needed.  Requires SN Utils Pro extension.
pub async fn handler(
    State(s): State<AppState>,
    Json(r): Json<RestReq>,
) -> Result<Json<Value>, AppError> {
    if r.endpoint.trim().is_empty() {
        return Err(AppError::BadRequest("endpoint cannot be empty".into()));
    }

    let instance = s.get_sn_instance().await?;
    let resp = s.call(WsCommand::RestApi {
        instance,
        method:      r.method.to_uppercase(),
        endpoint:    r.endpoint,
        body:        r.body,
        query_params: r.query_params,
    }).await?;

    check_rest_response(&resp)?;

    Ok(Json(json!({
        "status": resp["status"],
        "data":   resp["data"],
    })))
}
