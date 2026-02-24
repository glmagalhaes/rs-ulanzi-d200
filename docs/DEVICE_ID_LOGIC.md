# Device ID Generation Logic

## Overview

The `UlanziDevice` uses a namespaced ID generation strategy to ensure uniqueness and consistency within the OpenDeck ecosystem.

## Constants

-   `DEVICE_NAMESPACE`: `"e9"` - Unique identifier for the Ulanzi D200 device type.

## ID Generation

The device ID is generated using the following logic:

1.  **Retrieve Serial Number:** Attempt to read the serial number from the device descriptor using `async-hid`.
2.  **Format ID:**
    -   **If Serial Number Exists:** `e9-<serial_number>`
    -   **If Serial Number is Missing:** `e9-<platform_specific_device_id>` (Fallback)

## Implementation Details

The logic is encapsulated in the `UlanziDevice::generate_id` helper function within `src/device.rs`.

```rust
fn generate_id(serial: Option<&str>, fallback_id: &str) -> String {
    if let Some(s) = serial {
        format!("{}-{}", DEVICE_NAMESPACE, s)
    } else {
        format!("{}-{}", DEVICE_NAMESPACE, fallback_id)
    }
}
```

This ensures that even if the firmware or OS fails to report a serial number, the device still receives a prefixed ID, distinguishing it from other potential devices in the system.
