use thiserror::Error;
use hidapi::{HidApi, HidDevice};
use std::convert::TryInto;
use std::io::{Write, Cursor};
use serde::Serialize;
use zip::write::{FileOptions, ZipWriter};
use zip::result::ZipError;
use std::sync::mpsc::{self, Sender, Receiver, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use image::codecs::png::PngEncoder;
use image::{ColorType, ImageEncoder, ImageError};

const VENDOR_ID: u16 = 0x2207;
const PRODUCT_ID: u16 = 0x0019;

#[repr(u16)]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CommandProtocol {
    OutSetButtons = 0x0001,
    OutSetSmallWindowData = 0x0006,
    OutSetBrightness = 0x000a,
    OutSetLabelStyle = 0x000b,
    OutPartiallyUpdateButtons = 0x000d,
    InButton = 0x0101,
    InButton2 = 0x0102,
    InDeviceInfo = 0x0303,
    Unknown = 0xFFFF,
}

impl From<u16> for CommandProtocol {
    fn from(item: u16) -> Self {
        match item {
            0x0001 => CommandProtocol::OutSetButtons,
            0x0006 => CommandProtocol::OutSetSmallWindowData,
            0x000a => CommandProtocol::OutSetBrightness,
            0x000b => CommandProtocol::OutSetLabelStyle,
            0x000d => CommandProtocol::OutPartiallyUpdateButtons,
            0x0101 => CommandProtocol::InButton,
            0x0102 => CommandProtocol::InButton2,
            0x0303 => CommandProtocol::InDeviceInfo,
            _ => CommandProtocol::Unknown,
        }
    }
}

const PACKET_SIZE: usize = 1024;
const HEADER: [u8; 2] = [0x7c, 0x7c];

