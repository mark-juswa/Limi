import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./styles.css";
import logo from "./logo.png";
import { ago, isReset } from "./format";
import type { AppState, LayoutInfo, Pomodoro, Settings, Usage } from "./types";
import { icons, paint, paintPomo, railHtml, relaxHtml, settingsHtml, usageHtml, visibleProviders } from "./views";

// Geometry (logical px). The widget sits flush on the screen edge; PAD is
// transparent room for the shadow on the other three sides.
const PAD = 18;
const RAIL_W = 72;
const PANEL_W = 340;
const PANEL_MAX_H = 720;

const appWindow = getCurrentWindow();
const $ = <T extends HTMLElement>(sel: string) => document.querySelector(sel) as T;

let state: AppState;
// relaxed: small resting gauge · compact: the full rail (on hover) · expanded: the panel
type Mode = "relaxed" | "compact" | "expanded";
let mode: Mode = "relaxed";
let view: "usage" | "settings" = "usage";
let busy = false;
let railTop = 0;
let shapeKey = "";

document.body.innerHTML = `
  <div id="stage">
    <div id="shell" class="shell glass">
      <div class="relax" aria-hidden="true"></div>
      <div class="rail" role="button" tabindex="0" aria-label="Show usage details"></div>
      <div class="panel" aria-hidden="true">
        <header class="panel-head">
          <button class="orb back" data-action="back" aria-label="Back">${icons.back}</button>
          <div class="brand"><span class="brand-name display"><img class="logo" src="${logo}" alt="" />Limi</span><span class="brand-sub"></span></div>
          <button class="orb prism" data-action="refresh" aria-label="Refresh">${icons.refresh}</button>
          <button class="orb prism" data-action="settings" aria-label="Settings">${icons.gear}</button>
          <button class="orb prism collapse" data-action="collapse" aria-label="Collapse">${icons.collapse}</button>
        </header>
        <div class="panel-body"><div class="view"></div></div>
      </div>
    </div>
  </div>`;

const stage = $("#stage");
const shell = $("#shell");
const rail = $(".rail");
const relax = $(".relax");
const RELAX_W = 60;
stage.dataset.mode = mode;
stage.dataset.place = "side";
const panel = $(".panel");
const viewEl = $(".view");

/**
 * Lens refraction. Each lens size gets its own filter in pixel units: a
 * displacement map that leaves the middle untouched and bends the rim, with
 * the color channels offset slightly for the prism fringe.
 */
let filterSvg: SVGSVGElement | null = null;
function defs(): SVGSVGElement {
  if (filterSvg?.isConnected) return filterSvg;
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("width", "0");
  svg.setAttribute("height", "0");
  svg.setAttribute("aria-hidden", "true");
  svg.style.position = "absolute";
  document.body.appendChild(svg);
  madeFilters.clear();
  filterSvg = svg;
  return svg;
}
const madeFilters = new Set<string>();

