Here is the complete Technical Discovery Brief, formatted as a set of Markdown files ready to be passed to a coding agent.

### **File 1: `01_PROJECT_OVERVIEW.md`**

```markdown
# Project Overview: Ulanzi D200 OpenDeck Plugin

## 1. Executive Summary
We are developing a **Rust-based hardware plugin** for the [OpenDeck](https://github.com/nekename/OpenDeck) platform to support the **Ulanzi D200** stream controller.

*   **Current State:** Device support exists only as a monolithic Python script (`racerxdl/ulanzi-d200-linux`).
*   **Target State:** A modular Rust library (`.so`/`.dll`) that implements the OpenDeck `Device` trait, similar to `4ndv/opendeck-akp153` but with significantly improved architectural robustness.

## 2. Core Architectural Shift
We are moving from a **Monolithic Application** to a **Pure Hardware Driver**.

| Feature | Python Reference (`racerxdl`) | OpenDeck Plugin (Target) | Action |
| :--- | :--- | :--- | :--- |
| **Connectivity** | `device.py` manages HID/USB directly. | **Rust Driver.** | **PORT.** Re-implement `device.py` logic using `hidapi`. |
| **Logic** | `actions.py` handles OBS/Hotkeys. | **OpenDeck Host.** | **DISCARD.** The plugin only reports "Button X pressed". |
| **Config** | Parsing `config.yaml`. | **OpenDeck Host.** | **DISCARD.** OpenDeck loads `manifest.json` before loading the plugin. |
| **Graphics** | Draws text on images using PIL. | **OpenDeck Host.** | **ADAPT.** Receive raw pixel buffer -> Encode to PNG -> Send to USB. |

## 3. Reference Material
*   **Logic Source:** `racerxdl/ulanzi-d200-linux` (Python) - *Use this to reverse-engineer the protocol.*
*   **Structure Source:** `4ndv/opendeck-akp153` (Rust) - *Use this for folder structure only. Do not copy its error handling or lack of async patterns.*
*   **Host API:** `elgato-streamdeck` crate (Rust) - *Use this for Trait definitions only*.
```

---

### **File 2: `02_PROTOCOL_SPEC.md`**

```markdown
# Protocol Specification: Ulanzi D200

## 1. Hardware Definition
*   **Vendor ID (VID):** `2207`.
*   **Product ID (PID):** *[ACTION ITEM]* Extract this from the Python `99-ulanzi.rules` file or `device.py`.
*   **Grid:** 13 Buttons (Indices 0–12).
*   **Resolution:** 196x196 pixels per button.

## 2. USB Communication Logic
The Ulanzi D200 does **not** use Feature Reports. It uses standard **Interrupt Transfers**.

### A. Input (Buttons)
*   **Source:** `device.py` (Input Loop).
*   **Target:** Implement a read loop using `hidapi`.
*   **Task:** Listen for Interrupt IN packets. Map specific bytes to button indices (0-12). Debounce if necessary. Emitting a `ButtonDown` event to the host.

### B. Output (Images)
*   **Source:** `device.py` (Image Sender).
*   **Target:** `hidapi::HidDevice::write()` (Output Report).
*   **Payload Structure:** `[ Header Bytes ] + [ PNG Data ]`.
*   **Critical Requirement:** The device requires **PNG format**. You cannot send raw bitmaps. You must encode the OpenDeck buffer into PNG format in memory before sending.

### C. Heartbeat (Keep-Alive)
*   **Requirement:** The device will sleep or reset if the USB line is idle.
*   **Implementation:** The Python script runs a daemon. In Rust, you must spawn a background task (via `tokio`) that sends a specific "ping" packet every few seconds.
```

---

### **File 3: `03_DEPENDENCIES.md`**

```markdown
# Dependency Configuration (`Cargo.toml`)

To implement the driver, include the following crates. Note specific feature flags required to avoid bloat.

## 1. Essential Driver Stack
| Crate | Configuration | Purpose |
| :--- | :--- | :--- |
| **`hidapi`** | Standard | **The Driver Core.** Replaces `device.py` for USB I/O. |
| **`image`** | `default-features = false`, `features = ["png"]` | **The Translator.** Encodes raw OpenDeck buffers into the PNG format required by the hardware. **Must disable defaults** to save binary size. |
| **`tokio`** | `features = ["full"]` | **Concurrency.** Required for the Heartbeat thread and Connection Manager. |
| **`byteorder`** | Standard | **Protocol.** Ensures Little Endian headers are written correctly. |
| **`anyhow`** | Standard | **Error Handling.** For robust result propagation. |
| **`thiserror`** | Standard | **Error Definitions.** For defining `DeviceNotFound` states. |

## 2. Integration Stack
| Crate | Configuration | Purpose |
| :--- | :--- | :--- |
| **`elgato-streamdeck`** | *Check OpenDeck version* | **Interface Only.** Use this ONLY to import the `Device` trait. **DO NOT** use its `StreamDeck::connect` logic, as it does not support VID 2207. |
| **`serde`** | `features = ["derive"]` | **Compat.** Required for Trait data structures and IPC. |
```

---

### **File 4: `04_ARCHITECTURE_AND_ERRORS.md`**

```markdown
# Architecture & Error Handling Strategy

## 1. The "Connection Manager" Pattern
The reference plugin (`akp153`) is prone to panics. We must implement a "State Machine" to handle hot-plugging robustly.

### The Loop
Do not block the main thread. Create a background task using `tokio::spawn`:
1.  **Shared State:** `Arc<Mutex<Option<HidDevice>>>`.
2.  **Manager Task:**
    *   Loops forever.
    *   If `Option` is `None`: Attempt `hidapi.open(2207, PID)`.
    *   If Success: Store handle in Mutex.
    *   If Fail: Sleep 1s and retry.
3.  **Heartbeat Task:**
    *   Reads the Mutex.
    *   If valid, sends "Ping".
    *   If write fails: Sets Mutex to `None` (triggering Manager to reconnect).

## 2. Manifest & Configuration
*   **File:** `manifest.json` (Required in plugin root).
*   **Loading:** OpenDeck handles this. You do not need to parse JSON in Rust.
*   **Capabilities:** Hardcode capabilities (13 buttons, 196px) in the Rust Trait implementation methods (e.g., `fn buttons(&self) -> u32 { 13 }`).

## 3. Deployment Requirements
*   **Permissions:** `hidapi` will fail on Linux without udev rules.
*   **Deliverable:** You must generate a `40-opendeck-ulanzi.rules` file containing the `uaccess` tag for VID 2207.
```