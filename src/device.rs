use anyhow::{Result, anyhow};
use async_hid::{AsyncHidWrite, DeviceReader, DeviceWriter, HidBackend};
use byteorder::{BigEndian, LittleEndian, WriteBytesExt};
use data_url::DataUrl;
use futures_util::StreamExt;
use log::{debug, info};
use rand::RngExt;
use serde_json::json;
use std::collections::HashMap;
use std::io::{Cursor, Write};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use zip::write::FileOptions;

pub const VENDOR_ID: u16 = 0x2207;
pub const PRODUCT_ID: u16 = 0x0019;
pub const DEVICE_NAMESPACE: &str = "e9";

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

#[derive(Debug, Clone, Copy)]
pub struct ButtonEvent {
    pub index: usize,
    pub pressed: bool,
    pub state: u8,
}

pub struct UlanziDevice {
    writer: Arc<Mutex<DeviceWriter>>,
    reader: Option<DeviceReader>,
    id: String,
    button_images: HashMap<usize, Vec<u8>>,
}

impl UlanziDevice {
    const PACKET_SIZE: usize = 1024;
    const HEADER: [u8; 2] = [0x7c, 0x7c];
    const USAGE_PAGE: u16 = 0x000c;

    /// Connects to the first available Ulanzi D200 device.
    pub async fn connect() -> Result<Self> {
        let backend = HidBackend::default();
        let devices: Vec<_> = backend.enumerate().await?.collect().await;

        let device_info = devices
            .into_iter()
            .find(|d| {
                d.vendor_id == VENDOR_ID
                    && d.product_id == PRODUCT_ID
                    && d.usage_page == Self::USAGE_PAGE
            })
            .ok_or_else(|| anyhow!("Ulanzi D200 device not found"))?;

        let (reader, writer) = device_info.open().await?;

        let id = Self::generate_id(
            device_info.serial_number.as_deref(),
            &format!("{:?}", device_info.id),
        );

        info!("Connected to Ulanzi D200 (ID: {})", id);

        Ok(Self {
            writer: Arc::new(Mutex::new(writer)),
            reader: Some(reader),
            id,
            button_images: HashMap::new(),
        })
    }

    fn generate_id(serial: Option<&str>, fallback_id: &str) -> String {
        if let Some(s) = serial {
            format!("{}-{}", DEVICE_NAMESPACE, s)
        } else {
            format!("{}-{}", DEVICE_NAMESPACE, fallback_id)
        }
    }

    pub fn get_id(&self) -> &str {
        &self.id
    }

    pub fn take_reader(&mut self) -> Option<DeviceReader> {
        self.reader.take()
    }
    // ...

    pub fn parse_report(buf: &[u8]) -> Option<ButtonEvent> {
        if buf.len() < 12 {
            return None;
        }

        if &buf[0..2] != Self::HEADER {
            return None;
        }

        let command = u16::from_be_bytes([buf[2], buf[3]]);
        if command != CommandProtocol::InButton as u16
            && command != CommandProtocol::InButton2 as u16
        {
            return None;
        }

        let state = buf[8];
        let index = buf[9] as usize;
        let pressed = buf[11] == 0x01;

        Some(ButtonEvent {
            index,
            pressed,
            state,
        })
    }

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
            .await?;
        Ok(())
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

    pub async fn set_button_image(&mut self, index: usize, image_data: &str) -> Result<()> {
        // 1. Parse Data URL
        let url = DataUrl::process(image_data).map_err(|_| anyhow!("Invalid data URL"))?;
        let (body, _) = url
            .decode_to_vec()
            .map_err(|_| anyhow!("Failed to decode data URL"))?;

        // 2. Load image using image crate
        let img = image::load_from_memory(&body)?;

        // 3. Resize/Process
        let resized = img.resize_exact(196, 196, image::imageops::FilterType::Lanczos3);

        // 4. Convert to JPEG
        let mut jpeg_data = Vec::new();
        let mut cursor = Cursor::new(&mut jpeg_data);
        resized.write_to(&mut cursor, image::ImageFormat::Png)?;

        // 5. Store in internal buffer
        self.button_images.insert(index, jpeg_data);

        Ok(())
    }

    pub fn clear_button_image(&mut self, index: usize) {
        self.button_images.remove(&index);
    }

