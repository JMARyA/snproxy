use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use reqwest::{Client, Method};
use serde_json::{json, Map, Value};
use std::path::PathBuf;

// ── Top-level CLI ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "sncli",
    about = "CLI client for snproxy — interact with ServiceNow via SN Utils"
)]
struct Cli {
    /// snproxy server base URL
    #[arg(long, default_value = "http://localhost:8766", env = "SNPROXY_URL")]
    server: String,
    /// Print compact JSON instead of pretty-printed
    #[arg(long, short = 'r')]
    raw: bool,
    /// Log every request and raw response to stderr
    #[arg(long, short = 'v')]
    verbose: bool,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Check snproxy server health
    Health,
    /// CRUD + schema inspection for ServiceNow table records
    Records(RecordsArgs),
    /// Run server-side scripts or slash commands
    Scripts(ScriptsArgs),
    /// Proxy any ServiceNow REST API call via the browser session
    Rest(RestArgs),
    /// Browser automation: form, navigate, click, screenshot, tab
    Browser(BrowserArgs),
    /// Switch update set, application scope, or domain
    Context(ContextArgs),
    /// Create a dev artifact (adds to update set + opens in browser)
    Artifact(ArtifactArgs),
    /// Stream live WebSocket events from the Helper Tab (SSE; Ctrl-C to stop)
    Events,
    /// Send a raw JSON payload directly to the WebSocket bridge
    Raw(RawArgs),
}

// ── Records ──────────────────────────────────────────────────────────────────

#[derive(Args)]
struct RecordsArgs {
    #[command(subcommand)]
    action: RecordsCmd,
}

#[derive(Subcommand)]
enum RecordsCmd {
    /// List records from a table
    List(RecordsListArgs),
    /// Fetch a single record by sys_id
    Get(RecordsGetArgs),
    /// Create a new record
    Create(RecordsCreateArgs),
    /// Update an existing record
    Update(RecordsUpdateArgs),
    /// Delete a record
    Delete(RecordsDeleteArgs),
    /// Inspect a table's field metadata
    Schema(RecordsSchemaArgs),
}

#[derive(Args)]
struct RecordsListArgs {
    table: String,
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: String,
    /// ServiceNow encoded query, e.g. "active=true^category=software"
    #[arg(long, short = 'q')]
    query: Option<String>,
    /// Comma-separated field list
    #[arg(long, short = 'f')]
    fields: Option<String>,
    #[arg(long, short = 'l', default_value_t = 20)]
    limit: u32,
    /// ORDER BY clause, e.g. "ORDERBYname"
    #[arg(long)]
    order_by: Option<String>,
}

#[derive(Args)]
struct RecordsGetArgs {
    table: String,
    sys_id: String,
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: String,
    /// Comma-separated field list (omit for all)
    #[arg(long, short = 'f')]
    fields: Option<String>,
}

#[derive(Args)]
struct RecordsCreateArgs {
    table: String,
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: String,
    /// JSON object of field values, e.g. '{"short_description":"Test"}'
    #[arg(long, short = 'f')]
    fields: String,
}

#[derive(Args)]
struct RecordsUpdateArgs {
    table: String,
    sys_id: String,
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: String,
    /// JSON object of fields to update
    #[arg(long, short = 'f')]
    fields: String,
}

#[derive(Args)]
struct RecordsDeleteArgs {
    table: String,
    sys_id: String,
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: String,
}

#[derive(Args)]
struct RecordsSchemaArgs {
    table: String,
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: String,
}

// ── Scripts ───────────────────────────────────────────────────────────────────

#[derive(Args)]
struct ScriptsArgs {
    #[command(subcommand)]
    action: ScriptsCmd,
}

#[derive(Subcommand)]
enum ScriptsCmd {
    /// Run a server-side background script and capture its output
    Bg(ScriptsBgArgs),
    /// Execute an SN Utils slash command
    Slash(ScriptsSlashArgs),
}

#[derive(Args)]
struct ScriptsBgArgs {
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: String,
    /// JavaScript to execute inline
    #[arg(long, short = 's', conflicts_with = "file")]
    script: Option<String>,
    /// Read script from a .js file
    #[arg(long, conflicts_with = "script")]
    file: Option<PathBuf>,
}

