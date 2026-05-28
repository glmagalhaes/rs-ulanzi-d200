use std::collections::HashMap;
use std::io::{Cursor, Write};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{anyhow, Result};
use async_hid::{AsyncHidWrite, DeviceReader, DeviceWriter, HidBackend};
use byteorder::{BigEndian, LittleEndian, WriteBytesExt};
use data_url::DataUrl;
use futures_util::StreamExt;
use log::{debug, info, warn};
use rand::{rngs, RngExt, distr::Alphanumeric};
use rand::seq::SliceRandom;
use serde_json::json;
use tokio::sync::Mutex as TokioMutex;
use zip::write::FileOptions;

use image::{DynamicImage, GenericImageView, RgbImage, RgbaImage};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const VENDOR_ID: u16 = 0x2207;
pub const PRODUCT_ID: u16 = 0x0019;
pub const DEVICE_NAMESPACE: &str = "e9";

const PACKET_SIZE: usize = 1024;
const HEADER: [u8; 2] = [0x7c, 0x7c];
const USAGE_PAGE: u16 = 0x000c;
pub const NUM_BUTTONS: usize = 14;

const MAX_COMMAND_PAYLOAD: usize = PACKET_SIZE - 8; // 1016

static FLUSH_COUNTER: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Command protocol
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
// Button event
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct ButtonEvent {
    pub index: usize,
    pub pressed: bool,
    pub state: u8,
}

// ---------------------------------------------------------------------------
// Device handle
// ---------------------------------------------------------------------------

pub struct UlanziDevice {
    writer: Arc<TokioMutex<DeviceWriter>>,
    reader: Option<DeviceReader>,
    id: String,
    button_images: Mutex<HashMap<usize, Vec<u8>>>,
}

// ---------------------------------------------------------------------------
// Aspect‑ratio‑preserving resize helper
// ---------------------------------------------------------------------------

