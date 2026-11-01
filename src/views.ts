import {
  ago,
  clampPct,
  clockShort,
  compactTokens,
  countdown,
  escapeHtml,
  isReset,
  level,
  resetClock,
  richText,
  sessionWindow,
  weeklyWindow,
} from "./format";
import type { AppState, Pomodoro, Provider, Settings, TokenTotals, Usage } from "./types";

type Pid = "claude" | "codex";

// ------------------------------------------------------------------ icons

export const icons = {
  claude: `<svg viewBox="0 0 24 24" aria-hidden="true"><g stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M12 3.5v17"/><path d="M3.5 12h17"/><path d="M6 6l12 12"/><path d="M18 6L6 18"/></g></svg>`,
  codex: `<svg viewBox="0 0 24 24" aria-hidden="true"><g fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M6 8l4 4-4 4"/><path d="M12.5 16.5H18"/></g></svg>`,
  refresh: `<svg viewBox="0 0 24 24" aria-hidden="true"><g fill="none" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round"><path d="M20 11a8 8 0 1 0-2.3 5.7"/><path d="M20 4v7h-7"/></g></svg>`,
  gear: `<svg viewBox="0 0 24 24" aria-hidden="true"><g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/></g></svg>`,
  collapse: `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 6l6 6-6 6" fill="none" stroke="currentColor" stroke-width="2.3" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
  back: `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M15 6l-6 6 6 6" fill="none" stroke="currentColor" stroke-width="2.3" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
  alert: `<svg viewBox="0 0 24 24" aria-hidden="true"><g fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M12 7.5v5.5"/><path d="M12 16.5v.01"/></g></svg>`,
  plug: `<svg viewBox="0 0 24 24" aria-hidden="true"><g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 3v5M15 3v5M6 8h12v3a6 6 0 0 1-12 0z"/><path d="M12 17v4"/></g></svg>`,
  clock: `<svg viewBox="0 0 24 24" aria-hidden="true"><g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="8"/><path d="M12 8v4l2.5 2"/></g></svg>`,
  sun: `<svg viewBox="0 0 24 24" aria-hidden="true"><g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="4" fill="currentColor"/><path d="M12 2.5v2.2M12 19.3v2.2M2.5 12h2.2M19.3 12h2.2M5.3 5.3l1.6 1.6M17.1 17.1l1.6 1.6M5.3 18.7l1.6-1.6M17.1 6.9l1.6-1.6"/></g></svg>`,
  moon: `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z" fill="currentColor"/></svg>`,
  cloud: `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7.5 18.5h9.2a3.8 3.8 0 0 0 .6-7.55A5.6 5.6 0 0 0 6.6 10.6a4 4 0 0 0 .9 7.9z" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"/></svg>`,
  timer: `<svg viewBox="0 0 24 24" aria-hidden="true"><g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="13.5" r="7.5"/><path d="M12 13.5V9.5M9.5 2.5h5M18.5 6l1.3-1.3"/></g></svg>`,
  skip: `<svg viewBox="0 0 24 24" aria-hidden="true"><g fill="currentColor"><path d="M5.5 6.2v11.6c0 .8.9 1.3 1.6.8l8.2-5.8a1 1 0 0 0 0-1.6L7.1 5.4c-.7-.5-1.6 0-1.6.8z"/><rect x="16.6" y="5.5" width="2.6" height="13" rx="1.3"/></g></svg>`,
  auto: `<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="8" fill="none" stroke="currentColor" stroke-width="2"/><path d="M12 4a8 8 0 0 1 0 16z" fill="currentColor"/></svg>`,
};

// ------------------------------------------------------------------ gauges

/** 300° arc, gap at the bottom. */
function arc(r: number): string {
  const rad = (deg: number) => (deg * Math.PI) / 180;
  const p = (deg: number) => `${(50 + r * Math.cos(rad(deg))).toFixed(2)} ${(50 + r * Math.sin(rad(deg))).toFixed(2)}`;
  return `M ${p(120)} A ${r} ${r} 0 1 1 ${p(60)}`;
}

