use std::collections::HashMap;
use std::io::{Cursor, Write};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use async_hid::{AsyncHidWrite, DeviceReader, DeviceWriter, HidBackend};
use byteorder::{BigEndian, LittleEndian, WriteBytesExt};
use data_url::DataUrl;
use futures_util::StreamExt;
use log::{debug, info};
use serde_json::json;
use tokio::sync::Mutex as TokioMutex;
use zip::write::FileOptions;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const VENDOR_ID: u16 = 0x2207;
pub const PRODUCT_ID: u16 = 0x0019;
pub const DEVICE_NAMESPACE: &str = "e9";

const PACKET_SIZE: usize = 1024;
const HEADER: [u8; 2] = [0x7c, 0x7c];
const USAGE_PAGE: u16 = 0x000c;
const NUM_BUTTONS: usize = 14;

// ---------------------------------------------------------------------------
// Command protocol (device‑specific)
// ---------------------------------------------------------------------------

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommandProtocol {
    OutSetButtons = 0x0001,
    OutSetSmallWindowData = 0x0006,
    OutSetBrightness = 0x000a,
    OutSetLabelStyle = 0x000b,
    InButton = 0x0101,
    InButton2 = 0x0102,
}

// ---------------------------------------------------------------------------
// Event from physical button
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct ButtonEvent {
    pub index: usize,
    pub pressed: bool,
    pub state: u8,
}

// ---------------------------------------------------------------------------
// Device handle
// ---------------------------------------------------------------------------

/// A connection to an Ulanzi D200 macro‑pad.
/// Button images are stored in a `Mutex` and persisted across `flush()` calls.
pub struct UlanziDevice {
    writer: Arc<TokioMutex<DeviceWriter>>,
    reader: Option<DeviceReader>,
    id: String,
    /// PNG data for each button index (196x196, encoded). Never cleared by `flush()`.
    button_images: Mutex<HashMap<usize, Vec<u8>>>,
}

impl UlanziDevice {
    // -- Construction -------------------------------------------------------

    pub async fn connect() -> Result<Self> {
        let backend = HidBackend::default();
        let devices: Vec<_> = backend.enumerate().await?.collect().await;

        let device_info = devices
            .into_iter()
            .find(|d| {
                d.vendor_id == VENDOR_ID
                    && d.product_id == PRODUCT_ID
                    && d.usage_page == USAGE_PAGE
            })
            .ok_or_else(|| anyhow!("Ulanzi D200 device not found"))?;

        let (reader, writer) = device_info.open().await?;

        let id = Self::generate_id(
            device_info.serial_number.as_deref(),
            &format!("{:?}", device_info.id),
        );

        info!("Connected to Ulanzi D200 (ID: {})", id);

        Ok(Self {
            writer: Arc::new(TokioMutex::new(writer)),
            reader: Some(reader),
            id,
            button_images: Mutex::new(HashMap::new()),
        })
    }

    fn generate_id(serial: Option<&str>, fallback: &str) -> String {
        match serial {
            Some(s) => format!("{}-{}", DEVICE_NAMESPACE, s),
            None => format!("{}-{}", DEVICE_NAMESPACE, fallback),
        }
    }

    // -- Accessors ----------------------------------------------------------

    pub fn get_id(&self) -> &str {
        &self.id
    }

    pub fn take_reader(&mut self) -> Option<DeviceReader> {
        self.reader.take()
    }

    // -- Report parsing -----------------------------------------------------

    pub fn parse_report(buf: &[u8]) -> Option<ButtonEvent> {
        if buf.len() < 12 || &buf[0..2] != &HEADER {
            return None;
        }

        let command = u16::from_be_bytes([buf[2], buf[3]]);
        if command != CommandProtocol::InButton as u16
            && command != CommandProtocol::InButton2 as u16
        {
            return None;
        }

        Some(ButtonEvent {
            state: buf[8],
            index: buf[9] as usize,
            pressed: buf[11] == 0x01,
        })
    }

    // -- High‑level commands ------------------------------------------------

    pub async fn set_small_window_data(
        &self,
        mode: u8,
        cpu: u8,
        mem: u8,
        time_str: &str,
        gpu: u8,
    ) -> Result<()> {
        let payload = format!("{}|{}|{}|{}|{}", mode, cpu, mem, time_str, gpu).into_bytes();
        self.send_command(CommandProtocol::OutSetSmallWindowData, &payload)
            .await
    }

    pub async fn set_brightness(&self, brightness: u8) -> Result<()> {
        let brightness = brightness.min(100);
        let payload = brightness.to_string().into_bytes();
        self.send_command(CommandProtocol::OutSetBrightness, &payload)
            .await?;
        debug!("Set brightness to {}%", brightness);
        Ok(())
    }

    pub async fn set_label_style(&self, style: &serde_json::Value) -> Result<()> {
        let payload = serde_json::to_vec(style)?;
        self.send_command(CommandProtocol::OutSetLabelStyle, &payload)
            .await?;
        debug!("Set label style");
        Ok(())
    }


    /// Replace **all** button images from a configuration, then send to device immediately.
    pub async fn set_buttons(&self, config: &crate::config::Config) -> Result<()> {
        let mut new_images = HashMap::new();
        for button in &config.buttons {
            if let Some(ref img_path) = button.image {
                let path = std::path::Path::new(img_path);
                if path.exists() {
                    if let Ok(img) = image::open(path) {
                        let resized = img.resize_exact(196, 196, image::imageops::FilterType::Triangle);
                        let mut png_data = Vec::new();
                        {
                            let mut cursor = std::io::Cursor::new(&mut png_data);
                            resized.write_to(&mut cursor, image::ImageFormat::Png)?;
                        }
                        new_images.insert(button.index, png_data);
                    }
                }
            }
        }
        *self.button_images.lock().unwrap() = new_images;
        self.flush().await
    }

