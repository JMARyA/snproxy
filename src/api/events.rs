use axum::{
    extract::State,
    response::{
        sse::{Event, KeepAlive},
        Sse,
    },
};
use futures_util::StreamExt;
use serde_json::Value;
use tokio_stream::wrappers::BroadcastStream;

use crate::state::AppState;

// ---------------------------------------------------------------------------
// GET /events  — SSE stream of all raw WebSocket messages from the browser
// ---------------------------------------------------------------------------

pub async fn stream(State(s): State<AppState>) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = s.event_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|msg| async move {
        match msg {
            Ok(val) => {
                let data = serialize_event(&val);
                Some(Ok(Event::default().data(data)))
            }
            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn serialize_event(val: &Value) -> String {
    serde_json::to_string(val).unwrap_or_else(|_| "{}".to_string())
}