/** Duo gauge: thick ink progress over a light remainder. */
function gauge(key: string, r: number, w: number): string {
  const d = arc(r);
  return `<svg class="ring-svg" viewBox="0 0 100 100" aria-hidden="true">
    <path class="track" d="${d}" stroke-width="${w}"/>
    <path class="prog" data-ring="${key}" d="${d}" stroke-width="${w}" pathLength="100"/>
  </svg>`;
}

/** Arc between two angles (degrees, clockwise from 3 o'clock), radius r. */
function arcBetween(r: number, from: number, to: number, clockwise: boolean): string {
  const rad = (deg: number) => (deg * Math.PI) / 180;
  const p = (deg: number) => `${(50 + r * Math.cos(rad(deg))).toFixed(2)} ${(50 + r * Math.sin(rad(deg))).toFixed(2)}`;
  const span = Math.abs(to - from);
  return `M ${p(from)} A ${r} ${r} 0 ${span > 180 ? 1 : 0} ${clockwise ? 1 : 0} ${p(to)}`;
}

/**
 * Resting state: one small Duo gauge for everything. With two services the
 * ring splits in half (Claude left, Codex right), each filling upward from the
 * bottom. Dots below show weekly usage, two per service.
 */
export function relaxHtml(state: AppState): string {
  if (state.settings.placement !== "side") return relaxBarHtml(state);
  const ps = visibleProviders(state).filter(hasNumbers);
  const r = 40;
  const w = 11;
  let arcs = "";
  if (ps.length >= 2) {
    const halves: [Provider, string][] = [
      [ps[0], arcBetween(r, 120, 264, true)],
      [ps[1], arcBetween(r, 60, -84, false)],
    ];
    arcs = halves
      .map(
        ([p, d]) => `<path class="track" d="${d}" stroke-width="${w}"/>
        <path class="prog" data-ring="${p.id}:session" d="${d}" stroke-width="${w}" pathLength="100"/>`,
      )
      .join("");
  } else {
    const d = arc(r);
    const key = ps[0] ? `${ps[0].id}:session` : "none";
    arcs = `<path class="track" d="${d}" stroke-width="${w}"/><path class="prog" data-ring="${key}" d="${d}" stroke-width="${w}" pathLength="100"/>`;
  }
  const dots = ps.length >= 2 ? ps.slice(0, 2).map((p) => `<span class="rdots" data-rdots="${p.id}:2"><i></i><i></i></span>`).join("") : ps[0] ? `<span class="rdots" data-rdots="${ps[0].id}:4"><i></i><i></i><i></i><i></i></span>` : "";
  const names = ps.map((p) => p.name).join(" and ");
  return `<div class="relax-ring" title="${escapeHtml(names || "Limi")}">
      <svg class="ring-svg" viewBox="0 0 100 100" aria-hidden="true">${arcs}</svg>
      <span class="relax-center" data-relax-center></span>
    </div>
    <div class="relax-dots">${dots}</div>
    ${pomoOn(state) ? `<div class="relax-pomo" data-pomo>${flipClock("stack")}</div>` : ""}`;
}

/** Top/bottom resting pill: a tiny gauge and percentage per service, side by side. */
/**
 * Top/bottom bar, one row per state. Resting: gauge and percentage per
 * service. Hover (`wide`): the same row grows sideways with each service's
 * reset time and weekly usage — never taller.
 */
function barHtml(state: AppState, wide: boolean): string {
  // Same language as the side rail: gauge with the number inside, bold reset
  // time, icon and weekly dots — laid out in a row so it only grows sideways.
  const items = visibleProviders(state).map((p) => {
    const key = `${p.id}:session`;
    const inner = hasNumbers(p)
      ? `<b class="num display" data-pct="${key}">0</b>`
      : `<span class="glyph${p.status === "waiting" ? "" : " warn"}">${p.status === "waiting" ? icons.clock : icons.alert}</span>`;
    const ring = `<span class="ring hring">${gauge(key, 41, 11)}${inner}</span>`;
    const side =
      wide && hasNumbers(p)
        ? `<span class="hcol">
            <span class="clock display" data-clock="${key}">—</span>
            <span class="hsub"><span class="pglyph">${icons[p.id]}</span><span class="dots" data-dots="${p.id}:weekly"><i></i><i></i><i></i><i></i></span></span>
          </span>`
        : `<span class="pglyph">${icons[p.id]}</span>`;
    return `<span class="hitem rail-item" data-provider="${p.id}" data-level-of="${p.id}" title="${escapeHtml(p.name)}">${ring}${side}</span>`;
  });
  if (pomoOn(state)) {
    items.push(
      `<span class="hitem pomo-hitem" data-pomo>${flipClock("row")}${wide ? `<span class="word" data-pomo-word></span>` : ""}</span>`,
    );
  }
  return `<div class="hrelax${wide ? " wide" : ""}">${items.join(`<span class="hsep"></span>`) || icons.cloud}</div>`;
}

