# Design Document

## Ulanzi D200 Driver Architecture

This section outlines the design for the `rs-ulanzi-d200-linux` driver, which uses a dedicated I/O thread to provide a robust, thread-safe, and non-blocking public API.

### 1. Core Architectural Pattern: Dedicated I/O Thread
*   **Problem:** The underlying `hidapi` library is synchronous and not thread-safe (`!Send`). Direct use in a multi-threaded or asynchronous application is complex and unsafe.
*   **Solution:** The library's public-facing struct, `UlanziDevice`, acts as a lightweight handle. All hardware interaction is delegated to a dedicated background I/O thread that is spawned when `UlanziDevice::new()` is called.
*   **Communication:** Communication between the `UlanziDevice` handle and the I/O thread is achieved using standard `std::sync::mpsc` channels:
    *   **Command Channel:** The handle sends commands (like `SetAppearance`, `Shutdown`) to the I/O thread.
    *   **Event Channel:** The I/O thread sends button press events back to the handle.

### 2. Connection Management & Hot-plugging
*   **Responsibility:** The dedicated I/O thread (`DeviceIO`) is solely responsible for managing the device's connection lifecycle.
*   **Connection Loop:**
    1.  The thread continuously attempts to connect to the Ulanzi D200 via the `attempt_connect()` free function.
    2.  If connection fails, it sleeps for a few seconds and retries, ensuring the application can start before the device is plugged in.
    3.  Once connected, it enters the main event loop.
*   **Auto-reconnect:** If any `hidapi` read/write operation fails within the event loop (signifying a disconnection), the `DeviceIO` thread calls its `disconnect()` method, which sets its internal `hid_device` to `None`, and then returns to the connection loop to begin scanning again.

### 3. Public API (`UlanziDevice`)
*   **`new() -> Self`:** The constructor. Creates the command and event channels and spawns the `DeviceIO` background thread.
*   **`read_input(&self) -> Result<ButtonPress, std::sync::mpsc::TryRecvError>`:** A non-blocking method that polls the event channel for button press data.
*   **`set_button_appearance(...)`:** Sends a `SetAppearance` command (containing button index and image data) to the I/O thread's command channel. This method now takes a `ButtonAppearance` struct.
*   **`shutdown(&mut self)`:** Sends a `Shutdown` command and joins the I/O thread for a clean exit. This is also called automatically when `UlanziDevice` is dropped.
*   **`encode_raw_to_png(...)`:** A public, static utility function to encode raw pixel buffers into PNG data.

### 4. Internal I/O Thread Logic (`DeviceIO`)
*   **Event Loop (`run` method):** Once connected, the `DeviceIO` thread's `run` method enters an event loop that continuously performs the following:
    1.  **Handles Commands:** Checks for incoming commands (e.g., `SetAppearance`, `Shutdown`) from the `command_receiver` via `try_recv()`. `Shutdown` commands cause the thread to exit.
    2.  **Handles Reads:** Calls `hid_device.read_timeout()` to poll for button presses. If a `ButtonPress` is detected, it is parsed by `parse_button_press_packet()` and sent back over the `button_sender` event channel.
    3.  **Handles Heartbeat:** Every 4 seconds, it sends a "keep-alive" ping (an empty `OUT_SET_SMALL_WINDOW_DATA` command) to the device to prevent it from sleeping.
*   **Internal Helpers:** Methods like `set_button_appearance_internal()` handle the actual device interaction after receiving a command.

### 5. Stateless Helper Functions & Image Encoding
*   **Image Encoding (`encode_raw_to_png`):** A public, stateless utility function is provided to convert raw pixel buffers (e.g., RGBA8) into a `Vec<u8>` of PNG-encoded data. This uses the `image` crate and allows host applications to easily create the required image format.
*   **Packet Parsing (`parse_button_press_packet`):** Parses raw HID packet bytes into a `ButtonPress` struct.
*   **ZIP Archive Creation (`_build_zip_archive`):** Creates an in-memory ZIP archive containing the `manifest.json` and the image data, applying the device-specific protocol workaround, and conditionally including the label.
*   **Low-Level Data Sending (`_send_file`):** Handles the packetization and low-level writing of data to the `HidDevice`.

### 6. Heartbeat Mechanism
The heartbeat mechanism is fully integrated into the `DeviceIO` thread's event loop, automatically managed as part of the connection and active device state.

### 7. Dependencies
*   `hidapi`: For low-level HID device interaction.
*   `thiserror`: For declarative error type definition.
*   `serde`, `serde_json`: For `manifest.json` serialization.
*   `zip`: For creating the image ZIP archive in memory.
*   `image`: For encoding raw pixel buffers to PNG.
*   `std::sync::mpsc`, `std::thread`, `std::time`: For the core I/O thread, channel architecture, and timing.