    pub async fn flush(&self) -> Result<()> {
        let zip_data = {
            let mut buf = Vec::new();
            {
                let mut zip = zip::ZipWriter::new(Cursor::new(&mut buf));
                // Use STORED (no compression) for dummy/sentinel, Deflated for content
                let stored: FileOptions<()> =
                    FileOptions::default().compression_method(zip::CompressionMethod::Stored);
                let deflated: FileOptions<()> =
                    FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

                // 1. dummy.txt FIRST - shifts subsequent file offsets away from
                //    bad 1024-byte boundaries when retried
                zip.start_file("dummy.txt", stored)?;
                zip.write_all(b"")?;

                // 2. Icons
                let mut manifest = json!({});
                for i in 0..15 {
                    let col = i % 5;
                    let row = i / 5;
                    let key = format!("{}_{}", col, row);

                    let mut view_param = json!({ "Text": "" });

                    if let Some(img_data) = self.button_images.get(&i) {
                        let icon_name = format!("{}.png", i);
                        zip.start_file(format!("icons/{}", icon_name), deflated)?;
                        zip.write_all(img_data)?;
                        view_param["Icon"] = json!(format!("icons/{}", icon_name));
                    }

                    manifest[key] = json!({
                        "State": 0,
                        "ViewParam": [view_param]
                    });
                }

                // 3. manifest.json
                zip.start_file("manifest.json", deflated)?;
                zip.write_all(serde_json::to_string(&manifest)?.as_bytes())?;

                // 4. sentinel.txt LAST - the firmware parser drops the final
                //    archive entry, so this absorbs that loss
                zip.start_file("sentinel.txt", stored)?;
                zip.write_all(b"")?;

                zip.finish()?;
            }
            buf
        };

        self.send_file(&zip_data).await?;
        info!(
            "Successfully sent button configuration ({} bytes)",
            zip_data.len()
        );
        Ok(())
    }

    pub async fn set_buttons(&mut self, config: &crate::config::Config) -> Result<()> {
        for button in &config.buttons {
            if let Some(ref img_path) = button.image {
                let path = Path::new(img_path);
                if path.exists() {
                    if let Ok(img) = image::open(path) {
                        let resized = img.resize_exact(196, 196, image::imageops::FilterType::Lanczos3);
                        let mut jpeg_data = Vec::new();
                        let mut cursor = Cursor::new(&mut jpeg_data);
                        resized.write_to(&mut cursor, image::ImageFormat::Png)?;
                        self.button_images.insert(button.index, jpeg_data);
                    }
                }
            }
        }
        self.flush().await
    }

    async fn send_command(&self, command: CommandProtocol, payload: &[u8]) -> Result<()> {
        let packet = self.build_packet(command, payload, payload.len() as u32);
        self.writer
            .lock()
            .await
            .write_output_report(&packet)
            .await?;
        Ok(())
    }

    async fn send_file(&self, data: &[u8]) -> Result<()> {
        let file_size = data.len() as u32;

        // First chunk
        let first_chunk_data = if data.len() >= 1016 {
            &data[0..1016]
        } else {
            data
        };
        let first_packet =
            self.build_packet(CommandProtocol::OutSetButtons, first_chunk_data, file_size);
        self.writer
            .lock()
            .await
            .write_output_report(&first_packet)
            .await?;

        // Remaining chunks
        if data.len() > 1016 {
            for chunk in data[1016..].chunks(1024) {
                let mut packet = [0u8; 1024];
                let len = chunk.len().min(1024);
                packet[..len].copy_from_slice(&chunk[..len]);
                self.writer
                    .lock()
                    .await
                    .write_output_report(&packet)
                    .await?;
            }
        }

        Ok(())
    }

    fn build_packet(&self, command: CommandProtocol, data: &[u8], total_length: u32) -> Vec<u8> {
        let mut packet = Vec::with_capacity(Self::PACKET_SIZE);

        // Header (2 bytes)
        packet.extend_from_slice(&Self::HEADER);

        // Command (2 bytes, Big Endian)
        let mut cmd_buf = [0u8; 2];
        (&mut cmd_buf[..])
            .write_u16::<BigEndian>(command as u16)
            .unwrap();
        packet.extend_from_slice(&cmd_buf);

        // Total Length (4 bytes, Little Endian)
        let mut len_buf = [0u8; 4];
        (&mut len_buf[..])
            .write_u32::<LittleEndian>(total_length)
            .unwrap();
        packet.extend_from_slice(&len_buf);

        // Payload
        let data_len = data.len().min(Self::PACKET_SIZE - 8);
        packet.extend_from_slice(&data[..data_len]);

        // Padding
        if packet.len() < Self::PACKET_SIZE {
            packet.resize(Self::PACKET_SIZE, 0);
        }

        packet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_id_with_serial() {
        let serial = Some("1234567890");
        let fallback = "fallback_id";
        let id = UlanziDevice::generate_id(serial, fallback);
        assert_eq!(id, "e9-1234567890");
    }

    #[test]
    fn test_generate_id_without_serial() {
        let serial = None;
        let fallback = "fallback_id";
        let id = UlanziDevice::generate_id(serial, fallback);
        assert_eq!(id, "e9-fallback_id");
    }
}
