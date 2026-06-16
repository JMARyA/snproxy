use crate::state::AppState;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

pub async fn serve(state: AppState, host: String, port: u16) {
    let addr = format!("{host}:{port}");
    let listener = TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("cannot bind WebSocket on {addr}: {e}"));
    info!("WebSocket server listening on ws://{addr}");

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                tokio::spawn(handle_client(stream, peer.to_string(), state.clone()));
            }
            Err(e) => error!("WS accept: {e}"),
        }
    }
}

async fn handle_client(stream: tokio::net::TcpStream, peer: String, state: AppState) {
    info!("incoming WS connection from {peer}");

    let ws = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            error!("WS handshake failed ({peer}): {e}");
            return;
        }
    };

    let (mut sink, mut ws_stream) = ws.split();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // Register as the active Helper Tab (last connection wins)
    *state.ws_tx.lock().await = Some(tx);
    info!("Helper Tab connected: {peer}");

    // Welcome sequence — mirrors what sn-scriptsync sends so the extension
    // initialises normally and shows a banner in the Helper Tab.
    let _ = sink
        .send(Message::Text(json!(["Connected to snproxy"]).to_string()))
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

    // Pump outbound messages (HTTP handlers → Helper Tab)
    let out_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            // Log the action + req_id for every outbound frame
            if let Ok(val) = serde_json::from_str::<Value>(&msg) {
                let action = val.get("action").and_then(|v| v.as_str()).unwrap_or("?");
                let req_id = val
                    .get("agentRequestId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                debug!(%action, %req_id, "→ WS send");
            }
            if sink.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    // Route inbound messages (Helper Tab → pending callers + SSE broadcast)
    while let Some(result) = ws_stream.next().await {
        match result {
            Ok(Message::Text(text)) => match serde_json::from_str::<Value>(&text) {
                Ok(val) => {
                    let action = val
                        .get("action")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");

                    // Route to a specific pending caller if agentRequestId is present
                    if let Some(id) = val.get("agentRequestId").and_then(|v| v.as_str()) {
                        let mut pending = state.pending.lock().await;
                        if let Some(tx) = pending.remove(id) {
                            debug!(%action, req_id = %id, "← WS recv (matched)");
                            let _ = tx.send(val.clone());
                        } else {
                            // Response arrived after timeout or for a fire-and-forget
                            debug!(%action, req_id = %id, "← WS recv (no pending caller)");
                        }
                    } else {
                        // Unsolicited message (e.g. async push from the Helper Tab)
                        debug!(%action, "← WS recv (unsolicited)");
                    }

                    // Always broadcast to SSE regardless
                    let _ = state.event_tx.send(val);
                }
                Err(e) => {
                    warn!("← WS recv non-JSON ({e}): {}", &text[..text.len().min(120)]);
                    let _ = state.event_tx.send(json!({"raw": text.as_str()}));
                }
            },
            Ok(Message::Close(_)) | Err(_) => break,
            _ => {} // Ping/Pong handled automatically by tungstenite
        }
    }

    out_task.abort();
    *state.ws_tx.lock().await = None;
    warn!("Helper Tab disconnected: {peer}");
}
