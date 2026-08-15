/**
 * Kinesis Advantage 360 Pro mirror.
 *
 * Geometry and legends come from the generated assets, which are derived from
 * the upstream ZMK config (see tools/gen_layout.py) — so what you see matches
 * the firmware rather than a hand-drawn guess.
 */

const SVG_NS = "http://www.w3.org/2000/svg";
const SCALE = 100; // layout.json is in 1u units; SVG works in centi-units
const AFTERGLOW_MS = 480;
const LAYER_HOLD_MS = 2500;

const tauri = globalThis.__TAURI__;

const els = {
  board: document.getElementById("board"),
  layer: document.getElementById("layer"),
  hint: document.getElementById("hint"),
  hud: document.getElementById("hud"),
  permission: document.getElementById("permission"),
  grant: document.getElementById("grant"),
  ontop: document.getElementById("ontop"),
  zoomIn: document.getElementById("zoom-in"),
  zoomOut: document.getElementById("zoom-out"),
};

/* Window chrome around the board, in logical px. The board itself always
   scales to fit whatever is left, so these only matter for picking a window
   height that leaves no letterboxing. */
const CHROME = {
  padX: 32, // .hud left + right padding
  padY: 24, // .hud top + bottom padding
  gap: 6, // flex gap between board and status bar
  status: 20, // status bar row
  title: 28, // native title bar, outside the webview
};
const ZOOM_STEP = 1.15;
const MIN_W = 560;
const MAX_W = 2400;
const WIDTH_KEY = "kinesis-mirror.width";

/** @type {SVGGElement[]} indexed by physical key position */
const keyEls = [];
/** @type {SVGTextElement[]} indexed by physical key position */
const legendEls = [];
const warmTimers = new Map();

let layout;
let keymap;
let usageToPositions = {};
/** usage -> Set of layer indices that can emit it */
let usageLayers = new Map();
let activeLayer = 0;
let inferred = false;
let layerTimer = null;
/** Width/height of the drawn board, measured once it exists. */
let boardAspect = 2.28;

/* ------------------------------------------------------------------ */
/* rendering                                                           */
/* ------------------------------------------------------------------ */

/** Resolve &trans by falling through to the base layer, like ZMK does. */
function bindingAt(layerIndex, pos) {
  const binding = keymap.layers[layerIndex].bindings[pos];
  if (binding.kind === "trans" && layerIndex > 0) {
    return keymap.layers[0].bindings[pos];
  }
  return binding;
}

/** Long legends need to shrink or they spill over the cap. */
function legendSize(text) {
  const n = [...text].length;
  if (n <= 2) return 30;
  if (n <= 4) return 23;
  if (n <= 6) return 17; // "keypad" / "shift" must not touch the cap edge
  return 14;
}

function buildBoard() {
  const group = document.createElementNS(SVG_NS, "g");

  layout.keys.forEach((k, pos) => {
    const g = document.createElementNS(SVG_NS, "g");
    g.setAttribute("class", "key");

    // Thumb clusters are rotated about an origin outside the key itself.
    if (k.r) {
      g.setAttribute(
        "transform",
        `rotate(${k.r} ${k.rx * SCALE} ${k.ry * SCALE})`
      );
    }

    const rect = document.createElementNS(SVG_NS, "rect");
    rect.setAttribute("class", "cap");
    rect.setAttribute("x", k.x * SCALE + 3);
    rect.setAttribute("y", k.y * SCALE + 3);
    rect.setAttribute("width", k.w * SCALE - 6);
    rect.setAttribute("height", k.h * SCALE - 6);
    rect.setAttribute("rx", 14);
    g.appendChild(rect);

    const text = document.createElementNS(SVG_NS, "text");
    text.setAttribute("class", "legend");
    text.setAttribute("x", (k.x + k.w / 2) * SCALE);
    text.setAttribute("y", (k.y + k.h / 2) * SCALE);
    g.appendChild(text);

    group.appendChild(g);
    keyEls[pos] = g;
    legendEls[pos] = text;
  });

  els.board.replaceChildren(group);

  // Rotated thumb keys stick out past the nominal extent, so derive the
  // viewBox from what actually got drawn.
  const box = group.getBBox();
  const pad = 12;
  els.board.setAttribute(
    "viewBox",
    `${box.x - pad} ${box.y - pad} ${box.width + pad * 2} ${box.height + pad * 2}`
  );
  boardAspect = (box.width + pad * 2) / (box.height + pad * 2);

  paintLegends();
}

