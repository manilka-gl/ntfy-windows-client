# ntfy Windows client

A native ntfy desktop client focused on low idle CPU, low RAM use, and a small release executable.

## Efficiency design

- Rust and Slint with the software renderer.
- Native Windows WinHTTP; no Tokio, Reqwest, OpenSSL, or bundled browser engine.
- One cancellable blocking subscription thread while connected.
- No timer-based network polling while a subscription is active.
- Notification history is bounded to 64 entries.
- Incoming event lines are capped at 1 MiB.
- Release builds use size optimization, fat LTO, one codegen unit, symbol stripping, and abort-on-panic.
- Only required Windows API and Slint features are enabled.

## Features

- Subscribe to ntfy topics over HTTP or HTTPS.
- Bearer-token authentication.
- Native Windows toast notifications.
- Publish text messages.
- Self-hosted server paths and custom ports.
- System tray support.
- x64 and ARM64 executable artifacts from GitHub Actions.

## Build and validation

All official checks and builds run on GitHub-hosted Windows runners:

```text
cargo fmt --all -- --check
cargo test --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings
cargo build --release --locked --target <target>
```

The CI workflow uploads executables as workflow artifacts. Pushing a `v*` tag creates a GitHub release with x64 and ARM64 executables.

## Security notes

The optional access token is stored in `%APPDATA%\\NtfyWindowsClient\\settings.json` as plain text. A future version can add Windows Credential Manager storage without increasing idle resource use.

## Supported topics

For predictable URLs and header safety, this first version accepts topic names containing 1-64 ASCII letters, digits, hyphens, or underscores.
