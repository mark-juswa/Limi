// Dev-only harness: fakes the Rust backend so the UI can be reviewed in a browser.
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";

const q = new URLSearchParams(location.search);
const s = q.get("s") ?? "normal";
const theme = q.get("t") ?? "light";
const now = Date.now() / 1000;
const win = (id: string, label: string, pct: number, inSec: number, mins: number) => ({
  id, label, usedPercent: pct, resetsAt: Math.round(now + inSec), windowMinutes: mins,
});

const claudeOk = {
  id: "claude", name: "Claude", status: "ok", message: null, plan: null,
  windows: [win("session", "5-hour session", s === "crit" ? 93 : 42, 2 * 3600 + 14 * 60, 300), win("weekly", "Weekly", s === "crit" ? 78 : 18, 3 * 86400 + 5 * 3600, 10080)],
  capturedAt: Date.now() - 6 * 60_000, source: "Claude Code status line", live: false,
};
const codexOk = {
  id: "codex", name: "Codex", status: "ok", message: null, plan: "Plus",
  windows: [win("session", "5-hour session", 69, 57 * 60, 300), win("weekly", "Weekly", 64, 4 * 86400 + 20 * 3600, 10080)],
  capturedAt: Date.now() - 5_000, source: "Codex app-server", live: true,
};
const usage =
  s === "states"
    ? {
        claude: { ...claudeOk, status: "not_connected", windows: [], message: "Connect Claude Code so Headroom can read the limits it reports." },
        codex: { ...codexOk, status: "signed_out", windows: [], plan: null, message: "Codex isn't signed in. Run `codex login` with your ChatGPT account." },
      }
    : { claude: claudeOk, codex: codexOk };
const tok = (session: number, weekly: number) => ({
  session: { input: 1200, output: 84_000, cacheRead: session * 0.9, cacheWrite: session * 0.08, total: session, requests: 212 },
  weekly: { input: 9800, output: 610_000, cacheRead: weekly * 0.9, cacheWrite: weekly * 0.08, total: weekly, requests: 1840 },
  lastActivityAt: Date.now() - (s === "behind" ? 3 * 60_000 : 7 * 60_000),
});
(usage as any).tokens = { claude: tok(12_400_000, 88_100_000), codex: tok(3_200_000, 41_700_000) };

const settings = {
  theme,
  placement: q.get("p") ?? "side",
  barStyle: q.get("b") ?? "island",
  claudeEnabled: true, codexEnabled: true, codexPath: null, pollSeconds: 60, alertThreshold: 85,
  alwaysOnTop: true, edge: s === "left" ? "left" : "right", topRatio: 0.3, monitor: null,
};

mockWindows("main");
let pendingRailTop: ((v: number) => void) | null = null;
addEventListener("message", (e) => pendingRailTop?.(e.data.railTop));

mockIPC(async (cmd, args: any) => {
  switch (cmd) {
    case "get_state":
      return { settings, usage, version: "0.1.0", autostart: true, claudeStatusLine: false };
    case "set_layout": {
      const railTop = new Promise<number>((r) => (pendingRailTop = r));
      parent.postMessage({ ...args, edge: settings.edge, topRatio: settings.topRatio, placement: settings.placement }, "*");
      return { edge: settings.edge, railTop: await railTop, height: args.height };
    }
    case "update_settings":
      Object.assign(settings, args.patch);
      return settings;
    case "snap_to_edge":
      return settings;
    case "check_update":
      return null;
    default:
      return null;
  }
}, { shouldMockEvents: true });

await import("../src/main.ts");
