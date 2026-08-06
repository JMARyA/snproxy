mod client;
mod provider;
mod resource;

use anyhow::Result;
use provider::SnProvider;
use tf_provider::serve;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    serve("snterra", SnProvider).await
}