const relaxBarHtml = (state: AppState) => barHtml(state, false);

export function visibleProviders(state: AppState): Provider[] {
  const { claude, codex } = state.usage;
  return [claude, codex].filter((p) => p.status !== "disabled");
}

const hasNumbers = (p: Provider) => (p.status === "ok" || p.status === "error") && p.windows.length > 0;

// ------------------------------------------------------------------ rail

const railWord: Partial<Record<Provider["status"], string>> = {
  waiting: "Waiting",
  not_installed: "Missing",
  signed_out: "Sign in",
  no_limits: "No limit",
  error: "Error",
};

function railItem(p: Provider): string {
  const key = `${p.id}:session`;
  if (hasNumbers(p)) {
    return `<div class="rail-item" data-provider="${p.id}" data-level-of="${p.id}">
      <div class="ring">${gauge(key, 41, 11)}<b class="num display" data-pct="${key}">0</b></div>
      <div class="clock display" data-clock="${key}">—</div>
      <div class="pglyph">${icons[p.id]}</div>
      <div class="dots" data-dots="${p.id}:weekly"><i></i><i></i><i></i><i></i></div>
    </div>`;
  }
  const glyph =
    p.status === "waiting" ? icons.clock : icons.alert;
  const warn = p.status === "waiting" ? "" : " warn";
  return `<div class="rail-item" data-provider="${p.id}">
    <div class="ring">${gauge(key, 41, 11)}<span class="glyph${warn}">${glyph}</span></div>
    <div class="word">${railWord[p.status] ?? ""}</div>
    <div class="pglyph">${icons[p.id]}</div>
  </div>`;
}

export function railHtml(state: AppState): string {
  if (state.settings.placement !== "side") return barHtml(state, true);
  const providers = visibleProviders(state);
  const items = providers.length
    ? providers.map(railItem).join("")
    : `<div class="rail-item"><div class="ring">${gauge("none", 41, 11)}<span class="glyph">${icons.gear}</span></div><div class="word">Setup</div></div>`;
  const pomo = pomoOn(state)
    ? `<div class="rail-item pomo-item" data-pomo>${flipClock("stack")}<div class="word" data-pomo-word></div></div>`
    : "";
  return `<span class="lens prism" aria-hidden="true"></span>${items}${pomo}`;
}

// ------------------------------------------------------------------ focus timer

const pomoOn = (state: AppState) => state.settings.pomodoroEnabled;

/** One flip card: static halves plus two flaps that turn on each change. */
const flipCard = (key: string) =>
  `<span class="flip" data-flip="${key}"><span class="fl top"><b></b></span><span class="fl bot"><b></b></span><span class="fl flap-top"><b></b></span><span class="fl flap-bot"><b></b></span></span>`;

/** Minutes and seconds as two flip cards. */
function flipClock(kind: "stack" | "row" | "big"): string {
  return `<span class="flipclock ${kind}" data-pomo-clock aria-hidden="true">${flipCard("mm")}${flipCard("ss")}</span>`;
}

const phaseName: Record<Pomodoro["phase"], string> = { focus: "Focus", break: "Short break", longBreak: "Long break" };