function refraction(w: number, h: number): string {
  w = Math.round(w);
  h = Math.round(h);
  const id = `q-lens-${w}x${h}`;
  const host = defs();
  if (madeFilters.has(id)) return id;
  madeFilters.add(id);
  const r = Math.min(w, h) / 2;
  const edge = Math.max(4, Math.min(w, h) * 0.22);
  const map = `<svg xmlns='http://www.w3.org/2000/svg' width='${w}' height='${h}'><defs><linearGradient id='x'><stop offset='0' stop-color='#f00'/><stop offset='1' stop-color='#000'/></linearGradient><linearGradient id='y' x2='0' y2='1'><stop offset='0' stop-color='#0f0'/><stop offset='1' stop-color='#000'/></linearGradient><filter id='b' x='-50%' y='-50%' width='200%' height='200%'><feGaussianBlur stdDeviation='${(edge / 2).toFixed(1)}'/></filter></defs><rect width='${w}' height='${h}' fill='url(#x)'/><rect width='${w}' height='${h}' fill='url(#y)' style='mix-blend-mode:screen'/><rect x='${edge}' y='${edge}' width='${w - 2 * edge}' height='${h - 2 * edge}' rx='${Math.max(0, r - edge)}' fill='#808000' filter='url(#b)'/></svg>`;
  const href = `data:image/svg+xml,${encodeURIComponent(map)}`;
  const k = Math.min(10, Math.min(w, h) * 0.18);
  const channel = (scale: number, m: string, out: string) =>
    `<feDisplacementMap in="SourceGraphic" in2="map" scale="${scale.toFixed(1)}" xChannelSelector="R" yChannelSelector="G" result="d${out}"/><feColorMatrix in="d${out}" type="matrix" values="${m}" result="${out}"/>`;
  host.insertAdjacentHTML(
    "beforeend",
    `<filter id="${id}" filterUnits="userSpaceOnUse" primitiveUnits="userSpaceOnUse" x="0" y="0" width="${w}" height="${h}" color-interpolation-filters="sRGB">
      <feImage href="${href}" x="0" y="0" width="${w}" height="${h}" preserveAspectRatio="none" result="map"/>
      ${channel(k, "1 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 1 0", "R")}
      ${channel(k * 1.08, "0 0 0 0 0  0 1 0 0 0  0 0 0 0 0  0 0 0 1 0", "G")}
      ${channel(k * 1.16, "0 0 0 0 0  0 0 0 0 0  0 0 1 0 0  0 0 0 1 0", "B")}
      <feBlend in="R" in2="G" mode="screen" result="RG"/><feBlend in="RG" in2="B" mode="screen"/>
    </filter>`,
  );
  return id;
}

/** Give a lens element glass refraction sized to it. */
function refract(lens: HTMLElement, w: number, h: number) {
  lens.style.backdropFilter = `url(#${refraction(w, h)}) brightness(1.05) saturate(1.25)`;
}

// ------------------------------------------------------------------ theme

const darkQuery = matchMedia("(prefers-color-scheme: dark)");

