# ntfy Windows Client

A native Windows desktop client for [ntfy](https://ntfy.sh), written in Rust and Slint.

## Features

- Native WinHTTP streaming and publishing; no browser engine, WebView, async runtime, bundled TLS stack, or polling loop.
- Runs from the system tray and keeps the subscription active after the main window closes.
- Opaque Slint interface forced to the winit software renderer; no GPU renderer, transparency, blur, or idle animation.
- Compact paged interface for connection, notification, publishing, and bounded history settings.
- Custom desktop popup with nine positions across the usable Windows work area.
- Selectable notification audio output: Windows system default or a specific waveform output device.
- Automatic reconnect with bounded exponential backoff and resume from the last ntfy message ID.
- Optional bearer authentication. Tokens remain in memory and are never written to settings.

## Efficiency design

- Closing the main window releases its Slint component tree instead of merely hiding it.
- Starting with `--background` creates the tray listener without opening the main window.
- The notification popup is created only when needed and released after dismissal.
- History is bounded to 64 messages and 2 KiB per retained body.
- One blocking subscription worker uses a 320 KiB stack; short-lived publishing workers use 256 KiB stacks.
- Native Windows TLS, proxy, connection handling, system sound, and waveform output APIs are used directly.
- Release builds use full optimization, fat LTO, one codegen unit, symbol stripping, disabled incremental compilation, and abort-on-panic.

The process working set includes mapped Windows, Slint, winit, font, and networking code. It cannot realistically be guaranteed below 1 MiB. The design instead minimizes private allocations and releases UI resources while running in the tray.

## Build

Official builds use Rust 1.97.1 and Slint 1.17.1 on GitHub-hosted Windows runners.

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
cargo build --locked --release --target x86_64-pc-windows-msvc
```

The manual **CI** workflow verifies the project and uploads x64 and ARM64 ZIP artifacts. The manual **Release** workflow can create or update a GitHub release when a tag is supplied.

## Behaviour

Closing the main window releases it and leaves the subscription running from the system tray. Select **Open ntfy** to construct the interface again, or **Quit** to stop the worker and exit.

Settings are stored in `%APPDATA%\ntfy-windows-client\settings.json`. The bearer token is deliberately excluded.
