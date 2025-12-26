## User Story 9: Create a Scaffolding OpenDeck Plugin for Ulanzi D200

**As a** developer,
**I want** to create a basic, native Rust-based OpenDeck plugin that can be successfully loaded and registered by the OpenDeck host application,
**So that** I have a foundational structure to build upon for implementing Ulanzi D200-specific device control features.

**Acceptance Criteria:**

1.  A new Rust project for the plugin is created in the `/home/michael/Documents/ulanzi-d200-linux/opendeck-ulanzi-plugin` directory.
2.  A `manifest.json` file is created for the plugin, defining its essential metadata (UUID, Name, Author, etc.) and specifying the path to the compiled Rust binary via the `CodePathLin` property.
3.  The plugin's Rust binary can parse the command-line arguments (`-port`, `-pluginUUID`, `-registerEvent`, `-info`) passed by OpenDeck upon launch.
4.  The plugin successfully establishes a WebSocket connection to the port provided by OpenDeck.
5.  Upon connecting, the plugin sends the required registration event over the WebSocket to the OpenDeck host.
6.  The plugin includes basic logging to a file to confirm that it receives core lifecycle events from OpenDeck (e.g., `deviceDidConnect`, `willAppear`, `keyDown`).
7.  The plugin is structured to include the `rs-ulanzi-d200-linux` library as a local dependency (using a path dependency in `Cargo.toml`) for future use.

---