function applyTheme() {
  const pref = state?.settings.theme ?? "system";
  const dark = pref === "dark" || (pref === "system" && darkQuery.matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}
darkQuery.addEventListener("change", applyTheme);

// ------------------------------------------------------------------ sizes

function railHeight(): number {
  return Math.ceil(rail.scrollHeight);
}

// ------------------------------------------------------------------ motion

/**
 * A damped spring sampled into a CSS `linear()` curve, so the glass moves
 * with real spring physics (like iOS sheets) while the browser animates it.
 */
function spring(stiffness: number, damping: number) {
  const dt = 1 / 240;
  let x = 0;
  let v = 0;
  let t = 0;
  const samples = [0];
  while (t < 2) {
    v += (-stiffness * (x - 1) - damping * v) * dt;
    x += v * dt;
    t += dt;
    samples.push(x);
    if (Math.abs(x - 1) < 0.0008 && Math.abs(v) < 0.02) break;
  }
  const n = 64;
  const pts = Array.from({ length: n + 1 }, (_, i) => samples[Math.round((i * (samples.length - 1)) / n)].toFixed(4));
  pts[n] = "1";
  return { easing: `linear(${pts.join(", ")})`, ms: Math.round(t * 1000) };
}

const OPEN = spring(230, 23); // a touch of overshoot, like a sheet popping open
const CLOSE = spring(360, 38); // settles without bouncing
const RESIZE = spring(300, 30);
const rootStyle = document.documentElement.style;
rootStyle.setProperty("--open-ease", OPEN.easing);
rootStyle.setProperty("--open-ms", `${OPEN.ms}ms`);
rootStyle.setProperty("--close-ease", CLOSE.easing);
rootStyle.setProperty("--close-ms", `${CLOSE.ms}ms`);

function motion(m: { easing: string; ms: number }) {
  const t = `${m.ms}ms ${m.easing}`;
  shell.style.transition = `top ${t}, left ${t}, width ${t}, height ${t}`;
}

// The window keeps one size (big enough for the open panel) so nothing ever
// resizes mid-animation; only the glass moves. Clicks outside the glass fall
// through to the desktop (see set_hit_rect).
let winH = PANEL_MAX_H + PAD * 2;

function panelHeight(): number {
  const head = $(".panel-head").offsetHeight;
  return Math.min(winH - PAD * 2, PANEL_MAX_H, Math.ceil(head + viewEl.scrollHeight + 18));
}

/** Gap between a floating island and the screen edge. */
const ISLAND_GAP = 10;
/** Room for the widest top/bottom bar (hover shows more per service). */
const BAR_MAX_W = 620;
const horizontal = () => !!state && state.settings.placement !== "side";

function shellRect(kind: Mode): [number, number, number, number] {
  if (horizontal()) {
    // Top/bottom: centered; a notch touches the edge, an island floats.
    const [w, h] =
      kind === "relaxed"
        ? [Math.ceil(relax.offsetWidth), Math.ceil(relax.offsetHeight)]
        : kind === "compact"
          ? [Math.ceil(rail.offsetWidth), Math.ceil(rail.offsetHeight)]
          : [PANEL_W, panelHeight()];
    const gap = state.settings.barStyle === "island" ? ISLAND_GAP : 0;
    const x = (BAR_MAX_W + PAD * 2 - w) / 2;
    const y = state.settings.placement === "top" ? gap : winH - h - gap;
    return [x, y, w, h];
  }
  let w = RAIL_W;
  let h = railHeight();
  let top = PAD + railTop;
  if (kind === "relaxed") {
    w = RELAX_W;
    h = Math.ceil(relax.scrollHeight);
  } else if (kind === "expanded") {
    w = PANEL_W;
    h = panelHeight();
    // Open downward from the rail; slide up only if the screen ends first.
    top = PAD + Math.max(0, Math.min(railTop, winH - PAD * 2 - h));
  }
  const x = stage.dataset.edge === "left" ? 0 : PANEL_W + PAD - w;
  return [x, top, w, h];
}

function applyShell(kind: Mode) {
  const [x, y, w, h] = shellRect(kind);
  shell.style.left = horizontal() ? `${x}px` : "";
  shell.style.top = `${y}px`;
  shell.style.width = `${w}px`;
  shell.style.height = `${h}px`;
}

function setHit(kind: Mode) {
  const [x, y, width, height] = shellRect(kind);
  invoke("set_hit_rect", { x, y, width, height }).catch(() => {});
}

function instant(fn: () => void) {
  shell.classList.add("no-anim");
  fn();
  void shell.offsetHeight;
  shell.classList.remove("no-anim");
}

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Place the window for the current rail and redraw the glass where it belongs. */
async function layoutCompact(animate = false) {
  stage.dataset.place = state?.settings.placement ?? "side";
  stage.dataset.bar = state?.settings.barStyle ?? "island";
  const info = await invoke<LayoutInfo>("set_layout", {
    width: horizontal() ? BAR_MAX_W + PAD * 2 : PANEL_W + PAD,
    height: horizontal() ? PANEL_MAX_H + PAD + ISLAND_GAP : PANEL_MAX_H + PAD * 2,
    railHeight: railHeight() + PAD * 2,
    animate,
  });
  railTop = info.railTop;
  winH = info.height;
  stage.dataset.edge = info.edge;
  instant(() => applyShell(mode));
  setHit(mode);
}

// ------------------------------------------------------------------ expand / collapse

async function expand(to: "usage" | "settings" = "usage") {
  if (busy) return;
  if (mode === "expanded") {
    showView(to);
    return;
  }
  busy = true;
  hideRailLens();
  // The usage view is kept rendered while folded, so opening starts instantly.
  if (view !== to || !viewEl.childElementCount) {
    view = to;
    renderView();
  }
  setMode("expanded");
  setHit("expanded");
  motion(OPEN);
  stage.classList.add("expanded");
  panel.setAttribute("aria-hidden", "false");
  applyShell("expanded");
  appWindow.setFocus().catch(() => {});
  invoke("refresh").catch(() => {});
  await wait(OPEN.ms);
  placeSegLenses(false);
  busy = false;
}

async function collapse() {
  if (busy || mode !== "expanded") return;
  busy = true;
  setMode("compact");
  motion(CLOSE);
  stage.classList.remove("expanded");
  panel.setAttribute("aria-hidden", "true");
  applyShell("compact");
  await wait(CLOSE.ms);
  setHit("compact");
  if (view !== "usage") {
    view = "usage";
    renderView();
  }
  busy = false;
}

function setMode(next: Mode) {
  mode = next;
  stage.dataset.mode = next;
}

// ------------------------------------------------------------------ resting ⇄ rail

let restTimer = 0;

/** Pointer arrived: grow the resting gauge into the full rail. */
function wake() {
  clearTimeout(restTimer);
  if (mode !== "relaxed" || busy) return;
  setMode("compact");
  motion(RESIZE);
  applyShell("compact");
  setHit("compact");
}

/** Pointer left: settle back into the resting gauge after a short pause. */
function restSoon() {
  clearTimeout(restTimer);
  restTimer = window.setTimeout(() => {
    if (mode !== "compact" || busy || stage.classList.contains("dragging")) return;
    hideRailLens();
    setMode("relaxed");
    motion(CLOSE);
    applyShell("relaxed");
    setHit("relaxed");
  }, 600);
}

async function showView(next: "usage" | "settings") {
  if (view === next) return;
  view = next;
  viewEl.classList.add("swap");
  await wait(150);
  renderView();
  resizeExpanded();
  viewEl.classList.remove("swap");
}

/** Fit the open panel to its content. */
function resizeExpanded() {
  if (mode !== "expanded") return;
  const h = panelHeight();
  if (Math.abs(h - shell.offsetHeight) < 2) return;
  motion(RESIZE);
  applyShell("expanded");
  setHit("expanded");
}

// ------------------------------------------------------------------ liquid lenses

/** Slide each segmented control's lens over its selected option. */
function placeSegLenses(animate = true) {
  viewEl.querySelectorAll<HTMLElement>(".seg").forEach((seg) => {
    const lens = seg.querySelector<HTMLElement>(".lens");
    const on = seg.querySelector<HTMLElement>('[aria-checked="true"]');
    if (!lens || !on) return;
    if (!animate) lens.style.transition = "none";
    lens.style.setProperty("--lx", `${on.offsetLeft}px`);
    lens.style.setProperty("--lw", `${on.offsetWidth}px`);
    refract(lens, on.offsetWidth, seg.clientHeight - 2);
    if (!animate) {
      void lens.offsetWidth;
      lens.style.transition = "";
    } else {
      lens.classList.remove("moving");
      void lens.offsetWidth;
      lens.classList.add("moving");
    }
  });
}

let lensTimer = 0;
function railLens(): HTMLElement | null {
  return rail.querySelector<HTMLElement>(".lens");
}

function moveRailLens(item: HTMLElement) {
  const lens = railLens();
  const ring = item.querySelector<HTMLElement>(".ring");
  if (!lens || !ring || mode !== "compact" || horizontal()) return;
  const y = item.offsetTop + ring.offsetTop + ring.offsetHeight / 2 - 31;
  if (!lens.style.backdropFilter) refract(lens, 62, 62);
  const wasShown = lens.classList.contains("show");
  if (!wasShown) {
    lens.style.transition = "none";
    lens.style.setProperty("--ly", `${y}px`);
    void lens.offsetWidth;
    lens.style.transition = "";
    lens.classList.add("show");
    return;
  }
  if (lens.style.getPropertyValue("--ly") === `${y}px`) return;
  lens.classList.add("moving");
  lens.style.setProperty("--ly", `${y}px`);
  clearTimeout(lensTimer);
  lensTimer = window.setTimeout(() => lens.classList.remove("moving"), 220);
}

function hideRailLens() {
  railLens()?.classList.remove("show", "moving");
}

rail.addEventListener("pointerover", (e) => {
  const item = (e.target as HTMLElement).closest<HTMLElement>(".rail-item");
  if (item) moveRailLens(item);
});
rail.addEventListener("pointerleave", hideRailLens);

// ------------------------------------------------------------------ rendering

function computeShape(s: AppState): string {
  return JSON.stringify(
    [s.usage.claude, s.usage.codex].map((p) => [p.id, p.status, p.plan, p.message, p.windows.map((w) => w.id + w.label).join()]),
  );
}

function renderRail() {
  rail.innerHTML = railHtml(state);
  relax.innerHTML = relaxHtml(state);
  const names = visibleProviders(state)
    .map((p) => {
      const w = p.windows[0];
      return w ? `${p.name} ${Math.round(w.usedPercent)}%` : `${p.name} ${p.status.replace("_", " ")}`;
    })
    .join(", ");
  rail.setAttribute("aria-label", `Usage: ${names || "not set up"}. Click for details.`);
}

function renderView() {
  stage.dataset.view = view;
  viewEl.innerHTML = view === "settings" ? settingsHtml(state) : usageHtml(state);
  paint(viewEl, state);
  placeSegLenses(false);
}

function paintHeader() {
  const times = visibleProviders(state)
    .map((p) => p.capturedAt)
    .filter((t): t is number => t !== null);
  const sub = $(".brand-sub");
  sub.textContent = view === "settings" ? "Settings" : times.length ? `Updated ${ago(Math.max(...times))}` : "Waiting for usage";
}

let lastRailH = 0;
async function render(force = false) {
  const key = computeShape(state);
  if (force || key !== shapeKey) {
    shapeKey = key;
    renderRail();
    if (view === "usage") renderView();
    // Next frame so new elements animate from zero.
    requestAnimationFrame(() => paint(shell, state));
    const rh = railHeight() * 1000 + relax.scrollHeight;
    if (!busy && rh !== lastRailH) {
      lastRailH = rh;
      await layoutCompact();
    }
    resizeExpanded();
  } else {
    paint(shell, state);
  }
  paintHeader();
}

// ------------------------------------------------------------------ actions

async function updateSettings(patch: Partial<Settings>) {
  const prev = state.settings;
  state.settings = await invoke<Settings>("update_settings", { patch });
  applyTheme();
  const s = state.settings;
  if (s.placement !== prev.placement || s.barStyle !== prev.barStyle || s.pomodoroEnabled !== prev.pomodoroEnabled) {
    // New orientation, or the timer shown/hidden: redraw the rail and
    // resting gauge, then move the window.
    renderRail();
    paint(shell, state);
    if (view === "settings") renderView();
    resizeExpanded();
    await layoutCompact();
  } else if (s.edge !== prev.edge) await layoutCompact();
}

async function act(action: string, el: HTMLElement) {
  switch (action) {
    case "collapse":
      return collapse();
    case "settings":
      return mode === "expanded" ? showView("settings") : expand("settings");
    case "back":
      return showView("usage");
    case "refresh": {
      el.classList.add("spin");
      setTimeout(() => el.classList.remove("spin"), 900);
      return invoke("refresh");
    }
    case "claude-connect":
    case "claude-disconnect": {
      el.setAttribute("disabled", "");
      try {
        await invoke(action === "claude-connect" ? "claude_connect" : "claude_disconnect");
      } catch (e) {
        showToast(String(e));
      }
      await reload();
      if (view === "settings") renderView();
      return;
    }
    case "check-update": {
      const line = viewEl.querySelector<HTMLElement>("[data-update-status]");
      if (el.dataset.install === "1") {
        if (line) line.textContent = "Downloading update…";
        return invoke("install_update").catch((e) => line && (line.textContent = `Update failed: ${e}`));
      }
      if (line) line.textContent = "Checking for updates…";
      try {
        const v = await invoke<string | null>("check_update");
        if (v) {
          if (line) line.textContent = `Version ${v} is available.`;
          el.textContent = "Install";
          el.dataset.install = "1";
        } else if (line) line.textContent = "You're up to date.";
      } catch {
        if (line) line.textContent = "Couldn't check for updates right now.";
      }
      return;
    }
    case "pomo-toggle":
    case "pomo-reset":
    case "pomo-skip": {
      state.pomodoro = await invoke<Pomodoro>("pomodoro", { action: action.slice(5) });
      paintPomo(shell, state);
      return;
    }
    case "hide":
      await collapse();
      return invoke("hide_widget");
    case "quit":
      return invoke("quit");
  }
}

let toastTimer = 0;
function showToast(text: string) {
  let t = document.querySelector<HTMLElement>(".toast");
  if (!t) {
    t = document.createElement("div");
    t.className = "toast";
    panel.appendChild(t);
  }
  t.textContent = text;
  t.classList.add("show");
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => t!.classList.remove("show"), 5000);
}

