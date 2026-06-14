import init, { process_image } from "./pkg/spritefusion_pixel_snapper.js";

const $ = (id) => document.getElementById(id);

let inputBytes = null;
let ready = false;
let palette = []; // { r, g, b, enabled }[] - colors parsed from the textarea
let baseIndex = -1; // index of the base color highlighted by a relationship preset

function loadImageFile(file) {
  if (!file) return;
  file.arrayBuffer().then((buf) => {
    inputBytes = new Uint8Array(buf);
    $("before").src = URL.createObjectURL(file);
    $("after").removeAttribute("src");
    $("download").hidden = true;
    updateRunEnabled();
    setStatus("Image loaded. Ready to snap.");
  });
}

function updateRunEnabled() {
  $("run").disabled = !(ready && inputBytes);
}

function setStatus(msg) {
  $("status").textContent = msg;
}

// Parse hex colors from textarea / .hex file text into a flat [r,g,b,...] array.
// Accepts comma- or whitespace-separated tokens; `#` optional; 3- or 6-digit hex.
// Lospec `.hex` quirks (one color per line, `;` comments, blank lines) are tolerated.
// Invalid tokens are silently skipped so typing in the textarea never throws.
function parsePalette(text) {
  const rgb = [];
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.split(";")[0];
    for (const token of line.split(/[\s,]+/)) {
      const t = token.trim().replace(/^#/, "");
      let hex;
      if (/^[0-9a-fA-F]{6}$/.test(t)) hex = t;
      else if (/^[0-9a-fA-F]{3}$/.test(t)) hex = t[0] + t[0] + t[1] + t[1] + t[2] + t[2];
      else continue;
      rgb.push(parseInt(hex.slice(0, 2), 16), parseInt(hex.slice(2, 4), 16), parseInt(hex.slice(4, 6), 16));
    }
  }
  return rgb;
}

// --- Color helpers for the "concept" presets ---
// Returns [hue(0-360), saturation(0-1), lightness(0-1)].
// Saturation is the HSV/HSB definition (chroma / value): unlike HSL saturation
// it does NOT blow up for near-white/near-black tints, so "vivid" and the
// neutral test stay intuitive (e.g. an off-white reads as neutral, not vivid).
// Lightness is the HSL definition, which is what "light/dark" should track.
function rgbTraits(r, g, b) {
  r /= 255; g /= 255; b /= 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b), c = max - min;
  const l = (max + min) / 2;
  const s = max === 0 ? 0 : c / max;
  let h = 0;
  if (c !== 0) {
    if (max === r) h = (g - b) / c + (g < b ? 6 : 0);
    else if (max === g) h = (b - r) / c + 2;
    else h = (r - g) / c + 4;
    h *= 60;
  }
  return [h, s, l];
}

// Circular distance between two hues, in degrees (0-180).
function hueDist(a, b) {
  const d = Math.abs(((a - b) % 360 + 360) % 360);
  return Math.min(d, 360 - d);
}

// Near-grayscale colors are hue-agnostic; keep them on for hue/relationship presets.
const isNeutral = (s) => s < 0.15;

function median(arr) {
  if (!arr.length) return 0;
  const a = [...arr].sort((x, y) => x - y);
  const n = a.length;
  return n % 2 ? a[(n - 1) / 2] : (a[n / 2 - 1] + a[n / 2]) / 2;
}

// Rebuild the swatch row from `palette`, reflecting enabled/base state, and
// update the "使う色: N / M" count. Clicking a swatch toggles that color.
function renderPalette() {
  const swatches = $("swatches");
  swatches.innerHTML = "";
  palette.forEach((c, i) => {
    const span = document.createElement("span");
    span.style.background = `rgb(${c.r}, ${c.g}, ${c.b})`;
    const hex = [c.r, c.g, c.b].map((v) => v.toString(16).padStart(2, "0")).join("");
    span.title = `#${hex}`;
    if (!c.enabled) span.classList.add("off");
    if (i === baseIndex) span.classList.add("base");
    span.addEventListener("click", () => {
      c.enabled = !c.enabled;
      renderPalette();
    });
    swatches.appendChild(span);
  });
  const on = palette.filter((c) => c.enabled).length;
  $("paletteCount").textContent = palette.length ? `使う色: ${on} / ${palette.length}` : "";
}

// Re-parse the textarea into `palette` (all colors enabled) and re-render.
function refreshPalette() {
  const rgb = parsePalette($("paletteText").value);
  palette = [];
  for (let i = 0; i < rgb.length; i += 3) {
    palette.push({ r: rgb[i], g: rgb[i + 1], b: rgb[i + 2], enabled: true });
  }
  baseIndex = -1;
  renderPalette();
}

