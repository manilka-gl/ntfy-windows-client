# ntfy Windows Client

A small native Windows desktop client for [ntfy](https://ntfy.sh), written in Rust with Slint.

## Current features

- Subscribe to an ntfy JSON stream with automatic reconnect and incremental backoff.
- Recover recent cached messages and resume from the last message ID after reconnects.
- Publish messages with an optional title.
- Optional bearer-token authentication. Tokens are kept in memory and are not written to disk.
- Native Windows toast notifications through `windows-rs`.
- Bounded in-memory history of 200 messages and a 1 MiB per-message stream safety limit.
- Persistent server, topic, and notification preference under `%APPDATA%\ntfy-windows-client\settings.json`.

## Efficiency choices

- Slint software renderer; no browser engine, Electron, WebView, GPU renderer, or async runtime.
- One blocking subscription worker and short-lived publish workers.
- Bounded models and input sizes.
- Release builds use size optimization, full LTO, one codegen unit, stripped symbols, and abort-on-panic.

The supplied ntfy server executable is not embedded. The client talks directly to ntfy's HTTP JSON stream, avoiding the roughly 74 MB server/CLI binary.

## Build

Requires Rust 1.85 or newer.

```powershell
cargo build --release
```

The executable is created at `target\release\ntfy-windows-client.exe`.

## Verification and binaries

GitHub Actions runs formatting, Clippy with warnings denied, tests, debug builds, and optimized Windows builds. Every CI run uploads a Windows x64 ZIP artifact. Pushing a tag such as `v0.1.0` also creates a GitHub release with the ZIP attached.

## Notes

- Topic names follow ntfy's `[-_A-Za-z0-9]{1,64}` rule.
- Windows may suppress notifications through Focus Assist or notification settings.
- Native toast delivery from an unpackaged executable is best-effort on Windows installations with stricter AppUserModelID registration policies. The in-app feed remains functional if Windows rejects a toast.