function paintLegends() {
  layout.keys.forEach((_, pos) => {
    const binding = bindingAt(activeLayer, pos);
    const label = binding.label || "";
    const text = legendEls[pos];
    text.textContent = label;
    text.setAttribute("font-size", legendSize(label));
    // Keys that exist physically but do nothing on this layer (the numbered
    // inner-column keys) still show their printed legend, just dimmed.
    keyEls[pos].classList.toggle("blank", binding.kind === "none" || label === "");
  });

  els.layer.textContent = keymap.layers[activeLayer].name;
  els.layer.classList.toggle("inferred", inferred && activeLayer !== 0);
}

/* ------------------------------------------------------------------ */
/* layer inference                                                     */
/* ------------------------------------------------------------------ */

/**
 * ZMK resolves layers onboard and a momentary-layer key emits no HID report
 * at all, so the host cannot observe layer state directly. What we *can* do is
 * notice a usage that only exists on one non-base layer (F-keys on Fn, KP_* on
 * Kp) and infer from that. The badge is marked with "?" because this is a
 * guess that lags the first keypress on the layer.
 */
function inferLayer(usage) {
  // usageLayers is keyed by string, mirroring the JSON it was built from.
  const layers = usageLayers.get(String(usage));
  if (!layers || layers.has(0)) return; // could be base; no signal

  const candidates = [...layers];
  if (candidates.length !== 1) return; // ambiguous, leave the display alone

  setLayer(candidates[0], true);
}

function setLayer(index, isInferred) {
  clearTimeout(layerTimer);
  if (index !== activeLayer || isInferred !== inferred) {
    activeLayer = index;
    inferred = isInferred;
    paintLegends();
  }
  if (index !== 0) {
    layerTimer = setTimeout(() => setLayer(0, false), LAYER_HOLD_MS);
  }
}

/* ------------------------------------------------------------------ */
/* key events                                                          */
/* ------------------------------------------------------------------ */

function onKey({ usage, down }) {
  const positions = usageToPositions[usage];
  if (!positions) return;

  // Events arriving is proof capture works, whatever TCC reported at boot.
  hideGate();

  if (down) {
    inferLayer(usage);
    // Clear the startup hint on the first real key; the status line is for
    // problems, not for tallying keystrokes.
    if (els.hint.textContent) els.hint.textContent = "";
  }

  for (const pos of positions) {
    const el = keyEls[pos];
    if (!el) continue;

    if (down) {
      clearTimeout(warmTimers.get(pos));
      warmTimers.delete(pos);
      el.classList.add("down");
      el.classList.remove("warm");
    } else {
      el.classList.remove("down");
      el.classList.add("warm");
      warmTimers.set(
        pos,
        setTimeout(() => {
          el.classList.remove("warm");
          warmTimers.delete(pos);
        }, AFTERGLOW_MS)
      );
    }
  }
}

/* ------------------------------------------------------------------ */
/* boot                                                                */
/* ------------------------------------------------------------------ */

function indexKeymap() {
  usageToPositions = keymap.usageToPositions;

  usageLayers = new Map();
  keymap.usageToPositionsByLayer.forEach((table, layerIndex) => {
    for (const usage of Object.keys(table)) {
      if (!usageLayers.has(usage)) usageLayers.set(usage, new Set());
      usageLayers.get(usage).add(layerIndex);
    }
  });
}

/** The webview console is invisible in a bundled app, so mirror to stderr. */
function log(message) {
  console.log(message);
  tauri?.core.invoke("ui_log", { message: String(message) }).catch(() => {});
}

let gateVisible = false;
let accessPoll = null;

function hideGate() {
  if (!gateVisible) return;
  gateVisible = false;
  clearInterval(accessPoll);
  accessPoll = null;
  els.permission.hidden = true;
  els.hud.hidden = false;
  log("permission gate dismissed");
}

/**
 * TCC can report `unknown` transiently — notably right after the app's code
 * signature changes, which happens on every rebuild of an ad-hoc signed
 * build. A one-shot check at boot therefore strands the user behind a gate
 * for a permission they already granted. So: re-poll, and treat an actual
 * key event as proof that capture works no matter what TCC claims.
 */
async function showPermissionGateIfNeeded() {
  if (!tauri) return true; // running in a plain browser for layout work
  const access = await tauri.core.invoke("hid_access");
  log(`hid access: ${access}`);
  if (access === "granted") return true;

  gateVisible = true;
  els.hud.hidden = true;
  els.permission.hidden = false;
  // The gate needs clicks, so stop passing them through.
  await tauri.core.invoke("set_click_through", { enabled: false });

  accessPoll = setInterval(async () => {
    const now = await tauri.core.invoke("hid_access").catch(() => null);
    if (now === "granted") hideGate();
  }, 2000);

  return false;
}