// Apply a "concept" preset: set each color's enabled flag by a rule, then re-render.
// Presets overwrite the current ON/OFF state; manual swatch toggles still work after.
function applyPreset(name) {
  if (!palette.length) return;
  if (name === "all") {
    palette.forEach((c) => (c.enabled = true));
    baseIndex = -1;
    renderPalette();
    setStatus("すべての色をONにしたよ。");
    return;
  }

  const traits = palette.map((c) => rgbTraits(c.r, c.g, c.b));
  baseIndex = -1;
  let pred;

  if (name === "warm") pred = (h, s) => isNeutral(s) || h <= 90 || h >= 300;
  else if (name === "cool") pred = (h, s) => isNeutral(s) || (h > 90 && h < 300);
  else if (name === "light" || name === "dark") {
    const med = median(traits.map((x) => x[2]));
    pred = name === "light" ? (h, s, l) => l >= med : (h, s, l) => l < med;
  } else if (name === "vivid" || name === "muted") {
    const med = median(traits.map((x) => x[1]));
    pred = name === "vivid" ? (h, s) => s >= med : (h, s) => s < med;
  } else if (name === "complementary" || name === "analogous" || name === "triadic") {
    let bi = -1, bs = -1;
    traits.forEach(([h, s], i) => {
      if (!isNeutral(s) && s > bs) { bs = s; bi = i; }
    });
    if (bi < 0) {
      palette.forEach((c) => (c.enabled = true));
      renderPalette();
      setStatus("基準にできる鮮やかな色がない（全色ニュートラル）ので全色ONのままだよ。");
      return;
    }
    baseIndex = bi;
    const H = traits[bi][0];
    if (name === "complementary") pred = (h, s) => isNeutral(s) || hueDist(h, H) <= 35 || hueDist(h, H + 180) <= 35;
    else if (name === "analogous") pred = (h, s) => isNeutral(s) || hueDist(h, H) <= 45;
    else pred = (h, s) => isNeutral(s) || hueDist(h, H) <= 30 || hueDist(h, H + 120) <= 30 || hueDist(h, H + 240) <= 30;
  } else {
    return;
  }

  let on = 0;
  palette.forEach((c, i) => {
    c.enabled = pred(traits[i][0], traits[i][1], traits[i][2]);
    if (c.enabled) on++;
  });
  if (on === 0) {
    palette.forEach((c) => (c.enabled = true));
    baseIndex = -1;
    renderPalette();
    setStatus("このパレットには該当する色がなかったので全色ONに戻したよ。");
    return;
  }
  renderPalette();
  setStatus(`プリセット適用: ${on} / ${palette.length} 色を使用。`);
}

function run() {
  if (!inputBytes) return;
  setStatus("Processing...");
  $("run").disabled = true;
  setTimeout(() => {
    try {
      const k = $("limitColors").checked ? Number($("kSlider").value) : undefined;
      const flat = [];
      for (const c of palette) if (c.enabled) flat.push(c.r, c.g, c.b);
      if (palette.length && flat.length === 0) {
        setStatus("全色OFFだよ。1色以上ONにしてからSnapして。");
        return; // finally re-enables the button
      }
      const paletteRgb = flat.length ? new Uint8Array(flat) : undefined;
      const out = process_image(inputBytes, k, undefined, paletteRgb ?? undefined);
      const blob = new Blob([out], { type: "image/png" });
      const url = URL.createObjectURL(blob);
      $("after").src = url;
      const dl = $("download");
      dl.href = url;
      dl.hidden = false;
      setStatus("Done.");
    } catch (err) {
      setStatus("Error: " + err);
    } finally {
      $("run").disabled = false;
    }
  }, 0);
}

$("file").addEventListener("change", (e) => loadImageFile(e.target.files[0]));

const drop = $("drop");
["dragover", "dragenter"].forEach((ev) =>
  drop.addEventListener(ev, (e) => {
    e.preventDefault();
    drop.classList.add("dragover");
  })
);
["dragleave", "drop"].forEach((ev) =>
  drop.addEventListener(ev, (e) => {
    e.preventDefault();
    drop.classList.remove("dragover");
  })
);
drop.addEventListener("drop", (e) => loadImageFile(e.dataTransfer.files[0]));

$("limitColors").addEventListener("change", (e) => {
  $("kSlider").disabled = !e.target.checked;
});
$("kSlider").addEventListener("input", (e) => ($("kValue").textContent = e.target.value));

$("paletteText").addEventListener("input", refreshPalette);
$("paletteFile").addEventListener("change", (e) => {
  const file = e.target.files[0];
  if (!file) return;
  file.text().then((txt) => {
    $("paletteText").value = txt;
    refreshPalette();
  });
});

document.querySelector(".presets").addEventListener("click", (e) => {
  const btn = e.target.closest("button[data-preset]");
  if (btn) applyPreset(btn.dataset.preset);
});

$("run").addEventListener("click", run);

init()
  .then(() => {
    ready = true;
    setStatus("Ready. Drop an image to start.");
    updateRunEnabled();
  })
  .catch((err) => setStatus("Failed to load WASM: " + err));
