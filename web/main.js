import init, { process_image, extract_palette } from "./pkg/spritefusion_pixel_snapper.js";

const $ = (id) => document.getElementById(id);

let inputBytes = null;
let ready = false;
let palette = []; // { r, g, b, enabled }[] - colors parsed from the textarea
let baseHex = null; // hex of the base color for relationship presets (keyed by value so it survives re-parses)
let beforeUrl = null; // object URL backing #before (revoked before it is replaced)
let outUrl = null; // object URL backing #after / #actual / #download (shared; revoked before replaced)

const hexOf = (c) => [c.r, c.g, c.b].map((v) => v.toString(16).padStart(2, "0")).join("");

function loadImageFile(file) {
  if (!file) return;
  file.arrayBuffer().then((buf) => {
    inputBytes = new Uint8Array(buf);
    if (beforeUrl) URL.revokeObjectURL(beforeUrl);
    beforeUrl = URL.createObjectURL(file);
    $("before").src = beforeUrl;
    if (outUrl) { URL.revokeObjectURL(outUrl); outUrl = null; }
    $("after").removeAttribute("src");
    $("actual").removeAttribute("src");
    $("actualSize").textContent = "";
    $("actualFig").hidden = true;
    $("download").hidden = true;
    updateRunEnabled();
    setStatus("画像を読みこんだよ。「ドット絵に整える」を押してね。");
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
// All trait judgments run in OKLCh (the polar form of OKLab), mirroring the
// Rust core's palette matching: OKLab lightness/chroma/hue track perception
// far better than HSL/HSV (whose hue is bunched up around yellow and whose
// lightness ignores that yellow reads brighter than blue).
function srgbToOklab(r, g, b) {
  const lin = (c) => {
    c /= 255;
    return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
  };
  const [lr, lg, lb] = [lin(r), lin(g), lin(b)];
  const l = Math.cbrt(0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb);
  const m = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb);
  const s = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb);
  return [
    0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
  ];
}

// Returns [hue(0-360, OKLCh), chroma(~0-0.33), lightness(0-1, OKLab L)].
function rgbTraits(r, g, b) {
  const [L, a, bb] = srgbToOklab(r, g, b);
  const c = Math.hypot(a, bb);
  let h = (Math.atan2(bb, a) * 180) / Math.PI;
  if (h < 0) h += 360;
  return [h, c, L];
}

// Circular distance between two hues, in degrees (0-180).
function hueDist(a, b) {
  const d = Math.abs(((a - b) % 360 + 360) % 360);
  return Math.min(d, 360 - d);
}

// Near-grayscale colors are hue-agnostic; keep them on for hue/relationship presets.
// Threshold is OKLab chroma (saturated primaries reach ~0.25-0.33).
const isNeutral = (c) => c < 0.04;

function median(arr) {
  if (!arr.length) return 0;
  const a = [...arr].sort((x, y) => x - y);
  const n = a.length;
  return n % 2 ? a[(n - 1) / 2] : (a[n / 2 - 1] + a[n / 2]) / 2;
}

// Rebuild the swatch row from `palette`, reflecting enabled/base state, and
// update the "使う色: N / M" count. Clicking a swatch toggles that color;
// Shift+click sets/unsets it as the base for relationship presets.
function renderPalette() {
  const swatches = $("swatches");
  swatches.innerHTML = "";
  palette.forEach((c) => {
    const span = document.createElement("span");
    span.style.background = `rgb(${c.r}, ${c.g}, ${c.b})`;
    const hex = hexOf(c);
    span.title = `#${hex}（クリック: ON/OFF、Shift+クリック: 基準色）`;
    if (!c.enabled) span.classList.add("off");
    if (hex === baseHex) span.classList.add("base");
    span.addEventListener("click", (e) => {
      if (e.shiftKey) {
        baseHex = baseHex === hex ? null : hex;
        renderPalette();
        return;
      }
      c.enabled = !c.enabled;
      renderPalette();
    });
    swatches.appendChild(span);
  });
  const on = palette.filter((c) => c.enabled).length;
  $("paletteCount").textContent = palette.length ? `使う色: ${on} / ${palette.length}` : "";
  updateSeedEnabled();
}

// The seed only matters while k-means runs: with "Limit colors" on, or with no
// active palette (the pure nearest-snap path is deterministic). Gray it out
// otherwise so it doesn't look like a knob that does something.
function updateSeedEnabled() {
  const kmeansRuns = $("limitColors").checked || !palette.some((c) => c.enabled);
  $("seed").disabled = !kmeansRuns;
  $("randomSeed").disabled = !kmeansRuns;
}

// Re-parse the textarea into `palette` and re-render. ON/OFF state is keyed by
// hex value so editing the textarea keeps the curation of colors that survive;
// only genuinely new colors default to enabled.
function refreshPalette() {
  const prev = new Map(palette.map((c) => [hexOf(c), c.enabled]));
  const rgb = parsePalette($("paletteText").value);
  palette = [];
  for (let i = 0; i < rgb.length; i += 3) {
    const c = { r: rgb[i], g: rgb[i + 1], b: rgb[i + 2], enabled: true };
    const known = prev.get(hexOf(c));
    if (known !== undefined) c.enabled = known;
    palette.push(c);
  }
  if (baseHex && !palette.some((c) => hexOf(c) === baseHex)) baseHex = null;
  renderPalette();
}

// Apply a "concept" preset: set each color's enabled flag by a rule, then re-render.
// Presets overwrite the current ON/OFF state; manual swatch toggles still work after.
function applyPreset(name) {
  if (!palette.length) return;
  if (name === "all") {
    palette.forEach((c) => (c.enabled = true));
    renderPalette();
    setStatus("すべての色をONにしたよ。");
    return;
  }

  const traits = palette.map((c) => rgbTraits(c.r, c.g, c.b));
  let pred;

  if (name === "warm" || name === "cool") {
    // OKLCh hue anchors: red≈29°, yellow≈110°, green≈142°, blue≈264°, magenta≈328°.
    // Fixed split: warm = red→yellow plus magenta/pink, cool = the rest.
    const idxs = [];
    traits.forEach(([, c], i) => {
      if (!isNeutral(c)) idxs.push(i);
    });
    const isWarmFixed = (h) => h <= 120 || h >= 320;
    let warm = new Set(idxs.filter((i) => isWarmFixed(traits[i][0])));
    // Relative fallback: when the fixed boundary doesn't separate anything
    // (an all-warm or all-cool palette), split at the median "warmth"
    // (circular hue distance to orange-red) so the preset still halves the
    // palette into its warmer and cooler sides.
    if (idxs.length >= 2 && (warm.size === 0 || warm.size === idxs.length)) {
      const warmth = new Map(idxs.map((i) => [i, -hueDist(traits[i][0], 40)]));
      const med = median([...warmth.values()]);
      warm = new Set(idxs.filter((i) => warmth.get(i) >= med));
    }
    pred = (h, c, l, i) => isNeutral(c) || (name === "warm" ? warm.has(i) : !warm.has(i));
  } else if (name === "light" || name === "dark") {
    const med = median(traits.map((x) => x[2]));
    pred = name === "light" ? (h, s, l) => l >= med : (h, s, l) => l < med;
  } else if (name === "vivid" || name === "muted") {
    const med = median(traits.map((x) => x[1]));
    pred = name === "vivid" ? (h, s) => s >= med : (h, s) => s < med;
  } else if (name === "complementary" || name === "analogous" || name === "triadic") {
    // A Shift+clicked base color wins; otherwise fall back to the most
    // saturated non-neutral color.
    let bi = baseHex ? palette.findIndex((c, i) => hexOf(c) === baseHex && !isNeutral(traits[i][1])) : -1;
    if (bi < 0) {
      let bs = -1;
      traits.forEach(([h, s], i) => {
        if (!isNeutral(s) && s > bs) { bs = s; bi = i; }
      });
    }
    if (bi < 0) {
      palette.forEach((c) => (c.enabled = true));
      renderPalette();
      setStatus("基準にできる鮮やかな色がない（全色ニュートラル）ので全色ONのままだよ。");
      return;
    }
    baseHex = hexOf(palette[bi]);
    const H = traits[bi][0];
    if (name === "complementary") pred = (h, s) => isNeutral(s) || hueDist(h, H) <= 35 || hueDist(h, H + 180) <= 35;
    else if (name === "analogous") pred = (h, s) => isNeutral(s) || hueDist(h, H) <= 45;
    else pred = (h, s) => isNeutral(s) || hueDist(h, H) <= 30 || hueDist(h, H + 120) <= 30 || hueDist(h, H + 240) <= 30;
  } else {
    return;
  }

  let on = 0;
  palette.forEach((c, i) => {
    c.enabled = pred(traits[i][0], traits[i][1], traits[i][2], i);
    if (c.enabled) on++;
  });
  if (on === 0) {
    palette.forEach((c) => (c.enabled = true));
    renderPalette();
    setStatus("このパレットには該当する色がなかったので全色ONに戻したよ。");
    return;
  }
  renderPalette();
  setStatus(`プリセット適用: ${on} / ${palette.length} 色を使用。`);
}

function run() {
  if (!inputBytes) return;
  setStatus("変換中...");
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
      const seedRaw = $("seed").value.trim();
      const seedNum = Number(seedRaw);
      const seedVal = seedRaw === "" || !Number.isFinite(seedNum) ? undefined : seedNum >>> 0;
      const dither = $("dither").checked || undefined;
      const out = process_image(inputBytes, k, undefined, paletteRgb, seedVal, dither);
      const blob = new Blob([out], { type: "image/png" });
      if (outUrl) URL.revokeObjectURL(outUrl);
      outUrl = URL.createObjectURL(blob);
      $("after").src = outUrl;
      const actual = $("actual");
      actual.onload = () => {
        $("actualSize").textContent = `${actual.naturalWidth} × ${actual.naturalHeight} px`;
      };
      actual.src = outUrl;
      $("actualFig").hidden = false;
      const dl = $("download");
      dl.href = outUrl;
      dl.hidden = false;
      setStatus("できあがり！下の「画像を保存（PNG）」から保存できるよ。");
    } catch (err) {
      setStatus("エラー: " + err);
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
  updateSeedEnabled();
});
$("kSlider").addEventListener("input", (e) => ($("kValue").textContent = e.target.value));

