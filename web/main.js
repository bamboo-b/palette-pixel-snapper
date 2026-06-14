import init, { process_image } from "./pkg/spritefusion_pixel_snapper.js";

const $ = (id) => document.getElementById(id);

let inputBytes = null;
let ready = false;

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

// Re-render the display-only swatches from the current textarea contents.
function refreshPalette() {
  const rgb = parsePalette($("paletteText").value);
  const swatches = $("swatches");
  swatches.innerHTML = "";
  for (let i = 0; i < rgb.length; i += 3) {
    const r = rgb[i], g = rgb[i + 1], b = rgb[i + 2];
    const span = document.createElement("span");
    span.style.background = `rgb(${r}, ${g}, ${b})`;
    const hex = [r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("");
    span.title = `#${hex}`;
    swatches.appendChild(span);
  }
}

function run() {
  if (!inputBytes) return;
  setStatus("Processing...");
  $("run").disabled = true;
  setTimeout(() => {
    try {
      const k = $("limitColors").checked ? Number($("kSlider").value) : undefined;
      const rgb = parsePalette($("paletteText").value);
      const paletteRgb = rgb.length ? new Uint8Array(rgb) : undefined;
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

$("run").addEventListener("click", run);

init()
  .then(() => {
    ready = true;
    setStatus("Ready. Drop an image to start.");
    updateRunEnabled();
  })
  .catch((err) => setStatus("Failed to load WASM: " + err));
