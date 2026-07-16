pub mod widgets;

use crate::ast::Pipeline;
use crate::error::Result;
use sncore::Client;

pub async fn run(
    _pipeline: Pipeline,
    _client: Client,
    _instance: String,
    _output: Option<String>,
) -> Result<()> {
    Err(crate::error::SnpipeError::other("TUI not yet implemented"))
}
