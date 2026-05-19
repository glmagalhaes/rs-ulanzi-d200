use anyhow::Result;
use async_hid::AsyncHidRead;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::signal;
use tokio::sync::mpsc;
use tokio::time::{interval, sleep_until};

use crate::config::Config;
use crate::config::WindowMode;
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
    gpu_usage: u8,
    plugin_cmd_rx: Option<mpsc::Receiver<BridgeEvent>>,
    hw_event_tx: Option<mpsc::Sender<HardwareEvent>>,
    device_input_rx: mpsc::Receiver<(String, ButtonEvent)>,
    device_input_tx: mpsc::Sender<(String, ButtonEvent)>,
    // Debouncing & rate limiting
    flush_deadline: Option<Instant>,
    last_flush_time: Option<Instant>,
    debounce_delay: Duration,
    min_flush_interval: Duration,
    // Cycle command channel
    cycle_rx: mpsc::Receiver<()>,
}

impl UlanziDaemon {
    pub async fn new(
        config: Config,
        plugin_cmd_rx: Option<mpsc::Receiver<BridgeEvent>>,
        hw_event_tx: Option<mpsc::Sender<HardwareEvent>>,
        cycle_rx: mpsc::Receiver<()>,
    ) -> Result<Self> {
        let (device_input_tx, device_input_rx) = mpsc::channel(100);
        let mut devices = HashMap::new();

        // Try to connect to the first available device
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
            gpu_usage: 0,
            plugin_cmd_rx,
            hw_event_tx,
            device_input_rx,
            device_input_tx,
            flush_deadline: None,
            last_flush_time: None,
            debounce_delay: Duration::from_millis(50),
            min_flush_interval: Duration::from_millis(20),
            cycle_rx,
        })
    }

    pub async fn run(mut self) -> Result<()> {
        info!("Ulanzi Daemon started (debounced flush)");

        // --- Initial device setup for all connected devices ---
        for device in self.devices.values_mut() {
            // 1. Clear the screen (all 14 buttons empty)
            if let Err(e) = device.clear_all_images().await {
                error!("Failed to clear buttons for {}: {}", device.get_id(), e);
            }

            // 2. Apply brightness and label style from config
            if let Err(e) = device.set_brightness(self.config.brightness).await {
                error!("Failed to set brightness for {}: {}", device.get_id(), e);
            }
            if let Ok(label_style) = serde_json::to_value(&self.config.label_style) {
                let _ = device.set_label_style(&label_style).await;
            }

            // 3. Start the small‑window data with zeros
            let _ = device
                .set_small_window_data(self.config.display_mode, 0, 0, "", 0)
                .await;

            // 4. Notify plugins that a device is connected
            if let Some(ref tx) = self.hw_event_tx {
                let _ = tx
                    .send(HardwareEvent::DeviceConnected {
                        device_id: device.get_id().to_string(),
                    })
                    .await;
            }

            // 5. Spawn reader task for button events
            if let Some(mut reader) = device.take_reader() {
                let tx = self.device_input_tx.clone();
                let device_id = device.get_id().to_string();
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    loop {
                        match reader.read_input_report(&mut buf).await {
                            Ok(len) if len > 0 => {
                                if let Some(event) = UlanziDevice::parse_report(&buf[..len]) {
                                    if tx.send((device_id.clone(), event)).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            Ok(_) => continue,
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

        // --- Drain any initial plugin commands that arrived before the main loop ---
        let mut initial_commands = Vec::new();
        if let Some(rx) = &mut self.plugin_cmd_rx {
            while let Ok(cmd) = rx.try_recv() {
                initial_commands.push(cmd);
            }
        }
        if !initial_commands.is_empty() {
            info!(
                "Processing {} initial plugin commands",
                initial_commands.len()
            );
            for cmd in initial_commands {
                self.handle_plugin_command(cmd).await;
            }
            // Schedule a flush after the initial batch
            self.schedule_flush();
        }

        // --- Timers and shutdown signal ---
        let mut keep_alive_interval = interval(Duration::from_millis(100));
        let mut telemetry_interval =
            interval(Duration::from_millis(self.config.stats_interval_ms));

        let shutdown = async {
            #[cfg(unix)]
            {
                let mut sigint =
                    signal::unix::signal(signal::unix::SignalKind::interrupt()).unwrap();
                let mut sigterm =
                    signal::unix::signal(signal::unix::SignalKind::terminate()).unwrap();
                tokio::select! {
                    _ = sigint.recv() => info!("Received SIGINT, shutting down..."),
                    _ = sigterm.recv() => info!("Received SIGTERM, shutting down..."),
                }
            }
            #[cfg(windows)]
            {
                let _ = signal::ctrl_c().await;
                info!("Received Ctrl-C, shutting down...");
            }
        };
        tokio::pin!(shutdown);

        // --- Main event loop ---
        loop {
            // Copy the current deadline (if any) so the future doesn't borrow self.
            let deadline = self.flush_deadline;

            tokio::select! {
                _ = &mut shutdown => break,

                // Handle WebSocket commands from OpenDeck plugin
                Some(cmd) = async {
                    if let Some(rx) = &mut self.plugin_cmd_rx {
                        rx.recv().await
                    } else {
                        std::future::pending::<Option<BridgeEvent>>().await
                    }
                } => {
                    // Collect all pending commands
                    let mut commands = vec![cmd];
                    if let Some(rx) = &mut self.plugin_cmd_rx {
                        while let Ok(c) = rx.try_recv() {
                            commands.push(c);
                        }
                    }

                    for cmd in commands {
                        self.handle_plugin_command(cmd).await;
                    }
                    self.schedule_flush();
                }

                // Flush deadline reached (debounced)
                _ = async move {
                    if let Some(d) = deadline {
                        sleep_until(d.into()).await;
                    } else {
                        std::future::pending::<()>().await;
                    }
                }, if deadline.is_some() => {
                    self.perform_flush().await;
                }

                // Keep‑alive: update small window with current stats
                _ = keep_alive_interval.tick() => {
                    use chrono::Local;
                    let now = Local::now();
                    let time_str = now.format("%H:%M:%S").to_string();
                    for device in self.devices.values() {
                        if let Err(e) = device.set_small_window_data(
                            self.config.display_mode,
                            self.cpu_usage,
                            self.mem_usage,
                            &time_str,
                            self.gpu_usage,
                        ).await {
                            debug!("Failed to send keep-alive to {}: {}", device.get_id(), e);
                        }
                    }
                }

                // Forward hardware button events to plugins
                Some((device_id, event)) = self.device_input_rx.recv() => {
                    self.handle_device_event(&device_id, event).await;
                }

                // Update telemetry every `stats_interval_ms`
                _ = telemetry_interval.tick() => {
                    let (cpu, mem, gpu) = self.telemetry.get_metrics();
                    self.cpu_usage = cpu;
                    self.mem_usage = mem;
                    self.gpu_usage = gpu;
                }

                // Handle cycle command from action
                Some(()) = self.cycle_rx.recv() => {
                    self.cycle_small_window().await;
                }
            }
        }

        info!("Shutdown complete.");
        Ok(())
    }

    /// Schedule a flush after the debounce delay, respecting the minimum interval
    /// since the last flush.
    fn schedule_flush(&mut self) {
        let now = Instant::now();
        let mut deadline = now + self.debounce_delay;
        if let Some(last) = self.last_flush_time {
            let earliest = last + self.min_flush_interval;
            if deadline < earliest {
                deadline = earliest;
            }
        }
        self.flush_deadline = Some(deadline);
    }

    /// Perform the actual flush (send all staged button images to the device).
    /// Uses a timeout to avoid hanging, and logs errors without crashing.
    async fn perform_flush(&mut self) {
        self.flush_deadline = None;

        // Wrap the flush operation in a timeout.
        let flush_future = async {
            for device in self.devices.values() {
                if let Err(e) = device.flush().await {
                    info!("Failed to flush device {}: {}", device.get_id(), e);
                    // Continue with other devices (if any) – don't break.
                }
            }
            self.last_flush_time = Some(Instant::now());
            debug!("Flush completed (or attempted)");
        };

        match tokio::time::timeout(Duration::from_secs(2), flush_future).await {
            Ok(_) => {
                // Flush finished within timeout (success or logged error).
            }
            Err(_) => {
                info!("Flush timed out after 2 seconds – device may be stuck");
                // Still update last_flush_time to avoid immediate retry storms.
                self.last_flush_time = Some(Instant::now());
            }
        }
    }

    async fn handle_device_event(&mut self, device_id: &str, event: ButtonEvent) {
        debug!("Button event from {}: {:?}", device_id, event);
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
                    match dev.set_button_image(index, &image_base64).await {
                        Ok(true) => {
                            debug!(
                                "SetImage: device={} position={} image_len={} (staged)",
                                dev.get_id(),
                                index,
                                image_base64.len()
                            );
                        }
                        Ok(false) => {
                            debug!("Image unchanged for button {}, skipping", index);
                        }
                        Err(e) => error!("Failed to set image: {}", e),
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
                    debug!(
                        "ClearImage: device={} position={} (staged)",
                        dev.get_id(),
                        index
                    );
                    dev.clear_button_image(index);
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
            BridgeEvent::DeviceConnected(_) | BridgeEvent::DeviceDisconnected(_) => {}
        }
    }

    async fn cycle_small_window(&mut self) {
        let current = self.config.display_mode;
        let new_mode = match current {
            0 => 1, // Status -> Clock
            1 => 2, // Clock -> Clear
            2 => 0, // Clear -> Status
            _ => 0,
        };
        self.config.display_mode = new_mode;
        info!("Small‑window mode cycled: {:?} -> {:?}", current, new_mode);

        // Convert to u8 for device command
        let mode_byte = new_mode as u8;
        let now = chrono::Local::now();
        let time_str = now.format("%H:%M:%S").to_string();
        for device in self.devices.values() {
            if let Err(e) = device
                .set_small_window_data(
                    mode_byte,
                    self.cpu_usage,
                    self.mem_usage,
                    &time_str,
                    self.gpu_usage,
                )
                .await
            {
                warn!("Failed to update small window after mode cycle: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_handle_device_event_keydown() {
        let (hw_event_tx, mut hw_event_rx) = mpsc::channel(1);
        let config = Config::default();
        let (_device_input_tx, device_input_rx) = mpsc::channel(1);
        let (device_input_tx_internal, _device_input_rx_internal) = mpsc::channel(1);
        let (_cycle_tx, cycle_rx) = mpsc::channel(1);

        let mut daemon = UlanziDaemon {
            devices: HashMap::new(),
            config,
            telemetry: SystemMonitor::new(),
            cpu_usage: 0,
            mem_usage: 0,
            gpu_usage: 0,
            plugin_cmd_rx: None,
            hw_event_tx: Some(hw_event_tx),
            device_input_rx,
            device_input_tx: device_input_tx_internal,
            flush_deadline: None,
            last_flush_time: None,
            debounce_delay: Duration::from_millis(50),
            min_flush_interval: Duration::from_millis(20),
            cycle_rx,
        };

        let event = ButtonEvent {
            index: 5,
            pressed: true,
            state: 1,
        };
        daemon.handle_device_event("test_device", event).await;

        let received = hw_event_rx.recv().await.unwrap();
        match received {
            HardwareEvent::KeyDown { device_id, key_index } => {
                assert_eq!(device_id, "test_device");
                assert_eq!(key_index, 5);
            }
            _ => panic!("Expected KeyDown event"),
        }
    }

    #[tokio::test]
    async fn test_handle_device_event_keyup() {
        let (hw_event_tx, mut hw_event_rx) = mpsc::channel(1);
        let config = Config::default();
        let (_device_input_tx, device_input_rx) = mpsc::channel(1);
        let (device_input_tx_internal, _device_input_rx_internal) = mpsc::channel(1);
        let (_cycle_tx, cycle_rx) = mpsc::channel(1);

        let mut daemon = UlanziDaemon {
            devices: HashMap::new(),
            config,
            telemetry: SystemMonitor::new(),
            cpu_usage: 0,
            mem_usage: 0,
            gpu_usage: 0,
            plugin_cmd_rx: None,
            hw_event_tx: Some(hw_event_tx),
            device_input_rx,
            device_input_tx: device_input_tx_internal,
            flush_deadline: None,
            last_flush_time: None,
            debounce_delay: Duration::from_millis(50),
            min_flush_interval: Duration::from_millis(20),
            cycle_rx,
        };

        let event = ButtonEvent {
            index: 3,
            pressed: false,
            state: 0,
        };
        daemon.handle_device_event("test_device", event).await;

        let received = hw_event_rx.recv().await.unwrap();
        match received {
            HardwareEvent::KeyUp { device_id, key_index } => {
                assert_eq!(device_id, "test_device");
                assert_eq!(key_index, 3);
            }
            _ => panic!("Expected KeyUp event"),
        }
    }
}