function pomoCardHtml(state: AppState): string {
  const rounds = Array.from({ length: state.pomodoro.rounds }, () => "<i></i>").join("");
  return `<section class="card well pomo-card" data-pomo style="--i:0">
    <header class="card-head">
      <span class="pglyph">${icons.timer}</span>
      <span class="pname">Focus timer</span>
      <span class="fresh" data-pomo-phase></span>
    </header>
    <div class="pomo-clock">${flipClock("big")}</div>
    <div class="pomo-foot">
      <button class="orb" data-action="pomo-reset" aria-label="Restart">${icons.refresh}</button>
      <div class="pomo-mid">
        <button class="btn primary" data-action="pomo-toggle" data-pomo-toggle>Start</button>
        <span class="dots pomo-rounds" data-pomo-rounds title="Focus rounds before a long break">${rounds}</span>
      </div>
      <button class="orb" data-action="pomo-skip" aria-label="Skip to next">${icons.skip}</button>
    </div>
  </section>`;
}

const flipTimers = new WeakMap<Element, number>();

/** Show `value` on a flip card, turning the flaps when it changes. */
function setFlip(card: HTMLElement, value: string) {
  const old = card.dataset.v;
  if (old === value) return;
  card.dataset.v = value;
  const [top, bot, flapTop, flapBot] = Array.from(card.querySelectorAll("b"));
  const still = old === undefined || matchMedia("(prefers-reduced-motion: reduce)").matches;
  if (still) {
    for (const b of [top, bot, flapTop, flapBot]) b.textContent = value;
    return;
  }
  top.textContent = value;
  bot.textContent = old;
  flapTop.textContent = old;
  flapBot.textContent = value;
  card.classList.remove("go");
  void card.offsetWidth;
  card.classList.add("go");
  clearTimeout(flipTimers.get(card));
  flipTimers.set(
    card,
    window.setTimeout(() => {
      bot.textContent = card.dataset.v ?? value;
      card.classList.remove("go");
    }, 560),
  );
}

export function pomoRemaining(t: Pomodoro): number {
  return t.running && t.endsAt !== null ? Math.max(0, t.endsAt - Date.now()) : t.remainingMs;
}

/** Update every timer on screen. Cheap when nothing changed. */
export function paintPomo(root: HTMLElement, state: AppState) {
  const t = state.pomodoro;
  if (!t || !state.settings.pomodoroEnabled) return;
  const secs = Math.ceil(pomoRemaining(t) / 1000);
  const mm = String(Math.floor(secs / 60)).padStart(2, "0");
  const ss = String(secs % 60).padStart(2, "0");
  const fresh = !t.running && t.remainingMs === t.totalMs;
  const word = t.running ? (t.phase === "focus" ? "Focus" : "Break") : fresh ? (t.phase === "focus" ? "Ready" : "Break") : "Paused";
  root.querySelectorAll<HTMLElement>("[data-pomo]").forEach((el) => {
    el.dataset.run = t.running ? "1" : "0";
    el.dataset.phase = t.phase;
  });
  root.querySelectorAll<HTMLElement>("[data-pomo-clock]").forEach((el) => {
    const cards = el.querySelectorAll<HTMLElement>("[data-flip]");
    setFlip(cards[0], mm);
    setFlip(cards[1], ss);
  });
  root.querySelectorAll<HTMLElement>("[data-pomo-word]").forEach((el) => {
    if (el.textContent !== word) el.textContent = word;
  });
  root.querySelectorAll<HTMLElement>("[data-pomo-phase]").forEach((el) => {
    const text = `${phaseName[t.phase]}${!t.running && !fresh ? " · Paused" : ""}`;
    if (el.textContent !== text) el.textContent = text;
  });
  root.querySelectorAll<HTMLElement>("[data-pomo-toggle]").forEach((el) => {
    const text = t.running ? "Pause" : fresh ? "Start" : "Resume";
    if (el.textContent !== text) el.textContent = text;
  });
  root.querySelectorAll<HTMLElement>("[data-pomo-rounds]").forEach((el) => {
    // Finished rounds are solid; the one in progress is outlined.
    const lit = t.done + (t.phase === "focus" && !fresh ? 1 : 0);
    el.querySelectorAll("i").forEach((d, i) => {
      d.classList.toggle("on", i < t.done);
      d.classList.toggle("now", i >= t.done && i < lit);
    });
  });
  const label = `${phaseName[t.phase]}: ${Math.floor(secs / 60)} min ${secs % 60} s left${t.running ? "" : " (paused)"}`;
  root.querySelectorAll<HTMLElement>("[data-pomo]").forEach((el) => {
    if (el.title !== label) el.title = label;
  });
}

