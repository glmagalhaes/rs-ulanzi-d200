use anyhow::Result;
use async_hid::AsyncHidRead;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::time::Duration;
use tokio::signal;
use tokio::sync::mpsc;
use tokio::time::interval;

use crate::config::Config;
use crate::device::{ButtonEvent, UlanziDevice};
use crate::openaction_client::BridgeEvent;
use crate::telemetry::SystemMonitor;

#[derive(Debug, Clone)]
pub enum HardwareEvent {
    KeyDown { device_id: String, key_index: u8 },
    KeyUp { device_id: String, key_index: u8 },
    DeviceConnected { device_id: String },
}

pub struct UlanziDaemon {
    devices: HashMap<String, UlanziDevice>,
    config: Config,
    telemetry: SystemMonitor,
    cpu_usage: u8,
    mem_usage: u8,
    // WebSocket communication channels
    plugin_cmd_rx: Option<mpsc::Receiver<BridgeEvent>>,
    hw_event_tx: Option<mpsc::Sender<HardwareEvent>>,
    // Internal channel for device inputs
    device_input_rx: mpsc::Receiver<(String, ButtonEvent)>,
    device_input_tx: mpsc::Sender<(String, ButtonEvent)>,
}

impl UlanziDaemon {
    pub async fn new(
        config: Config,
        plugin_cmd_rx: Option<mpsc::Receiver<BridgeEvent>>,
        hw_event_tx: Option<mpsc::Sender<HardwareEvent>>,
    ) -> Result<Self> {
        let (device_input_tx, device_input_rx) = mpsc::channel(100);

        let mut devices = HashMap::new();

        match UlanziDevice::connect().await {
            Ok(device) => {
                devices.insert(device.get_id().to_string(), device);
            }
            Err(e) => {
                warn!("No devices found at startup: {}", e);
            }
        }

        let telemetry = SystemMonitor::new();

        Ok(Self {
            devices,
            config,
            telemetry,
            cpu_usage: 0,
            mem_usage: 0,
            plugin_cmd_rx,
            hw_event_tx,
            device_input_rx,
            device_input_tx,
        })
    }

    pub async fn run(mut self) -> Result<()> {
        info!("Ulanzi Daemon started");

        // Initial setup for connected devices
        for device in self.devices.values_mut() {
            if let Err(e) = device.set_brightness(self.config.brightness).await {
                error!("Failed to set brightness for {}: {}", device.get_id(), e);
            }
            if let Ok(label_style) = serde_json::to_value(&self.config.label_style) {
                let _ = device.set_label_style(&label_style).await;
            }
            if let Err(e) = device.set_buttons(&self.config).await {
                error!(
                    "Failed to set initial buttons for {}: {}",
                    device.get_id(),
                    e
                );
            }

            // Notify plugins about this device
            if let Some(ref tx) = self.hw_event_tx {
                let event = HardwareEvent::DeviceConnected {
                    device_id: device.get_id().to_string(),
                };
                let _ = tx.send(event).await;
            }

            // Spawn reader task
            if let Some(mut reader) = device.take_reader() {
                let tx = self.device_input_tx.clone();
                let device_id = device.get_id().to_string();
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    loop {
                        match reader.read_input_report(&mut buf).await {
                            Ok(len) => {
                                if len > 0 {
                                    if let Some(event) = UlanziDevice::parse_report(&buf[..len]) {
                                        if let Err(_) = tx.send((device_id.clone(), event)).await {
                                            break; // Receiver dropped
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Device {} read error: {}", device_id, e);
                                break;
                            }
                        }
                    }
                    info!("Reader task finished for {}", device_id);
                });
            }
        }

        let mut keep_alive_interval = interval(Duration::from_millis(100));
        let mut telemetry_interval = interval(Duration::from_millis(self.config.stats_interval_ms));

        // Signal handling for graceful shutdown
        let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())?;
        let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())?;

