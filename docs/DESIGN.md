# Design: Implement Stream Deck Plugin Client Mode (Implemented)

## 1. Module: `src/openaction_client.rs`

This module adapts the `openaction` crate's event model to the internal `UlanziDaemon` events.

**Key Components:**
-   `OpenActionBridge`: Implements `openaction::GlobalEventHandler`.
    -   Registers itself as the global event handler for the `openaction` runtime.
    -   Translates `openaction` events (e.g., `SetImage`, `SetBrightness`) into `BridgeEvent`s for the `UlanziDaemon`.
-   `BridgeEvent`: Internal enum used to pass commands from the OpenAction runtime to the Daemon.

## 2. Integration in `src/main.rs`

The `main` function supports multiple modes:
-   **Plugin Mode:** Triggered by `-pluginUUID`. Uses the `openaction` crate.
    -   Spawns the `openaction::run` loop in a separate task.
    -   Spawns a task to forward `HardwareEvent`s to `openaction::device_plugin` functions.
-   **Server Mode:** Triggered by `--daemon --websocket`. (Currently disabled/unsupported).
-   **One-Shot Mode:** Default behavior for initialization.

## 3. Data Flow

-   **Host to Plugin:** Host JSON -> `openaction` Runtime -> `OpenActionBridge` -> `BridgeEvent` -> `UlanziDaemon`.
-   **Plugin to Host:** `UlanziDaemon` -> `HardwareEvent` -> `main.rs` loop -> `openaction::device_plugin` -> Host JSON.

## 4. Device Identification

See [Device ID Generation Logic](./DEVICE_ID_LOGIC.md) for details on how unique device IDs are generated using the `e9` namespace.