$("paletteText").addEventListener("input", refreshPalette);

// Convert GIMP .gpl / JASC .pal text to plain hex lines so the textarea stays
// the single source of truth and parsePalette stays hex-only.
function paletteTextFromGplOrPal(text, name) {
  let rows = text.split(/\r?\n/).map((l) => l.trim());
  if (/\.pal$/.test(name)) rows = rows.filter((l) => l).slice(3); // JASC-PAL / version / count
  const out = [];
  for (const line of rows) {
    const m = line.match(/^(\d{1,3})\s+(\d{1,3})\s+(\d{1,3})/);
    if (!m) continue;
    out.push([+m[1], +m[2], +m[3]].map((v) => Math.min(255, v).toString(16).padStart(2, "0")).join(""));
  }
  return out.join("\n");
}

$("paletteFile").addEventListener("change", async (e) => {
  const file = e.target.files[0];
  if (!file) return;
  const name = file.name.toLowerCase();
  try {
    if (file.type.startsWith("image/") || /\.(png|jpe?g)$/.test(name)) {
      // Extract a palette from the image via WASM (unique colors, k-means
      // reduced when there are more than 64).
      const flat = extract_palette(new Uint8Array(await file.arrayBuffer()), 64);
      const lines = [];
      for (let i = 0; i < flat.length; i += 3) {
        lines.push(hexOf({ r: flat[i], g: flat[i + 1], b: flat[i + 2] }));
      }
      $("paletteText").value = lines.join("\n");
      setStatus(`画像から ${lines.length} 色を抽出したよ。`);
    } else {
      let txt = await file.text();
      if (/\.(gpl|pal)$/.test(name)) txt = paletteTextFromGplOrPal(txt, name);
      $("paletteText").value = txt;
    }
    refreshPalette();
  } catch (err) {
    setStatus("パレット読み込みエラー: " + err);
  }
});