shell.addEventListener("click", (e) => {
  const el = (e.target as HTMLElement).closest<HTMLElement>("[data-action]");
  if (el) {
    e.preventDefault();
    act(el.dataset.action!, el);
  }
  const seg = (e.target as HTMLElement).closest<HTMLElement>("[data-seg]");
  if (seg && seg.getAttribute("aria-checked") !== "true") {
    const id = seg.dataset.seg as keyof Settings;
    const raw = seg.dataset.value!;
    const value = ["edge", "theme", "placement", "barStyle"].includes(id) ? raw : Number(raw);
    seg.parentElement?.querySelectorAll("[data-seg]").forEach((b) => b.setAttribute("aria-checked", String(b === seg)));
    placeSegLenses(true);
    updateSettings({ [id]: value } as Partial<Settings>);
  }
});

shell.addEventListener("change", async (e) => {
  const el = e.target as HTMLInputElement;
  if (el.dataset.setting === "autostart") {
    try {
      state.autostart = await invoke<boolean>("set_autostart", { enabled: el.checked });
    } catch (err) {
      el.checked = !el.checked;
      showToast(String(err));
    }
    return;
  }
  if (el.dataset.setting) updateSettings({ [el.dataset.setting]: el.checked } as Partial<Settings>);
  if (el.dataset.text === "codexPath") updateSettings({ codexPath: el.value } as Partial<Settings>);
  if (el.dataset.text === "claudePath") updateSettings({ claudePath: el.value } as Partial<Settings>);
});