#[derive(Error, Debug)]
pub enum DeviceError {
    #[error("Device is not connected")]
    NotConnected,
    #[error("HID API Error: {0}")]
    HidError(#[from] hidapi::HidError),
    #[error("ZIP Error: {0}")]
    ZipError(#[from] ZipError),
    #[error("JSON Error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("IO Error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Image Error: {0}")]
    ImageError(#[from] ImageError),
    #[error("Channel send error")]
    ChannelSendError,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ButtonPress {
    pub index: u8,
    pub pressed: bool,
    pub state: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonAppearance {
    pub image_data: Vec<u8>,
    pub label: Option<String>,
}

#[derive(Serialize)]
struct ViewParam {
    #[serde(rename = "Icon")]
    icon: String,
    #[serde(rename = "Text", skip_serializing_if = "Option::is_none")]
    text: Option<String>,
}

#[derive(Serialize)]
struct ButtonManifest {
    #[serde(rename = "ViewParam")]
    view_param: Vec<ViewParam>,
}

#[derive(Debug, PartialEq, Eq)]
enum DeviceCommand {
    SetAppearance{ index: u8, appearance: ButtonAppearance },
    Shutdown,
}

pub struct UlanziDevice {
    command_sender: Sender<DeviceCommand>,
    pub button_receiver: Receiver<ButtonPress>,
    manager_handle: Option<JoinHandle<()>>,
}

struct DeviceIO {
    hid_device: Option<HidDevice>,
    command_receiver: Receiver<DeviceCommand>,
    button_sender: Sender<ButtonPress>,
    last_heartbeat: Instant,
}

impl UlanziDevice {
    pub fn new() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (btn_tx, btn_rx) = mpsc::channel();

        let mut io_manager = DeviceIO {
            hid_device: None,
            command_receiver: cmd_rx,
            button_sender: btn_tx,
            last_heartbeat: Instant::now(),
        };

        let handle = thread::spawn(move || {
            io_manager.run();
        });

        UlanziDevice {
            command_sender: cmd_tx,
            button_receiver: btn_rx,
            manager_handle: Some(handle),
        }
    }
    
    pub fn read_input(&self) -> Result<ButtonPress, TryRecvError> {
        self.button_receiver.try_recv()
    }

    pub fn set_button_appearance(&self, button_index: u8, appearance: ButtonAppearance) -> Result<(), DeviceError> {
        self.command_sender.send(DeviceCommand::SetAppearance { index: button_index, appearance })
            .map_err(|_| DeviceError::ChannelSendError)
    }

    pub fn shutdown(&mut self) {
        if let Some(handle) = self.manager_handle.take() {
            let _ = self.command_sender.send(DeviceCommand::Shutdown);
            handle.join().expect("Device IO thread panicked");
        }
    }
}

#[derive(PartialEq, Eq)]
enum LoopState { Continue, Shutdown }

impl DeviceIO {
    fn run(&mut self) {
        loop {
            if self.hid_device.is_none() {
                if let Ok(device) = attempt_connect() {
                    println!("Device connected!");
                    self.hid_device = Some(device);
                } else {
                    thread::sleep(Duration::from_secs(2));
                    if let Ok(DeviceCommand::Shutdown) = self.command_receiver.try_recv() { break; }
                    continue;
                }
            }

            if self.hid_device.is_some() {
                if self.handle_commands() == LoopState::Shutdown { break; }
                self.handle_reads();
                self.handle_heartbeat();
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn handle_commands(&mut self) -> LoopState {
        match self.command_receiver.try_recv() {
            Ok(DeviceCommand::Shutdown) => return LoopState::Shutdown,
            Ok(DeviceCommand::SetAppearance { index, appearance }) => {
                if self.set_button_appearance_internal(index, appearance).is_err() {
                    self.disconnect();
                }
            },
            Err(_) => { /* No command, continue */ },
        }
        LoopState::Continue
    }
    
    fn handle_reads(&mut self) {
        if let Some(device) = self.hid_device.as_mut() {
            let mut buf = [0u8; PACKET_SIZE];
            match device.read_timeout(&mut buf, 0) {
                Ok(bytes) if bytes > 0 => {
                    if let Some(btn) = parse_button_press_packet(&buf[..bytes]) {
                        if self.button_sender.send(btn).is_err() {
                           // Main handle dropped, we can probably shut down.
                           // For now, just ignore.
                        }
                    }
                },
                Err(_) => self.disconnect(),
                _ => { /* No data, continue */ }
            }
        }
    }

    fn handle_heartbeat(&mut self) {
        if self.last_heartbeat.elapsed() >= Duration::from_secs(4) {
            if let Some(device) = self.hid_device.as_mut() {
                let mut packet = [0u8; PACKET_SIZE];
                packet[0..2].copy_from_slice(&HEADER);
                packet[2..4].copy_from_slice(&(CommandProtocol::OutSetSmallWindowData as u16).to_be_bytes());
                if device.write(&packet).is_err() {
                    self.disconnect();
                }
            }
            self.last_heartbeat = Instant::now();
        }
    }

    fn set_button_appearance_internal(&mut self, index: u8, appearance: ButtonAppearance) -> Result<(), DeviceError> {
        if let Some(device) = self.hid_device.as_mut() {
            let zip_data = _build_zip_archive(index, appearance)?;
            return _send_file(device, CommandProtocol::OutSetButtons, &zip_data);
        }
        Err(DeviceError::NotConnected)
    }

    fn disconnect(&mut self) {
        println!("Device disconnected.");
        self.hid_device = None;
    }
}

pub fn encode_raw_to_png(pixels: &[u8], width: u32, height: u32, color: ColorType) -> Result<Vec<u8>, DeviceError> {
    let mut buffer = Vec::new();
    let encoder = PngEncoder::new(&mut buffer);
    encoder.write_image(pixels, width, height, color.into())?;
    Ok(buffer)
}

fn attempt_connect() -> Result<HidDevice, hidapi::HidError> {
    let api = HidApi::new()?;
    let device_path = {
        api.device_list()
            .find(|d| d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID)
            .map(|d| d.path().to_owned())
    };

    if let Some(path) = device_path {
        let device = api.open_path(&path)?;
        device.set_blocking_mode(false)?;
        Ok(device)
    } else {
        Err(hidapi::HidError::HidApiErrorEmpty)
    }
}
    
fn parse_button_press_packet(buf: &[u8]) -> Option<ButtonPress> {
    if buf.len() < 12 { return None; }
    if buf[0..2] != HEADER { return None; }
    let command_val = u16::from_be_bytes(buf[2..4].try_into().unwrap());
    let command = CommandProtocol::from(command_val);
    if !matches!(command, CommandProtocol::InButton | CommandProtocol::InButton2) { return None; }
    let state = buf[8];
    let index = buf[9];
    let pressed = buf[11] == 0x01;
    Some(ButtonPress { index, pressed, state })
}

fn _build_zip_archive(button_index: u8, appearance: ButtonAppearance) -> Result<Vec<u8>, DeviceError> {
    let mut dummy_str = "".to_string();
    loop {
        let mut buf = Vec::new();
        {
            let cursor = Cursor::new(&mut buf);
            let mut zip = ZipWriter::new(cursor);
            let options: FileOptions<'_, ()> = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
            let key = format!("{}_{}", button_index % 5, button_index / 5);
            
            let mut view_param_map = serde_json::Map::new();
            view_param_map.insert("Icon".to_string(), serde_json::json!("icons/icon.png"));
            if let Some(ref label) = appearance.label {
                view_param_map.insert("Text".to_string(), serde_json::json!(label.clone()));
            }

            let manifest = serde_json::json!({ key: { "ViewParam": [view_param_map] } });
            
            zip.start_file("manifest.json", options)?;
            zip.write_all(serde_json::to_string(&manifest)?.as_bytes())?;
            zip.start_file("icons/icon.png", options)?;
            zip.write_all(&appearance.image_data)?;
            zip.start_file("dummy.txt", options)?;
            zip.write_all(dummy_str.as_bytes())?;
            zip.finish()?;
        } 
        
        let has_bad_bytes = buf.iter().enumerate()
            .filter(|(i, _)| *i >= 1016 && (*i - 1016) % 1024 == 0)
            .any(|(_, &byte)| byte == 0x00 || byte == 0x7c);
        
        if !has_bad_bytes { return Ok(buf); }
        dummy_str.push_str(" "); 
    }
}

fn _send_file(hid_device: &HidDevice, command: CommandProtocol, data: &[u8]) -> Result<(), DeviceError> {
    let file_size = data.len();
    let first_chunk_len = (PACKET_SIZE - 8).min(file_size);
    let mut packet = [0u8; PACKET_SIZE];
    packet[0..2].copy_from_slice(&HEADER);
    packet[2..4].copy_from_slice(&(command as u16).to_be_bytes());
    packet[4..8].copy_from_slice(&(file_size as u32).to_le_bytes());
    packet[8..8 + first_chunk_len].copy_from_slice(&data[..first_chunk_len]);
    hid_device.write(&packet)?;
    
    let mut bytes_sent = first_chunk_len;
    while bytes_sent < file_size {
        let chunk_end = (bytes_sent + PACKET_SIZE).min(file_size);
        let chunk = &data[bytes_sent..chunk_end];
        let mut chunk_packet = [0u8; PACKET_SIZE];
        chunk_packet[..chunk.len()].copy_from_slice(chunk);
        hid_device.write(&chunk_packet)?;
        bytes_sent = chunk_end;
    }
    Ok(())
}

impl Drop for UlanziDevice {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::ZipArchive;
    use std::io::Read; // Add this import

    // A minimal valid PNG (1x1 black pixel)
    const DUMMY_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
        0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
        0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
        0x42, 0x60, 0x82,
    ];

    #[test]
    fn test_device_constants_correct() {
        assert_eq!(VENDOR_ID, 0x2207);
        assert_eq!(PRODUCT_ID, 0x0019);
    }

    #[test]
    fn test_parse_valid_press_packet() {
        let mut packet = [0u8; 12];
        packet[0..2].copy_from_slice(&HEADER);
        packet[2..4].copy_from_slice(&(CommandProtocol::InButton as u16).to_be_bytes());
        packet[8] = 1; // state
        packet[9] = 5; // index
        packet[11] = 1; // pressed

        let result = parse_button_press_packet(&packet);
        assert_eq!(result, Some(ButtonPress { index: 5, pressed: true, state: 1 }));
    }

    #[test]
    fn test_build_zip_archive_structure_and_content() {
        let button_index = 0;
        let image_data = DUMMY_PNG.to_vec();
        let appearance = ButtonAppearance { image_data, label: None };
        let zip_data = _build_zip_archive(button_index, appearance).unwrap();

        let cursor = Cursor::new(zip_data);
        let mut archive = ZipArchive::new(cursor).unwrap();

        assert!(archive.by_name("manifest.json").is_ok());
        assert!(archive.by_name("icons/icon.png").is_ok());
    }

    #[test]
    fn test_manager_thread_lifecycle() {
        let mut device = UlanziDevice::new();
        // Immediately after creation, there should be no button data,
        // and the manager thread is trying to connect in the background.
        assert_eq!(device.read_input(), Err(TryRecvError::Empty));

        // Shutdown should happen cleanly
        device.shutdown();
    }

    #[test]
    fn test_encode_raw_to_png() {
        // A 2x2 raw RGBA image (red, green, blue, black)
        let pixels: &[u8] = &[
            255, 0, 0, 255,   // Red
            0, 255, 0, 255,   // Green
            0, 0, 255, 255,   // Blue
            0, 0, 0, 255,     // Black
        ];
        let width = 2;
        let height = 2;

        let result = encode_raw_to_png(pixels, width, height, ColorType::Rgba8);
        assert!(result.is_ok());
        let png_data = result.unwrap();

        // Check for PNG header
        let png_header: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        assert!(png_data.starts_with(png_header));
    }

    #[test]
    #[ignore]
    fn test_hardware_connect_and_shutdown() {
        println!("--- Running Hardware Test: Connect and Shutdown ---");
        println!("(This test will fail if a Ulanzi D200 is not connected)");
        let mut device = UlanziDevice::new();
        // Give the manager thread time to find and connect to the device.
        thread::sleep(Duration::from_secs(3));
        
        // We can't easily assert that it's connected without more complex channel feedback,
        // but if it panics or shutdown hangs, the test will fail.
        
        device.shutdown();
        println!("--- Hardware Test Complete ---");
    }

    #[test]
    #[ignore]
    fn test_hardware_set_image() {
        println!("--- Running Hardware Test: Set Image ---");
        println!("(This test will fail if a Ulanzi D200 is not connected)");
        let mut device = UlanziDevice::new();
        thread::sleep(Duration::from_secs(3));

        // Create a test image
        let pixels: &[u8] = &[255, 0, 255, 255]; // 1x1 magenta pixel
        let png_data = encode_raw_to_png(pixels, 1, 1, ColorType::Rgba8).unwrap();
        
        let appearance = ButtonAppearance { image_data: png_data, label: None };
        let result = device.set_button_appearance(0, appearance); // Renamed call
        assert!(result.is_ok());
        
        device.shutdown();
        println!("--- Hardware Test Complete: Check button 0 on your device ---");
    }

    #[test]
    fn test_set_button_appearance_with_label() {
        let button_index = 1;
        let image_data = DUMMY_PNG.to_vec();
        let label_text = "Test Label".to_string();
        let appearance = ButtonAppearance { image_data, label: Some(label_text.clone()) };

        let zip_data = _build_zip_archive(button_index, appearance).unwrap();

        let cursor = Cursor::new(zip_data);
        let mut archive = ZipArchive::new(cursor).unwrap();

        let mut manifest_file = archive.by_name("manifest.json").unwrap();
        let mut manifest_content = String::new();
        manifest_file.read_to_string(&mut manifest_content).unwrap();
        let manifest_json: serde_json::Value = serde_json::from_str(&manifest_content).unwrap();
        
        let expected_key = format!("{}_{}", button_index % 5, button_index / 5);
        assert_eq!(
            manifest_json[&expected_key]["ViewParam"][0]["Text"],
            label_text
        );
        assert_eq!(
            manifest_json[&expected_key]["ViewParam"][0]["Icon"],
            "icons/icon.png"
        );
    }
}