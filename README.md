# Ulanzi D200 Linux Driver (rs-ulanzi-d200-linux)

This is a Rust-based userspace driver for the Ulanzi D200 macro deck, providing a simple and safe API to control the device on Linux.

## Features

- **Non-Blocking API:** Uses a dedicated I/O thread to ensure the public API never blocks.
- **Thread-Safe:** All communication with the device is handled through thread-safe channels.
- **Hot-plugging:** The driver automatically detects when the device is connected or disconnected.
- **Button Press Events:** Read button presses from the device as events.
- **Set Button Appearance:** Set the appearance of buttons with PNG images.

## Design

The driver uses a dedicated I/O thread to manage the device, communicating with the main thread via channels. This allows for a non-blocking, thread-safe public API. For more details, see the [design document](docs/DESIGN.md).

## Usage

There is a [configuration guide](CONFIGURATION_GUIDE.md) with additional details.

Add this to your `Cargo.toml`:

```toml
[dependencies]
rs-ulanzi-d200-linux = "0.1.0"
```

Then, you can use the driver like this:

```rust
use rs_ulanzi_d200_linux::UlanziDevice;

fn main() {
    let mut device = UlanziDevice::new();

    loop {
        if let Ok(button_press) = device.read_input() {
            println!("Button pressed: {:?}", button_press);
        }
    }
}
```

## Dependencies

- `hidapi`
- `anyhow`
- `thiserror`
- `tokio`
- `image`
- `byteorder`
- `zip`
- `serde`
- `serde_json`

## Rust Daemon & OpenDeck Plugin System

The project includes a high-performance Rust implementation (`rs-ulanzi-d200-linux`) that supports the OpenDeck Plugin Protocol.

### Running the Rust Daemon

a. Build the Rust binary:

  ```bash
  cd rs-ulanzi-d200-linux
  cargo build --release
  ```

b. Run with WebSocket support:

```bash
./target/release/rs-ulanzi-d200-linux --daemon --websocket --port 57116
```

### Developing an OpenDeck Plugin

The Rust daemon acts as a host for OpenDeck-compatible plugins. To create a plugin that interacts with the Ulanzi D200:

#### 1. Connection Lifecycle

- **Discovery**: Your plugin should be launched by a Plugin Manager (or manually for testing).
- **Handshake**: Connect to `ws://localhost:57116` (or the configured port).
- **Registration**: Immediately send the registration event:

```json
{
  "event": "register",
  "uuid": "com.yourname.plugin"
}
```

#### 2. Handling Events

Listen for incoming JSON events from the host:

- `keyDown`: Sent when a physical button is pressed.
- `keyUp`: Sent when a button is released.
- `willAppear`: Sent when your action becomes active (future).

Example `keyDown` payload:

```json
{
  "event": "keyDown",
  "action": "unknown",
  "context": "btn_5",
  "device": "ulanzi_d200",
  "payload": {
    "coordinates": { "column": 0, "row": 1 },
    "isInMultiAction": false
  }
}
```

#### 3. Sending Commands

Control the device by sending JSON commands to the host:

- **Set Image**:

```json
{
  "event": "setImage",
  "context": "btn_5",
  "payload": {
    "image": "data:image/png;base64,તાઓ"
  }
}
```

- **Set Title**:

```json
{
  "event": "setTitle",
  "context": "btn_5",
  "payload": {
    "title": "New Label"
  }
}
```
