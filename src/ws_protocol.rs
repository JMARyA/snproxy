/// Typed WebSocket commands sent to the SN Utils Helper Tab.
///
/// Serde's `tag = "action"` serializes each variant with the `"action"` key
/// that scriptsync.js dispatches on.  Fields use camelCase (the extension's
/// convention) via per-field `rename` attributes.
///
/// `agentRequestId` and `appName` are injected by `AppState::call()` — they
/// must not appear in these variants.
use serde::Serialize;
use serde_json::{Map, Value};

#[derive(Serialize)]
#[serde(tag = "action")]
pub enum WsCommand {
    // ── Records ───────────────────────────────────────────────────────────

    /// List / query records (free tier — agentQueryRecords).
    #[serde(rename = "agentQueryRecords")]
    QueryRecords {
        instance: Value,
        #[serde(rename = "tableName")]
        table_name: String,
        /// Raw sysparm_* query string, e.g. "sysparm_fields=...&sysparm_limit=20"
        #[serde(rename = "queryString")]
        query_string: String,
    },

    /// Fetch table field metadata from /api/now/ui/meta/:table (free tier).
    /// Response: { result: { fields: { field_name: { label, type, ... } } } }
    #[serde(rename = "requestTableStructure")]
    TableStructure {
        instance: Value,
        #[serde(rename = "tableName")]
        table_name: String,
    },

    // ── Generic REST passthrough (Pro) ────────────────────────────────────

    /// Call any ServiceNow REST endpoint through the browser session.
    /// Response: { success, status, data }
    #[serde(rename = "agentRestApi")]
    RestApi {
        instance: Value,
        method: String,
        endpoint: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        body: Option<Value>,
        /// Object of query parameters, e.g. {"sysparm_limit": "10"}
        #[serde(rename = "queryParams", skip_serializing_if = "Option::is_none")]
        query_params: Option<Value>,
    },

    // ── Scripts ───────────────────────────────────────────────────────────

    /// Execute a server-side Glide script via /sys.scripts.do.
    /// Response: { success, output } — output is raw HTML from SN.
    #[serde(rename = "agentRunBackgroundScript")]
    BackgroundScript {
        instance: Value,
        script: String,
    },

    /// Run an SN Utils slash command on the active ServiceNow tab.
    /// The extension locates the tab by `url` pattern; `instance` is not used.
    #[serde(rename = "runSlashCommand")]
    SlashCommand {
        command: String,
        #[serde(rename = "autoRun")]
        auto_run: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(rename = "tabId", skip_serializing_if = "Option::is_none")]
        tab_id: Option<Value>,
    },

    // ── Browser — form ────────────────────────────────────────────────────

    /// Read the live g_form state from the active SN tab.
    #[serde(rename = "agentGetFormState")]
    FormState {
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(rename = "tabId", skip_serializing_if = "Option::is_none")]
        tab_id: Option<Value>,
        /// Subset of field names to return; omit for all fields.
        #[serde(skip_serializing_if = "Option::is_none")]
        fields: Option<Vec<String>>,
    },

    /// Set a field value via g_form.setValue (fires client scripts).
    #[serde(rename = "agentSetField")]
    SetField {
        field: String,
        value: Value,
        /// Display value for reference fields (e.g. user name alongside sys_id).
        #[serde(rename = "displayValue", skip_serializing_if = "Option::is_none")]
        display_value: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(rename = "tabId", skip_serializing_if = "Option::is_none")]
        tab_id: Option<Value>,
    },

    /// Trigger a UI action button: "save", "submit", or any sysverb_* name.
    #[serde(rename = "agentRunUiAction")]
    UiAction {
        #[serde(rename = "uiAction")]
        ui_action: String,
        #[serde(rename = "suppressDialogs")]
        suppress_dialogs: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(rename = "tabId", skip_serializing_if = "Option::is_none")]
        tab_id: Option<Value>,
    },

    // ── Browser — navigation ──────────────────────────────────────────────

    /// Navigate a browser tab to `url`, optionally waiting for page load.
    #[serde(rename = "agentNavigate")]
    Navigate {
        url: String,
        #[serde(rename = "newTab")]
        new_tab: bool,
        #[serde(rename = "waitForLoad")]
        wait_for_load: bool,
        /// Navigate away even when the form has unsaved changes.
        #[serde(rename = "discardUnsaved")]
        discard_unsaved: bool,
        #[serde(rename = "tabId", skip_serializing_if = "Option::is_none")]
        tab_id: Option<Value>,
    },

    /// Click a DOM element by CSS selector.
    #[serde(rename = "agentClickElement")]
    ClickElement {
        selector: String,
        #[serde(rename = "suppressDialogs")]
        suppress_dialogs: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(rename = "tabId", skip_serializing_if = "Option::is_none")]
        tab_id: Option<Value>,
    },

    /// Capture a browser tab as a PNG (base64 imageData in response).
    #[serde(rename = "takeScreenshot")]
    Screenshot {
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(rename = "tabId", skip_serializing_if = "Option::is_none")]
        tab_id: Option<Value>,
        /// Match `url` exactly; when false the extension does a prefix/substring match.
        #[serde(rename = "exactUrl")]
        exact_url: bool,
        #[serde(rename = "fileName", skip_serializing_if = "Option::is_none")]
        file_name: Option<String>,
    },

    /// Bring a browser tab to the foreground, optionally reloading it.
    #[serde(rename = "activateTab")]
    ActivateTab {
        url: String,
        reload: bool,
        #[serde(rename = "waitForLoad")]
        wait_for_load: bool,
        /// Open a new tab with `url` if none matching is found.
        #[serde(rename = "openIfNotFound")]
        open_if_not_found: bool,
    },

    // ── Context switching ─────────────────────────────────────────────────

    /// Switch the active update set, application scope, or domain.
    /// Uses PUT /api/now/ui/concoursepicker/:switchType.
    #[serde(rename = "switchContext")]
    SwitchContext {
        instance: Value,
        /// One of: "updateset" | "application" | "domain"
        #[serde(rename = "switchType")]
        switch_type: String,
        value: String,
        /// Reload the active SN tab after switching.
        #[serde(rename = "reloadTab")]
        reload_tab: bool,
    },

    // ── Development artifacts ─────────────────────────────────────────────

    /// Create a development artifact, add it to the active update set, and
    /// open it in the browser editor.  Different from `RestApi` POST because
    /// the extension handles update-set tracking and browser navigation.
    #[serde(rename = "createRecord")]
    CreateArtifact {
        instance: Value,
        #[serde(rename = "tableName")]
        table_name: String,
        scope: String,
        payload: Map<String, Value>,
    },
}
