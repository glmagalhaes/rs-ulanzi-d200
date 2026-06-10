use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Write};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use async_hid::{AsyncHidWrite, DeviceReader, DeviceWriter, HidBackend};
use byteorder::{BigEndian, LittleEndian, WriteBytesExt};
use data_url::DataUrl;
use futures_util::StreamExt;
use log::{debug, info, warn};
use rand::{rngs, RngExt, distr::Alphanumeric};
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

// ---------------------------------------------------------------------------
// Command protocol
// ---------------------------------------------------------------------------

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommandProtocol {
    OutSetButtons = 0x0001,
    OutSetSmallWindowData = 0x0006,
    OutSetButtonIcon = 0x000d,
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
    /// Staged button images keyed by button index. Represents the desired
    /// on-device state; mutated by `set_button_image` / `clear_button_image`.
    button_images: Mutex<HashMap<usize, Vec<u8>>>,
    /// Button indices that changed since the last successful device sync.
    dirty: Mutex<HashSet<usize>>,
    /// Whether a full layout (`OutSetButtons`) has been pushed to the device.
    /// While `false`, the next `flush()` performs a full resync.
    synced: Mutex<bool>,
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
            dirty: Mutex::new(HashSet::new()),
            synced: Mutex::new(false),
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

        let changed = {
            let mut map = self.button_images.lock().unwrap();
            let changed = match map.get(&index) {
                Some(old) => *old != png_data,
                None => true,
            };
            if changed {
                map.insert(index, png_data);
            }
            changed
        };

        if changed {
            self.dirty.lock().unwrap().insert(index);
        }

        Ok(changed)
    }

    /// Remove a staged button image (will be cleared on next `flush()`).
    pub fn clear_button_image(&self, index: usize) {
        if index >= NUM_BUTTONS {
            warn!("Attempt to clear out‑of‑range button index {}", index);
            return;
        }
        let removed = self.button_images.lock().unwrap().remove(&index).is_some();
        if removed {
            self.dirty.lock().unwrap().insert(index);
        }
    }

    /// Remove **all** staged button images and send an empty configuration
    /// to the device (clears all buttons). Forces a full layout resync.
    pub async fn clear_all_images(&self) -> Result<()> {
        self.button_images.lock().unwrap().clear();
        self.dirty.lock().unwrap().clear();
        *self.synced.lock().unwrap() = false;
        self.flush().await
    }

    /// Map a button index to the manifest grid key(s) it occupies.
    ///
    /// The layout is a 5×3 grid addressed as `col_row`. Button 13 is the wide
    /// button, which spans the bottom two right‑hand cells (`3_2` and `4_2`).
    fn index_to_keys(index: usize) -> Vec<String> {
        if index == 13 {
            vec!["3_2".to_string(), "4_2".to_string()]
        } else {
            let col = index % 5;
            let row = index / 5;
            vec![format!("{}_{}", col, row)]
        }
    }

    /// A random `Images/<id>.png` archive path. A fresh name per send forces
    /// the device to reload the icon (it caches by path), mirroring the
    /// official app's per‑transfer UUID filenames.
    fn random_image_path() -> String {
        let name: String = rngs::ThreadRng::default()
            .sample_iter(&Alphanumeric)
            .take(32)
            .map(char::from)
            .collect();
        format!("Images/{}.png", name)
    }

    /// `ViewParam` entry for a button that displays an icon, matching the
    /// official app's manifest format.
    fn icon_view_param(icon_path: &str) -> serde_json::Value {
        json!({
            "Font": {
                "Align": "bottom",
                "Color": 16777215,
                "FontName": "Source Han Sans SC",
                "ShowTitle": true,
                "Size": 10,
                "Weight": 80
            },
            "Icon": icon_path,
            "Text": ""
        })
    }

    /// `ViewParam` entry for an empty (iconless) button.
    fn empty_view_param() -> serde_json::Value {
        json!({ "Font": "", "Text": "" })
    }

    /// Build a clean configuration ZIP in the device's native format:
    /// an `Images/` directory, one `Images/<id>.png` per icon, and a
    /// `manifest.json` mapping grid keys to their view parameters.
    ///
    /// `entries` is a list of `(keys, image)` pairs; all keys in a pair share
    /// the same (single) image when one is present.
    fn build_zip(entries: &[(Vec<String>, Option<Vec<u8>>)]) -> Result<Vec<u8>> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut cursor);
            let deflated =
                FileOptions::<()>::default().compression_method(zip::CompressionMethod::Deflated);

            zip.add_directory("Images", deflated)?;

            let mut manifest = serde_json::Map::new();
            for (keys, image) in entries {
                let view_param = if let Some(png) = image {
                    let path = Self::random_image_path();
                    zip.start_file(&path, deflated)?;
                    zip.write_all(png)?;
                    Self::icon_view_param(&path)
                } else {
                    Self::empty_view_param()
                };
                let entry = json!({ "State": 0, "ViewParam": [view_param] });
                for key in keys {
                    manifest.insert(key.clone(), entry.clone());
                }
            }

            zip.start_file("manifest.json", deflated)?;
            zip.write_all(serde_json::to_string(&serde_json::Value::Object(manifest))?.as_bytes())?;

            zip.finish()?;
        }
        Ok(cursor.into_inner())
    }

    /// Push staged button images to the device using the native protocol.
    ///
    /// The first sync after connecting (or after `clear_all_images`) sends the
    /// complete layout via `OutSetButtons` (`0x0001`). Subsequent changes are
    /// sent as per‑button incremental updates via `OutSetButtonIcon` (`0x000d`),
    /// exactly as the official application does. This keeps each transfer small
    /// and avoids re‑uploading every button on every change.
    pub async fn flush(&self) -> Result<()> {
        let images = {
            let map = self.button_images.lock().unwrap();
            map.clone()
        };

        let need_full = !*self.synced.lock().unwrap();

        if need_full {
            let entries: Vec<(Vec<String>, Option<Vec<u8>>)> = (0..NUM_BUTTONS)
                .map(|id| (Self::index_to_keys(id), images.get(&id).cloned()))
                .collect();

            let zip = Self::build_zip(&entries)?;
            self.send_file(CommandProtocol::OutSetButtons, &zip).await?;

            *self.synced.lock().unwrap() = true;
            self.dirty.lock().unwrap().clear();
            debug!("Sent full button layout ({} bytes)", zip.len());
        } else {
            let dirty: Vec<usize> = {
                let set = self.dirty.lock().unwrap();
                let mut v: Vec<usize> = set.iter().copied().collect();
                v.sort_unstable();
                v
            };

            for id in dirty {
                let entry = (Self::index_to_keys(id), images.get(&id).cloned());
                let zip = Self::build_zip(std::slice::from_ref(&entry))?;
                self.send_file(CommandProtocol::OutSetButtonIcon, &zip).await?;
                // Only clear once the transfer succeeded, so a failure leaves
                // the button dirty for the next flush.
                self.dirty.lock().unwrap().remove(&id);
                debug!("Sent incremental update for button {} ({} bytes)", id, zip.len());
            }
        }

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

    /// Stream a payload to the device as a framed multi‑packet transfer.
    ///
    /// The first packet carries the 8‑byte header (`7c 7c | cmd | total_len`)
    /// followed by up to `MAX_COMMAND_PAYLOAD` payload bytes; every subsequent
    /// packet is a raw 1024‑byte continuation chunk of the remaining payload.
    /// `command` selects the transfer type (e.g. full layout vs. icon update).
    /// The writer lock is held for the whole transfer so no other command can
    /// be interleaved between continuation packets.
    async fn send_file(&self, command: CommandProtocol, data: &[u8]) -> Result<()> {
        let file_size = data.len() as u32;
        debug!(
            "Sending file via command 0x{:04x} ({} bytes)",
            command as u16, file_size
        );

        let first_chunk = if data.len() >= MAX_COMMAND_PAYLOAD {
            &data[..MAX_COMMAND_PAYLOAD]
        } else {
            data
        };
        let first_packet = self.build_packet(command, first_chunk, file_size);

        let mut writer = self.writer.lock().await;
        writer.write_output_report(&first_packet).await?;

        if data.len() > MAX_COMMAND_PAYLOAD {
            for chunk in data[MAX_COMMAND_PAYLOAD..].chunks(PACKET_SIZE) {
                let mut packet = [0u8; PACKET_SIZE];
                packet[..chunk.len()].copy_from_slice(chunk);
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

    #[test]
    fn test_index_to_keys() {
        assert_eq!(UlanziDevice::index_to_keys(0), vec!["0_0"]);
        assert_eq!(UlanziDevice::index_to_keys(4), vec!["4_0"]);
        assert_eq!(UlanziDevice::index_to_keys(5), vec!["0_1"]);
        assert_eq!(UlanziDevice::index_to_keys(12), vec!["2_2"]);
        // Wide button spans the bottom two right‑hand cells.
        assert_eq!(UlanziDevice::index_to_keys(13), vec!["3_2", "4_2"]);
    }

    fn read_zip(bytes: &[u8]) -> zip::ZipArchive<Cursor<Vec<u8>>> {
        zip::ZipArchive::new(Cursor::new(bytes.to_vec())).expect("valid zip")
    }

    fn manifest_of(zip: &mut zip::ZipArchive<Cursor<Vec<u8>>>) -> serde_json::Value {
        use std::io::Read;
        let mut s = String::new();
        zip.by_name("manifest.json")
            .expect("manifest.json present")
            .read_to_string(&mut s)
            .unwrap();
        serde_json::from_str(&s).expect("valid manifest json")
    }

    #[test]
    fn test_incremental_zip_matches_official_format() {
        // A single-button update, as the official app sends via 0x000d.
        let png = b"\x89PNG\r\n\x1a\nFAKEIMAGEDATA".to_vec();
        let entry = (UlanziDevice::index_to_keys(0), Some(png.clone()));
        let bytes = UlanziDevice::build_zip(std::slice::from_ref(&entry)).unwrap();

        let mut zip = read_zip(&bytes);
        let names: Vec<String> = zip.file_names().map(|s| s.to_string()).collect();

        // Native container: an Images/ directory entry + exactly one png + manifest.
        assert!(names.iter().any(|n| n == "Images/"), "missing Images/ dir: {names:?}");
        assert!(names.iter().any(|n| n == "manifest.json"));
        let png_name = names
            .iter()
            .find(|n| n.starts_with("Images/") && n.ends_with(".png"))
            .expect("one Images/<id>.png entry")
            .clone();

        // No leftover hack files from the previous implementation.
        assert!(!names.iter().any(|n| n == "dummy.txt"));
        assert!(!names.iter().any(|n| n == "sentinel.txt"));

        // The stored png bytes round-trip unchanged.
        {
            use std::io::Read;
            let mut data = Vec::new();
            zip.by_name(&png_name).unwrap().read_to_end(&mut data).unwrap();
            assert_eq!(data, png);
        }

        // Manifest references the icon with the official ViewParam shape.
        let manifest = manifest_of(&mut zip);
        let vp = &manifest["0_0"]["ViewParam"][0];
        assert_eq!(vp["Icon"], serde_json::json!(png_name));
        assert_eq!(vp["Text"], serde_json::json!(""));
        assert_eq!(vp["Font"]["FontName"], serde_json::json!("Source Han Sans SC"));
        assert_eq!(manifest["0_0"]["State"], serde_json::json!(0));
    }

    #[test]
    fn test_full_layout_zip_has_all_keys() {
        // Full layout (0x0001): every grid cell present, only button 7 has an icon.
        let png = b"\x89PNGicon".to_vec();
        let mut images: HashMap<usize, Vec<u8>> = HashMap::new();
        images.insert(7, png.clone());

        let entries: Vec<(Vec<String>, Option<Vec<u8>>)> = (0..NUM_BUTTONS)
            .map(|id| (UlanziDevice::index_to_keys(id), images.get(&id).cloned()))
            .collect();
        let bytes = UlanziDevice::build_zip(&entries).unwrap();

        let mut zip = read_zip(&bytes);
        let manifest = manifest_of(&mut zip);
        let obj = manifest.as_object().unwrap();

        // 13 single-cell buttons + the wide button's two cells = 15 grid keys.
        assert_eq!(obj.len(), 15);
        for key in ["0_0", "4_0", "0_1", "2_2", "3_2", "4_2"] {
            assert!(obj.contains_key(key), "missing key {key}");
        }

        // Button 7 -> key 2_1 carries the icon; an empty cell uses Font: "".
        assert!(manifest["2_1"]["ViewParam"][0]["Icon"].is_string());
        assert_eq!(manifest["0_0"]["ViewParam"][0]["Font"], serde_json::json!(""));
        assert!(manifest["0_0"]["ViewParam"][0].get("Icon").is_none());
    }
}