// ------------------------------------------------------------------ panel

function tokenTitle(t: TokenTotals): string {
  const f = (n: number) => n.toLocaleString();
  return `${f(t.requests)} requests · ${f(t.input)} input · ${f(t.output)} output · ${f(t.cacheRead)} cache read · ${f(t.cacheWrite)} cache write`;
}

function tokensHtml(pid: Pid): string {
  return `<div class="tokens">
    <div class="tok" data-tok="${pid}:session"><small>Session tokens</small><b class="display">—</b></div>
    <div class="tok" data-tok="${pid}:weekly"><small>Week tokens</small><b class="display">—</b></div>
  </div>`;
}

const statusCopy: Record<string, { title: string; action?: { id: string; label: string } }> = {
  waiting: { title: "Checking usage…" },
  not_installed: { title: "Not found", action: { id: "refresh", label: "Check again" } },
  signed_out: { title: "Not signed in", action: { id: "refresh", label: "Check again" } },
  no_limits: { title: "No plan limits" },
  error: { title: "Couldn't load usage", action: { id: "refresh", label: "Try again" } },
};

function cardHtml(p: Provider, i: number): string {
  const head = `<header class="card-head">
      <span class="pglyph">${icons[p.id]}</span>
      <span class="pname">${escapeHtml(p.name)}</span>
      ${p.plan ? `<span class="chip">${escapeHtml(p.plan)}</span>` : ""}
      <span class="fresh" data-fresh="${p.id}"></span>
    </header>`;

  if (hasNumbers(p)) {
    const s = sessionWindow(p);
    const w = weeklyWindow(p);
    const error = p.status === "error" && p.message ? `<p class="note warn">${richText(p.message)}</p>` : "";
    return `<section class="card well" data-provider="${p.id}" style="--i:${i}">
      ${head}
      <div class="card-main">
        <div class="big-ring">${gauge(`${p.id}:session`, 42, 10)}
          <div class="big-num"><b class="display" data-pct="${p.id}:session">0</b><small>%</small></div>
        </div>
        <div>
          <div class="eyebrow">${escapeHtml(s?.label ?? "Session")} resets</div>
          <div class="big-clock display" data-clock="${p.id}:session">—</div>
          <div class="sub" data-left="${p.id}:session"></div>
        </div>
      </div>
      ${
        w
          ? `<div class="weekly">
        <div class="weekly-top"><span class="eyebrow">${escapeHtml(w.label)}</span><b class="display"><span data-pct="${p.id}:weekly">0</span>%</b></div>
        <div class="bar"><i data-bar="${p.id}:weekly"></i></div>
        <div class="sub" data-reset-line="${p.id}:weekly"></div>
      </div>`
          : ""
      }
      ${tokensHtml(p.id)}
      <p class="note" data-note="${p.id}" hidden></p>
      ${error}
    </section>`;
  }

  const copy = statusCopy[p.status] ?? { title: p.status };
  return `<section class="card well state" data-provider="${p.id}" style="--i:${i}">
    ${head}
    <div class="state-body">
      <h3>${escapeHtml(copy.title)}</h3>
      ${p.message ? `<p>${richText(p.message)}</p>` : ""}
      ${copy.action ? `<button class="btn primary" data-action="${copy.action.id}">${copy.action.label}</button>` : ""}
    </div>
  </section>`;
}

export function usageHtml(state: AppState): string {
  const providers = visibleProviders(state);
  if (!providers.length && !pomoOn(state)) {
    return `<section class="card well state"><div class="state-body"><h3>No services turned on</h3><p>Turn on Claude or Codex in Settings.</p><button class="btn primary" data-action="settings">Open Settings</button></div></section>`;
  }
  const timer = pomoOn(state) ? pomoCardHtml(state) : "";
  return timer + providers.map((p, i) => cardHtml(p, i + (timer ? 1 : 0))).join("");
}

// ------------------------------------------------------------------ settings

