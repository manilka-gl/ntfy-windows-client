# ntfy Windows Client

A lightweight native Windows desktop client for [ntfy](https://ntfy.sh), written in Rust and Slint.

## Features

- Native WinHTTP streaming and publishing; no browser engine, WebView, Tokio, Reqwest, OpenSSL bundle, or background polling loop.
- Runs in the system tray after the main window is closed and keeps the subscription active.
- Reusable custom desktop popup with nine user-selectable positions: top, middle, and bottom across left, centre, and right.
- Optional Windows system notification sound.
- Bounded 100-message in-memory history and 1 MiB safety limits for incoming stream lines and published messages.
- Automatic reconnect with bounded exponential backoff and resume from the last ntfy message ID.
- Optional bearer authentication. Tokens stay in memory and are never written to the settings file.
- Consistent Fluent dark interface and software rendering.

## Efficiency design

- One blocking subscription worker with a 320 KiB stack.
- Short-lived publish workers with 256 KiB stacks.
- One reusable popup window and one timer instead of allocating a new notification window per event.
- Native Windows TLS, proxy, and connection handling through WinHTTP.
- Release builds use size optimisation, fat LTO, one codegen unit, symbol stripping, disabled incremental compilation, and abort-on-panic.
- Only required Slint and Windows API features are enabled.

## Build

Official builds use Rust 1.97.1 and Slint 1.17.1 on GitHub-hosted Windows runners.

```powershell
cargo generate-lockfile
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
cargo build --locked --release --target x86_64-pc-windows-msvc
```

The manual **CI** workflow verifies the project and uploads x64 and ARM64 ZIP artifacts. The manual **Release** workflow can also create or update a GitHub release when a tag is supplied.

## Behaviour

Closing the main window hides it and leaves the process running in the Windows system tray. Use the tray menu's **Quit** command to stop the subscription and exit.

Settings are stored in `%APPDATA%\ntfy-windows-client\settings.json`. The bearer token is deliberately excluded.
