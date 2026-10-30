<p align="center">
  <img src="assets/icon.png" alt="Limi" width="140" />
</p>

<h1 align="center">Limi</h1>

<p align="center">
  Your <b>Claude</b> and <b>Codex</b> usage limits, always one glance away.<br />
  A tiny liquid-glass widget for your Windows desktop.
</p>

<p align="center">
  <a href="https://github.com/mark-juswa/Limi/releases/latest">Download</a> ·
  <a href="#how-the-numbers-stay-accurate">Accuracy</a> ·
  <a href="#privacy">Privacy</a> ·
  <a href="CONTRIBUTING.md">Contribute</a>
</p>

---

Working with AI coding assistants means constantly wondering how much of your 5-hour session or weekly limit is left, and when it resets. Limi puts both on the edge of your screen, so you stop opening dashboards to check.

## What it looks like

A slim glass rail sits flush against the screen edge, inspired by the iPhone Duo side rail. Per service it shows:

- **Gauge and number:** how much of the 5-hour session limit is used
- **Big clock:** when that limit resets
- **Four dots:** weekly usage, one dot per 25%

Click it and it springs open into a liquid-glass panel with session and weekly detail, reset times, and exact token counts. Click away and it folds back. Drag it to move it; it docks to the nearest screen edge.

- Light, dark, or Auto (follows Windows)
- Monochrome by design: amber at 75% and red at 90% are the only colors, so a warning stands out
- Windows notification when a limit crosses your chosen threshold (default 85%)
- Lives in the tray, starts with Windows, around 35 MB of memory

## Install

1. Download `Limi_<version>_x64-setup.exe` from the [latest release](https://github.com/mark-juswa/Limi/releases/latest).
2. Run it. It installs for your user only (no admin prompt).

Windows will show a SmartScreen warning because the installer isn't code-signed yet: choose **More info → Run anyway**.

**You need** Windows 10 or 11 (with the WebView2 runtime, built into Windows 11), plus either or both of:

- [Claude Code](https://claude.com/claude-code), signed in on a Pro or Max plan
- [Codex CLI](https://github.com/openai/codex), signed in with your ChatGPT account

Limi finds them automatically; you can set paths in Settings. Updates arrive through the app (Settings → Updates).

## How the numbers stay accurate

Limi only shows numbers the providers report. It never estimates usage.

| Service | Source | Freshness |
|---|---|---|
| Claude | Claude Code's own `get_usage` request, the interface it offers for usage meters. Limi starts Claude Code headless, asks for usage, and exits. No prompt is sent, so checks don't count against your limits. | Every 30 s to 5 min (your choice), and right after any Claude Code activity. |
| Codex | `codex app-server` and its `account/rateLimits/read` request. | Same interval, and right after any Codex activity. |

Both match the usage screens in the Claude and Codex apps, including use from claude.ai and other devices, because the numbers come from your account, not from this PC.

Claude checks run with your hooks, MCP servers and slash commands turned off, so they take about a second and leave nothing behind.

**Token counts.** Limits are reported only as percentages, so Limi adds the exact tokens behind them for activity on this PC. It reads the logs the tools already keep (`~/.claude/projects` and `~/.codex/sessions`) and counts each request once: Claude transcripts repeat a reply's usage across lines and resumed sessions, so they are deduplicated by message and request id; Codex's repeated events are ignored by tracking the running total. Hover a count for the input, output and cache breakdown.

**Optional status line.** Settings → *Show usage in Claude Code's status line* prints your limits inside the Claude Code CLI. Limi backs up your settings file first, keeps any status line you already have working, and restores it when you turn this off or uninstall.

### Limits

- Claude needs Claude Code installed and signed in on a Pro or Max plan. API-key accounts have no plan limits to show; Limi says so instead of showing zeros.
- Token counts cover this PC only. Percentages are account-wide.
- Windows only for now. The code is built on Tauri, so macOS and Linux are possible later.

## Privacy

- **No accounts, no telemetry, no analytics.** The only network request Limi makes itself is the update check to GitHub.
- **No credentials.** Limi never reads, stores or sends passwords, tokens or API keys. Claude Code and Codex handle their own sign-in; Limi just runs them.
- **Local files it reads:** Claude Code and Codex usage logs (for token counts), only to count tokens. Message contents are not stored.
- **Local files it writes:** its own settings (`%LOCALAPPDATA%\app.limi.widget`) and, only if you turn on the status line option, `~/.claude/settings.json` (with a `.limi-backup` copy).
- Open source, so you can check all of the above.

## Build from source

Needs Node 20+, Rust (stable) and, on Windows, the Visual Studio C++ Build Tools.

```bash
npm install
npm run tauri dev        # run the app
npm run tauri build      # build the installer
cd src-tauri && cargo test
```

To work on the UI without Rust, run `npm run dev` and open `http://localhost:1420/dev/preview.html` for a browser preview with mocked data (`?s=crit`, `?s=states`, `?s=left`, `&t=dark`).

```
src/                 widget UI (TypeScript, no framework)
src-tauri/src/
  claude_live.rs     live Claude usage via Claude Code's get_usage
  codex.rs           Codex app-server client
  tokens.rs          token counting from local logs
  claude.rs          optional status line and reading-merge rules
  statusline.rs      `limi --statusline` entry point
  window.rs          docking, edge snapping, click-through outside the glass
  app.rs             state, alerts, commands, tray
```

## Releasing

Pushing a tag like `v0.1.1` runs the [release workflow](.github/workflows/release.yml), which tests, builds, signs the installer and publishes it with the `latest.json` the in-app updater reads. One-time setup: add the updater private key as the repository secret `TAURI_SIGNING_PRIVATE_KEY`. Bump `version` in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json` first.

## Contributing

Issues and pull requests are welcome; see [CONTRIBUTING.md](CONTRIBUTING.md). Security reports: [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE)
