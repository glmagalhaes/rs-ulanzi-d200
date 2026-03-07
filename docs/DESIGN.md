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

## 5. CI/CD Build Pipeline

A cross-platform build pipeline is configured via GitLab CI (`.gitlab-ci.yml`) to automatically compile the Rust application for multiple target architectures:
- **Windows:** `x86_64-pc-windows-msvc`
- **Linux:** `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`
- **macOS:** `x86_64-apple-darwin` and `aarch64-apple-darwin`

The pipeline leverages GitLab's native shared runners (Linux, macOS) and containerized cross-compilation on Linux (using `cargo-xwin` for Windows binaries) to perform reliable compilation. 

**Trigger:**
The build and release process is triggered by pushing a git tag that matches the `beta-release*` pattern. 

**Artifacts & Releases:**
Upon a successful build across all architectures, the pipeline automatically generates a GitLab Release. The compiled standalone binaries are attached directly to the release page as downloadable asset links. This provides an easy way for beta testers to download the specific binary for their operating system without needing to navigate pipeline job artifacts or compile the code manually.
