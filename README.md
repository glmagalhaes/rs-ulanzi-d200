# Ulanzi D200 Linux Driver (rs-ulanzi-d200-linux)

This is a Rust-based userspace driver for the Ulanzi D200 smart clock, providing a simple and safe API to control the device on Linux.

## Features

*   **Non-Blocking API:** Uses a dedicated I/O thread to ensure the public API never blocks.
*   **Thread-Safe:** All communication with the device is handled through thread-safe channels.
*   **Hot-plugging:** The driver automatically detects when the device is connected or disconnected.
*   **Button Press Events:** Read button presses from the device as events.
*   **Set Button Appearance:** Set the appearance of buttons with PNG images.

## Design

The driver uses a dedicated I/O thread to manage the device, communicating with the main thread via channels. This allows for a non-blocking, thread-safe public API. For more details, see the [design document](docs/DESIGN.md).

## Usage

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

*   `hidapi`
*   `anyhow`
*   `thiserror`
*   `tokio`
*   `image`
*   `byteorder`
*   `zip`
*   `serde`
*   `serde_json`
