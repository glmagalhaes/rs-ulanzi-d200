mod config;
mod daemon;
mod device;
mod openaction_client;
mod telemetry;

use anyhow::Result;
use clap::Parser;
use log::{error, info, warn};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the configuration YAML file
    #[arg(short, long, default_value = "config.yaml")]
    config: PathBuf,

    /// Log level (info, debug, trace)
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// Run as a daemon
    #[arg(short, long)]
    daemon: bool,

    /// Enable WebSocket server for plugin communication
    #[arg(long)]
    websocket: bool,

    /// WebSocket server port
    #[arg(long, short = 'p', default_value_t = 57116)]
    port: u16,

    /// Stream Deck / OpenDeck Registration: Plugin UUID
    #[arg(long = "pluginUUID", hide = true)]
    plugin_uuid: Option<String>,

    /// Stream Deck / OpenDeck Registration: Register Event
    #[arg(long = "registerEvent", hide = true)]
    register_event: Option<String>,

    /// Stream Deck / OpenDeck Registration: Info
    #[arg(long = "info", hide = true)]
    info: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Preprocess arguments to handle Stream Deck's single-dash parameters
    let args_iter = std::env::args().map(|arg| match arg.as_str() {
        "-port" => "--port".to_string(),
        "-pluginUUID" => "--pluginUUID".to_string(),
        "-registerEvent" => "--registerEvent".to_string(),
        "-info" => "--info".to_string(),
        _ => arg,
    });

    let args = Args::parse_from(args_iter);

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(&args.log_level))
        .init();

    info!("Starting Ulanzi D200 Rust Driver (MQ-RUST-002)");

    // 1. Load configuration
    let config = match config::Config::load(&args.config) {
        Ok(c) => c,
        Err(e) => {
            if args.plugin_uuid.is_some() || args.daemon {
                warn!(
                    "Configuration file not found or invalid ({}), using defaults: {}",
                    args.config.display(),
                    e
                );
                config::Config::default()
            } else {
                error!("Failed to load configuration: {}", e);
                return Err(e);
            }
        }
    };

    // 2. Determine Communication Mode
    let (plugin_cmd_rx, hw_event_tx, openaction_handle) = if let Some(ref uuid) = args.plugin_uuid {
        // PLUGIN MODE (Client)
        let port = args.port;
        let event = args
            .register_event
            .clone()
            .unwrap_or_else(|| "register".to_string());
        let info_str = args.info.clone().unwrap_or_else(|| "{}".to_string());

        info!("Starting in Plugin Mode (UUID: {}, Port: {})", uuid, port);

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(100);
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(100);

        let bridge = crate::openaction_client::OpenActionBridge::new(cmd_tx);
        bridge.register();

        let oa_args = vec![
            "rs-ulanzi-d200-linux".to_string(),
            "-port".to_string(),
            port.to_string(),
            "-pluginUUID".to_string(),
            uuid.clone(),
            "-registerEvent".to_string(),
            event,
            "-info".to_string(),
            info_str,
        ];

        // Keep handle so we can detect when OpenDeck disconnects
        let oa_handle = tokio::spawn(async move {
            info!("Running OpenAction Runtime...");
            if let Err(e) = openaction::run(oa_args).await {
                error!("OpenAction Runtime Error: {}", e);
            }
            info!("OpenAction Runtime exited");
        });

        // Outbound events forwarder (unchanged)
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                match event {
                    daemon::HardwareEvent::KeyDown {
                        device_id,
                        key_index,
                    } => {
                        let _ = openaction::device_plugin::key_down(device_id, key_index).await;
                    }
                    daemon::HardwareEvent::KeyUp {
                        device_id,
                        key_index,
                    } => {
                        let _ = openaction::device_plugin::key_up(device_id, key_index).await;
                    }
                    daemon::HardwareEvent::DeviceConnected { device_id } => {
                        let _ = openaction::device_plugin::register_device(
                            device_id,
                            "Ulanzi D200".to_string(),
                            3,
                            5,
                            0,
                            0,
                        )
                        .await;
                    }
                }
            }
        });

        (Some(cmd_rx), Some(event_tx), Some(oa_handle))
    } else if args.daemon && args.websocket {
        warn!("Server Mode is currently disabled/unsupported in this version.");
        (None, None, None)
    } else {
        (None, None, None)
    };

    // 3. Start Daemon or One-Shot
    if args.daemon || args.plugin_uuid.is_some() {
        let daemon = daemon::UlanziDaemon::new(config, plugin_cmd_rx, hw_event_tx).await?;

        if let Some(oa_handle) = openaction_handle {
            // Plugin mode: exit when EITHER the daemon or the OpenAction runtime finishes
            tokio::select! {
                res = daemon.run() => {
                    info!("Daemon exited");
                    res?;
                }
                _ = oa_handle => {
                    info!("OpenAction runtime ended; shutting down driver");
                }
            }
        } else {
            // Standalone daemon mode: just run until daemon exits (Ctrl-C, etc)
            daemon.run().await?;
        }
    } else {
        // One‑shot mode: connect, clear screen, apply config, then exit
        let device = device::UlanziDevice::connect().await?;
        info!("Initializing device state...");

        // 1. Clear all buttons (send empty configuration)
        if let Err(e) = device.clear_all_images().await {
            error!("Failed to clear buttons: {}", e);
        }

        // 2. Apply brightness and label style
        if let Err(e) = device.set_brightness(config.brightness).await {
            error!("Failed to set brightness: {}", e);
        }
        let label_style = serde_json::to_value(&config.label_style)?;
        if let Err(e) = device.set_label_style(&label_style).await {
            error!("Failed to set label style: {}", e);
        }

        // 3. Send the button images defined in the config file
        if let Err(e) = device.set_buttons(&config).await {
            error!("Failed to set buttons: {}", e);
        } else {
            info!("Initialization complete.");
        }
    }

    Ok(())
}