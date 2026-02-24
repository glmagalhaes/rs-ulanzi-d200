# mirajazz Analysis

**Repository:** `https://github.com/4ndv/mirajazz`
**Key Dependencies:** `async-hid`, `tokio`, `image`.

## Architecture Overview
`mirajazz` provides an async Rust library for interfacing with Mirabox/Ajazz stream controllers. It abstracts the HID communication details and provides a high-level API for device discovery, input reading, and state management.

## Key Components

### 1. Device Abstraction (`src/device.rs`)
-   **`Device` Struct**:
    -   Manages HID reader/writer (`Arc<Mutex<DeviceReader/Writer>>`).
    -   **Protocol**: Uses a specific command format (often starting with "CRT" ASCII sequence) to control the device (brightness, mode, images).
    -   **Buffering**: `set_button_image` stores data in `image_cache`. `flush()` sends cached images and commits changes (often with an "STP" command).
    -   **Async I/O**: Uses `async-hid` for non-blocking I/O, integrating well with Tokio.

### 2. Device Discovery (`src/device.rs`, `src/lib.rs`)
-   **`DeviceWatcher`**: An async stream that yields `DeviceLifecycleEvent::Connected` or `Disconnected`.
-   **`DeviceQuery`**: Struct to filter devices by VID/PID/Usage.
-   **`list_devices`**: Returns currently connected devices.

### 3. State Management (`src/state.rs`)
-   **`DeviceStateReader`**:
    -   Reads raw HID reports via `raw_read_data`.
    -   Processes raw bytes into `DeviceInput` using a callback (`process_input`).
    -   Maintains current state (`DeviceState`) of buttons and encoders to detect changes.
    -   Emits `DeviceStateUpdate` events (ButtonDown, ButtonUp, EncoderTwist, etc.) only when state changes.
-   **Protocol Handling**: Handles different firmware versions (ACK prefixes, state reporting differences).

### 4. Comparison to Ulanzi D200
-   **Protocol**: Mirabox/Ajazz uses a text-based/binary hybrid command structure (e.g., "CRT...LIG..."). Ulanzi D200 uses a binary protocol with fixed headers (`0x7c 0x7c`).
-   **I/O Model**: `mirajazz` uses fully async `async-hid`. Our current Ulanzi driver uses `hidapi` (blocking/polling). Adopting an async-hid approach would be beneficial for the Daemon architecture.
-   **State Tracking**: `mirajazz` abstracts state tracking (diffing previous vs current state) in the library. Our Ulanzi driver currently does this in `daemon.rs` / `device.rs`.

## Logic Flow for Reference
1.  **Connect**: `Device::connect` opens HID device.
2.  **Init**: Sends initialization commands (Mode, reset).
3.  **Loop**:
    -   **Read**: `DeviceStateReader` (from `get_reader`) continuously reads input reports.
    -   **Write**: Commands are sent via `write_extended_data`. Images are chunked and sent via `write_image_data_reports`.
4.  **Keepalive**: Periodic "CONNECT" commands sent to device.
