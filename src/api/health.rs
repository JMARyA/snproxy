use axum::{extract::State, Json};
use serde_json::{json, Value};

use crate::state::AppState;

pub async fn handler(State(s): State<AppState>) -> Json<Value> {
    let connected = s.connected().await;
    Json(json!({
        "status": if connected { "ready" } else { "waiting" },
        "helper_tab_connected": connected,
    }))
}
