use axum::{extract::State, Json};
use serde_json::{json, Value};

use crate::state::AppState;

pub async fn handler(State(s): State<AppState>) -> Json<Value> {
    let connected = s.connected().await;
    let inst = s.sn_instance.lock().await;
    let (instance_url, instance_name) = if let Some(ref i) = *inst {
        (
            i.get("url").and_then(|u| u.as_str()).unwrap_or("").to_string(),
            i.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
        )
    } else {
        (String::new(), String::new())
    };
    let has_session = !instance_url.is_empty();
    Json(json!({
        "status": if connected && has_session { "ready" } else if connected { "no_session" } else { "waiting" },
        "helper_tab_connected": connected,
        "sn_session": has_session,
        "instance_url": instance_url,
        "instance_name": instance_name,
    }))
}