/// Resize an image to fit inside `size`×`size`, preserving aspect ratio.
/// The padding is transparent if the source has an alpha channel,
/// otherwise opaque black.
fn resize_square(img: &DynamicImage, size: u32) -> DynamicImage {
    let (w, h) = img.dimensions();
    if w == size && h == size {
        return img.clone();
    }

    // Scale factor to fit entirely inside the square
    let scale = (size as f64 / w as f64).min(size as f64 / h as f64);
    let new_w = (w as f64 * scale).round() as u32;
    let new_h = (h as f64 * scale).round() as u32;

    // Resize to the new dimensions (keeps colour type)
    let resized = img.resize(new_w, new_h, image::imageops::FilterType::Triangle);

    // Determine if the source had alpha (RGBA8, LA8, etc.)
    let has_alpha = matches!(
        img.color(),
        image::ColorType::Rgba8 | image::ColorType::La8 | image::ColorType::Rgba16 | image::ColorType::La16
    );

    let x = ((size - new_w) / 2) as i64;
    let y = ((size - new_h) / 2) as i64;

    if has_alpha {
        // Transparent canvas
        let mut canvas = RgbaImage::from_pixel(size, size, image::Rgba([0, 0, 0, 0]));
        image::imageops::overlay(&mut canvas, &resized.to_rgba8(), x, y);
        DynamicImage::ImageRgba8(canvas)
    } else {
        // Opaque black canvas
        let mut canvas = RgbImage::from_pixel(size, size, image::Rgb([0, 0, 0]));
        let rgb = resized.to_rgb8();
        image::imageops::overlay(&mut canvas, &rgb, x, y);
        DynamicImage::ImageRgb8(canvas)
    }
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
        if buf.len() < 12 || buf[0..2] != HEADER {
            return None;
        }

        let command = u16::from_be_bytes([buf[2], buf[3]]);
        if command != CommandProtocol::InButton as u16
            && command != CommandProtocol::InButton2 as u16
        {
            return None;
        }

        let index = buf[9] as usize;
        if index >= NUM_BUTTONS {
            warn!("Received button event with out-of-range index {}", index);
            return None;
        }

        Some(ButtonEvent {
            state: buf[8],
            index,
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



    /// Stage a button image from a data URL (Base64) or a file path.
    /// Returns `Ok(true)` if the image was new/different, `Ok(false)` if unchanged.
    /// Call `flush()` to apply all staged images.
    pub async fn set_button_image(&self, index: usize, image_data: &str) -> Result<bool> {
        if index >= NUM_BUTTONS {
            return Err(anyhow!(
                "Button index {} out of range (0..{})",
                index,
                NUM_BUTTONS - 1
            ));
        }

        let png_data = if image_data.starts_with("data:") {
            // Data URL (Base64)
            let url = DataUrl::process(image_data).map_err(|_| anyhow!("Invalid data URL"))?;
            let (body, _) = url
                .decode_to_vec()
                .map_err(|_| anyhow!("Failed to decode data URL"))?;
            let img = image::load_from_memory(&body)?;
            let resized = resize_square(&img, 196);
            let mut buf = Vec::new();
            {
                let mut cursor = Cursor::new(&mut buf);
                resized.write_to(&mut cursor, image::ImageFormat::Png)?;
            }
            buf
        } else {
            // File path
            let path = std::path::Path::new(image_data);
            if !path.exists() {
                return Err(anyhow!("Image file not found: {}", image_data));
            }
            let img = image::open(path)
                .map_err(|e| anyhow!("Failed to open image {}: {}", image_data, e))?;
            let resized = resize_square(&img, 196);
            let mut buf = Vec::new();
            {
                let mut cursor = Cursor::new(&mut buf);
                resized.write_to(&mut cursor, image::ImageFormat::Png)?;
            }
            buf
        };

        let mut map = self.button_images.lock().unwrap();
        let changed = match map.get(&index) {
            Some(old) => *old != png_data,
            None => true,
        };

        if changed {
            map.insert(index, png_data);
        }

        Ok(changed)
    }

    /// Remove a staged button image (will be cleared on next `flush()`).
    pub fn clear_button_image(&self, index: usize) {
        if index >= NUM_BUTTONS {
            warn!("Attempt to clear out‑of‑range button index {}", index);
            return;
        }
        self.button_images.lock().unwrap().remove(&index);
    }

    /// Remove **all** staged button images and send an empty configuration
    /// to the device (clears all buttons).
    pub async fn clear_all_images(&self) -> Result<()> {
        self.button_images.lock().unwrap().clear();
        self.flush().await
    }

    /// Send the currently staged button images to the device.
    /// Uses unique filenames per flush to force device to reload icons.
    /// Bounded retries – returns error if a valid ZIP cannot be built.
    pub async fn flush(&self) -> Result<()> {
        debug!("Building button configuration ZIP with bug workaround");

        let images_snapshot = {
            let map = self.button_images.lock().unwrap();
            map.clone()
        };

        const INVALID_BYTES: [u8; 2] = [0x00, 0x7c];
        const MAX_RETRIES: usize = 1000;

        let mut dummy_retries = 0;
        let mut zip_data = Vec::new();

        loop {
            let flush_id = FLUSH_COUNTER.fetch_add(1, Ordering::Relaxed);
            zip_data.clear();
            let mut cursor = Cursor::new(Vec::new());
            {
                let mut zip = zip::ZipWriter::new(&mut cursor);
                let stored = FileOptions::<()>::default()
                    .compression_method(zip::CompressionMethod::Stored);
                let deflated = FileOptions::<()>::default()
                    .compression_method(zip::CompressionMethod::Deflated);

                // Dummy file – content grows aggressively
                // let dummy_content = "x".repeat(128 * dummy_retries);
                let dummy_content: String = rngs::ThreadRng::default()
                    .sample_iter(&Alphanumeric)
                    .take(237 * dummy_retries)
                    .map(char::from)
                    .collect();

                zip.start_file("dummy.txt", deflated)?;
                zip.write_all(dummy_content.as_bytes())?;

                let mut manifest = json!({});

                let mut numbers: Vec<usize> = (0..NUM_BUTTONS).collect();
                numbers.shuffle(&mut rngs::ThreadRng::default());
                for i in numbers {
                    if i == 13 {
                        let key1 = "3_2";
                        let key2 = "4_2";
                        let mut view_param = json!({ "Text": "" });

                        if let Some(img_data) = images_snapshot.get(&13) {
                            let icon_name = format!("{}_{}.png", i, flush_id);
                            zip.start_file(format!("icons/{}", icon_name), deflated)?;
                            zip.write_all(img_data)?;
                            view_param["Icon"] = json!(format!("icons/{}", icon_name));
                        } else {
                            view_param["Icon"] = json!("");
                        }

                        let entry = json!({ "State": 0, "ViewParam": [view_param] });
                        manifest[key1] = entry.clone();
                        manifest[key2] = entry;
                    } else {
                        let col = i % 5;
                        let row = i / 5;
                        let key = format!("{}_{}", col, row);
                        let mut view_param = json!({ "Text": "" });

                        if let Some(img_data) = images_snapshot.get(&i) {
                            let icon_name = format!("{}_{}.png", i, flush_id);
                            zip.start_file(format!("icons/{}", icon_name), deflated)?;
                            zip.write_all(img_data)?;
                            view_param["Icon"] = json!(format!("icons/{}", icon_name));
                        } else {
                            view_param["Icon"] = json!("");
                        }

                        manifest[key] = json!({ "State": 0, "ViewParam": [view_param] });
                    }
                }

                zip.start_file("manifest.json", deflated)?;
                zip.write_all(serde_json::to_string(&manifest)?.as_bytes())?;

                zip.start_file("sentinel.txt", deflated)?;
                zip.write_all(b"")?;

                zip.finish()?;
            }

            zip_data = cursor.into_inner();

            let file_size = zip_data.len();
            let mut valid = true;
            for offset in (1016..file_size).step_by(1024) {
                if let Some(&byte) = zip_data.get(offset) {
                    if INVALID_BYTES.contains(&byte) {
                        debug!(
                            "Invalid byte 0x{:02x} at offset {} (retry {})",
                            byte, offset, dummy_retries
                        );
                        valid = false;
                        break;
                    }
                }
            }

            if valid {
                debug!("ZIP archive passed the byte‑offset check ({} retries)", dummy_retries);
                break;
            }

            dummy_retries += 1;
            if dummy_retries >= MAX_RETRIES {
                return Err(anyhow!(
                    "Failed to build a valid ZIP after {} retries – giving up",
                    MAX_RETRIES
                ));
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }

        self.send_file(&zip_data).await?;
        debug!("Successfully sent button configuration ({} bytes)", zip_data.len());
        Ok(())
    }

    // -- Low‑level packet I/O -----------------------------------------------

    async fn send_command(&self, command: CommandProtocol, payload: &[u8]) -> Result<()> {
        if payload.len() > MAX_COMMAND_PAYLOAD {
            return Err(anyhow!(
                "Command payload too large: {} bytes (max {})",
                payload.len(),
                MAX_COMMAND_PAYLOAD
            ));
        }
        let packet = self.build_packet(command, payload, payload.len() as u32);
        self.writer.lock().await.write_output_report(&packet).await?;
        Ok(())
    }

    async fn send_file(&self, data: &[u8]) -> Result<()> {
        let file_size = data.len() as u32;
        debug!("Sending icon data! ({} bytes)", file_size);
        let first_chunk = if data.len() >= 1016 {
            &data[..1016]
        } else {
            data
        };
        let first_packet =
            self.build_packet(CommandProtocol::OutSetButtons, first_chunk, file_size);

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

        // Device expects total_length as little‑endian (working protocol).
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