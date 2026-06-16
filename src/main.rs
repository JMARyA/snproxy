use axum::{
    extract::State,
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json,
    },
    routing::{get, post},
    Router,
};
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::{broadcast, mpsc, oneshot, Mutex},
    time::timeout,
};
use tokio_stream::wrappers::BroadcastStream;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tracing::{error, info, warn};

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "snproxy",
    about = "ServiceNow REST proxy — impersonates sn-scriptsync so the SN Utils\n\
             Helper Tab connects here, then exposes a local HTTP API for tooling."
)]
struct Args {
    /// Bind host (use 0.0.0.0 to expose externally)
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// WebSocket port — Helper Tab connects here (same as sn-scriptsync default)
    #[arg(long, default_value_t = 1978)]
    ws_port: u16,

    /// HTTP REST API port
    #[arg(long, default_value_t = 8766)]
    http_port: u16,

    /// Seconds to wait for a response from the Helper Tab before timing out
    #[arg(long, default_value_t = 30)]
    timeout: u64,
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct AppState {
    /// Sender half of the channel into the active Helper Tab WebSocket
    ws_tx: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
    /// Pending HTTP requests blocked waiting for a specific WS response action
    pending: Arc<Mutex<HashMap<String, VecDeque<oneshot::Sender<Value>>>>>,
    /// Broadcast channel — every message from the Helper Tab is forwarded here (SSE)
    event_tx: broadcast::Sender<Value>,
    timeout_secs: u64,
}

impl AppState {
    fn new(timeout_secs: u64) -> Self {
        let (event_tx, _) = broadcast::channel(512);
        Self {
            ws_tx: Arc::new(Mutex::new(None)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            event_tx,
            timeout_secs,
        }
    }

    async fn connected(&self) -> bool {
        self.ws_tx.lock().await.is_some()
    }

    /// Send a message to the Helper Tab without waiting for a reply.
    async fn fire(&self, payload: Value) -> Result<(), ApiError> {
        let s = payload.to_string();
        let guard = self.ws_tx.lock().await;
        guard
            .as_ref()
            .ok_or(ApiError::NoClient)?
            .send(s)
            .map_err(|_| ApiError::SendFailed)
    }

    /// Send a message and block until the Helper Tab replies with `resp_action`.
    ///
    /// Uses a FIFO queue per action type — concurrent requests of the same type
    /// are matched in the order they were sent. Dead waiters (timed-out callers)
    /// are skipped automatically when their oneshot receiver has been dropped.
    async fn fire_await(&self, payload: Value, resp_action: &str) -> Result<Value, ApiError> {
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending
                .entry(resp_action.to_string())
                .or_default()
                .push_back(tx);
        }
        // If send fails, `tx` is already in the queue but `rx` will be dropped
        // when we return, so the next response delivery attempt will skip it.
        self.fire(payload).await?;

        match timeout(Duration::from_secs(self.timeout_secs), rx).await {
            Ok(Ok(val)) => Ok(val),
            Ok(Err(_)) => Err(ApiError::ChannelClosed),
            Err(_) => Err(ApiError::Timeout(resp_action.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------
// API error — maps to appropriate HTTP status
// ---------------------------------------------------------------------------

enum ApiError {
    NoClient,
    SendFailed,
    Timeout(String),
    ChannelClosed,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, msg) = match self {
            ApiError::NoClient => (StatusCode::SERVICE_UNAVAILABLE, "no Helper Tab connected".to_string()),
            ApiError::SendFailed => (StatusCode::SERVICE_UNAVAILABLE, "WebSocket send failed".to_string()),
            ApiError::Timeout(action) => (StatusCode::GATEWAY_TIMEOUT, format!("timeout waiting for {action}")),
            ApiError::ChannelClosed => (StatusCode::INTERNAL_SERVER_ERROR, "response channel closed".to_string()),
        };
        (status, Json(json!({"error": msg}))).into_response()
    }
}

// ---------------------------------------------------------------------------
// HTTP handlers
// ---------------------------------------------------------------------------

async fn health(State(s): State<AppState>) -> Json<Value> {
    Json(json!({
        "status": if s.connected().await { "ready" } else { "waiting" },
        "helper_tab_connected": s.connected().await,
    }))
}

#[derive(Deserialize)]
struct BgReq {
    instance: String,
    code: String,
}

/// Run a background script (Glide API) on the ServiceNow instance.
async fn bg(State(s): State<AppState>, Json(r): Json<BgReq>) -> Result<Json<Value>, ApiError> {
    s.fire(json!({
        "action": "runSlashCommand",
        "instance": r.instance,
        "command": format!("/bg {}", r.code),
    }))
    .await?;
    Ok(Json(json!({"status": "sent"})))
}

#[derive(Deserialize)]
struct QueryReq {
    instance: String,
    table: String,
    /// ServiceNow encoded query string, e.g. "active=true^category=software"
    #[serde(alias = "query")]
    encoded_query: String,
}

/// Query records from any table and return the results synchronously.
async fn query(State(s): State<AppState>, Json(r): Json<QueryReq>) -> Result<Json<Value>, ApiError> {
    let val = s
        .fire_await(
            json!({
                "action": "agentQueryRecords",
                "instance": r.instance,
                "table": r.table,
                "encodedQuery": r.encoded_query,
            }),
            "agentQueryRecordsResponse",
        )
        .await?;
    Ok(Json(val))
}

#[derive(Deserialize)]
struct UpdateReq {
    instance: String,
    table: String,
    sys_id: String,
    field: String,
    content: String,
}

/// Write a field on an existing record.
async fn update(State(s): State<AppState>, Json(r): Json<UpdateReq>) -> Result<Json<Value>, ApiError> {
    s.fire(json!({
        "action": "saveFieldAsFile",
        "instance": r.instance,
        "tableName": r.table,
        "sys_id": r.sys_id,
        "fieldName": r.field,
        "content": r.content,
    }))
    .await?;
    Ok(Json(json!({"status": "sent"})))
}

#[derive(Deserialize)]
struct SlashReq {
    instance: String,
    /// Full slash command including the leading slash, e.g. "/token" or "/bg gs.info('hi')"
    command: String,
}

/// Run an SN Utils slash command.
async fn slash(State(s): State<AppState>, Json(r): Json<SlashReq>) -> Result<Json<Value>, ApiError> {
    s.fire(json!({
        "action": "runSlashCommand",
        "instance": r.instance,
        "command": r.command,
    }))
    .await?;
    Ok(Json(json!({"status": "sent"})))
}

#[derive(Deserialize)]
struct ScreenshotReq {
    instance: String,
    /// Optional relative URL to navigate to before capturing, e.g. "/now/nav/ui/classic/..."
    url: Option<String>,
}

/// Capture a screenshot and return the response (includes base64 image data).
async fn screenshot(
    State(s): State<AppState>,
    Json(r): Json<ScreenshotReq>,
) -> Result<Json<Value>, ApiError> {
    let mut payload = json!({"action": "takeScreenshot", "instance": r.instance});
    if let Some(url) = r.url {
        payload["url"] = json!(url);
    }
    let val = s.fire_await(payload, "screenshotResponse").await?;
    Ok(Json(val))
}

#[derive(Deserialize)]
struct SwitchReq {
    instance: String,
    /// "updateSet" | "scope" | "domain"
    switch_type: String,
    value: String,
}

/// Switch update set, scope, or domain on the connected instance.
async fn switch_ctx(
    State(s): State<AppState>,
    Json(r): Json<SwitchReq>,
) -> Result<Json<Value>, ApiError> {
    s.fire(json!({
        "action": "switchContext",
        "instance": r.instance,
        "switchType": r.switch_type,
        "value": r.value,
    }))
    .await?;
    Ok(Json(json!({"status": "sent"})))
}

/// Generic passthrough — send any JSON payload.
/// If the `action` has a known synchronous response, blocks and returns it.
async fn command(
    State(s): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let action = payload
        .get("action")
        .and_then(|a| a.as_str())
        .unwrap_or("");

    let resp_action = match action {
        "agentQueryRecords" => Some("agentQueryRecordsResponse"),
        "takeScreenshot"    => Some("screenshotResponse"),
        "createArtifact"    => Some("createRecordResponse"),
        _                   => None,
    };

    if let Some(ra) = resp_action {
        let val = s.fire_await(payload, ra).await?;
        Ok(Json(val))
    } else {
        s.fire(payload).await?;
        Ok(Json(json!({"status": "sent"})))
    }
}

/// Server-Sent Events stream — every message received from the Helper Tab
/// is forwarded here as a JSON event.  Use this to observe async output
/// (background script results, sync events, etc.).
async fn events(
    State(s): State<AppState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = s.event_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|r| async move {
        let val = r.ok()?;
        let data = serde_json::to_string(&val).ok()?;
        Some(Ok(Event::default().data(data)))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

// ---------------------------------------------------------------------------
// WebSocket server — impersonates sn-scriptsync at ws://127.0.0.1:1978/
// ---------------------------------------------------------------------------

async fn run_ws_server(state: AppState, host: String, port: u16) {
    let addr = format!("{host}:{port}");
    let listener = TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("cannot bind WebSocket on {addr}: {e}"));
    info!("WebSocket server listening on ws://{addr}");

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let state = state.clone();
                tokio::spawn(handle_ws_client(stream, peer.to_string(), state));
            }
            Err(e) => error!("WS accept: {e}"),
        }
    }
}

async fn handle_ws_client(stream: tokio::net::TcpStream, peer: String, state: AppState) {
    info!("incoming WS connection from {peer}");

    let ws = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            error!("WS handshake failed ({peer}): {e}");
            return;
        }
    };