#[derive(Args)]
struct ScriptsSlashArgs {
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: String,
    /// Slash command including the leading slash, e.g. "/token"
    #[arg(long, short = 'c')]
    command: String,
    /// URL pattern to target a specific tab
    #[arg(long)]
    url: Option<String>,
    #[arg(long)]
    tab_id: Option<String>,
    /// Disable auto-run (default: auto-run is on)
    #[arg(long)]
    no_auto_run: bool,
}

// ── REST ──────────────────────────────────────────────────────────────────────

#[derive(Args)]
struct RestArgs {
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: String,
    /// HTTP method (GET POST PUT PATCH DELETE)
    #[arg(long, short = 'm', default_value = "GET")]
    method: String,
    /// ServiceNow API path, e.g. "/api/now/table/incident"
    #[arg(long, short = 'e')]
    endpoint: String,
    /// Request body as a JSON string
    #[arg(long, short = 'b')]
    body: Option<String>,
    /// Query parameters as a JSON object, e.g. '{"sysparm_limit":"10"}'
    #[arg(long, short = 'p')]
    params: Option<String>,
}

// ── Browser ───────────────────────────────────────────────────────────────────

#[derive(Args)]
struct BrowserArgs {
    #[command(subcommand)]
    action: BrowserCmd,
}

#[derive(Subcommand)]
enum BrowserCmd {
    /// Read the live form state from the active SN tab
    Form(BrowserFormArgs),
    /// Set a field value on the current form (fires client scripts)
    SetField(BrowserSetFieldArgs),
    /// Trigger a UI action on the current form (save, submit, custom verb)
    Action(BrowserActionArgs),
    /// Navigate a browser tab to a URL
    Navigate(BrowserNavigateArgs),
    /// Click a DOM element by CSS selector
    Click(BrowserClickArgs),
    /// Capture a tab as a PNG (base64 image_data in response)
    Screenshot(BrowserScreenshotArgs),
    /// Activate or open a browser tab
    Tab(BrowserTabArgs),
}

#[derive(Args)]
struct BrowserFormArgs {
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: String,
    /// Target tab by URL pattern
    #[arg(long)]
    url: Option<String>,
    #[arg(long)]
    tab_id: Option<String>,
    /// Comma-separated list of fields to include (omit for all)
    #[arg(long, short = 'f')]
    fields: Option<String>,
}

#[derive(Args)]
struct BrowserSetFieldArgs {
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: String,
    /// Field name (e.g. "short_description")
    #[arg(long)]
    field: String,
    /// Value — parsed as JSON if valid, otherwise treated as a string
    #[arg(long)]
    value: String,
    /// Display value for reference fields
    #[arg(long)]
    display_value: Option<String>,
    #[arg(long)]
    url: Option<String>,
    #[arg(long)]
    tab_id: Option<String>,
}

#[derive(Args)]
struct BrowserActionArgs {
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: String,
    /// UI action name: "save", "submit", or "sysverb_*"
    #[arg(long, short = 'a')]
    action: String,
    #[arg(long)]
    url: Option<String>,
    #[arg(long)]
    tab_id: Option<String>,
    /// Allow browser dialogs during the action
    #[arg(long)]
    allow_dialogs: bool,
}

#[derive(Args)]
struct BrowserNavigateArgs {
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: String,
    /// URL to navigate to
    #[arg(long, short = 'u')]
    url: String,
    #[arg(long)]
    tab_id: Option<String>,
    /// Open in a new tab
    #[arg(long)]
    new_tab: bool,
    /// Return immediately without waiting for page load
    #[arg(long)]
    no_wait: bool,
    /// Navigate away even if the form has unsaved changes
    #[arg(long)]
    discard_unsaved: bool,
}

#[derive(Args)]
struct BrowserClickArgs {
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: String,
    /// CSS selector of the element to click
    #[arg(long, short = 's')]
    selector: String,
    #[arg(long)]
    url: Option<String>,
    #[arg(long)]
    tab_id: Option<String>,
    /// Allow browser dialogs triggered by the click
    #[arg(long)]
    allow_dialogs: bool,
}

