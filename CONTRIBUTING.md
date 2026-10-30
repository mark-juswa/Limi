# Contributing

Issues and pull requests are welcome.

## Setup

Needs Node 20+, Rust (stable) and, on Windows, the Visual Studio C++ Build Tools.

```bash
npm install
npm run tauri dev
```

## Before opening a pull request

```bash
npx tsc --noEmit          # frontend types
cd src-tauri && cargo test
```

Use the browser preview (`npm run dev`, then `/dev/preview.html`) to check UI changes against mocked data in light and dark.

## Ground rules

- **Accuracy first.** Show only numbers a provider reports. Never estimate or invent usage.
- **No credentials.** Limi must not read, store or send passwords, tokens or API keys. Providers' own CLIs handle sign-in.
- **Everything stays local.** No telemetry, no network calls except the update check.
- Keep dependencies few. Prefer the platform and standard library.

## Adding a provider

A provider returns a `ProviderSnapshot` (see `src-tauri/src/model.rs`): windows with `used_percent`, `resets_at` and `window_minutes`, plus a status. Follow `codex.rs` or `claude_live.rs`, parse with unit tests built from a real captured response, and document the source in the README table.