    let (mut sink, mut ws_stream) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // Register as the active Helper Tab connection (last writer wins)
    *state.ws_tx.lock().await = Some(tx);
    info!("Helper Tab connected: {peer}");

    // Send the same welcome sequence that sn-scriptsync sends so the
    // extension initialises normally.
    let _ = sink
        .send(Message::Text(
            json!(["Connected to snproxy"]).to_string(),
        ))
        .await;
    let _ = sink
        .send(Message::Text(
            json!({
                "action": "bannerMessage",
                "message": "snproxy active — REST API on http://127.0.0.1:8766",
                "class": "alert alert-primary",
            })
            .to_string(),
        ))
        .await;

    // Forward outbound commands (from HTTP handlers) to the Helper Tab.
    let out_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sink.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    // Receive inbound messages from the Helper Tab and route them.
    while let Some(result) = ws_stream.next().await {
        match result {
            Ok(Message::Text(text)) => {
                match serde_json::from_str::<Value>(&text) {
                    Ok(val) => {
                        let action = val
                            .get("action")
                            .and_then(|a| a.as_str())
                            .unwrap_or("")
                            .to_string();
                        info!("<< {action}");

                        // Deliver to the first live waiter for this action type.
                        {
                            let mut pending = state.pending.lock().await;
                            if let Some(queue) = pending.get_mut(&action) {
                                while let Some(waiter) = queue.pop_front() {
                                    if waiter.send(val.clone()).is_ok() {
                                        break;
                                    }
                                    // receiver was dropped (timeout), try next waiter
                                }
                            }
                        }

                        // Also push to SSE broadcast (best-effort, no error on lag)
                        let _ = state.event_tx.send(val);
                    }
                    Err(_) => {
                        // Non-JSON frame — forward raw as a string event
                        let _ = state.event_tx.send(json!({"raw": text.as_str()}));
                    }
                }
            }
            Ok(Message::Close(_)) => break,
            Err(e) => {
                warn!("WS error from {peer}: {e}");
                break;
            }
            _ => {} // Ping/Pong handled automatically by tungstenite
        }
    }