#[derive(Args)]
struct BrowserScreenshotArgs {
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: String,
    /// URL to match/navigate to before capturing
    #[arg(long, short = 'u')]
    url: Option<String>,
    #[arg(long)]
    tab_id: Option<String>,
    /// Match the URL exactly (default: substring match)
    #[arg(long)]
    exact_url: bool,
    /// File name hint (default: screenshot.png)
    #[arg(long, short = 'o', default_value = "screenshot.png")]
    output: String,
}

#[derive(Args)]
struct BrowserTabArgs {
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: String,
    /// URL pattern to activate
    #[arg(long, short = 'u')]
    url: String,
    /// Reload the tab after activating
    #[arg(long)]
    reload: bool,
    /// Wait for the page to fully load
    #[arg(long)]
    wait: bool,
    /// Do not open a new tab if none matches the URL
    #[arg(long)]
    no_open: bool,
}

// ── Context ───────────────────────────────────────────────────────────────────

#[derive(Args)]
struct ContextArgs {
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: String,
    /// "updateset", "application", or "domain"
    #[arg(long, short = 't')]
    r#type: String,
    /// sys_id or name of the target update set / scope / domain
    #[arg(long, short = 'v')]
    value: String,
    /// Skip reloading the active tab after switching
    #[arg(long)]
    no_reload: bool,
}

// ── Artifact ──────────────────────────────────────────────────────────────────

#[derive(Args)]
struct ArtifactArgs {
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE")]
    instance: String,
    /// Table name, e.g. "sys_script_include"
    #[arg(long, short = 't')]
    table: String,
    /// Artifact name (required by all SN artifact tables)
    #[arg(long, short = 'n')]
    name: String,
    /// Application scope (default: global)
    #[arg(long, short = 's', default_value = "global")]
    scope: String,
    /// Additional field values as a JSON object
    #[arg(long, short = 'f')]
    fields: Option<String>,
}

// ── Raw ───────────────────────────────────────────────────────────────────────

#[derive(Args)]
struct RawArgs {
    /// JSON payload; must include an "action" field
    payload: String,
    /// Send and return without waiting for a correlated response
    #[arg(long)]
    fire_and_forget: bool,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn print_json(v: &Value, raw: bool) {
    if raw {
        println!("{}", v);
    } else {
        println!("{}", serde_json::to_string_pretty(v).unwrap());
    }
}

fn parse_fields(s: &str) -> Result<Map<String, Value>> {
    let v: Value = serde_json::from_str(s).context("--fields must be a valid JSON object")?;
    v.as_object()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("--fields must be a JSON object ({{...}})"))
}

fn parse_json(s: &str, flag: &str) -> Result<Value> {
    serde_json::from_str(s).with_context(|| format!("{flag} must be valid JSON"))
}

/// Coerce a CLI string to a JSON Value: try JSON parse first, else treat as string.
fn coerce_value(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.to_string()))
}

/// Normalise an instance identifier to a full https:// URL.
/// Returns bare hostname so the Helper Tab can prepend https:// itself.
/// "dev"                         → "dev.service-now.com"
/// "dev.service-now.com"         → "dev.service-now.com"
/// "https://dev.service-now.com" → "dev.service-now.com"
fn normalize_instance(s: &str) -> String {
    let s = s.trim_end_matches('/');
    let s = s.strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .unwrap_or(s);
    if s.contains('.') {
        s.to_string()
    } else {
        format!("{s}.service-now.com")
    }
}

