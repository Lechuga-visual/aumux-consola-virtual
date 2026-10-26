"use strict";
// Real EQ spectra only; audio remains in the engine and FFTs run on its worker.
let eqSpectrumSettings = {
  mode: "compact",
  point: "post",
  enabled: true,
  peaks: false,
};
try {
  const saved = JSON.parse(localStorage.getItem("aumux-eq-spectrum") || "{}");
  if (["compact", "compare", "history"].includes(saved.mode))
    eqSpectrumSettings.mode = saved.mode;
  if (["pre", "post"].includes(saved.point))
    eqSpectrumSettings.point = saved.point;
  if (typeof saved.enabled === "boolean")
    eqSpectrumSettings.enabled = saved.enabled;
  if (typeof saved.peaks === "boolean") eqSpectrumSettings.peaks = saved.peaks;
} catch {}
let eqSpectrum,
  eqSpectrumTimer,
  eqSpectrumRequest,
  eqSpectrumEpoch = 0;
function stopEqSpectrum() {
  clearTimeout(eqSpectrumTimer);
  eqSpectrumRequest?.abort();
  eqSpectrumEpoch++;
}
function spectrumPath(values) {
  return values
    .map(
      (db, i) =>
        `${i ? "L" : "M"}${eqChart.left + (i / (values.length - 1)) * (eqChart.right - eqChart.left)} ${eqChart.top + Math.max(0, Math.min(1, -db / 96)) * (eqChart.bottom - eqChart.top)}`,
    )
    .join(" ");
}
function clearEqSpectrum() {
  for (const id of ["eq-spectrum-pre", "eq-spectrum-post", "eq-spectrum-peaks"])
    document.getElementById(id)?.setAttribute("d", "");
  if (eqSpectrum) {
    eqSpectrum.pre = [];
    eqSpectrum.post = [];
    eqSpectrum.peak = [];
    eqSpectrum.canvas.getContext("2d").clearRect(0, 0, 256, 100);
    eqSpectrum.historyTime = null;
    eqSpectrum.historyRemainder = 0;
  }
}
function updateSpectrumAppearance() {
  if (!eqSpectrum) return;
  const s = eqSpectrumSettings;
  eqSpectrum.history.hidden = s.mode !== "history" || !s.enabled;
  eqSpectrum.axis.style.display = s.enabled ? "" : "none";
  eqSpectrum.bar.querySelector("button").textContent =
    `${s.mode === "compact" ? "A" : s.mode === "compare" ? "B" : "C"} · Analizador ⋮`;
  document.getElementById("eq-graph").dataset.spectrumMode = s.mode;
  drawEqSpectrum();
}
function drawEqSpectrum() {
  if (!eqSpectrum) return;
  const s = eqSpectrumSettings;
  for (const point of ["pre", "post"]) {
    const path = document.getElementById("eq-spectrum-" + point),
      show = s.enabled && (s.mode !== "compact" || s.point === point),
      values = eqSpectrum[point];
    let d = show && values.length ? spectrumPath(values) : "";
    if (d && s.mode === "compact")
      d += ` L${eqChart.right} ${eqChart.bottom} L${eqChart.left} ${eqChart.bottom} Z`;
    path.setAttribute("d", d);
    path.style.fill =
      s.mode === "compact"
        ? point === "pre"
          ? "#7c89952b"
          : "#36bfd22b"
        : "none";
  }
  document
    .getElementById("eq-spectrum-peaks")
    .setAttribute(
      "d",
      s.enabled && s.peaks && eqSpectrum.peak.length
        ? spectrumPath(eqSpectrum.peak)
        : "",
    );
  if (!s.enabled)
    eqSpectrum.label.textContent = "Apagado · clic derecho para configurar";
  else if (eqSpectrum.frozen)
    eqSpectrum.label.textContent =
      "Congelado · solo la imagen; el audio continúa";
}
function saveSpectrumSettings(change) {
  const pointChanged =
    change.point && change.point !== eqSpectrumSettings.point;
  Object.assign(eqSpectrumSettings, change);
  try {
    localStorage.setItem(
      "aumux-eq-spectrum",
      JSON.stringify(eqSpectrumSettings),
    );
  } catch {}
  if (pointChanged) clearEqSpectrum();
  if (eqSpectrum) eqSpectrum.peak = [];
  updateSpectrumAppearance();
  if (!eqSpectrumSettings.enabled) clearEqSpectrum();
}
function openSpectrumMenu(x, y) {
  const s = eqSpectrumSettings;
  showContextActions(
    "Analizador del EQ",
    [
      [
        `${s.mode === "compact" ? "✓ " : ""}A · Compacto`,
        () => saveSpectrumSettings({ mode: "compact" }),
      ],
      [
        `${s.mode === "compare" ? "✓ " : ""}B · Comparar PRE / POST`,
        () => saveSpectrumSettings({ mode: "compare" }),
      ],
      [
        `${s.mode === "history" ? "✓ " : ""}C · Espectro + historial`,
        () => saveSpectrumSettings({ mode: "history" }),
      ],
      [
        `Fuente compacta: ${s.point.toUpperCase()}`,
        () =>
          saveSpectrumSettings({ point: s.point === "post" ? "pre" : "post" }),
      ],
      [
        eqSpectrum?.frozen ? "Reanudar imagen" : "Congelar imagen",
        () => {
          eqSpectrum.frozen = !eqSpectrum.frozen;
          drawEqSpectrum();
        },
      ],
      [
        s.peaks
          ? "Desactivar retención de picos"
          : "Activar retención de picos",
        () => saveSpectrumSettings({ peaks: !s.peaks }),
      ],
      [
        "Restablecer picos",
        () => {
          eqSpectrum.peak = [];
          drawEqSpectrum();
        },
      ],
      [
        s.enabled ? "Apagar analizador" : "Encender analizador",
        () => saveSpectrumSettings({ enabled: !s.enabled }),
      ],
    ],
    x,
    y,
  );
}
function attachEqSpectrum() {
  stopEqSpectrum();
  const graph = document.getElementById("eq-graph"),
    svg = graph.querySelector("svg"),
    paths = svgEl("g", { class: "eq-spectrum-traces", "aria-hidden": "true" });
  for (const point of ["pre", "post", "peaks"])
    paths.append(
      svgEl("path", {
        id: "eq-spectrum-" + point,
        class: "eq-spectrum-" + point,
      }),
    );
  svg.insertBefore(paths, svg.querySelector("#eq-selected-fill"));
  const axis = svgEl("g", { class: "eq-spectrum-axis", "aria-hidden": "true" });
  for (const db of [0, -24, -48, -72, -96]) {
    const label = svgEl("text", {
      x: 898,
      y: eqChart.top + (-db / 96) * (eqChart.bottom - eqChart.top) + 3,
      "text-anchor": "end",
      class: "eq-axis-text",
    });
    label.textContent = String(db);
    axis.append(label);
  }
  svg.append(axis);
  const bar = el("div", "eq-spectrum-bar"),
    button = el("button"),
    label = el("span", "", "Esperando audio · FFT 4096 · 85,3 ms de análisis"),
    history = el("div", "eq-spectrum-history"),
    canvas = document.createElement("canvas");
  button.type = "button";
  button.setAttribute("aria-label", "Opciones del analizador de espectro");
  button.onclick = () => {
    const r = button.getBoundingClientRect();
    openSpectrumMenu(r.left, r.bottom);
  };
  canvas.width = 256;
  canvas.height = 100;
  canvas.setAttribute("role", "img");
  canvas.setAttribute(
    "aria-label",
    "Historial espectral de cinco segundos: arriba lo más reciente",
  );
  history.append(
    canvas,
    el("span", "", "Historial 5 s · POST · arriba: ahora"),
  );
  bar.append(button, label);
  graph.append(bar, history);
  eqSpectrum = {
    target: { ...eqTarget },
    bar,
    label,
    history,
    canvas,
    axis,
    pre: [],
    post: [],
    peak: [],
    frozen: false,
    sequence: null,
    generation: null,
    historyTime: null,
    historyRemainder: 0,
  };
  graph.oncontextmenu = (e) => {
    e.preventDefault();
    e.stopPropagation();
    openSpectrumMenu(e.clientX, e.clientY);
  };
  svg.tabIndex = 0;
  graph.onkeydown = (e) => {
    if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
      e.preventDefault();
      e.stopPropagation();
      const r = graph.getBoundingClientRect();
      openSpectrumMenu(r.left + 35, r.top + 35);
    }
  };
  updateSpectrumAppearance();
  pollEqSpectrum(eqSpectrumEpoch);
}
function drawSpectrumHistory(data) {
  const s = eqSpectrum,
    context = s.canvas.getContext("2d");
  if (s.historyTime !== null && data.timestamp_ms - s.historyTime > 500) {
    context.clearRect(0, 0, 256, 100);
    s.historyTime = null;
    s.historyRemainder = 0;
  }
  if (s.historyTime === null) s.historyTime = data.timestamp_ms - 50;
  s.historyRemainder +=
    (Math.max(0, data.timestamp_ms - s.historyTime) * 20) / 1000;
  s.historyTime = data.timestamp_ms;
  const rows = Math.min(100, Math.floor(s.historyRemainder));
  s.historyRemainder -= Math.floor(s.historyRemainder);
  if (!rows) return;
  if (rows < 100)
    context.drawImage(
      s.canvas,
      0,
      0,
      256,
      100 - rows,
      0,
      rows,
      256,
      100 - rows,
    );
  const image = context.createImageData(256, rows);
  for (let i = 0; i < 256; i++) {
    const v = Math.max(0, Math.min(1, (data.post[i] + 96) / 96));
    for (let row = 0; row < rows; row++) {
      const p = (row * 256 + i) * 4;
      image.data[p] = Math.round((Math.max(0, v - 0.65) / 0.35) * 255);
      image.data[p + 1] = Math.round(v * 225);
      image.data[p + 2] = Math.round(20 + Math.min(1, v * 2) * 175);
      image.data[p + 3] = 255;
    }
  }
  context.putImageData(image, 0, 0);
}
async function pollEqSpectrum(epoch) {
  if (epoch !== eqSpectrumEpoch || !document.getElementById("eq-dialog").open)
    return;
  let delay = 80;
  try {
    if (!eqSpectrumSettings.enabled || eqSpectrum.frozen || document.hidden)
      return;
    const target = eqSpectrum.target;
    const controller = new AbortController();
    eqSpectrumRequest = controller;
    const timeout = setTimeout(() => controller.abort(), 2000);
    let response;
    try {
      response = await webFetch("/api/spectrum", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "X-Monitor-Token": token,
        },
        body: JSON.stringify({
          ...target,
          trace:
            eqSpectrumSettings.mode === "compact"
              ? eqSpectrumSettings.point
              : "both",
        }),
        signal: controller.signal,
      });
    } finally {
      clearTimeout(timeout);
    }
    const result = await response.json();
    if (epoch !== eqSpectrumEpoch) return;
    if (!response.ok) throw Error(result.error || "Analizador no disponible");
    if (!eqSpectrumSettings.enabled || eqSpectrum.frozen) return;
    const data = result.data;
    if (!result.running || !data || result.now_ms - data.timestamp_ms > 500) {
      clearEqSpectrum();
      eqSpectrum.label.textContent = result.running
        ? "Esperando muestras del EQ…"
        : "Motor detenido · sin señal";
      return;
    }
    if (
      data.sequence === eqSpectrum.sequence &&
      data.generation === eqSpectrum.generation
    )
      return;
    if (data.generation !== eqSpectrum.generation) clearEqSpectrum();
    eqSpectrum.sequence = data.sequence;
    eqSpectrum.generation = data.generation;
    for (const point of ["pre", "post"]) {
      if (data[point].length !== 256) continue;
      const previous = eqSpectrum[point];
      eqSpectrum[point] = data[point].map((db, i) =>
        previous.length
          ? 10 *
            Math.log10(0.5 * 10 ** (db / 10) + 0.5 * 10 ** (previous[i] / 10))
          : db,
      );
    }
    const values =
      eqSpectrumSettings.mode === "compact"
        ? eqSpectrum[eqSpectrumSettings.point]
        : eqSpectrum.post;
    eqSpectrum.peak = values.map((v, i) =>
      Math.max(v, eqSpectrum.peak[i] ?? -120),
    );
    if (eqSpectrumSettings.mode === "history" && data.post.length === 256)
      drawSpectrumHistory(data);
    eqSpectrum.label.textContent = `${eqSpectrumSettings.mode === "compact" ? eqSpectrumSettings.point.toUpperCase() : "PRE gris / POST cian"} · pico FFT dBFS · 4096 / 85,3 ms${result.dropped_blocks ? " · actualizaciones omitidas: " + result.dropped_blocks : ""}`;
    drawEqSpectrum();
  } catch (e) {
    if (epoch !== eqSpectrumEpoch) return;
    delay = 750;
    eqSpectrum.label.textContent = `Reconectando analizador · ${e.message}`;
  } finally {
    if (epoch === eqSpectrumEpoch && document.getElementById("eq-dialog").open)
      eqSpectrumTimer = setTimeout(() => pollEqSpectrum(epoch), delay);
  }
}
document.getElementById("eq-dialog").addEventListener("close", stopEqSpectrum);
window.addEventListener("beforeunload", stopEqSpectrum);
window.addEventListener("storage", (e) => {
  if (e.key !== "aumux-eq-spectrum" || !e.newValue) return;
  try {
    const s = JSON.parse(e.newValue);
    if (
      !["compact", "compare", "history"].includes(s.mode) ||
      !["pre", "post"].includes(s.point) ||
      typeof s.enabled !== "boolean" ||
      typeof s.peaks !== "boolean"
    )
      return;
    eqSpectrumSettings = s;
    clearEqSpectrum();
    updateSpectrumAppearance();
  } catch {}
});
