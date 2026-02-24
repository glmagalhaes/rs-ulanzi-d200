# opendeck-m18 Analysis

**Repository:** `https://github.com/ibanks42/opendeck-m18`
**Key Dependencies:** `openaction` (1.1.5), `mirajazz` (0.9.0), `tokio`.

## Architecture Overview
`opendeck-m18` serves as a bridge between the `mirajazz` library (which handles physical device communication) and the `openaction` ecosystem.

## Key Components

### 1. Main Entry Point (`src/main.rs`)
-   **Initialization**: Uses `openaction::init_plugin`.
-   **Event Handling**: Implements `GlobalEventHandler` to handle lifecycle events (`plugin_ready`) and commands (`set_image`, `set_brightness`).
-   **State Management**: Manages a collection of active devices (`DEVICES` RwLock) and cancellation tokens for tasks.

### 2. Device Management (`src/device.rs`)
-   **`device_task`**: The lifecycle manager for a single device.
    -   Connects to the device using `mirajazz::Device`.
    -   Registers the device with `openaction` via `OUTBOUND_EVENT_MANAGER.register_device`.
    -   Spawns sub-tasks: `device_events_task` (input) and `keepalive_task`.
-   **`device_events_task`**: Reads input events (ButtonDown, ButtonUp, Encoder changes) from the `mirajazz` reader and forwards them to `openaction` using `OUTBOUND_EVENT_MANAGER`.
-   **`handle_set_image`**: Handles `SetImageEvent`.
    -   Parses Base64 Data URLs (supports JPEG).
    -   Uses the `image` crate to load/process the image.
    -   Calls `device.set_button_image` and `device.flush` via `mirajazz`.

### 3. Event Flow
-   **Inbound (Host -> Device)**: `GlobalEventHandler` -> `handle_set_image`/`set_brightness` -> `mirajazz::Device` method.
-   **Outbound (Device -> Host)**: `mirajazz` Reader -> `DeviceStateUpdate` -> `OUTBOUND_EVENT_MANAGER` -> Host.

### 4. Device Discovery (`src/watcher.rs`)
-   **`watcher_task`**: Spawns a `DeviceWatcher` (from `mirajazz`) to listen for hotplug events.
-   **`get_candidates`**: Initial scan for devices using `mirajazz::list_devices`.
-   **Device Lifecycle**:
    -   **Connected**: Generates a stable ID (based on serial), creates a cancellation token, and spawns a `device_task`.
    -   **Disconnected**: Cancels the device's task token, removes it from `DEVICES`, and deregisters it from `openaction`.

## Observations vs Current Project
-   `opendeck-m18` uses an older version of `openaction` (1.1.5) which relies on `OUTBOUND_EVENT_MANAGER` and `init_plugin`.
-   Our project (`rs-ulanzi-d200-linux`) uses `openaction` 2.5.0 which uses `run(args)` and `openaction::device_plugin` module for outbound events.
-   The core logic of mapping inputs to outbound events and mapping inbound commands to device calls remains conceptually similar.
