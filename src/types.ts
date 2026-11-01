export type Status =
  | "ok"
  | "waiting"
  | "not_installed"
  | "signed_out"
  | "no_limits"
  | "error"
  | "disabled";

export interface UsageWindow {
  id: string;
  label: string;
  usedPercent: number;
  /** Epoch seconds. */
  resetsAt: number | null;
  windowMinutes: number | null;
}

export interface Provider {
  id: "claude" | "codex";
  name: string;
  status: Status;
  message: string | null;
  plan: string | null;
  windows: UsageWindow[];
  /** Epoch ms. */
  capturedAt: number | null;
  source: string;
  live: boolean;
}

export interface TokenTotals {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  total: number;
  requests: number;
}

export interface TokenUsage {
  session: TokenTotals;
  weekly: TokenTotals;
  /** Epoch ms of the latest request in the local logs. */
  lastActivityAt: number | null;
}

export interface Usage {
  claude: Provider;
  codex: Provider;
  tokens: { claude: TokenUsage | null; codex: TokenUsage | null };
}

export type Theme = "system" | "light" | "dark";

export type Edge = "left" | "right";
export type Placement = "side" | "top" | "bottom";
export type BarStyle = "notch" | "island";

export interface Settings {
  theme: Theme;
  placement: Placement;
  barStyle: BarStyle;
  claudeEnabled: boolean;
  codexEnabled: boolean;
  codexPath: string | null;
  claudePath: string | null;
  pollSeconds: number;
  alertThreshold: number;
  alwaysOnTop: boolean;
  edge: Edge;
  topRatio: number;
  monitor: string | null;
  pomodoroEnabled: boolean;
  focusMinutes: number;
  breakMinutes: number;
  longBreakMinutes: number;
}

export type Phase = "focus" | "break" | "longBreak";

export interface Pomodoro {
  phase: Phase;
  running: boolean;
  /** Epoch ms when the phase ends (while running). */
  endsAt: number | null;
  remainingMs: number;
  totalMs: number;
  /** Focus rounds finished in this cycle. */
  done: number;
  rounds: number;
}

export interface AppState {
  settings: Settings;
  usage: Usage;
  version: string;
  autostart: boolean;
  claudeStatusLine: boolean;
  pomodoro: Pomodoro;
}

export interface LayoutInfo {
  edge: Edge;
  railTop: number;
  height: number;
}
