mod api;
mod state;
mod ws;

use clap::Parser;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Parser)]
#[command(
    name = "snproxy",
    about = "ServiceNow REST proxy — impersonates sn-scriptsync so the SN Utils\n\
             Helper Tab connects here, then exposes a local HTTP API for tooling."
)]
struct Cli {
    /// Bind host (use 0.0.0.0 to expose to the network)
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    /// WebSocket port — Helper Tab always connects to 1978
    #[arg(long, default_value_t = 1978)]
    ws_port: u16,
    /// HTTP REST API port
    #[arg(long, default_value_t = 8766)]
    port: u16,
    /// Seconds to wait for a Helper Tab response before returning 504
    #[arg(long, default_value_t = 30)]
    timeout: u64,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let state = state::AppState::new(cli.timeout);

    let ws_state = state.clone();
    let ws_host = cli.host.clone();
    let ws_port = cli.ws_port;
    tokio::spawn(async move {
        ws::serve(ws_state, ws_host, ws_port).await;
    });

    let app = api::router(state);
    let http_addr = format!("{}:{}", cli.host, cli.port);
    let listener = tokio::net::TcpListener::bind(&http_addr)
        .await
        .unwrap_or_else(|e| panic!("cannot bind HTTP API on {http_addr}: {e}"));

    println!();
    println!("snproxy");
    println!("  WebSocket (Helper Tab) : ws://{}:{}", cli.host, cli.ws_port);
    println!("  HTTP REST API          : http://{}:{}", cli.host, cli.port);
    println!("  Event stream (SSE)     : http://{}:{}/events", cli.host, cli.port);
    println!();
    println!("Waiting for SN Utils Helper Tab to connect...");
    println!(
        "(Make sure VS Code / sn-scriptsync is NOT running on port {})",
        cli.ws_port
    );
    println!();

    info!("HTTP API listening on http://{http_addr}");
    axum::serve(listener, app)
        .await
        .expect("HTTP server failed");
}
