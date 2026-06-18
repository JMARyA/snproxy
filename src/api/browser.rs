use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::{AppError, AppState};
use crate::ws_protocol::WsCommand;

fn default_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// GET /browser/form  — read live form state from the active SN tab
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct FormStateParams {
    #[allow(dead_code)] pub instance: String,
    /// URL pattern to target a specific tab
    pub url: Option<String>,
    pub tab_id: Option<String>,
    /// Comma-separated field names to include; omit for all
    pub fields: Option<String>,
}

pub async fn form_state(
    State(s): State<AppState>,
    Query(p): Query<FormStateParams>,
) -> Result<Json<Value>, AppError> {
    let fields = p.fields.map(|f| f.split(',').map(|s| s.trim().to_string()).collect());
    let resp = s.call(WsCommand::FormState {
        url:    p.url,
        tab_id: p.tab_id.map(Value::String),
        fields,
    }).await?;

    Ok(Json(json!({
        "table":         resp["table"],
        "sys_id":        resp["sysId"],
        "is_new_record": resp["isNewRecord"],
        "fields":        resp.get("fields").cloned().unwrap_or(json!({})),
    })))
}

// ---------------------------------------------------------------------------
// POST /browser/form  — set a field value via g_form (fires client scripts)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SetFieldReq {
    #[allow(dead_code)] pub instance: String,
    pub field: String,
    pub value: Value,
    /// Display value for reference fields
    pub display_value: Option<Value>,
    pub url: Option<String>,
    pub tab_id: Option<Value>,
}

pub async fn set_field(
    State(s): State<AppState>,
    Json(r): Json<SetFieldReq>,
) -> Result<Json<Value>, AppError> {
    if r.field.trim().is_empty() {
        return Err(AppError::BadRequest("field cannot be empty".into()));
    }

    let resp = s.call(WsCommand::SetField {
        field:         r.field,
        value:         r.value,
        display_value: r.display_value,
        url:           r.url,
        tab_id:        r.tab_id,
    }).await?;

    if resp.get("success").and_then(|v| v.as_bool()) == Some(false) {
        let msg = resp["error"].as_str().unwrap_or("set_field failed").to_string();
        return Err(AppError::Remote(msg));
    }

    Ok(Json(json!({
        "set":   true,
        "field": resp["field"],
        "value": resp["value"],
    })))
}

// ---------------------------------------------------------------------------
// POST /browser/form/action  — trigger a UI action (save/submit/custom verb)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct UiActionReq {
    #[allow(dead_code)] pub instance: String,
    /// "save" | "submit" | "sysverb_*" | any UI action name
    pub ui_action: String,
    #[serde(default = "default_true")]
    pub suppress_dialogs: bool,
    pub url: Option<String>,
    pub tab_id: Option<Value>,
}

pub async fn ui_action(
    State(s): State<AppState>,
    Json(r): Json<UiActionReq>,
) -> Result<Json<Value>, AppError> {
    if r.ui_action.trim().is_empty() {
        return Err(AppError::BadRequest("ui_action cannot be empty".into()));
    }

    let resp = s.call(WsCommand::UiAction {
        ui_action:        r.ui_action,
        suppress_dialogs: r.suppress_dialogs,
        url:              r.url,
        tab_id:           r.tab_id,
    }).await?;

    if resp.get("success").and_then(|v| v.as_bool()) == Some(false) {
        let msg = resp["error"].as_str().unwrap_or("ui_action failed").to_string();
        return Err(AppError::Remote(msg));
    }

    Ok(Json(json!({
        "triggered": true,
        "ui_action": resp["uiAction"],
    })))
}

// ---------------------------------------------------------------------------
// POST /browser/navigate  — navigate a tab to a URL
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct NavigateReq {
    #[allow(dead_code)] pub instance: String,
    pub url: String,
    pub tab_id: Option<Value>,
    #[serde(default)]
    pub new_tab: bool,
    #[serde(default = "default_true")]
    pub wait_for_load: bool,
    #[serde(default)]
    pub discard_unsaved: bool,
}