    out_task.abort();
    *state.ws_tx.lock().await = None;
    warn!("Helper Tab disconnected: {peer}");
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "snproxy=info".into()),
        )
        .init();

    let state = AppState::new(args.timeout);

    // Start WebSocket server in background
    let ws_state = state.clone();
    let ws_host = args.host.clone();
    let ws_port = args.ws_port;
    tokio::spawn(async move {
        run_ws_server(ws_state, ws_host, ws_port).await;
    });

    // Build HTTP router
    let app = Router::new()
        .route("/health",     get(health))
        .route("/bg",         post(bg))
        .route("/query",      post(query))
        .route("/update",     post(update))
        .route("/slash",      post(slash))
        .route("/screenshot", post(screenshot))
        .route("/switch",     post(switch_ctx))
        .route("/command",    post(command))
        .route("/events",     get(events))
        .with_state(state);

    let http_addr = format!("{}:{}", args.host, args.http_port);
    let listener = TcpListener::bind(&http_addr)
        .await
        .unwrap_or_else(|e| panic!("cannot bind HTTP on {http_addr}: {e}"));

    println!();
    println!("snproxy");
    println!("  WebSocket (Helper Tab) : ws://{}:{}", args.host, args.ws_port);
    println!("  HTTP REST API          : http://{}:{}", args.host, args.http_port);
    println!("  Event stream (SSE)     : http://{}:{}/events", args.host, args.http_port);
    println!();
    println!("Waiting for SN Utils Helper Tab to connect...");
    println!("(Make sure VS Code / sn-scriptsync is NOT running on port {})", args.ws_port);
    println!();

    axum::serve(listener, app).await.unwrap();
}