// ------------------------------------------------------------------ rail: click vs drag

let press: { x: number; y: number } | null = null;

const onPress = (e: MouseEvent) => {
  if (e.button !== 0 || mode === "expanded" || busy) return;
  press = { x: e.screenX, y: e.screenY };
};
rail.addEventListener("mousedown", onPress);
relax.addEventListener("mousedown", onPress);

window.addEventListener("mousemove", async (e) => {
  if (!press) return;
  if (Math.hypot(e.screenX - press.x, e.screenY - press.y) < 4) return;
  press = null;
  // Top/bottom placements stay centered on their edge.
  if (horizontal()) return;
  hideRailLens();
  stage.classList.add("dragging");
  await appWindow.startDragging().catch(() => {});
  // The OS owns the drag now; wait for the button to be released, then dock.
  for (let i = 0; i < 400; i++) {
    const settings = await invoke<Settings | null>("snap_to_edge", {
      railX: stage.dataset.edge === "left" ? RAIL_W / 2 : PANEL_W + PAD - RAIL_W / 2,
      railTop,
      railHeight: railHeight() + PAD * 2,
    });
    if (settings) {
      state.settings = settings;
      break;
    }
    await wait(60);
  }
  stage.classList.remove("dragging");
  await layoutCompact(true);
});