pub async fn navigate(
    State(s): State<AppState>,
    Json(r): Json<NavigateReq>,
) -> Result<Json<Value>, AppError> {
    if r.url.trim().is_empty() {
        return Err(AppError::BadRequest("url cannot be empty".into()));
    }

    let resp = s.call(WsCommand::Navigate {
        url:             r.url,
        new_tab:         r.new_tab,
        wait_for_load:   r.wait_for_load,
        discard_unsaved: r.discard_unsaved,
        tab_id:          r.tab_id,
    }).await?;

    Ok(Json(json!({
        "navigated": true,
        "tab_id":    resp["tabId"],
        "url":       resp["url"],
        "title":     resp["title"],
    })))
}

// ---------------------------------------------------------------------------
// POST /browser/click  — click a DOM element by CSS selector
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ClickReq {
    #[allow(dead_code)] pub instance: String,
    pub selector: String,
    #[serde(default = "default_true")]
    pub suppress_dialogs: bool,
    pub url: Option<String>,
    pub tab_id: Option<Value>,
}

pub async fn click(
    State(s): State<AppState>,
    Json(r): Json<ClickReq>,
) -> Result<Json<Value>, AppError> {
    if r.selector.trim().is_empty() {
        return Err(AppError::BadRequest("selector cannot be empty".into()));
    }

    let resp = s.call(WsCommand::ClickElement {
        selector:         r.selector,
        suppress_dialogs: r.suppress_dialogs,
        url:              r.url,
        tab_id:           r.tab_id,
    }).await?;

    if resp.get("success").and_then(|v| v.as_bool()) == Some(false) {
        let msg = resp["error"].as_str().unwrap_or("click failed").to_string();
        return Err(AppError::Remote(msg));
    }

    Ok(Json(json!({
        "clicked":  true,
        "selector": resp["selector"],
    })))
}

// ---------------------------------------------------------------------------
// POST /browser/screenshot  — capture a tab as a PNG (base64)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ScreenshotReq {
    #[allow(dead_code)] pub instance: String,
    /// URL to match / navigate to before capturing
    pub url: Option<String>,
    pub tab_id: Option<Value>,
    #[serde(default)]
    pub exact_url: bool,
    pub file_name: Option<String>,
}

pub async fn screenshot(
    State(s): State<AppState>,
    Json(r): Json<ScreenshotReq>,
) -> Result<Json<Value>, AppError> {
    if r.url.is_none() && r.tab_id.is_none() {
        return Err(AppError::BadRequest("url or tab_id is required".into()));
    }

    let resp = s.call(WsCommand::Screenshot {
        url:       r.url,
        tab_id:    r.tab_id,
        exact_url: r.exact_url,
        file_name: r.file_name,
    }).await?;

    if resp.get("success").and_then(|v| v.as_bool()) == Some(false) {
        let msg = resp["error"].as_str().unwrap_or("screenshot failed").to_string();
        return Err(AppError::Remote(msg));
    }

    Ok(Json(json!({
        "image_data": resp["imageData"],
        "url":        resp["url"],
        "tab_id":     resp["tabId"],
        "tab_title":  resp["tabTitle"],
    })))
}

// ---------------------------------------------------------------------------
// POST /browser/tab  — activate or open a browser tab
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct TabReq {
    #[allow(dead_code)] pub instance: String,
    pub url: String,
    #[serde(default)]
    pub reload: bool,
    #[serde(default)]
    pub wait_for_load: bool,
    #[serde(default = "default_true")]
    pub open_if_not_found: bool,
}

pub async fn tab(
    State(s): State<AppState>,
    Json(r): Json<TabReq>,
) -> Result<Json<Value>, AppError> {
    if r.url.trim().is_empty() {
        return Err(AppError::BadRequest("url cannot be empty".into()));
    }

    let resp = s.call(WsCommand::ActivateTab {
        url:             r.url,
        reload:          r.reload,
        wait_for_load:   r.wait_for_load,
        open_if_not_found: r.open_if_not_found,
    }).await?;

    Ok(Json(json!({
        "tab_id":   resp["tabId"],
        "url":      resp["url"],
        "title":    resp["title"],
        "opened":   resp["opened"],
        "reloaded": resp["reloaded"],
    })))
}
