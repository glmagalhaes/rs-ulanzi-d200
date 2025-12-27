# Class Structure

## `rs-ulanzi-d200-linux` Crate

### Module: `device` (`src/device.rs`)

This module encapsulates all low-level USB communication and device-specific logic for the Ulanzi D200. It uses a dedicated I/O thread to handle all hardware interaction, providing a thread-safe public API.

#### Public Struct: `UlanziDevice`
The main public entry point for interacting with the device. This struct acts as a handle, sending commands to and receiving events from a dedicated background I/O thread.
*   **Fields:**
    *   `command_sender: std::sync::mpsc::Sender<DeviceCommand>`: Used to send commands (e.g., set image, shutdown) to the background I/O thread.
    *   `button_receiver: std::sync::mpsc::Receiver<ButtonPress>`: Used to receive button press events from the background I/O thread.
    *   `manager_handle: Option<std::thread::JoinHandle<()>>`: A handle to the background I/O thread, allowing it to be joined on shutdown.
*   **Public API:**
    *   `new() -> Self`: Creates a new `UlanziDevice` instance and spawns the dedicated I/O thread.
    *   `read_input(&self) -> Result<ButtonPress, std::sync::mpsc::TryRecvError>`: Attempts to receive a `ButtonPress` event from the device in a non-blocking manner. Returns `Err(TryRecvError::Empty)` if no event is available.
    *   `set_button_appearance(&self, button_index: u8, appearance: ButtonAppearance) -> Result<(), DeviceError>`: Sends a command to the I/O thread to set a button's appearance (image and optional label). Returns an error if the command cannot be sent.
    *   `shutdown(&mut self)`: Sends a `Shutdown` command to the I/O thread and waits for the thread to terminate gracefully.

#### Public Struct: `ButtonPress`
Represents a button press or release event received from the device.
*   **Fields:**
    *   `index: u8`: The numerical index of the button (0-12, or 13 for the large button).
    *   `pressed: bool`: `true` if the button was pressed, `false` if released.
    *   `state: u8`: An additional state value provided by the device.

#### Public Struct: `ButtonAppearance`
Encapsulates the visual data for a button on the Ulanzi D200.
*   **Fields:**
    *   `image_data: Vec<u8>`: The PNG-encoded image data for the button.
    *   `label: Option<String>`: An optional text label to display on the button.

#### Public Enum: `DeviceError`
A custom error type for device-related operations.
*   **Variants:**
    *   `NotConnected`: Indicates that the device is not currently connected.
    *   `HidError(hidapi::HidError)`: Wraps errors originating from the `hidapi` library.
    *   `ZipError(zip::result::ZipError)`: Wraps errors encountered during ZIP archive creation.
    *   `JsonError(serde_json::Error)`: Wraps errors encountered during JSON serialization.
    *   `IoError(std::io::Error)`: Wraps general I/O errors.
    *   `ImageError(image::ImageError)`: Wraps errors from the `image` crate during encoding.
    *   `ChannelSendError`: Indicates a failure to send a command to the I/O thread.

#### Public Free Function: `encode_raw_to_png(...) -> Result<Vec<u8>, DeviceError>`
A utility function to encode a raw pixel buffer into PNG format.
*   **Parameters:** `pixels: &[u8]`, `width: u32`, `height: u32`, `color: image::ColorType`
*   **Returns:** A `Vec<u8>` containing the PNG data on success.

---
### Internal Architecture (within `device.rs`)

#### Struct: `DeviceIO`
This struct encapsulates the state and logic for the dedicated background I/O thread.
*   **Fields:**
    *   `hid_device: Option<hidapi::HidDevice>`: Holds the active HID device connection if the device is connected. `None` if disconnected.
    *   `command_receiver: std::sync::mpsc::Receiver<DeviceCommand>`: Receives commands from the `UlanziDevice` handle.
    *   `button_sender: std::sync::mpsc::Sender<ButtonPress>`: Sends button events back to the `UlanziDevice` handle.
    *   `last_heartbeat: std::time::Instant`: Tracks when the last heartbeat signal was sent to the device.
*   **Internal Methods:**
    *   `run()`: The main loop of the I/O thread. Handles connection attempts, command processing, button reading, and heartbeats.
    *   `handle_commands()`: Processes commands received from the `command_receiver`.
    *   `handle_reads()`: Polls the HID device for button presses.
    *   `handle_heartbeat()`: Sends periodic keep-alive signals.
    *   `set_button_appearance_internal()`: Internal logic to process a `SetAppearance` command, handling both image and label.
    *   `disconnect()`: Sets `hid_device` to `None` and logs the disconnection.

#### Enum: `DeviceCommand`
Represents the types of commands that can be sent to the `DeviceIO` thread.
*   **Variants:** `SetAppearance { index: u8, appearance: ButtonAppearance }`, `Shutdown`.

#### Enum: `LoopState`
Internal enum used by the I/O thread's `handle_commands` to determine if the main loop should continue or shut down.

#### Free Functions (Stateless Helpers)
These functions perform specific tasks and do not rely on the `DeviceIO` struct's internal state.
*   `attempt_connect() -> Result<hidapi::HidDevice, hidapi::HidError>`: Scans for the Ulanzi D200 device and attempts to open a connection.
*   `parse_button_press_packet(buf: &[u8]) -> Option<ButtonPress>`: Parses raw HID packet bytes into a `ButtonPress` struct.
*   `_build_zip_archive(button_index: u8, appearance: ButtonAppearance) -> Result<Vec<u8>, DeviceError>`: Creates an in-memory ZIP archive containing `manifest.json` and the image data, applying the device-specific protocol workaround, and conditionally including the label.
*   `_send_file(hid_device: &HidDevice, command: CommandProtocol, data: &[u8]) -> Result<(), DeviceError>`: Handles packetization and low-level writing of data to the `HidDevice`.

#### Trait Implementation: `Drop for UlanziDevice`
Automatically calls `shutdown()` when an `UlanziDevice` instance goes out of scope, ensuring the background I/O thread is gracefully terminated.