function toggle(id: string, on: boolean, label: string, sub = ""): string {
  return `<label class="row">
    <span class="row-text"><span>${label}</span>${sub ? `<small>${sub}</small>` : ""}</span>
    <input type="checkbox" class="switch" data-setting="${id}" ${on ? "checked" : ""}/>
  </label>`;
}

function segmented(id: string, value: string | number, options: [string | number, string][], big = false): string {
  return `<div class="seg${big ? " big" : ""}" role="radiogroup"><span class="lens prism" aria-hidden="true"></span>${options
    .map(
      ([v, l]) =>
        `<button role="radio" aria-checked="${String(v) === String(value)}" data-seg="${id}" data-value="${v}">${l}</button>`,
    )
    .join("")}</div>`;
}

export function settingsHtml(state: AppState): string {
  const s: Settings = state.settings;
  const statusLine = state.claudeStatusLine;
  return `<div class="settings">
    <h4>Appearance</h4>
    ${segmented(
      "theme",
      s.theme,
      [
        ["system", `${icons.auto}Auto`],
        ["light", `${icons.sun}Light`],
        ["dark", `${icons.moon}Dark`],
      ],
      true,
    )}

    <h4>Services</h4>
    <div class="group well">
      ${toggle("claudeEnabled", s.claudeEnabled, "Claude", "Live plan limits via Claude Code")}
      <div class="row sub-row">
        <span class="row-text"><small>Show usage in Claude Code's status line</small></span>
        <button class="btn small" data-action="${statusLine ? "claude-disconnect" : "claude-connect"}">${statusLine ? "Turn off" : "Turn on"}</button>
      </div>
      <div class="row sub-row">
        <input class="text" data-text="claudePath" placeholder="Claude Code path · auto-detect" value="${escapeHtml(s.claudePath ?? "")}" spellcheck="false"/>
      </div>
      ${toggle("codexEnabled", s.codexEnabled, "Codex", "Plan limits from the Codex CLI")}
      <div class="row sub-row">
        <input class="text" data-text="codexPath" placeholder="Codex path · auto-detect" value="${escapeHtml(s.codexPath ?? "")}" spellcheck="false"/>
      </div>
    </div>

    <h4>Focus timer</h4>
    <div class="group well">
      ${toggle("pomodoroEnabled", s.pomodoroEnabled, "Pomodoro", "Focus rounds with short breaks, and a long break after every 4")}
      ${
        s.pomodoroEnabled
          ? `<div class="row"><span class="row-text"><span>Focus</span><small>Minutes</small></span>
      ${segmented("focusMinutes", s.focusMinutes, [[15, "15"], [25, "25"], [45, "45"], [50, "50"]])}</div>
      <div class="row"><span class="row-text"><span>Short break</span></span>
      ${segmented("breakMinutes", s.breakMinutes, [[5, "5"], [10, "10"]])}</div>
      <div class="row"><span class="row-text"><span>Long break</span></span>
      ${segmented("longBreakMinutes", s.longBreakMinutes, [[15, "15"], [20, "20"], [30, "30"]])}</div>`
          : ""
      }
    </div>

    <h4>Alerts</h4>
    <div class="group well">
      <div class="row"><span class="row-text"><span>Notify at</span><small>Once per limit window</small></span>
      ${segmented("alertThreshold", s.alertThreshold, [[0, "Off"], [75, "75"], [85, "85"], [95, "95"]])}</div>
    </div>

    <h4>Widget</h4>
    <div class="group well">
      ${toggle("alwaysOnTop", s.alwaysOnTop, "Keep on top")}
      ${toggle("autostart", state.autostart, "Start with Windows")}
      <div class="row"><span class="row-text"><span>Placement</span></span>
      ${segmented("placement", s.placement, [["side", "Side"], ["top", "Top"], ["bottom", "Bottom"]])}</div>
      ${
        s.placement === "side"
          ? `<div class="row"><span class="row-text"><span>Screen edge</span></span>
      ${segmented("edge", s.edge, [["left", "Left"], ["right", "Right"]])}</div>`
          : `<div class="row"><span class="row-text"><span>Style</span><small>Attached to the edge, or floating</small></span>
      ${segmented("barStyle", s.barStyle, [["notch", "Notch"], ["island", "Island"]])}</div>`
      }
      <div class="row"><span class="row-text"><span>Check every</span><small>Codex; Claude every 5 min (its limit). Both also check after you use them.</small></span>
      ${segmented("pollSeconds", s.pollSeconds, [[60, "1m"], [120, "2m"], [300, "5m"]])}</div>
    </div>

    <h4>About</h4>
    <div class="group well">
      <div class="row"><span class="row-text"><span>Limi ${escapeHtml(state.version)}</span><small data-update-status>Drag the rail to move it.</small></span>
      <button class="btn small" data-action="check-update">Updates</button></div>
      <div class="row"><button class="btn small ghost" data-action="hide">Hide widget</button><button class="btn small danger" data-action="quit">Quit</button></div>
    </div>
  </div>`;
}

