/// airdrop-bridge — session D-Bus bridge for airdrop-gnome
///
/// Responsibilities:
///   - Holds a radio lease from owlyd (system bus, io.github.t4t5.Owly.Radio1)
///   - Wraps `owly browse`, `owly send`, `owly receive` subprocesses
///   - Exposes io.github.t4t5.AirDropGnome1 on the session bus
///   - Manages state machines for radio, visibility, and transfers
///   - Handles incoming offer approval signals to the extension

mod dbus_bridge;
mod owly_radio;
mod owly_process;
mod state;
mod config;
mod mock;

use anyhow::Result;
use tracing::{info, warn};
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging; AIRDROP_LOG=debug for verbose output
    fmt()
        .with_env_filter(
            EnvFilter::try_from_env("AIRDROP_LOG")
                .unwrap_or_else(|_| EnvFilter::new("airdrop_bridge=info,warn")),
        )
        .init();

    info!("airdrop-bridge starting (owly wrapper, session D-Bus)");

    // Check for mock mode (for UI testing without hardware)
    let mock_mode = std::env::var("AIRDROP_MOCK").is_ok();
    if mock_mode {
        warn!("MOCK MODE: no real radio or transfers will occur");
    }

    // Load XDG config
    let cfg = config::Config::load()?;
    info!(receive_dir = %cfg.receive_dir.display(), "config loaded");

    // Start the bridge
    dbus_bridge::run(cfg, mock_mode).await
}
