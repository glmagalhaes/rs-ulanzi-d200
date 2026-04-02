# Ulanzi D200 Configuration Guide

This document explains all configuration options available for the Ulanzi D200 Linux driver, including default values, recommended settings, and environment variable overrides.

## Configuration File Location

By default, the driver looks for a configuration file named `config.yaml` in the current working directory. You can specify a custom path using the `--config` command-line argument or the `ULANZI_CONFIG` environment variable.

## Configuration Structure

The configuration is divided into several sections:

1. Global settings
2. Label styling
3. OBS Studio settings (optional)
4. Button definitions

## Detailed Configuration Options

### Global Settings

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `brightness` | integer (0-100) | `100` | Sets the overall brightness level of the device display. Higher values increase brightness and power consumption. |

### Label Styling

These options control the appearance of text labels on buttons:

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `label_style.Align` | string (`top`, `bottom`, `center`) | `bottom` | Vertical alignment of text on buttons |
| `label_style.Color` | hex integer | `0xFFFFFF` (white) | Text color in RGB hex format |
| `label_style.FontName` | string | `Roboto` | Font family for button labels |
| `label_style.ShowTitle` | boolean | `true` | Whether to display button labels |
| `label_style.Size` | integer (8-72) | `10` | Font size in points |
| `label_style.Weight` | integer (100-900) | `80` | Font weight (boldness) |

### Display Settings

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `display_mode` | integer | `1` (CLOCK) | Controls what appears in the small status window:\n- `0`: OFF\n- `1`: CLOCK (shows time)\n- `2`: CPU_USAGE\n- `3`: MEM_USAGE\n- `4`: CUSTOM (requires `set_small_window_data` API call) |
| `stats_interval_ms` | integer (milliseconds) | `1000` | Interval at which system statistics are updated when `display_mode` is set to show CPU or memory usage |

### OBS Studio Settings (Optional)

These settings enable integration with OBS Studio for scene control and monitoring:

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `obs.host` | string | `localhost` | Hostname or IP address of the OBS WebSocket server |
| `obs.port` | integer (1-65535) | `4444` | Port number of the OBS WebSocket server |
| `obs.password` | string or null | `null` | Password for OBS WebSocket authentication (set to `null` if no password is required) |

### Button Definitions

The `buttons` section defines the configuration for each of the 13 buttons (indices 0-12). Each button can have the following properties:

| Option | Type | Required | Description |
|--------|------|----------|-------------|
| `image` | string (file path) | No | Path to an image file (PNG, JPG, etc.) to display on the button. If omitted, only text label is shown. |
| `label` | string | Yes | Text label to display on the button |
| `action_type` | string | Yes | Type of action to perform when button is pressed. Valid values:\n- `app`: Launch an application\n- `obs`: Send command to OBS Studio\n- `command`: Execute a shell command\n- `key`: Send keyboard shortcut\n- `none`: No action (for display-only buttons) |
| `params` | object | No | Action-specific parameters (see Action Parameters below) |
| `state` | integer | No (defaults to 0) | Initial state value for the button (used by some action types) |

#### Action Parameters

Parameters vary depending on the `action_type`:

**For `app` action:**
- `name`: string - Name/executable of the application to launch

**For `obs` action:**
- `action`: string - OBS command to execute (e.g., `toggle_recording`, `toggle_scene`)
- Additional parameters depend on the specific OBS action:
  - For `toggle_scene`: `scene1` and `scene2` strings defining scenes to toggle between
  - For other actions: refer to OBS WebSocket documentation

**For `command` action:**
- `cmd`: string - Shell command to execute

**For `key` action:**
- `keys`: string - Keyboard shortcut in format like `ctrl+alt+t` or `f1`

## Default Configuration

When no configuration file is found or specified, the driver uses these built-in defaults:

```yaml
brightness: 100
label_style:
  Align: bottom
  Color: 0xFFFFFF
  FontName: Roboto
  ShowTitle: true
  Size: 10
  Weight: 80
display_mode: 1
stats_interval_ms: 1000
buttons: []  # Empty button configuration
```

## Environment Variable Overrides

All configuration options can be overridden using environment variables. The format is:

```
ULANZI_<SECTION>_<SUBSECTION>_<OPTION>=<value>
```

Where:
- `<SECTION>` is the top-level configuration section (empty for root options)
- `<SUBSECTION>` is for nested objects (empty for root options)
- `<OPTION>` is the configuration option name in uppercase

### Examples

Override brightness:
```bash
export ULANZI_BRIGHTNESS=70
```

Override label color:
```bash
export ULANZI_LABEL_STYLE_COLOR=0xFF0000
```

Override OBS host:
```bash
export ULANZI_OBS_HOST=192.168.1.100
```

Override button label (for button 0):
```bash
export ULANZI_BUTTONS_0_LABEL="My Button"
```

**Note:** Environment variable overrides apply after loading from file, so they take precedence over file-based configuration.

## Recommended Settings

### For Streaming Setups
```yaml
brightness: 80  # Reduced to save power during long streams
label_style:
  Align: bottom
  Color: 0x00FF00  # Green for better visibility
  FontName: Roboto
  ShowTitle: true
  Size: 12
  Weight: 900
display_mode: 1  # Show clock
stats_interval_ms: 2000  # Less frequent updates to reduce USB traffic
obs:
  host: localhost
  port: 4444
  password: null  # Set if OBS requires authentication
```

### For Productivity Workstations
```yaml
brightness: 60  # Comfortable for indoor lighting
label_style:
  Align: center
  Color: 0xFFFFFF
  FontName: Roboto
  ShowTitle: true
  Size: 11
  Weight: 700
display_mode: 2  # Show CPU usage
stats_interval_ms: 1000
```

### For Minimal/Power-Saving Usage
```yaml
brightness: 40  # Minimal brightness for status indication only
label_style:
  Align: center
  Color: 0xFFFF00  # Yellow for visibility
  FontName: Roboto
  ShowTitle: false  # Hide labels to reduce visual clutter
  Size: 8
  Weight: 600
display_mode: 0  # Turn off status window to save power
stats_interval_ms: 5000  # Infrequent updates
```

## Configuration Validation

The driver performs basic validation when loading configuration:
- Brightness is clamped to 0-100 range
- Invalid display modes default to 1 (CLOCK)
- Stats interval minimum is 100ms to prevent excessive USB traffic
- Missing required button fields use sensible defaults or skip invalid button definitions

## Tips

1. Use absolute paths for button images when possible, or place images in the same directory as your config file
2. Test configuration changes by restarting the driver (changes only take effect on startup)
3. For OBS integration, ensure the OBS WebSocket server is enabled in OBS Settings → WebSocket Server
4. When using environment variables in containerized deployments, remember they persist for the container lifetime
5. Consider version controlling your config.yaml but excluding any sensitive information like passwords