// ------------------------------------------------------------------ painting values

const tweens = new WeakMap<Element, number>();

function setNumber(el: Element, target: number) {
  const from = Number(el.textContent) || 0;
  if (from === target) return;
  const prev = tweens.get(el);
  if (prev) cancelAnimationFrame(prev);
  if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
    el.textContent = String(target);
    return;
  }
  const start = performance.now();
  const step = (t: number) => {
    const k = Math.min(1, (t - start) / 800);
    const eased = 1 - Math.pow(1 - k, 3);
    el.textContent = String(Math.round(from + (target - from) * eased));
    if (k < 1) tweens.set(el, requestAnimationFrame(step));
  };
  tweens.set(el, requestAnimationFrame(step));
}

function windowFor(usage: Usage, key: string) {
  const [pid, which] = key.split(":") as [Pid, "session" | "weekly"];
  const p = usage[pid];
  if (!p) return { p: undefined, w: undefined };
  return { p, w: which === "session" ? sessionWindow(p) : weeklyWindow(p) };
}

/** Update numbers, gauges and times in place so changes animate. */
export function paint(root: HTMLElement, state: AppState) {
  const usage = state.usage;
  // A window whose reset time passed: if nothing was used since (per the local
  // logs), the new window is empty; otherwise wait for fresh numbers.
  const emptySince = (key: string) => {
    const { p, w } = windowFor(usage, key);
    if (!p || !w || !isReset(w) || w.resetsAt === null) return false;
    const last = usage.tokens?.[p.id]?.lastActivityAt ?? null;
    return last === null || last < w.resetsAt * 1000;
  };
  const shown = (key: string) => {
    const { w } = windowFor(usage, key);
    if (!w) return null;
    if (isReset(w)) return emptySince(key) ? 0 : null;
    return clampPct(w.usedPercent);
  };

  root.querySelectorAll<SVGPathElement>("[data-ring]").forEach((el) => {
    const pct = shown(el.dataset.ring!) ?? 0;
    el.style.strokeDashoffset = String(100 - pct);
    el.classList.toggle("zero", pct < 0.5);
    el.dataset.level = level(pct);
  });
  root.querySelectorAll<HTMLElement>("[data-pct]").forEach((el) => {
    const pct = shown(el.dataset.pct!);
    if (pct === null) el.textContent = "–";
    else setNumber(el, Math.round(pct));
  });
  root.querySelectorAll<HTMLElement>("[data-bar]").forEach((el) => {
    const pct = shown(el.dataset.bar!) ?? 0;
    el.style.width = `${pct}%`;
    el.dataset.level = level(pct);
  });
  root.querySelectorAll<HTMLElement>("[data-dots]").forEach((el) => {
    const pct = shown(el.dataset.dots!);
    const on = pct === null ? 0 : Math.min(4, Math.ceil(pct / 25));
    el.querySelectorAll("i").forEach((d, i) => d.classList.toggle("on", i < on));
    el.dataset.level = level(pct ?? 0);
    el.title = pct === null ? "Weekly: reset" : `Weekly ${Math.round(pct)}%`;
  });
  root.querySelectorAll<HTMLElement>("[data-clock]").forEach((el) => {
    const { w } = windowFor(usage, el.dataset.clock!);
    el.textContent = w && !isReset(w) ? clockShort(w.resetsAt) : "—";
  });
  root.querySelectorAll<HTMLElement>("[data-left]").forEach((el) => {
    const { w } = windowFor(usage, el.dataset.left!);
    if (!w) return;
    if (isReset(w)) {
      el.textContent = emptySince(el.dataset.left!)
        ? "New session starts with your next message"
        : `Reset at ${resetClock(w.resetsAt)}`;
    } else if (w.resetsAt === null) el.textContent = "Reset time not reported";
    else el.textContent = `in ${countdown(w.resetsAt)} · ${resetClock(w.resetsAt)}`;
  });
  root.querySelectorAll<HTMLElement>("[data-reset-line]").forEach((el) => {
    const { w } = windowFor(usage, el.dataset.resetLine!);
    if (!w) return;
    if (isReset(w)) el.textContent = `Reset at ${resetClock(w.resetsAt)}`;
    else if (w.resetsAt === null) el.textContent = "Reset time not reported";
    else el.textContent = `Resets ${resetClock(w.resetsAt)} · in ${countdown(w.resetsAt)}`;
  });
  root.querySelectorAll<HTMLElement>("[data-tok]").forEach((el) => {
    const [pid, which] = el.dataset.tok!.split(":") as [Pid, "session" | "weekly"];
    const t = usage.tokens?.[pid]?.[which];
    const b = el.querySelector("b")!;
    b.textContent = t ? compactTokens(t.total) : "—";
    el.title = t ? tokenTitle(t) : "No local activity found";
  });
  root.querySelectorAll<HTMLElement>("[data-note]").forEach((el) => {
    const p = usage[el.dataset.note as Pid];
    const stale = p.windows.filter(isReset).some((w) => {
      const last = usage.tokens?.[p.id]?.lastActivityAt ?? null;
      return last !== null && w.resetsAt !== null && last >= w.resetsAt * 1000;
    });
    const text = stale ? "A limit just reset. Fetching fresh numbers…" : "";
    el.hidden = !text;
    el.textContent = text;
  });
  root.querySelectorAll<HTMLElement>("[data-fresh]").forEach((el) => {
    const p = usage[el.dataset.fresh as Pid];
    let text = "";
    let live = false;
    if (hasNumbers(p)) {
      // Claude is checked every 5 min at most (its service rate-limits checks).
      const every = p.id === "claude" ? Math.max(state.settings.pollSeconds, 300) : state.settings.pollSeconds;
      const fresh = p.capturedAt !== null && Date.now() - p.capturedAt < every * 2000;
      live = p.live && fresh && p.status === "ok";
      text = live ? "Live" : `Updated ${ago(p.capturedAt)}`;
    }
    el.innerHTML = text ? `<i class="dot${live ? " live" : ""}"></i>${text}` : "";
    el.title = `Source: ${p.source}`;
  });
  // Resting gauge: a number appears only when something needs attention.
  root.querySelectorAll<HTMLElement>("[data-relax-center]").forEach((el) => {
    let worst: { pct: number } | null = null;
    for (const p of [usage.claude, usage.codex]) {
      const pct = shown(`${p.id}:session`);
      if (pct !== null && pct >= 75 && (!worst || pct > worst.pct)) worst = { pct };
    }
    const next = worst ? `<b class="display" data-level="${level(worst.pct)}">${Math.round(worst.pct)}</b>` : icons.cloud;
    if (el.innerHTML !== next) el.innerHTML = next;
  });
  root.querySelectorAll<HTMLElement>("[data-rdots]").forEach((el) => {
    const [pid, n] = el.dataset.rdots!.split(":");
    const pct = shown(`${pid}:weekly`);
    const step = 100 / Number(n);
    const on = pct === null ? 0 : Math.min(Number(n), Math.ceil(pct / step));
    el.querySelectorAll("i").forEach((d, i) => d.classList.toggle("on", i < on));
    el.dataset.level = level(pct ?? 0);
  });
  paintPomo(root, state);
  root.querySelectorAll<HTMLElement>("[data-level-of]").forEach((el) => {
    const p = usage[el.dataset.levelOf as Pid];
    const worst = Math.max(0, ...p.windows.filter((w) => !isReset(w)).map((w) => w.usedPercent));
    el.dataset.level = hasNumbers(p) ? level(worst) : "ok";
  });
}