/* ------------------------------------------------------------------ */
/* zoom                                                                */
/* ------------------------------------------------------------------ */

/**
 * The SVG already scales itself to whatever space it gets, so "zoom" is really
 * about the window. Deriving the height from the board's own aspect ratio is
 * what stops the board floating in a letterboxed gap when the window's
 * proportions do not match its own.
 */
function windowSizeForWidth(width) {
  const boardH = (width - CHROME.padX) / boardAspect;
  return {
    width,
    height: Math.round(
      boardH + CHROME.padY + CHROME.gap + CHROME.status + CHROME.title
    ),
  };
}

function clampWidth(w) {
  // Never propose a window wider than the display it has to live on.
  const avail = globalThis.screen?.availWidth ?? MAX_W;
  return Math.round(Math.max(MIN_W, Math.min(Math.min(MAX_W, avail - 40), w)));
}

async function applyWidth(width) {
  const w = clampWidth(width);
  if (tauri) {
    await tauri.core.invoke("resize_window", windowSizeForWidth(w));
  }
  try {
    localStorage.setItem(WIDTH_KEY, String(w));
  } catch {
    /* private mode, not worth failing over */
  }
  els.zoomOut.disabled = w <= MIN_W;
  els.zoomIn.disabled = w >= clampWidth(MAX_W);
  return w;
}

async function wireZoom() {
  const stored = Number(localStorage.getItem(WIDTH_KEY));
  let width = await applyWidth(
    Number.isFinite(stored) && stored > 0 ? stored : window.innerWidth
  );

  els.zoomIn.addEventListener("click", async () => {
    width = await applyWidth(width * ZOOM_STEP);
  });
  els.zoomOut.addEventListener("click", async () => {
    width = await applyWidth(width / ZOOM_STEP);
  });

  // Dragging the window edge should be remembered too, not just the buttons.
  let debounce;
  window.addEventListener("resize", () => {
    clearTimeout(debounce);
    debounce = setTimeout(() => {
      width = clampWidth(window.innerWidth);
      try {
        localStorage.setItem(WIDTH_KEY, String(width));
      } catch {
        /* ignore */
      }
    }, 300);
  });
}

/**
 * "On top" pill. Reads the real window state rather than tracking its own
 * copy, so it stays honest if the tray menu changes it too.
 */
async function wireAlwaysOnTop() {
  if (!tauri) return;
  // Mirrors tauri.conf.json's alwaysOnTop, used only if the query fails.
  let assumed = true;

  const current = async () => {
    try {
      return await tauri.window.getCurrentWindow().isAlwaysOnTop();
    } catch {
      return assumed;
    }
  };

  const paint = (on) => els.ontop.setAttribute("aria-checked", String(on));
  paint(await current());

  els.ontop.addEventListener("click", async () => {
    const next = !(await current());
    await tauri.core.invoke("set_always_on_top", { enabled: next });
    assumed = next;
    paint(await current());
  });
}

async function boot() {
  const [layoutRes, keymapRes] = await Promise.all([
    fetch("/assets/layout.json"),
    fetch("/assets/keymap.json"),
  ]);
  layout = await layoutRes.json();
  keymap = await keymapRes.json();

  indexKeymap();
  buildBoard();

  els.grant?.addEventListener("click", async () => {
    await tauri.core.invoke("hid_request_access");
    // macOS only ever prompts once; after that the answer is in System
    // Settings and the app has to be relaunched to pick it up.
    els.hint.textContent = "relaunch after granting";
  });

  await wireAlwaysOnTop();
  await wireZoom();

  // ?demo=E1,04 lights those HID usages with no keyboard attached. Used by
  // tools/make_screenshot.sh to regenerate the README images.
  const demo = new URLSearchParams(location.search).get("demo");
  if (demo) {
    for (const hex of demo.split(",")) {
      onKey({ usage: parseInt(hex, 16), down: true });
    }
  }

  if (tauri) {
    await tauri.event.listen("key", (e) => onKey(e.payload));
    await tauri.event.listen("capture-error", (e) => {
      els.hint.textContent = String(e.payload);
    });
    await showPermissionGateIfNeeded();
  }
}

// Debug handle: lets you drive the board without a keyboard attached, e.g.
// `__mirror.onKey({ usage: 0x04, down: true })` from the web inspector.
globalThis.__mirror = { onKey, setLayer, get state() { return { layout, keymap }; } };

boot().catch((err) => {
  els.hint.textContent = `failed to start: ${err}`;
  log(`boot failed: ${err?.stack || err}`);
});