        loop {
            tokio::select! {
                // Handle Signals
                _ = sigint.recv() => {
                    info!("Received SIGINT, shutting down...");
                    break;
                }
                _ = sigterm.recv() => {
                    info!("Received SIGTERM, shutting down...");
                    break;
                }

                // Handle WebSocket Commands
                Some(cmd) = async {
                    if let Some(rx) = &mut self.plugin_cmd_rx {
                        rx.recv().await
                    } else {
                        std::future::pending::<Option<BridgeEvent>>().await
                    }
                } => {
                    self.handle_plugin_command(cmd).await;
                }

                _ = keep_alive_interval.tick() => {
                     // Update small window data for all devices
                     use chrono::Local;
                     let now = Local::now();
                     let time_str = now.format("%H:%M:%S").to_string();

                    for device in self.devices.values() {
                        if let Err(e) = device.set_small_window_data(
                            self.config.display_mode,
                            self.cpu_usage,
                            self.mem_usage,
                            &time_str,
                            0
                        ).await {
                            debug!("Failed to send keep-alive to {}: {}", device.get_id(), e);
                        }
                    }
                }

                // Handle Device Inputs (from reader tasks)
                Some((device_id, event)) = self.device_input_rx.recv() => {
                    self.handle_device_event(&device_id, event).await;
                }

                _ = telemetry_interval.tick() => {
                    let (cpu, mem) = self.telemetry.get_metrics();
                    self.cpu_usage = cpu;
                    self.mem_usage = mem;
                }
            }
        }

        info!("Shutdown complete.");
        Ok(())
    }

    async fn handle_device_event(&mut self, device_id: &str, event: ButtonEvent) {
        debug!("Button event from {}: {:?}", device_id, event);

        // 1. Notify Plugins
        if let Some(ref tx) = self.hw_event_tx {
            let outbound = if event.pressed {
                HardwareEvent::KeyDown {
                    device_id: device_id.to_string(),
                    key_index: event.index as u8,
                }
            } else {
                HardwareEvent::KeyUp {
                    device_id: device_id.to_string(),
                    key_index: event.index as u8,
                }
            };

            if let Err(e) = tx.send(outbound).await {
                warn!("Failed to broadcast hardware event: {}", e);
            }
        }

        // Local actions removed - entirely handled by OpenAction/OpenDeck
    }

    async fn handle_plugin_command(&mut self, cmd: BridgeEvent) {
        match cmd {
            BridgeEvent::SetImage {
                device_id,
                position,
                image_base64,
            } => {
                let dev = if let Some(d) = self.devices.get_mut(&device_id) {
                    Some(d)
                } else {
                    self.devices.values_mut().next()
                };

                if let Some(dev) = dev {
                    let index = position as usize;
                    debug!("Setting image for button {} on {}", index, dev.get_id());
                    if let Err(e) = dev.set_button_image(index, &image_base64).await {
                        error!("Failed to set image: {}", e);
                    } else {
                        if let Err(e) = dev.flush().await {
                            error!("Failed to flush device {}: {}", dev.get_id(), e);
                        }
                    }
                } else {
                    warn!("SetImage: No target device found for {}", device_id);
                }
            }
            BridgeEvent::ClearImage {
                device_id,
                position,
            } => {
                let dev = if let Some(d) = self.devices.get_mut(&device_id) {
                    Some(d)
                } else {
                    self.devices.values_mut().next()
                };

                if let Some(dev) = dev {
                    let index = position as usize;
                    debug!("Clearing image for button {} on {}", index, dev.get_id());
                    dev.clear_button_image(index);
                    if let Err(e) = dev.flush().await {
                        error!("Failed to flush device {}: {}", dev.get_id(), e);
                    }
                } else {
                    warn!("ClearImage: No target device found for {}", device_id);
                }
            }
            BridgeEvent::SetBrightness {
                device_id,
                brightness,
            } => {
                let dev = if let Some(d) = self.devices.get_mut(&device_id) {
                    Some(d)
                } else {
                    self.devices.values_mut().next()
                };

                if let Some(dev) = dev {
                    if let Err(e) = dev.set_brightness(brightness).await {
                        error!("Failed to set brightness: {}", e);
                    }
                } else {
                    warn!("SetBrightness: No target device found for {}", device_id);
                }
            }
            BridgeEvent::DeviceConnected(_) => {
                // Info only
            }
            BridgeEvent::DeviceDisconnected(_) => {
                // Info only
            }
        }
    }
}
