# Class Structure

## Main Components

### `Args` (in `src/main.rs`)
- Responsible for parsing command-line arguments.
- Identifies if the application should run in **Plugin Mode**, **Server Mode**, or **One-Shot Mode**.

### `OpenActionBridge` (in `src/openaction_client.rs`)
- **Role:** Event Adapter.
- **Responsibility:** Implements `openaction::GlobalEventHandler` to receive events from the Stream Deck Host (via `openaction` runtime) and forwards them to the `UlanziDaemon`.

### `WebSocketServer` (in `src/server.rs`)
- **Role:** WebSocket Server.
- **Responsibility:** Listens for incoming plugin connections (used in standalone Daemon mode).

### `UlanziDaemon` (in `src/daemon.rs`)
- **Role:** Central Orchestrator.
- **Responsibility:** Manages hardware connections, executes actions, and processes inbound/outbound events regardless of whether they come from a Server or Client connection.

### `UlanziDevice` (in `src/device.rs`)
- **Role:** Hardware Interface.
- **Responsibility:** Low-level HID communication with the Ulanzi D200/TC001 device.