/// Single HTTP helper — reads body as text first so non-JSON error
/// responses (e.g. axum's plain-text query-param rejections) are
/// always surfaced clearly rather than producing a cryptic EOF.
async fn api(
    client: &Client,
    method: Method,
    url: &str,
    body: Option<&Value>,
    verbose: bool,
) -> Result<Value> {
    if verbose {
        eprintln!("[sncli] → {} {}", method, url);
        if let Some(b) = body {
            eprintln!("[sncli]   body: {}", serde_json::to_string(b).unwrap_or_default());
        }
    }

    let mut req = client.request(method.clone(), url);
    if let Some(b) = body {
        req = req.json(b);
    }

    let resp = req
        .send()
        .await
        .with_context(|| format!("{} {}", method, url))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .with_context(|| format!("{} {} — reading response body", method, url))?;

    if verbose {
        eprintln!(
            "[sncli] ← {} {}  body: {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or(""),
            if text.is_empty() { "(empty)" } else { &text }
        );
    }

    if text.is_empty() {
        bail!("{} {} → HTTP {} with empty body", method, url, status);
    }

    match serde_json::from_str::<Value>(&text) {
        Ok(value) => {
            if !status.is_success() {
                let msg = value
                    .get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or(&text);
                bail!("{} {} → HTTP {}: {}", method, url, status, msg);
            }
            Ok(value)
        }
        Err(_) => {
            // Server sent non-JSON (e.g. axum rejected a bad query string)
            bail!("{} {} → HTTP {} (non-JSON): {}", method, url, status, text);
        }
    }
}

