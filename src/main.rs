mod config;
mod git;
mod gitlab;
mod markdown;
mod tui;

use config::load_config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = load_config();
    tui::run_tui(config).await
}