    /// Stage a button image from a data URL (Base64). Does **not** send to device.
    /// Call `flush()` to apply all staged images.
    pub async fn set_button_image(&self, index: usize, image_data: &str) -> Result<()> {
        let url = DataUrl::process(image_data).map_err(|_| anyhow!("Invalid data URL"))?;
        let (body, _) = url
            .decode_to_vec()
            .map_err(|_| anyhow!("Failed to decode data URL"))?;

        let img = image::load_from_memory(&body)?;
        let resized = img.resize_exact(196, 196, image::imageops::FilterType::Triangle);

        let mut png_data = Vec::new();
        {
            let mut cursor = Cursor::new(&mut png_data);
            resized.write_to(&mut cursor, image::ImageFormat::Png)?;
        }

        self.button_images
            .lock()
            .unwrap()
            .insert(index, png_data);
        Ok(())
    }

    /// Remove a staged button image (will be cleared on next `flush()`).
    pub fn clear_button_image(&self, index: usize) {
        self.button_images.lock().unwrap().remove(&index);
    }

    /// Remove **all** staged button images and send an empty configuration
    /// to the device (clears all buttons).
    pub async fn clear_all_images(&self) -> Result<()> {
        self.button_images.lock().unwrap().clear();
        self.flush().await
    }

    /// Send the currently staged button images to the device.
    /// The staged images are **not** cleared; they remain in the map.
    pub async fn flush(&self) -> Result<()> {
        // Take a snapshot (clone) – the map is not modified
        let images_snapshot = {
            let map = self.button_images.lock().unwrap();
            map.clone()
        };

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut buf));
            let stored: FileOptions<()> = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
            let deflated: FileOptions<()> = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

            // 1. dummy.txt – shifts offsets away from 1024-byte boundaries
            zip.start_file("dummy.txt", stored)?;
            zip.write_all(b"")?;

            // 2. Icons and manifest
            let mut manifest = json!({});

            for i in 0..NUM_BUTTONS {
                let col = i % 5;
                let row = i / 5;
                let key = format!("{}_{}", col, row);
                let mut view_param = json!({ "Text": "" });

                if let Some(img_data) = images_snapshot.get(&i) {
                    let icon_name = format!("{}.png", i);
                    zip.start_file(format!("icons/{}", icon_name), deflated)?;
                    zip.write_all(img_data)?;
                    view_param["Icon"] = json!(format!("icons/{}", icon_name));
                } else {
                    view_param["Icon"] = json!("");
                }

                manifest[key] = json!({ "State": 0, "ViewParam": [view_param] });
            }

            zip.start_file("manifest.json", deflated)?;
            zip.write_all(serde_json::to_string(&manifest)?.as_bytes())?;

            // 3. sentinel.txt – absorbs firmware's final‑entry drop
            zip.start_file("sentinel.txt", stored)?;
            zip.write_all(b"")?;

            zip.finish()?;
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        self.send_file(&buf).await?;
        info!("Successfully sent button configuration ({} bytes)", buf.len());
        Ok(())
    }

    // -- Low‑level packet I/O -----------------------------------------------

    async fn send_command(&self, command: CommandProtocol, payload: &[u8]) -> Result<()> {
        let packet = self.build_packet(command, payload, payload.len() as u32);
        self.writer.lock().await.write_output_report(&packet).await?;
        Ok(())
    }

    async fn send_file(&self, data: &[u8]) -> Result<()> {
        let file_size = data.len() as u32;
        let first_chunk = if data.len() >= 1016 {
            &data[..1016]
        } else {
            data
        };
        let first_packet = self.build_packet(CommandProtocol::OutSetButtons, first_chunk, file_size);

        let mut writer = self.writer.lock().await;
        writer.write_output_report(&first_packet).await?;

        if data.len() > 1016 {
            for chunk in data[1016..].chunks(1024) {
                let mut packet = [0u8; PACKET_SIZE];
                let len = chunk.len().min(PACKET_SIZE);
                packet[..len].copy_from_slice(&chunk[..len]);
                writer.write_output_report(&packet).await?;
            }
        }
        Ok(())
    }

    fn build_packet(&self, command: CommandProtocol, data: &[u8], total_length: u32) -> Vec<u8> {
        let mut packet = Vec::with_capacity(PACKET_SIZE);
        packet.extend_from_slice(&HEADER);

        let mut cmd_buf = [0u8; 2];
        (&mut cmd_buf[..])
            .write_u16::<BigEndian>(command as u16)
            .unwrap();
        packet.extend_from_slice(&cmd_buf);

        let mut len_buf = [0u8; 4];
        (&mut len_buf[..])
            .write_u32::<LittleEndian>(total_length)
            .unwrap();
        packet.extend_from_slice(&len_buf);

        let data_len = data.len().min(PACKET_SIZE - 8);
        packet.extend_from_slice(&data[..data_len]);

        if packet.len() < PACKET_SIZE {
            packet.resize(PACKET_SIZE, 0);
        }
        packet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_id_with_serial() {
        let id = UlanziDevice::generate_id(Some("1234567890"), "fallback");
        assert_eq!(id, "e9-1234567890");
    }

    #[test]
    fn test_generate_id_without_serial() {
        let id = UlanziDevice::generate_id(None, "fallback");
        assert_eq!(id, "e9-fallback");
    }
}