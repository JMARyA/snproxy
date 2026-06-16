pub mod artifacts;
pub mod browser;
pub mod context;
pub mod events;
pub mod health;
pub mod raw;
pub mod records;
pub mod rest;
pub mod scripts;

use axum::{
    routing::{delete, get, patch, post, put},
    Router,
};

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        // health
        .route("/health", get(health::handler))
        // CRUD records + schema inspection
        .route("/records/:table", get(records::list))
        .route("/records/:table", post(records::create))
        .route("/records/:table/schema", get(records::schema))
        .route("/records/:table/:sys_id", get(records::get))
        .route("/records/:table/:sys_id", patch(records::update))
        .route("/records/:table/:sys_id", delete(records::delete))
        // background scripts & slash commands
        .route("/scripts/bg", post(scripts::bg))
        .route("/scripts/slash", post(scripts::slash))
        // raw REST passthrough
        .route("/rest", post(rest::handler))
        // browser automation
        .route("/browser/form", get(browser::form_state))
        .route("/browser/form", post(browser::set_field))
        .route("/browser/form/action", post(browser::ui_action))
        .route("/browser/navigate", post(browser::navigate))
        .route("/browser/click", post(browser::click))
        .route("/browser/screenshot", post(browser::screenshot))
        .route("/browser/tab", post(browser::tab))
        // context switching
        .route("/context", put(context::switch))
        // development artifact creation (opens in browser, adds to update set)
        .route("/artifacts", post(artifacts::create))
        // SSE event stream
        .route("/events", get(events::stream))
        // raw WS passthrough
        .route("/raw", post(raw::handler))
        .with_state(state)
}