window.addEventListener("mouseup", () => {
  if (press) {
    press = null;
    expand();
  }
});

rail.addEventListener("keydown", (e) => {
  if (e.key === "Enter" || e.key === " ") {
    e.preventDefault();
    expand();
  }
});

window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") collapse();
});

appWindow.onFocusChanged(({ payload: focused }) => {
  if (!focused && mode === "expanded") collapse();
});

// ------------------------------------------------------------------ data

async function reload() {
  state = await invoke<AppState>("get_state");
  applyTheme();
  await render(true);
}

const refreshedForReset = new Set<string>();

function tick() {
  if (!state) return;
  paint(shell, state);
  paintHeader();
  // When a live provider's window resets, ask for fresh numbers once.
  for (const w of state.usage.codex.windows) {
    const key = `codex:${w.id}:${w.resetsAt}`;
    if (isReset(w) && !refreshedForReset.has(key)) {
      refreshedForReset.add(key);
      invoke("refresh").catch(() => {});
    }
  }
}

async function boot() {
  await listen<Usage>("usage", (e) => {
    if (!state) return;
    state.usage = e.payload;
    render();
  });
  await listen("open-settings", () => expand("settings"));
  await listen<Pomodoro>("pomodoro", (e) => {
    if (!state) return;
    state.pomodoro = e.payload;
    paintPomo(shell, state);
  });
  // The cursor left the glass (the window stops getting mouse events then).
  await listen<boolean>("hover", (e) => {
    if (e.payload) wake();
    else {
      hideRailLens();
      restSoon();
    }
  });
  await reload();
  setInterval(tick, 1000);
  // Check often so each flip lands on the second.
  setInterval(() => state && paintPomo(shell, state), 200);
}

boot();
