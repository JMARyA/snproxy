mod ast;
mod error;
mod eval;
mod output;
mod parser;
mod tui;

use clap::{Parser, Subcommand};
use error::Result;

#[derive(Parser)]
#[command(name = "snpipe", about = "Functional ServiceNow record pipeline")]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// snproxy base URL
    #[arg(long, env = "SNPROXY_URL", default_value = "http://localhost:8766", global = true)]
    server: String,

    /// ServiceNow instance (short name or full hostname)
    #[arg(long, short = 'i', env = "SNPROXY_INSTANCE", global = true)]
    instance: Option<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Execute a pipeline
    Run {
        file: String,
        /// Output file path (default: stdout)
        #[arg(short = 'o', long)]
        output: Option<String>,
        /// Show TUI visual mode
        #[arg(long)]
        visual: bool,
    },
    /// Parse and validate a pipeline file without executing
    Check {
        file: String,
    },
    /// Show what API calls a pipeline would make without executing
    Explain {
        file: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Check { file } => {
            let src = std::fs::read_to_string(&file)
                .map_err(|e| error::SnpipeError::other(format!("cannot read {file}: {e}")))?;
            let pipeline = parser::parse(&src)?;
            println!("ok — pipeline '{}' parsed successfully",
                pipeline.name.as_deref().unwrap_or("<unnamed>"));
            Ok(())
        }

        Command::Explain { file } => {
            let src = std::fs::read_to_string(&file)
                .map_err(|e| error::SnpipeError::other(format!("cannot read {file}: {e}")))?;
            let pipeline = parser::parse(&src)?;
            eval::explain(&pipeline);
            Ok(())
        }

        Command::Run { file, output, visual } => {
            let src = std::fs::read_to_string(&file)
                .map_err(|e| error::SnpipeError::other(format!("cannot read {file}: {e}")))?;
            let pipeline = parser::parse(&src)?;

            let client = sncore::Client::new_with_url(&cli.server);
            let instance = cli.instance.as_deref().unwrap_or("auto");

            if visual {
                tui::run(pipeline, client, instance.to_string(), output).await
            } else {
                eval::run(pipeline, client, instance.to_string(), output).await
            }
        }
    }
}