// ── main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli).await {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    let client = Client::new();
    let server = cli.server.trim_end_matches('/').to_string();
    let raw = cli.raw;
    let v = cli.verbose;

    macro_rules! get {
        ($url:expr) => {
            api(&client, Method::GET, &$url, None, v).await?
        };
    }
    macro_rules! post {
        ($url:expr, $body:expr) => {
            api(&client, Method::POST, &$url, Some(&$body), v).await?
        };
    }
    macro_rules! patch {
        ($url:expr, $body:expr) => {
            api(&client, Method::PATCH, &$url, Some(&$body), v).await?
        };
    }
    macro_rules! put {
        ($url:expr, $body:expr) => {
            api(&client, Method::PUT, &$url, Some(&$body), v).await?
        };
    }
    macro_rules! delete {
        ($url:expr) => {
            api(&client, Method::DELETE, &$url, None, v).await?
        };
    }

    match cli.command {
        Cmd::Health => {
            let res = get!(format!("{server}/health"));
            print_json(&res, raw);
        }

        Cmd::Records(a) => match a.action {
            RecordsCmd::List(a) => {
                let mut url = format!(
                    "{server}/records/{table}?instance={inst}&limit={limit}",
                    table = urlenc(&a.table),
                    inst = urlenc(&normalize_instance(&a.instance)),
                    limit = a.limit,
                );
                if let Some(q) = &a.query {
                    url.push_str(&format!("&q={}", urlenc(q)));
                }
                if let Some(f) = &a.fields {
                    url.push_str(&format!("&fields={}", urlenc(f)));
                }
                if let Some(ob) = &a.order_by {
                    url.push_str(&format!("&order_by={}", urlenc(ob)));
                }
                let res = get!(url);
                print_json(&res, raw);
            }

            RecordsCmd::Get(a) => {
                let mut url = format!(
                    "{server}/records/{table}/{sys_id}?instance={inst}",
                    table = urlenc(&a.table),
                    sys_id = urlenc(&a.sys_id),
                    inst = urlenc(&normalize_instance(&a.instance)),
                );
                if let Some(f) = &a.fields {
                    url.push_str(&format!("&fields={}", urlenc(f)));
                }
                let res = get!(url);
                print_json(&res, raw);
            }

            RecordsCmd::Schema(a) => {
                let url = format!(
                    "{server}/records/{table}/schema?instance={inst}",
                    table = urlenc(&a.table),
                    inst = urlenc(&normalize_instance(&a.instance)),
                );
                let res = get!(url);
                print_json(&res, raw);
            }

            RecordsCmd::Create(a) => {
                let fields = parse_fields(&a.fields)?;
                let body = json!({ "instance": normalize_instance(&a.instance), "fields": fields });
                let res = post!(format!("{server}/records/{}", urlenc(&a.table)), body);
                print_json(&res, raw);
            }

            RecordsCmd::Update(a) => {
                let fields = parse_fields(&a.fields)?;
                let body = json!({ "instance": normalize_instance(&a.instance), "fields": fields });
                let res = patch!(
                    format!("{server}/records/{}/{}", urlenc(&a.table), urlenc(&a.sys_id)),
                    body
                );
                print_json(&res, raw);
            }

            // DELETE uses a query param, not a body — the server handler uses Query<DeleteParams>
            RecordsCmd::Delete(a) => {
                let url = format!(
                    "{server}/records/{table}/{sys_id}?instance={inst}",
                    table = urlenc(&a.table),
                    sys_id = urlenc(&a.sys_id),
                    inst = urlenc(&normalize_instance(&a.instance)),
                );
                let res = delete!(url);
                print_json(&res, raw);
            }
        },

        Cmd::Scripts(a) => match a.action {
            ScriptsCmd::Bg(a) => {
                let script = match (&a.script, &a.file) {
                    (Some(s), _) => s.clone(),
                    (_, Some(path)) => std::fs::read_to_string(path)
                        .with_context(|| format!("reading {}", path.display()))?,
                    (None, None) => bail!("provide --script or --file"),
                };
                let body = json!({ "instance": normalize_instance(&a.instance), "script": script });
                let res = post!(format!("{server}/scripts/bg"), body);
                if raw {
                    print_json(&res, true);
                } else if let Some(out) = res.get("output").and_then(|o| o.as_str()) {
                    print!("{out}");
                } else {
                    print_json(&res, false);
                }
            }

            ScriptsCmd::Slash(a) => {
                let mut body = json!({
                    "instance": normalize_instance(&a.instance),
                    "command":  a.command,
                    "auto_run": !a.no_auto_run,
                });
                if let Some(url) = a.url {
                    body["url"] = json!(url);
                }
                if let Some(tab_id) = a.tab_id {
                    body["tab_id"] = json!(tab_id);
                }
                let res = post!(format!("{server}/scripts/slash"), body);
                print_json(&res, raw);
            }
        },

        Cmd::Rest(a) => {
            let mut body = json!({
                "instance": normalize_instance(&a.instance),
                "method":   a.method.to_uppercase(),
                "endpoint": a.endpoint,
            });
            if let Some(b) = a.body {
                body["body"] = parse_json(&b, "--body")?;
            }
            if let Some(p) = a.params {
                body["query_params"] = parse_json(&p, "--params")?;
            }
            let res = post!(format!("{server}/rest"), body);
            print_json(&res, raw);
        }

        Cmd::Browser(a) => match a.action {
            BrowserCmd::Form(a) => {
                let mut url = format!(
                    "{server}/browser/form?instance={inst}",
                    inst = urlenc(&normalize_instance(&a.instance)),
                );
                if let Some(u) = &a.url {
                    url.push_str(&format!("&url={}", urlenc(u)));
                }
                if let Some(t) = &a.tab_id {
                    url.push_str(&format!("&tab_id={}", urlenc(t)));
                }
                if let Some(f) = &a.fields {
                    url.push_str(&format!("&fields={}", urlenc(f)));
                }
                let res = get!(url);
                print_json(&res, raw);
            }

            BrowserCmd::SetField(a) => {
                let mut body = json!({
                    "instance": normalize_instance(&a.instance),
                    "field":    a.field,
                    "value":    coerce_value(&a.value),
                });
                if let Some(dv) = a.display_value {
                    body["display_value"] = json!(dv);
                }
                if let Some(url) = a.url {
                    body["url"] = json!(url);
                }
                if let Some(tab_id) = a.tab_id {
                    body["tab_id"] = json!(tab_id);
                }
                let res = post!(format!("{server}/browser/form"), body);
                print_json(&res, raw);
            }

            BrowserCmd::Action(a) => {
                let mut body = json!({
                    "instance":         a.instance,
                    "ui_action":        a.action,
                    "suppress_dialogs": !a.allow_dialogs,
                });
                if let Some(url) = a.url {
                    body["url"] = json!(url);
                }
                if let Some(tab_id) = a.tab_id {
                    body["tab_id"] = json!(tab_id);
                }
                let res = post!(format!("{server}/browser/form/action"), body);
                print_json(&res, raw);
            }

            BrowserCmd::Navigate(a) => {
                let mut body = json!({
                    "instance":         a.instance,
                    "url":              a.url,
                    "new_tab":          a.new_tab,
                    "wait_for_load":    !a.no_wait,
                    "discard_unsaved":  a.discard_unsaved,
                });
                if let Some(tab_id) = a.tab_id {
                    body["tab_id"] = json!(tab_id);
                }
                let res = post!(format!("{server}/browser/navigate"), body);
                print_json(&res, raw);
            }

            BrowserCmd::Click(a) => {
                let mut body = json!({
                    "instance":         a.instance,
                    "selector":         a.selector,
                    "suppress_dialogs": !a.allow_dialogs,
                });
                if let Some(url) = a.url {
                    body["url"] = json!(url);
                }
                if let Some(tab_id) = a.tab_id {
                    body["tab_id"] = json!(tab_id);
                }
                let res = post!(format!("{server}/browser/click"), body);
                print_json(&res, raw);
            }

            BrowserCmd::Screenshot(a) => {
                if a.url.is_none() && a.tab_id.is_none() {
                    bail!("--url or --tab-id is required");
                }
                let mut body = json!({
                    "instance":   a.instance,
                    "exact_url":  a.exact_url,
                    "file_name":  a.output,
                });
                if let Some(url) = a.url {
                    body["url"] = json!(url);
                }
                if let Some(tab_id) = a.tab_id {
                    body["tab_id"] = json!(tab_id);
                }
                let res = post!(format!("{server}/browser/screenshot"), body);
                print_json(&res, raw);
            }

            BrowserCmd::Tab(a) => {
                let body = json!({
                    "instance":          a.instance,
                    "url":               a.url,
                    "reload":            a.reload,
                    "wait_for_load":     a.wait,
                    "open_if_not_found": !a.no_open,
                });
                let res = post!(format!("{server}/browser/tab"), body);
                print_json(&res, raw);
            }
        },

        Cmd::Context(a) => {
            let body = json!({
                "instance":   a.instance,
                "type":       a.r#type,
                "value":      a.value,
                "reload_tab": !a.no_reload,
            });
            let res = put!(format!("{server}/context"), body);
            print_json(&res, raw);
        }

        Cmd::Artifact(a) => {
            let mut fields = match a.fields {
                Some(ref s) => parse_fields(s)?,
                None => Map::new(),
            };
            fields.insert("name".to_string(), json!(a.name));
            let body = json!({
                "instance": normalize_instance(&a.instance),
                "table":    a.table,
                "scope":    a.scope,
                "fields":   fields,
            });
            let res = post!(format!("{server}/artifacts"), body);
            print_json(&res, raw);
        }

        Cmd::Events => {
            eprintln!("Streaming events from {server}/events … (Ctrl-C to stop)");
            stream_sse(&client, format!("{server}/events"), raw).await?;
        }

        Cmd::Raw(a) => {
            let mut payload: Value =
                serde_json::from_str(&a.payload).context("payload must be valid JSON")?;
            if a.fire_and_forget {
                payload["fire_and_forget"] = json!(true);
            }
            let res = post!(format!("{server}/raw"), payload);
            print_json(&res, raw);
        }
    }

    Ok(())
}

// ── SSE streaming ─────────────────────────────────────────────────────────────

async fn stream_sse(client: &Client, url: String, raw: bool) -> Result<()> {
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;

    if !resp.status().is_success() {
        bail!("HTTP {}: could not open event stream", resp.status());
    }

    let mut buf: Vec<u8> = Vec::new();
    let mut stream = resp.bytes_stream();

    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        buf.extend_from_slice(&chunk.context("reading SSE stream")?);
        // Process any complete lines (SSE uses \n line endings)
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line = String::from_utf8_lossy(&buf[..pos]).into_owned();
            buf.drain(..=pos);
            let data = line.trim();
            if let Some(json_str) = data.strip_prefix("data: ") {
                if json_str.is_empty() {
                    continue;
                }
                if raw {
                    println!("{json_str}");
                } else {
                    match serde_json::from_str::<Value>(json_str) {
                        Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap()),
                        Err(_) => println!("{json_str}"),
                    }
                    println!(); // blank line between events for readability
                }
            }
        }
    }

    Ok(())
}

// ── URL encoding (minimal, for query params) ──────────────────────────────────

fn urlenc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