$("exportHex").addEventListener("click", () => {
  const lines = palette.filter((c) => c.enabled).map(hexOf);
  if (!lines.length) {
    setStatus("ONの色がないから書き出せないんだけど？");
    return;
  }
  const blob = new Blob([lines.join("\n") + "\n"], { type: "text/plain" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = "palette.hex";
  a.click();
  URL.revokeObjectURL(a.href);
  setStatus(`ONの ${lines.length} 色を palette.hex に書き出したよ。`);
});

$("randomSeed").addEventListener("click", () => {
  $("seed").value = Math.floor(Math.random() * 0x100000000);
  if (ready && inputBytes) run();
});

document.querySelector(".presets").addEventListener("click", (e) => {
  const btn = e.target.closest("button[data-preset]");
  if (btn) applyPreset(btn.dataset.preset);
});

$("run").addEventListener("click", run);

// --- Left sidebar tabs: toggle aria-selected on the tab and `hidden` on its panel ---
const tabs = [...document.querySelectorAll('.tabs [role="tab"]')];
tabs.forEach((tab) => {
  tab.addEventListener("click", () => {
    tabs.forEach((t) => {
      const selected = t === tab;
      t.setAttribute("aria-selected", selected ? "true" : "false");
      $(t.getAttribute("aria-controls")).hidden = !selected;
    });
  });
});

init()
  .then(() => {
    ready = true;
    setStatus("準備OK！画像を入れてスタートしよ。");
    updateRunEnabled();
  })
  .catch((err) => setStatus("読みこみに失敗しちゃった: " + err));
