# OpenDeck Ulanzi D200 Driver (Unofficial)

An unofficial plugin for [OpenDeck](https://github.com/nekename/OpenDeck) that adds support for the Ulanzi D200 and D200H devices.

> **Note**: This project is mirrored on GitHub for visibility, but the official source is on [GitLab](https://gitlab.com/glmagalhaes.mail/rs-ulanzi-d-200-linux). Please open issues there.

---

## Supported Devices

- Ulanzi D200
- Ulanzi D200H (USB ID `2207:0019`)

The D200H is identical to the D200 but includes two additional USB hubs (Genesys Logic, Inc., `05e3:0610`).

---

## Platform Support

| Platform | Status |
|----------|--------|
| Linux    | ✅ Supported (actively developed and tested) |
| Windows  | ❌ Planned (see roadmap) |
| macOS    | ❌ Planned (see roadmap) |

If you would like to help port the plugin to another platform, feel free to contribute!

---

## Installation

1. Download the latest file from the [releases page](https://gitlab.com/glmagalhaes.mail/rs-ulanzi-d-200-linux/-/releases).
2. In OpenDeck, go to **Plugins → Install from file** and select the archive.
3. The plugin will appear in your plugin list.

> The plugin will be available via the OpenDeck Store in a future release.

---

## Actions

### Screen Switch

Cycles the built‑in status display of the wide button through three modes:

- **Blank** – it will show empty or the icon of your choice
- **Clock** – show current time
- **PC stats** – displays CPU, RAM, and GPU load (GPU support added in v0.6.3)

This action does **not** affect the button’s ability to send key presses. It only changes the visual information shown on the device’s screen.

---

## Building from Source

Requirements: Rust, Cargo, and standard build tools (e.g., `git`, `make`).

The repository includes a `pack.sh` script that compiles the plugin and packages it as a `.zip` file.

```sh
# Debug build (output in target/debug/)
sh pack.sh

# Release build (optimized, output in target/release/)
sh pack.sh release
```

> **Note**: The script assumes a typical Rust environment. If you encounter issues, ensure cargo is in your $PATH.

---

## Known issues

There is at least one and will be lested soonish

---

## Road Map

The road map is really short because the plug-in is already working withou any problems and all the main features are already done

### v0.6.3
- [x] Support for GPU load in status window #8
- [ ] Better organization in code
- [x] Change in plugin namming, internal and external #11
- [ ] Stability updates
- [ ] Launch on OpenDeck Store #10

### v1.0.0
- [ ] Community testing phase completed
- [ ] Better icon for the Screen Switch action
- [x] Better naming for actions and categories (completed in v0.6.3)
- [ ] Stability updates

### Future (help wanted)
- [ ] Support for macOS
- [ ] Support for Windows

## Contributing

Contributions are welcome! Please:

- Use the [GitLab](https://gitlab.com/glmagalhaes.mail/rs-ulanzi-d-200-linux) repository (the GitHub mirror is read‑only).
- Open an issue first to discuss major changes.

## License
This project is licensed under the GNU Affero General Public License v3.0 – see the LICENSE file for details.