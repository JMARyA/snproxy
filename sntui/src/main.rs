mod app;
mod client;
mod column_config;
mod config;
mod tables;
mod ui;

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use app::App;

#[derive(Parser)]
#[command(name = "sntui", about = "ServiceNow TUI — like k9s but for ServiceNow")]
struct Cli {
    /// snproxy HTTP API port (overrides config file)
    #[arg(long)]
    port: Option<u16>,
    /// Path to config file (default: ./sntui.toml or ~/.config/sntui/config.toml)
    #[arg(long)]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let cli = Cli::parse();

    let (cfg, cfg_warn) = config::load(cli.config.as_deref());
    let port = cli.port.unwrap_or(cfg.snproxy.port);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal, port, cfg, cfg_warn).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("Error: {e}");
    }
    Ok(())
}

async fn run<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    port: u16,
    cfg: config::Config,
    cfg_warn: Option<String>,
) -> io::Result<()> {
    let mut app = App::new(port, cfg);
    if let Some(warn) = cfg_warn {
        app.status = Some(warn);
        app.status_is_error = true;
    }
    app.initial_health_check().await;

    loop {
        terminal.draw(|f| ui::render(f, &app))?;
        app.process_messages();

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if app.handle_key(key) {
                    break;
                }
            }
        }

        app.tick().await;
    }

    Ok(())
}
