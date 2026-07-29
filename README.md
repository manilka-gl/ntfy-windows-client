# ntfy Windows Client

A native Windows desktop client for [ntfy](https://ntfy.sh), written in Rust and Slint.

## Features

- Native WinHTTP streaming and publishing; no browser engine, WebView, async runtime, bundled TLS stack, or polling loop.
- Runs from the system tray and keeps the subscription active after the main window closes.
- Opaque Slint interface forced to the winit software renderer; no GPU renderer, transparency, blur, or idle animation.
- Compact tabbed interface for connection, notification, publishing, and bounded history settings.
- Readable 404 × 180 desktop popup with nine positions and physical-pixel fallback sizing so the complete window remains inside the Windows work area.
- Selectable notification audio output: Windows system default or a specific waveform output device.
- Automatic reconnect with bounded exponential backoff and resume from the last ntfy message ID.
- Optional bearer authentication. Tokens remain in memory and are never written to settings.

## Efficiency design

- Closing the main window destroys its Slint component tree instead of retaining a hidden interface.
- Starting with `--background` creates the tray listener without opening the main window.
- The notification popup is created only when needed, released after dismissal, and never retained through its own callback.
- Windows working-set trimming is requested after background startup, UI destruction, popup destruction, and subscription shutdown.
- Audio devices are enumerated only when the main interface opens, not during tray-only startup.
- History is bounded to 32 messages and 1 KiB per retained body.
- One blocking subscription worker uses a bounded stack; short-lived publishing and sound workers also use bounded stacks.
- Native Windows TLS, proxy, connection handling, system sound, and waveform output APIs are used directly.
- Release builds use full optimization, fat LTO, one codegen unit, symbol stripping, disabled incremental compilation, and abort-on-panic.

## Measured background memory

The continuous Windows worker builds the x64 release, opens the redesigned UI for a smoke test, then starts a real background subscription with the UI closed. On GitHub's Windows runner on July 29, 2026, three consecutive samples reported:

- Working set: **438,272 bytes (0.42 MiB)**.
- Private committed bytes: **3,432,448 bytes (3.27 MiB)**.
- Threads: **11**.

The enforced tray-listener working-set limit is 2 MiB. Working set and private committed memory are different Windows metrics: trimming can evict resident pages while preserving committed state that can be faulted back when notifications or the UI are used. Actual values can vary with Windows version, drivers, fonts, server behavior, and notification activity. The committed measurement is stored in `output/memory-report.json`.

## Build

Official builds use Rust 1.97.1 and Slint 1.17.1 on GitHub-hosted Windows runners.

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
cargo build --locked --release --target x86_64-pc-windows-msvc
```

The continuous worker additionally verifies foreground startup, an active background listener, the 2 MiB working-set ceiling, and creation of `output/ntfy-windows-client-x64.zip`.

## Behaviour

Closing the main window releases it and leaves the subscription running from the system tray. Select **Open ntfy** to construct the interface again, or **Quit** to stop the worker and exit.

Settings are stored in `%APPDATA%\ntfy-windows-client\settings.json`. The bearer token is deliberately excluded.
