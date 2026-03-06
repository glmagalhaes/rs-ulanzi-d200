## User Story 9: Configure Device Namespace

**As a** Developer,
**I want** to identify all usages of `DEVICE_NAMESPACE` in the reference `opendeck-m18` codebase and implement a similar namespace configuration (setting it to "e9") in `rs-ulanzi-d200-linux`,
**So that** the plugin uses a unique and correct namespace identifier within the OpenDeck ecosystem.

**Acceptance Criteria:**

1.  **Reference Analysis:** Locate where `DEVICE_NAMESPACE` is defined and used in `opendeck-m18`.
2.  **Implementation:** Define a `DEVICE_NAMESPACE` constant (value "e9") in `rs-ulanzi-d200-linux` (likely in `device.rs` or `config.rs`).
3.  **Usage:** Update the device ID generation logic to include this namespace, mimicking the reference pattern (e.g., `namespace-serial`).
4.  **Consistency:** Ensure the namespace matches any requirements in `manifest.json` if applicable.

## User Story 10: Cross-Platform Build Pipeline for Binaries

**As a** plugin developer,
**I want** an automated GitLab CI pipeline that builds the `rs-ulanzi-d200-linux` Rust project for Windows, macOS (Intel and Apple Silicon), and Linux (x86_64 and aarch64) using GitLab hosted runners,
**So that** I can easily provide compiled binaries for beta testing before setting up a full plugin packaging pipeline.

**Acceptance Criteria:**

1. The GitLab pipeline successfully builds the `rs-ulanzi-d200-linux` executable for the following target architectures:
   - `x86_64-pc-windows-msvc`
   - `x86_64-apple-darwin`
   - `aarch64-apple-darwin`
   - `x86_64-unknown-linux-gnu`
   - `aarch64-unknown-linux-gnu`
2. The pipeline makes the resulting binaries available as build artifacts so testers can download them directly.
3. The pipeline runs effectively on GitLab's shared/hosted runners.
4. The pipeline is triggered on the tag `beta-release`.
