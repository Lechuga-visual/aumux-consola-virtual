"use strict";
const $ = (id) => document.getElementById(id);
const token = document.querySelector('meta[name="monitor-token"]').content;
let cfg,
  view = -1,
  selected = { type: "channel", index: 0 },
  running = false,
  lastStatus;
let queue = Promise.resolve(),
  selectedFx,
  pendingFx,
  descriptors = [];
let meterWidgets = [];
let vstCatalogue = [],
  vstRoots = [],
  catalogueLoaded = false;
const faderTicks = [6, 0, -6, -12, -24, -36, -60];
const meterStates = new Map(),
  lastSignal = [];
let limits = { channels: 64, auxes: 32, inputs: 64, outputs: 64 };
let eqTarget;
const signalNames = {
  off: "Entrada física",
  white: "Ruido blanco",
  pink: "Ruido rosa",
  sine: "Tono seno",
};
const stripColour = "#479469";

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}
function defaultEq() {
  return {
    bypass: false,
    bands: [50, 100, 250, 500, 1000, 2500, 6000, 12000].map((frequency) => ({
      kind: 0,
      frequency,
      gain_db: 0,
      q: 0.707,
    })),
  };
}
function normalise(c) {
  c.pc_monitor ??= 0;
  const legacy = c.version === 1;
  c.channels.forEach((ch, i) => {
    ch.inserts ||= [];
    if (ch.fx) ch.inserts.unshift(ch.fx);
    ch.fx = null;
    ch.generator ||= { signal: "off", level_db: -30, frequency: 1000 };
    ch.eq ||= defaultEq();
    if (legacy) {
      ch.format = "mono";
      ch.input = i < c.inputs ? i : null;
      ch.input_right = i + 1 < c.inputs ? i + 1 : null;
    }
  });
  c.buses.forEach((b) => {
    b.inserts ||= [];
    if (b.fx) b.inserts.unshift(b.fx);
    b.fx = null;
    b.eq ||= defaultEq();
    if (legacy) b.format = "stereo";
  });
  if (legacy) c.version = 2;
  return c;
}
function error(e) {
  $("error").textContent = e.message || e;
  $("error").title = $("error").textContent;
}
async function api(route, body = {}) {
  const r = await fetch("/api/" + route, {
    method: "POST",
    headers: { "Content-Type": "application/json", "X-Monitor-Token": token },
    body: JSON.stringify(body),
  });
  const v = await r.json();
  if (!r.ok) throw Error(v.error || r.statusText);
  return v;
}
function publish() {
  const snapshot = structuredClone(cfg);
  const task = queue.then(() => api("config", snapshot));
  queue = task.catch(() => {});
  task.catch(error);
  return task;
}
function db(v, unit = "") {
  return v <= -89.9
    ? "−∞" + unit
    : (v > 0 ? "+" : "") + Number(v).toFixed(1) + unit;
}
function signalDb(v) {
  return v > 0 ? Math.max(-90, 20 * Math.log10(v)) : -90;
}
function faderPosition(value) {
  // A console-style taper gives most travel to the useful -30..+6 dB range.
  const points = [
    [0, -90],
    [0.1, -60],
    [0.25, -30],
    [0.5, -12],
    [0.75, 0],
    [1, 6],
  ];
  for (let i = 1; i < points.length; i++)
    if (value <= points[i][1]) {
      const [x0, y0] = points[i - 1],
        [x1, y1] = points[i];
      return (x0 + ((value - y0) / (y1 - y0)) * (x1 - x0)) * 1000;
    }
  return 1000;
}
function faderDb(position) {
  const points = [
    [0, -90],
    [100, -60],
    [250, -30],
    [500, -12],
    [750, 0],
    [1000, 6],
  ];
  for (let i = 1; i < points.length; i++)
    if (position <= points[i][0]) {
      const [x0, y0] = points[i - 1],
        [x1, y1] = points[i];
      return (
        Math.round((y0 + ((position - x0) / (x1 - x0)) * (y1 - y0)) * 10) / 10
      );
    }
  return 6;
}
function slider(value, min, max, step, onChange) {
  const input = el("input");
  input.type = "range";
  input.min = min;
  input.max = max;
  input.step = step;
  input.value = value;
  input.oninput = () => onChange(Number(input.value));
  return input;
}
function targetOf(type, index) {
  return type === "channel" ? cfg.channels[index] : cfg.buses[index];
}
function keyOf(type, index) {
  return (type === "channel" ? "c" : "b") + index;
}
function peakState(key) {
  if (!meterStates.has(key))
    meterStates.set(key, {
      target: [-90, -90],
      rmsTarget: [-90, -90],
      peak: [-90, -90],
      rms: [-90, -90],
      hold: [-90, -90],
      until: [0, 0],
      clip: false,
    });
  return meterStates.get(key);
}
function meter(key) {
  const widget = el("div", "meter-widget");
  widget.title =
    "Peak / RMS estéreo · post-fader · dBFS. Clic para borrar el pico.";
  const clip = el("i", "meter-clip");
  widget.append(clip);
  const target =
    key[0] === "c"
      ? cfg.channels[Number(key.slice(1))]
      : cfg.buses[Number(key.slice(1))];
  widget.classList.toggle("mono", target?.format === "mono");
  const axis = el("div", "meter-axis");
  for (const value of [0, -6, -12, -24, -36, -48, -60]) {
    const mark = el("span", "", String(value));
    mark.style.bottom = meterPercent(value) + "%";
    axis.append(mark);
  }
  widget.append(axis);
  const lanes = [];
  for (let side = 0; side < 2; side++) {
    const track = el("div", "meter-track"),
      peak = el("i", "meter-peak"),
      rms = el("i", "meter-rms"),
      hold = el("i", "meter-hold");
    track.append(peak, rms, hold);
    widget.append(track);
    lanes.push({ peak, rms, hold });
  }
  widget.onclick = (e) => {
    e.stopPropagation();
    const s = peakState(key);
    s.hold = [-90, -90];
    s.until = [0, 0];
    s.clip = false;
  };
  meterWidgets.push({ key, widget, lanes });
  return widget;
}
function resetPeaks() {
  for (const s of meterStates.values()) {
    s.hold = [-90, -90];
    s.until = [0, 0];
    s.clip = false;
  }
}
let lastAnimation = performance.now();
function animateMeters(now) {
  const dt = Math.min(0.1, (now - lastAnimation) / 1000);
  lastAnimation = now;
  for (const s of meterStates.values())
    for (let i = 0; i < 2; i++) {
      s.peak[i] = Math.max(s.target[i], s.peak[i] - dt * 32);
      s.rms[i] = s.rmsTarget[i];
      if (s.target[i] >= s.hold[i]) {
        s.hold[i] = s.target[i];
        s.until[i] = now + 1400;
      } else if (now > s.until[i])
        s.hold[i] = Math.max(s.target[i], s.hold[i] - dt * 14);
      if (s.target[i] >= 0) s.clip = true;
    }
  const percent = meterPercent;
  for (const w of meterWidgets) {
    const s = peakState(w.key);
    w.widget.classList.toggle("clipped", s.clip);
    w.widget.title = `Post-fader · dBFS\nPeak L/R: ${s.peak.map((v) => db(v)).join(" / ")}\nRMS L/R (300 ms): ${s.rms.map((v) => db(v)).join(" / ")}`;
    w.lanes.forEach((lane, i) => {
      lane.peak.style.clipPath = `inset(${100 - percent(s.peak[i])}% 0 0)`;
      lane.rms.style.height = percent(s.rms[i]) + "%";
      lane.hold.style.bottom = percent(s.hold[i]) + "%";
      lane.hold.style.opacity = s.hold[i] > -60 ? "1" : "0";
    });
  }
  document.querySelectorAll("[data-peak-readout]").forEach((e) => {
    const s = peakState(e.dataset.peakReadout);
    e.textContent = s.hold
      .map((v) => (v <= -60 ? "−∞" : v.toFixed(1)))
      .join(" / ");
    e.style.color = s.clip ? "#ed9294" : "";
  });
  requestAnimationFrame(animateMeters);
}
function meterPercent(value) {
  return Math.max(0, Math.min(100, ((value + 60) / 60) * 100));
}
function selectStrip(type, index) {
  selected = { type, index };
  document
    .querySelectorAll("[data-strip]")
    .forEach((e) =>
      e.classList.toggle("is-selected", e.dataset.strip === keyOf(type, index)),
    );
  renderInspector();
}
function fxName(fx) {
  return (
    fx.name ||
    fx.path
      .split(/[\\/]/)
      .pop()
      .replace(/\.vst3$/i, "") ||
    "VST3"
  );
}
function insertRack(type, index) {
  const target = targetOf(type, index),
    rack = el("div", "insert-rack");
  for (let slot = 0; slot < 4; slot++) {
    const fx = target.inserts[slot],
      row = el("div", "insert-row" + (fx?.bypass ? " bypassed" : ""));
    const button = el("button", "insert-slot" + (!fx ? " empty" : ""));
    button.disabled = running || (!fx && slot > target.inserts.length);
    button.append(
      el("span", "slot-number", String(slot + 1).padStart(2, "0")),
      el(
        "span",
        "insert-name",
        fx ? fxName(fx) : slot === target.inserts.length ? "＋ FX" : "—",
      ),
    );
    button.title = fx
      ? `${fxName(fx)}\n${fx.path}${running ? "\nDetén el audio para editar." : ""}`
      : "Añadir un efecto nativo o VST3";
    button.onclick = () => openFx(type, index, slot);
    row.append(button);
    if (fx) {
      const bypass = el("button", "bypass" + (fx.bypass ? " off" : ""), "⏻");
      bypass.title = fx.bypass ? "Activar insert" : "Bypass del insert";
      bypass.setAttribute("aria-label", `Bypass ${fxName(fx)}`);
      bypass.onclick = () => {
        fx.bypass = !fx.bypass;
        publish();
        row.classList.toggle("bypassed", fx.bypass);
        bypass.classList.toggle("off", fx.bypass);
      };
      row.append(bypass);
    }
    rack.append(row);
  }
  return rack;
}
function bridgeCell(type, index) {
  const key = keyOf(type, index),
    target = targetOf(type, index),
    cell = el("div", "bridge-cell");
  cell.dataset.strip = key;
  cell.style.setProperty(
    "--strip-color",
    type === "channel" ? stripColour : index === 0 ? "#e65539" : "#278ba5",
  );
  cell.append(
    el("span", "bridge-name", target.name),
    el(
      "span",
      "bridge-number",
      type === "channel"
        ? String(index + 1).padStart(2, "0")
        : index === 0
          ? "M"
          : "A" + index,
    ),
    meter(key),
  );
  cell.onclick = () => selectStrip(type, index);
  return cell;
}
function createFader(type, index) {
  const target = targetOf(type, index),
    key = keyOf(type, index),
    channel = type === "channel";
  const section = el("div", "fader-section"),
    scale = el("div", "fader-scale");
  for (const value of faderTicks) {
    const mark = el("span", "", String(value));
    mark.style.top = 100 - faderPosition(value) / 10 + "%";
    mark.title = `Fijar ${value} dB`;
    mark.onclick = (e) => {
      e.stopPropagation();
      input.value = faderPosition(value);
      input.dispatchEvent(new Event("input"));
    };
    scale.append(mark);
  }
  const fader = el("div", "fader");
  const level = el("input", "level");
  level.type = "text";
  level.value = db(
    channel && view >= 0 ? target.sends_db[view] : target.gain_db,
  );
  level.setAttribute("aria-label", `${target.name}: nivel exacto en dB`);
  level.title =
    "Escribe un nivel en dB; Enter para aplicar. Doble clic en el fader: 0 dB.";
  level.id = "fader-value-" + key;
  const current = channel && view >= 0 ? target.sends_db[view] : target.gain_db;
  const input = slider(faderPosition(current), 0, 1000, 0.1, (value) => {
    // Align to the printed marks and make the exact detents easy to hit.
    const nearest = faderTicks.find(
      (t) => Math.abs(faderPosition(t) - value) <= 5,
    );
    const gain = nearest === undefined ? faderDb(value) : nearest;
    input.value = faderPosition(gain);
    if (channel && view >= 0) target.sends_db[view] = gain;
    else target.gain_db = gain;
    level.value = db(gain);
    input.setAttribute("aria-valuetext", db(gain, " dB"));
    publish();
    updateSendLabels();
  });
  input.setAttribute(
    "aria-label",
    `${target.name} ${channel && view >= 0 ? "envío Monitor " + (view + 1) : "fader"}`,
  );
  level.onfocus = () => level.select();
  level.onchange = () => {
    const value = Number(level.value.replace("−", "-").replace(",", "."));
    if (!Number.isFinite(value) || value < -90 || value > 6) {
      level.value = db(
        channel && view >= 0 ? target.sends_db[view] : target.gain_db,
      );
      return;
    }
    // Numeric entry does not snap: arbitrary tenths are retained.
    const gain = Math.round(value * 10) / 10;
    if (channel && view >= 0) target.sends_db[view] = gain;
    else target.gain_db = gain;
    input.value = faderPosition(gain);
    input.setAttribute("aria-valuetext", db(gain, " dB"));
    level.value = db(gain);
    publish();
    updateSendLabels();
  };
  level.onkeydown = (e) => {
    if (e.key === "Enter") level.blur();
  };
  input.setAttribute("aria-valuetext", db(current, " dB"));
  input.onkeydown = (e) => {
    const current =
      channel && view >= 0 ? target.sends_db[view] : target.gain_db;
    const delta = e.shiftKey ? 0.1 : 1;
    const next = {
      ArrowUp: current + delta,
      ArrowRight: current + delta,
      ArrowDown: current - delta,
      ArrowLeft: current - delta,
      Home: 6,
      End: -90,
    }[e.key];
    if (next !== undefined) {
      e.preventDefault();
      level.value = Math.max(-90, Math.min(6, next)).toFixed(1);
      level.onchange();
    }
  };
  input.ondblclick = () => {
    input.value = faderPosition(0);
    input.dispatchEvent(new Event("input"));
  };
  fader.append(input);
  section.append(scale, fader, meter(key));
  const readout = el("div", "peak-readout");
  readout.dataset.peakReadout = key;
  return { section, level, readout };
}
function sourceShort(c) {
  return c.generator.signal === "off"
    ? "ASIO IN"
    : c.generator.signal === "sine"
      ? `${c.generator.frequency} Hz`
      : c.generator.signal === "white"
        ? "BLANCO"
        : "ROSA";
}
function refreshGenerator(index) {
  const c = cfg.channels[index];
  const button = $("generator-button-" + index);
  if (button) {
    button.classList.toggle("generator-on", c.generator.signal !== "off");
    button.textContent = sourceShort(c);
  }
  const toggle = $("generator-toggle-" + index);
  if (toggle)
    toggle.classList.toggle("generator-on", c.generator.signal !== "off");
  if (selected.type === "channel" && selected.index === index) {
    const source = $("generator-source");
    if (source) source.value = c.generator.signal;
  }
}
function channelStrip(index) {
  const c = cfg.channels[index],
    key = "c" + index,
    card = el("section", "strip");
  card.dataset.strip = key;
  card.style.setProperty("--strip-color", stripColour);
  card.addEventListener("pointerdown", () => {
    if (selected.type !== "channel" || selected.index !== index)
      selectStrip("channel", index);
  });
  const header = el("div", "strip-header"),
    id = el("div", "strip-id");
  id.append(
    el("span", "", String(index + 1).padStart(2, "0")),
    el("span", "", "MONO → ST"),
  );
  const name = el("input", "name");
  name.value = c.name;
  name.maxLength = 120;
  name.setAttribute("aria-label", `Nombre canal ${index + 1}`);
  name.onchange = () => {
    c.name = name.value;
    publish();
    render();
  };
  header.append(id, name);
  card.append(header);
  card.append(
    el("div", "strip-caption", "INSERTS"),
    insertRack("channel", index),
  );
  const source = el(
    "button",
    "source-button" + (c.generator.signal !== "off" ? " generator-on" : ""),
    sourceShort(c),
  );
  source.id = "generator-button-" + index;
  source.title = "Seleccionar el generador de este canal";
  source.onclick = () => {
    selectStrip("channel", index);
    $("generator-source")?.focus();
  };
  card.append(source);
  const pan = el("div", "pan-row"),
    panLabel = el(
      "span",
      "",
      c.pan === 0
        ? "PAN · C"
        : `PAN · ${c.pan < 0 ? "L" : "R"} ${Math.round(Math.abs(c.pan) * 100)}`,
    );
  const panInput = slider(c.pan, -1, 1, 0.01, (value) => {
    c.pan = value;
    panLabel.textContent =
      value === 0
        ? "PAN · C"
        : `PAN · ${value < 0 ? "L" : "R"} ${Math.round(Math.abs(value) * 100)}`;
    publish();
  });
  panInput.setAttribute("aria-label", `Pan ${c.name}`);
  panInput.ondblclick = () => {
    panInput.value = 0;
    panInput.dispatchEvent(new Event("input"));
  };
  pan.append(panInput, panLabel);
  card.append(pan);
  const sends = el("div", "send-grid");
  const auxes = cfg.buses.length - 1;
  if (auxes <= 4) {
    for (let a = 0; a < auxes; a++) {
      const b = el("button");
      b.dataset.send = `${index}:${a}`;
      b.onclick = () => {
        view = a;
        selectStrip("channel", index);
        render();
      };
      sends.append(b);
    }
  } else {
    sends.classList.add("many-aux");
    const picker = el("select");
    picker.setAttribute("aria-label", `Auxiliares de ${c.name}`);
    picker.append(new Option("Envíos Aux…", ""));
    for (let a = 0; a < auxes; a++) {
      const o = new Option("", a);
      o.dataset.send = `${index}:${a}`;
      picker.append(o);
    }
    picker.value = view >= 0 ? view : "";
    picker.onchange = () => {
      if (picker.value !== "") {
        view = Number(picker.value);
        selectStrip("channel", index);
        render();
      }
    };
    sends.append(picker);
  }
  card.append(sends);
  const buttons = el("div", "strip-buttons"),
    mute = el("button", c.mute ? "muted" : "", "MUTE");
  mute.title = "Mute de canal: afecta master y todos sus envíos";
  mute.onclick = () => {
    c.mute = !c.mute;
    mute.classList.toggle("muted", c.mute);
    publish();
  };
  const gen = el(
    "button",
    c.generator.signal !== "off" ? "generator-on" : "",
    "GEN",
  );
  gen.id = "generator-toggle-" + index;
  gen.title = "Activar/desactivar generador de este canal";
  gen.onclick = () => {
    if (c.generator.signal !== "off") {
      lastSignal[index] = c.generator.signal;
      c.generator.signal = "off";
    } else c.generator.signal = lastSignal[index] || "white";
    publish();
    refreshGenerator(index);
  };
  buttons.append(mute, gen);
  card.append(buttons);
  const fader = createFader("channel", index);
  card.append(fader.section, fader.level, fader.readout);
  const footer = el("div", "strip-footer");
  footer.append(
    el("span", "", "POST L/R"),
    el("span", "", view < 0 ? "MAIN" : `MON ${view + 1}`),
  );
  card.append(footer);
  arrangeStrip(card, "channel", index);
  return card;
}
function busStrip(index) {
  const b = cfg.buses[index],
    key = "b" + index,
    card = el("section", "strip" + (index === 0 ? " master" : ""));
  card.dataset.strip = key;
  card.addEventListener("pointerdown", () => {
    if (selected.type !== "bus" || selected.index !== index)
      selectStrip("bus", index);
  });
  const header = el("div", "strip-header"),
    id = el("div", "strip-id");
  id.append(
    el("span", "", index === 0 ? "MAIN" : "AUX " + index),
    el("span", "", "STEREO"),
  );
  const name = el("input", "name");
  name.value = b.name;
  name.maxLength = 120;
  name.setAttribute("aria-label", `Nombre bus ${index}`);
  name.onchange = () => {
    b.name = name.value;
    publish();
    render();
  };
  header.append(id, name);
  card.append(header);
  card.append(el("div", "strip-caption", "INSERTS"), insertRack("bus", index));
  const route = el("select", "bus-route");
  route.setAttribute("aria-label", `Salida física ${b.name}`);
  route.append(new Option("—", ""));
  for (let p = 0; p <= limits.outputs - (b.format === "stereo" ? 2 : 1); p++)
    route.append(
      new Option(b.format === "mono" ? `OUT ${p + 1}` : `${p + 1}/${p + 2}`, p),
    );
  route.value = b.output === null ? "" : b.output;
  route.title =
    b.output === null
      ? "Sin salida física"
      : `Salida ${b.output + 1}${b.format === "stereo" ? "/" + (b.output + 2) : ""}`;
  route.onchange = () =>
    setProperty(
      "bus",
      index,
      "output",
      route.value === "" ? null : Number(route.value),
    );
  card.append(route);
  const label = el("div", "output-spacer");
  label.append(
    el("strong", "", index === 0 ? "Σ" : "A" + index),
    el("span", "", index === 0 ? "MASTER / OBS" : "MONITOR ESTÉREO"),
  );
  card.append(label);
  const buttons = el("div", "strip-buttons"),
    mute = el("button", b.mute ? "muted" : "", "MUTE");
  mute.title = "Mute de este bus";
  mute.onclick = () => {
    b.mute = !b.mute;
    mute.classList.toggle("muted", b.mute);
    publish();
  };
  buttons.append(mute);
  card.append(buttons);
  const fader = createFader("bus", index);
  card.append(fader.section, fader.level, fader.readout);
  const footer = el("div", "strip-footer");
  footer.append(
    el("span", "", "POST L/R"),
    el("span", "", b.output === null ? "—" : `${b.output + 1}/${b.output + 2}`),
  );
  card.append(footer);
  arrangeStrip(card, "bus", index);
  return card;
}
// Shared rows keep every channel and bus module aligned, including empty cells.
function arrangeStrip(card, type, index) {
  const header = card.querySelector(".strip-header"),
    name = header.querySelector(".name"),
    nameRow = el("div", "name-row"),
    source = card.querySelector(
      type === "channel" ? ".source-button" : ".bus-route",
    ),
    rack = card.querySelector(".insert-rack"),
    sends =
      card.querySelector(".send-grid") || el("div", "module-empty aux-empty"),
    pan = card.querySelector(".pan-row") || el("div", "module-empty pan-empty"),
    main = el(
      "div",
      "main-route",
      type === "channel" ? "→ MAIN" : index === 0 ? "→ OBS" : "AUX " + index,
    ),
    buttons = card.querySelector(".strip-buttons"),
    level = card.querySelector(".level"),
    fader = card.querySelector(".fader-section"),
    peak = card.querySelector(".peak-readout"),
    footer = card.querySelector(".strip-footer");
  nameRow.append(name);
  const format = el(
    "button",
    "format-button",
    targetOf(type, index).format === "mono" ? "M" : "ST",
  );
  format.setAttribute(
    "aria-label",
    `Formato ${type === "channel" ? "canal" : "bus"} ${index + 1}`,
  );
  format.title = "Cambiar mono/estéreo en vivo";
  format.disabled = type === "bus" && index === 0;
  format.onclick = (e) => {
    e.stopPropagation();
    format.disabled = true;
    setProperty(
      type,
      index,
      "format",
      targetOf(type, index).format === "mono" ? "stereo" : "mono",
    );
  };
  header.append(format);
  source.title ||=
    type === "channel" ? "Entrada o generador del canal" : "Salida física ASIO";
  card.querySelector(".output-spacer")?.remove();
  card.querySelector(".strip-caption")?.remove();
  buttons.querySelectorAll("button").forEach((button) => {
    const gen = button.id.startsWith("generator-toggle-");
    button.textContent = gen ? "G" : "M";
    button.setAttribute(
      "aria-label",
      gen
        ? `Generador canal ${index + 1}`
        : `Mute ${type === "channel" ? "canal" : "bus"} ${index + 1}`,
    );
  });
  card.replaceChildren(
    header,
    source,
    rack,
    eqButton(type, index),
    sends,
    pan,
    main,
    nameRow,
    buttons,
    level,
    fader,
    peak,
    footer,
  );
}
function renderListenMeter() {
  $("listen-meter").replaceChildren(meter(keyOf("bus", cfg.pc_monitor)));
}
function updateSendLabels() {
  document.querySelectorAll("[data-send]").forEach((e) => {
    const [i, a] = e.dataset.send.split(":").map(Number);
    const value = cfg.channels[i].sends_db[a];
    e.textContent = `A${a + 1} ${value <= -90 ? "−∞" : value.toFixed(0)}`;
    e.classList.toggle("has-send", value > -90);
    e.classList.toggle("selected-send", view === a);
  });
  if (selected.type === "channel")
    document.querySelectorAll("[data-inspector-send]").forEach((e) => {
      const value =
        cfg.channels[selected.index].sends_db[Number(e.dataset.inspectorSend)];
      e.value = value;
      e.title = db(value, " dB");
    });
}
function render() {
  meterWidgets = [];
  document.querySelector(".input-bank .bank-heading strong span").textContent =
    `01—${String(cfg.channels.length).padStart(2, "0")}`;
  if (
    selected.index >=
    (selected.type === "channel" ? cfg.channels.length : cfg.buses.length)
  )
    selected = { type: "channel", index: 0 };
  if (view >= cfg.buses.length - 1) view = -1;
  document
    .querySelector(".app")
    .style.setProperty("--outputs", Math.min(5, cfg.buses.length) * 70 + "px");
  $("inputs").value = cfg.inputs;
  $("rate").value = cfg.sample_rate;
  $("buffer").value = cfg.buffer;
  $("bridge").checked = cfg.obs_bridge;
  $("pc-monitor").replaceChildren(
    ...cfg.buses.map((b, i) => new Option(b.name, i)),
  );
  $("pc-monitor").value = cfg.pc_monitor;
  renderListenMeter();
  $("fader-flip").replaceChildren(
    new Option("Main Level", -1),
    ...cfg.buses
      .slice(1)
      .map((b, i) => new Option(`Aux ${i + 1} · ${b.name}`, i)),
  );
  $("fader-flip").value = view;
  $("main-layer").classList.toggle("active", view === -1);
  if (cfg.device !== null) $("device").value = cfg.device;
  $("meterbridge").replaceChildren(
    ...cfg.channels.map((_, i) => bridgeCell("channel", i)),
  );
  const outputOrder = [...cfg.buses.slice(1).map((_, i) => i + 1), 0];
  $("outputbridge").replaceChildren(
    ...outputOrder.map((i) => bridgeCell("bus", i)),
  );
  $("mixer").replaceChildren(...cfg.channels.map((_, i) => channelStrip(i)));
  $("buses").replaceChildren(...outputOrder.map((i) => busStrip(i)));
  $("layer-label").textContent =
    view < 0
      ? "MEZCLA PRINCIPAL"
      : "ENVÍOS A " + cfg.buses[view + 1].name.toUpperCase();
  for (const id of [
    "device",
    "inputs",
    "rate",
    "buffer",
    "scan",
    "demo",
    "start",
    "pc",
    "load",
  ])
    $(id).disabled = running;
  $("demo").classList.toggle("active", lastStatus?.mode === "DEMO");
  $("start").classList.toggle("active", lastStatus?.mode === "ASIO");
  $("pc").classList.toggle("active", lastStatus?.mode === "PC");
  updateSendLabels();
  selectStrip(selected.type, selected.index);
}
function inspectorSection(title) {
  const section = el("section", "inspector-section");
  section.append(el("h3", "", title));
  return section;
}
function renderInspector() {
  const { type, index } = selected,
    target = targetOf(type, index),
    inspector = $("inspector");
  inspector.replaceChildren();
  inspector.append(
    el(
      "span",
      "eyebrow",
      type === "channel"
        ? `CANAL ${String(index + 1).padStart(2, "0")}`
        : index === 0
          ? "BUS MASTER"
          : `AUXILIAR ${index}`,
    ),
    el("h2", "selected-name", target.name),
    el(
      "div",
      "source-caption",
      type === "channel"
        ? target.format === "mono"
          ? "Canal mono"
          : "Canal estéreo L/R"
        : "Bus · salida independiente",
    ),
  );
  const label = el("label", "", "NOMBRE"),
    name = el("input", "name-edit");
  name.type = "text";
  name.value = target.name;
  name.onchange = () => {
    target.name = name.value;
    publish();
    render();
  };
  label.append(name);
  inspector.append(label);
  const routing = inspectorSection("FORMATO Y ENTRADAS");
  const mode = el("select");
  mode.setAttribute("aria-label", "Formato seleccionado");
  mode.append(new Option("Mono", "mono"), new Option("Estéreo", "stereo"));
  mode.value = target.format;
  mode.disabled = type === "bus" && index === 0;
  mode.onchange = () => setProperty(type, index, "format", mode.value);
  routing.append(mode);
  if (type === "channel") {
    for (const [key, title] of [
      ["input", "Entrada L / mono"],
      ...(target.format === "stereo" ? [["input_right", "Entrada R"]] : []),
    ]) {
      const label = el("label", "", title),
        input = el("select");
      input.setAttribute("aria-label", title);
      input.append(new Option("Sin entrada física", ""));
      for (let i = 0; i < cfg.inputs; i++)
        input.append(new Option(`ASIO ${i + 1}`, i));
      input.value = target[key] === null ? "" : target[key];
      input.onchange = () =>
        setProperty(
          type,
          index,
          key,
          input.value === "" ? null : Number(input.value),
        );
      label.append(input);
      routing.append(label);
    }
  }
  inspector.append(routing);
  const eq = inspectorSection("EQ PARAMÉTRICO");
  eq.append(eqButton(type, index));
  inspector.append(eq);
  if (type === "channel") {
    const section = inspectorSection("GENERADOR DE SEÑAL"),
      source = el("select");
    source.id = "generator-source";
    source.setAttribute("aria-label", "Señal del generador");
    for (const [value, text] of Object.entries(signalNames))
      source.append(new Option(text, value));
    source.value = target.generator.signal;
    source.onchange = () => {
      target.generator.signal = source.value;
      if (source.value !== "off") lastSignal[index] = source.value;
      publish();
      refreshGenerator(index);
    };
    section.append(source);
    const levelLabel = el("label"),
      readout = el("span", "readout", db(target.generator.level_db, " dBFS"));
    levelLabel.append(
      el("span", "", "Nivel del generador"),
      readout,
      slider(target.generator.level_db, -90, -6, 0.5, (value) => {
        target.generator.level_db = value;
        readout.textContent = db(value, " dBFS");
        publish();
      }),
    );
    section.append(levelLabel);
    const frequency = el("label", "mini-row", "Frecuencia de seno"),
      freq = el("input");
    freq.type = "number";
    freq.min = 20;
    freq.max = 20000;
    freq.value = target.generator.frequency;
    freq.setAttribute("aria-label", "Frecuencia del tono");
    freq.onchange = () => {
      target.generator.frequency = Number(freq.value);
      publish();
      refreshGenerator(index);
    };
    frequency.append(freq);
    section.append(
      frequency,
      el(
        "div",
        "hint",
        "El generador sustituye la entrada antes de los inserts. DEMO no abre el hardware.",
      ),
    );
    inspector.append(section);
    const sends = inspectorSection("ENVÍOS A MONITORES");
    target.sends_db.forEach((value, a) => {
      const row = el("div", "send-row"),
        title = el("span", "", cfg.buses[a + 1].name),
        pre = el(
          "button",
          "mini-toggle" + (target.pre[a] ? " on" : ""),
          target.pre[a] ? "PRE" : "POST",
        );
      pre.onclick = () => {
        target.pre[a] = !target.pre[a];
        pre.textContent = target.pre[a] ? "PRE" : "POST";
        pre.classList.toggle("on", target.pre[a]);
        publish();
      };
      const input = slider(value, -90, 6, 0.5, (v) => {
        target.sends_db[a] = v;
        input.title = db(v, " dB");
        publish();
        updateSendLabels();
        if (view === a) {
          const f = $("mixer").children[index].querySelector(".fader input");
          f.value = faderPosition(v);
          $("fader-value-c" + index).value = db(v);
        }
      });
      input.dataset.inspectorSend = a;
      input.title = db(value, " dB");
      input.setAttribute("aria-label", `Envío a ${cfg.buses[a + 1].name}`);
      row.append(title, pre, input);
      sends.append(row);
    });
    inspector.append(sends);
  } else {
    const route = inspectorSection("DESTINO DE SALIDA");
    route.append(
      el(
        "div",
        "readout",
        target.output === null
          ? "Sin salida física"
          : `ASIO ${target.output + 1}${target.format === "stereo" ? " / " + (target.output + 2) : ""}`,
      ),
      el(
        "p",
        "hint",
        index === 0
          ? "Master hacia OBS mediante el puente local. El par físico es opcional."
          : "Mezcla independiente del nivel y mute del master.",
      ),
    );
    inspector.append(route);
  }
  const inserts = inspectorSection("CADENA DE INSERTS");
  if (!target.inserts.length)
    inserts.append(
      el(
        "p",
        "hint",
        "Sin inserts. Añade un efecto nativo o VST3 desde la tira con el audio detenido.",
      ),
    );
  target.inserts.forEach((fx, slot) => {
    const row = el("div", "mini-row"),
      button = el(
        "button",
        "",
        `${String(slot + 1).padStart(2, "0")} · ${fxName(fx)}`,
      );
    button.style.maxWidth = "100%";
    button.style.overflow = "hidden";
    button.style.textOverflow = "ellipsis";
    button.disabled = running;
    button.onclick = () => openFx(type, index, slot);
    row.append(button);
    inserts.append(row);
  });
  const latency = el("div", "hint");
  latency.id = "selected-latency";
  inserts.append(
    latency,
    el("div", "hint", "Sin compensación automática de latencia."),
  );
  inspector.append(inserts);
  updateLatency();
}
function updateLatency() {
  const index =
    selected.type === "channel"
      ? selected.index
      : cfg.channels.length + selected.index;
  if ($("selected-latency"))
    $("selected-latency").textContent =
      `Latencia declarada: ${lastStatus?.latencies?.[index] || 0} muestras`;
}
async function poll() {
  try {
    const r = await fetch("/api/status");
    if (!r.ok) throw Error("status");
    const s = await r.json();
    lastStatus = s;
    const was = running;
    running = s.mode !== "DETENIDO";
    $("panic").textContent = s.emergency ? "REACTIVAR AUDIO" : "SILENCIO TOTAL";
    $("panic").setAttribute("aria-pressed", s.emergency);
    $("listen-status").textContent = s.emergency
      ? "Silencio total"
      : s.failed
        ? "Error de audio"
        : s.mode === "PC"
          ? "Reproduciendo"
          : s.mode === "DEMO"
            ? "DEMO · sin reproducción PC"
            : s.mode === "ASIO"
              ? "Escucha por salidas ASIO"
              : "Motor detenido";
    $("status").textContent =
      s.mode +
      (s.emergency ? " · SILENCIO TOTAL" : "") +
      (s.failed ? " · ERROR DE AUDIO" : "");
    $("status").classList.toggle("live", running && !s.emergency);
    $("status").classList.toggle("failed", s.failed || s.emergency);
    if (was !== running) render();
    $("engine-summary").textContent =
      `${cfg.sample_rate / 1000} kHz · ${s.actual_buffer} muestras`;
    $("stats").textContent =
      `${selected.type === "channel" ? "CH " + String(selected.index + 1).padStart(2, "0") : "BUS " + (selected.index === 0 ? "MASTER" : selected.index)} · Post-fader L/R · XRuns ${s.xruns} · Clips ${s.clipped_blocks} · UDP descartados ${s.bridge_drops}`;
    const update = (type, peaks, rms) =>
      peaks?.forEach((peak, i) => {
        const state = peakState(keyOf(type, i));
        state.target = peak.map(signalDb);
        state.rmsTarget = (rms?.[i] || [0, 0]).map(signalDb);
      });
    update("channel", s.channel_meters, s.channel_rms);
    update("bus", s.meters_stereo, s.rms_stereo);
    const bridge = $("obs-status");
    bridge.textContent = !cfg.obs_bridge
      ? "OBS desactivado"
      : !running
        ? "OBS: motor detenido"
        : s.bridge?.receiver_active
          ? "● OBS recibe"
          : "○ OBS sin receptor";
    bridge.classList.toggle(
      "connected",
      running && !!s.bridge?.receiver_active,
    );
    bridge.title = `Enviados: ${s.bridge?.sent || 0} paquetes · Descartados: ${s.bridge_drops}\nLa confirmación verifica recepción en la fuente; revisa también su mute y la escena en OBS.`;
    updateLatency();
  } catch (e) {
    $("status").textContent = "SIN CONEXIÓN";
    $("listen-status").textContent = "Sin conexión";
    $("obs-status").textContent = "OBS: sin conexión";
    $("obs-status").classList.remove("connected");
    for (const s of meterStates.values()) {
      s.target = [-90, -90];
      s.rmsTarget = [-90, -90];
    }
  } finally {
    setTimeout(poll, 80);
  }
}
async function scan() {
  try {
    const list = await api("devices");
    $("device").replaceChildren(new Option("Seleccionar dispositivo…", ""));
    list.forEach((d) =>
      $("device").append(
        new Option(`${d.name} (${d.inputs} in / ${d.outputs} out)`, d.id),
      ),
    );
    if (cfg.device !== null) $("device").value = cfg.device;
    if (!list.length)
      error(
        "No hay dispositivos disponibles. DEMO funciona sin controlador ASIO.",
      );
  } catch (e) {
    error(e);
  }
}
function openFx(type, index, slot) {
  selectedFx = { type, index, slot };
  const target = targetOf(type, index),
    fx = target.inserts[slot];
  pendingFx = fx ? structuredClone(fx) : null;
  descriptors = [];
  $("path").value = fx?.path || "";
  $("params").replaceChildren();
  $("classes").replaceChildren();
  $("fxerror").textContent = "";
  $("fx-title").textContent = fx ? fxName(fx) : "Añadir efecto";
  $("fx-location").textContent = `${target.name} · INSERT ${slot + 1}`;
  $("removefx").disabled = !fx;
  $("fxup").disabled = !fx || slot === 0;
  $("fxdown").disabled = !fx || slot === target.inserts.length - 1;
  $("fx").showModal();
  fetch("/api/native")
    .then((r) => {
      if (!r.ok) throw Error("No se pudo leer la lista nativa");
      return r.json();
    })
    .then((list) => {
      $("builtin-fx").replaceChildren(
        new Option("Seleccionar efecto nativo…", ""),
      );
      for (const p of list) $("builtin-fx").append(new Option(p.name, p.id));
    })
    .catch((e) => ($("fxerror").textContent = e.message));
  if (!catalogueLoaded)
    scanVst3().catch((e) => ($("vst-scan-status").textContent = e.message));
  else filterVst3();
}
function showParams() {
  const d = descriptors[Number($("classes").value)];
  if (!d) return;
  const old = targetOf(selectedFx.type, selectedFx.index).inserts[
    selectedFx.slot
  ];
  pendingFx = {
    path: $("path").value,
    id: d.id,
    name: d.name,
    bypass: old?.bypass || false,
    params: old?.id === d.id ? { ...old.params } : {},
  };
  $("fx-title").textContent = d.name;
  $("params").replaceChildren();
  d.params.forEach((p) => {
    const label = el("label", "", p.name),
      input = el("input");
    input.type = "number";
    input.min = p.min;
    input.max = p.max;
    input.step = "any";
    input.value = pendingFx.params[p.id] ?? p.value;
    pendingFx.params[p.id] = Number(input.value);
    input.onchange = () => (pendingFx.params[p.id] = Number(input.value));
    label.append(input);
    $("params").append(label);
  });
}
async function scanVst3() {
  $("vst-scan-status").textContent = "Buscando bundles VST3…";
  const result = await api("vst3", { directory: $("vst-directory").value });
  vstCatalogue = result.plugins.sort((a, b) => a.name.localeCompare(b.name));
  vstRoots = result.roots;
  catalogueLoaded = true;
  filterVst3();
}
function filterVst3() {
  const query = $("vst-search").value.toLocaleLowerCase();
  const list = $("vst-installed");
  list.replaceChildren();
  for (const p of vstCatalogue.filter((p) =>
    (p.name + " " + p.path).toLocaleLowerCase().includes(query),
  )) {
    const option = new Option(p.name, p.path);
    option.title = p.path;
    list.append(option);
  }
  $("vst-scan-status").textContent =
    `${vstCatalogue.length} bundles detectados · selecciona uno para leer sus efectos.`;
  $("vst-scan-status").title = vstRoots.join("\n");
}
$("builtin-fx").onchange = () => {
  if (!$("builtin-fx").value) return;
  $("path").value = "native:" + $("builtin-fx").value;
  pendingFx = null;
  $("inspect").click();
};
$("vst-rescan").onclick = () =>
  scanVst3().catch((e) => ($("vst-scan-status").textContent = e.message));
$("vst-search").oninput = filterVst3;
$("vst-installed").onchange = () => {
  $("path").value = $("vst-installed").value;
  pendingFx = null;
  $("inspect").click();
};
$("inspect").onclick = async () => {
  try {
    $("fxerror").textContent = "Leyendo efecto…";
    descriptors = await api("inspect", { path: $("path").value });
    $("classes").replaceChildren();
    descriptors.forEach((d, i) => $("classes").append(new Option(d.name, i)));
    if (!descriptors.length)
      throw Error("Este bundle no contiene efectos de audio.");
    showParams();
    $("fxerror").textContent = "";
  } catch (e) {
    $("fxerror").textContent = e.message;
  }
};
$("path").oninput = () => {
  pendingFx = null;
};
$("classes").onchange = showParams;
$("closefx").onclick = () => $("fx").close();
$("applyfx").onclick = async () => {
  if (!pendingFx) {
    $("fxerror").textContent =
      "Selecciona un efecto nativo o lee un VST3 primero.";
    return;
  }
  if (
    [...$("params").querySelectorAll("input")].some((e) => !e.reportValidity())
  )
    return;
  const target = targetOf(selectedFx.type, selectedFx.index),
    before = structuredClone(target.inserts);
  target.inserts[selectedFx.slot] = pendingFx;
  try {
    await publish();
    $("fx").close();
    render();
  } catch (e) {
    target.inserts = before;
    $("fxerror").textContent = e.message;
  }
};
$("removefx").onclick = async () => {
  const t = targetOf(selectedFx.type, selectedFx.index);
  t.inserts.splice(selectedFx.slot, 1);
  await publish();
  $("fx").close();
  render();
};
for (const [id, delta] of [
  ["fxup", -1],
  ["fxdown", 1],
])
  $(id).onclick = async () => {
    const t = targetOf(selectedFx.type, selectedFx.index),
      slot = selectedFx.slot;
    [t.inserts[slot], t.inserts[slot + delta]] = [
      t.inserts[slot + delta],
      t.inserts[slot],
    ];
    await publish();
    $("fx").close();
    render();
  };
$("device").onchange = () => {
  cfg.device = $("device").value === "" ? null : Number($("device").value);
  publish();
};
for (const [id, key] of [
  ["inputs", "inputs"],
  ["rate", "sample_rate"],
  ["buffer", "buffer"],
])
  $(id).onchange = () => {
    cfg[key] = Number($(id).value);
    if (key === "inputs")
      cfg.channels.forEach((c) => {
        if (c.input >= cfg.inputs) c.input = null;
        if (c.input_right >= cfg.inputs) c.input_right = null;
      });
    publish();
  };
$("bridge").onchange = () => {
  cfg.obs_bridge = $("bridge").checked;
  publish();
};
$("scan").onclick = scan;
for (const [id, mode] of [
  ["demo", "demo"],
  ["start", "asio"],
  ["pc", "system"],
])
  $(id).onclick = async () => {
    try {
      $("error").textContent = "";
      await publish();
      await api("start", { mode });
    } catch (e) {
      error(e);
    }
  };
$("pc-monitor").onchange = () => {
  cfg.pc_monitor = Number($("pc-monitor").value);
  publish();
  render();
};
$("stop").onclick = () => api("stop").catch(error);
$("panic").onclick = async () => {
  $("panic").disabled = true;
  try {
    lastStatus = await api(lastStatus?.emergency ? "resume" : "panic");
  } catch (e) {
    error(e);
  } finally {
    $("panic").disabled = false;
  }
};
$("reset-peaks").onclick = resetPeaks;
$("main-layer").onclick = () => {
  view = -1;
  render();
};
$("fader-flip").onchange = () => {
  view = Number($("fader-flip").value);
  render();
};
let deskPreferences;
try {
  deskPreferences = JSON.parse(
    localStorage.getItem("monitor-mix-desk") || "{}",
  );
  if (
    !deskPreferences ||
    typeof deskPreferences !== "object" ||
    Array.isArray(deskPreferences)
  )
    deskPreferences = {};
} catch {
  deskPreferences = {};
}
function applyDeskPreferences() {
  const app = document.querySelector(".app");
  app.classList.toggle("hide-meters", deskPreferences.meters === false);
  app.classList.toggle("hide-inspector", deskPreferences.inspector === false);
  app.classList.toggle("small-console", deskPreferences.layout === "small");
  $("toggle-meters").setAttribute(
    "aria-pressed",
    deskPreferences.meters !== false,
  );
  $("toggle-inspector").setAttribute(
    "aria-pressed",
    deskPreferences.inspector !== false,
  );
  $("mixer-layout").value =
    deskPreferences.layout === "small" ? "small" : "full";
  try {
    localStorage.setItem("monitor-mix-desk", JSON.stringify(deskPreferences));
  } catch {
    /* Storage can be unavailable in a private browser. */
  }
}
$("toggle-meters").onclick = () => {
  deskPreferences.meters = deskPreferences.meters === false;
  applyDeskPreferences();
};
$("toggle-inspector").onclick = () => {
  deskPreferences.inspector = deskPreferences.inspector === false;
  applyDeskPreferences();
};
$("mixer-layout").onchange = () => {
  deskPreferences.layout = $("mixer-layout").value;
  applyDeskPreferences();
};
applyDeskPreferences();
$("settings").onclick = () => $("audio-settings").showModal();
$("help").onclick = () => $("help-dialog").showModal();
for (const button of document.querySelectorAll("[data-close]"))
  button.onclick = () => $(button.dataset.close).close();
$("fullscreen").onclick = () => {
  if (document.fullscreenElement) document.exitFullscreen();
  else document.documentElement.requestFullscreen().catch(error);
};
$("save").onclick = () => {
  const a = el("a");
  a.href = URL.createObjectURL(
    new Blob([JSON.stringify(cfg, null, 2)], { type: "application/json" }),
  );
  a.download = "monitor-mix.json";
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
};
$("load").onclick = () => $("scene").click();
$("scene").onchange = async () => {
  try {
    const file = $("scene").files[0];
    if (!file) return;
    const candidate = normalise(JSON.parse(await file.text()));
    await queue;
    await api("config", candidate);
    cfg = candidate;
    $("scene-name").textContent = file.name.replace(/\.json$/i, "");
    render();
  } catch (e) {
    error(e);
  }
  $("scene").value = "";
};
async function setProperty(type, index, key, value) {
  const target = targetOf(type, index),
    before = target[key];
  target[key] = value;
  try {
    await publish();
  } catch (e) {
    target[key] = before;
    error(e);
  }
  render();
}
function eqResponse(eq, frequency) {
  if (eq.bypass) return 0;
  const rate = cfg.sample_rate,
    w = (2 * Math.PI * frequency) / rate;
  return eq.bands.reduce((gain, b) => {
    const a = 10 ** (b.gain_db / 40),
      omega = (2 * Math.PI * Math.min(b.frequency, rate * 0.45)) / rate,
      co = Math.cos(omega),
      si = Math.sin(omega),
      alpha = si / (2 * b.q);
    let num, den;
    switch (b.kind || 0) {
      case 1:
      case 2: {
        const slope = Math.max(0.1, Math.min(1, b.q / 2)),
          t =
            ((2 * Math.sqrt(a) * si) / 2) *
            Math.sqrt((a + 1 / a) * (1 / slope - 1) + 2);
        if (b.kind === 1) {
          num = [
            a * (a + 1 - (a - 1) * co + t),
            2 * a * (a - 1 - (a + 1) * co),
            a * (a + 1 - (a - 1) * co - t),
          ];
          den = [
            a + 1 + (a - 1) * co + t,
            -2 * (a - 1 + (a + 1) * co),
            a + 1 + (a - 1) * co - t,
          ];
        } else {
          num = [
            a * (a + 1 + (a - 1) * co + t),
            -2 * a * (a - 1 + (a + 1) * co),
            a * (a + 1 + (a - 1) * co - t),
          ];
          den = [
            a + 1 - (a - 1) * co + t,
            2 * (a - 1 - (a + 1) * co),
            a + 1 - (a - 1) * co - t,
          ];
        }
        break;
      }
      case 3:
        num = [(1 + co) / 2, -(1 + co), (1 + co) / 2];
        den = [1 + alpha, -2 * co, 1 - alpha];
        break;
      case 4:
        num = [(1 - co) / 2, 1 - co, (1 - co) / 2];
        den = [1 + alpha, -2 * co, 1 - alpha];
        break;
      case 5:
        num = [alpha, 0, -alpha];
        den = [1 + alpha, -2 * co, 1 - alpha];
        break;
      default:
        num = [1 + alpha * a, -2 * co, 1 - alpha * a];
        den = [1 + alpha / a, -2 * co, 1 - alpha / a];
    }
    const c = [...num.map((v) => v / den[0]), den[1] / den[0], den[2] / den[0]];
    const nr = c[0] + c[1] * Math.cos(w) + c[2] * Math.cos(2 * w),
      ni = -c[1] * Math.sin(w) - c[2] * Math.sin(2 * w),
      dr = 1 + c[3] * Math.cos(w) + c[4] * Math.cos(2 * w),
      di = -c[3] * Math.sin(w) - c[4] * Math.sin(2 * w);
    return (
      gain +
      10 *
        Math.log10(Math.max(1e-20, (nr * nr + ni * ni) / (dr * dr + di * di)))
    );
  }, 0);
}
function eqSvg(eq, large = false) {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  const width = large ? 500 : 60,
    height = large ? 120 : 22;
  svg.setAttribute("viewBox", `0 0 ${width} ${height}`);
  svg.setAttribute("aria-hidden", "true");
  const grid = document.createElementNS(svg.namespaceURI, "path");
  grid.setAttribute("d", `M0 ${height / 2} H${width}`);
  grid.setAttribute("class", "eq-grid");
  svg.append(grid);
  const curve = document.createElementNS(svg.namespaceURI, "path");
  let path = "";
  for (let i = 0; i <= 128; i++) {
    const frequency = 20 * 1000 ** (i / 128),
      value = eqResponse(eq, frequency),
      y = Math.max(1, Math.min(height - 1, height / 2 - (value * height) / 40));
    path += `${i ? "L" : "M"}${(width * i) / 128},${y} `;
  }
  curve.setAttribute("d", path);
  curve.setAttribute("class", "eq-curve");
  svg.append(curve);
  return svg;
}
function eqButton(type, index) {
  const eq = targetOf(type, index).eq,
    button = el("button", "eq-module" + (eq.bypass ? " eq-off" : ""));
  button.setAttribute(
    "aria-label",
    `EQ ${type === "channel" ? "canal" : "bus"} ${index + 1}`,
  );
  button.title = "EQ paramétrico de ocho bandas · clic para editar";
  button.append(el("span", "", "EQ"), eqSvg(eq));
  button.onclick = () => openEq(type, index);
  return button;
}
function openEq(type, index) {
  eqTarget = { type, index };
  const target = targetOf(type, index);
  $("eq-title").textContent = `FreeEQ8 · ${target.name}`;
  $("eq-bypass").checked = target.eq.bypass;
  const header = el("div", "eq-band-row eq-band-header");
  for (const name of [
    "Banda",
    "Tipo",
    "Frecuencia (Hz)",
    "Ganancia (dB)",
    "Q / pendiente",
  ])
    header.append(el("span", "", name));
  $("eq-bands").replaceChildren(header);
  target.eq.bands.forEach((band, i) => {
    const row = el("div", "eq-band-row");
    row.append(el("span", "", String(i + 1)));
    const kind = el("select");
    [
      "Campana",
      "Low shelf",
      "High shelf",
      "Pasa altos",
      "Pasa bajos",
      "Pasa banda",
    ].forEach((name, k) => kind.append(new Option(name, k)));
    kind.value = band.kind || 0;
    kind.setAttribute("aria-label", `EQ banda ${i + 1} tipo`);
    kind.onchange = async () => {
      const old = band.kind;
      band.kind = Number(kind.value);
      try {
        await publish();
      } catch (e) {
        band.kind = old;
        kind.value = old;
        error(e);
      }
      refreshEq();
    };
    row.append(kind);
    for (const [key, min, max, step] of [
      ["frequency", 20, 20000, 1],
      ["gain_db", -18, 18, 0.1],
      ["q", 0.1, 18, 0.01],
    ]) {
      const input = el("input");
      input.type = "number";
      input.min = min;
      input.max = max;
      input.step = step;
      input.value = band[key];
      input.setAttribute("aria-label", `EQ banda ${i + 1} ${key}`);
      input.onchange = async () => {
        if (!input.reportValidity()) return;
        const previous = band[key];
        band[key] = Number(input.value);
        try {
          await publish();
        } catch (e) {
          band[key] = previous;
          input.value = previous;
          error(e);
        }
        refreshEq();
      };
      row.append(input);
    }
    $("eq-bands").append(row);
  });
  refreshEq();
  $("eq-dialog").showModal();
}
function refreshEq() {
  if (!eqTarget) return;
  const target = targetOf(eqTarget.type, eqTarget.index);
  $("eq-graph").replaceChildren(eqSvg(target.eq, true));
  render();
}
$("eq-bypass").onchange = async () => {
  if (!eqTarget) return;
  const target = targetOf(eqTarget.type, eqTarget.index),
    previous = target.eq.bypass;
  target.eq.bypass = $("eq-bypass").checked;
  try {
    await publish();
  } catch (e) {
    target.eq.bypass = previous;
    $("eq-bypass").checked = previous;
    error(e);
  }
  refreshEq();
};
$("counts").onclick = () => {
  $("channel-count").value = cfg.channels.length;
  $("aux-count").value = cfg.buses.length - 1;
  $("counts-error").textContent = "";
  $("counts-dialog").showModal();
};
$("apply-counts").onclick = async () => {
  const button = $("apply-counts"),
    channels = Number($("channel-count").value),
    auxes = Number($("aux-count").value);
  if (
    !$("channel-count").reportValidity() ||
    !$("aux-count").reportValidity() ||
    !Number.isInteger(channels) ||
    !Number.isInteger(auxes)
  )
    return;
  const candidate = structuredClone(cfg);
  candidate.channels = candidate.channels.slice(0, channels);
  candidate.buses = candidate.buses.slice(0, auxes + 1);
  while (candidate.channels.length < channels) {
    const i = candidate.channels.length;
    candidate.channels.push({
      name: `Entrada ${i + 1}`,
      format: $("new-channel-format").value,
      input: i < cfg.inputs ? i : null,
      input_right: i + 1 < cfg.inputs ? i + 1 : null,
      gain_db: 0,
      pan: 0,
      mute: false,
      sends_db: [],
      pre: [],
      inserts: [],
      fx: null,
      generator: { signal: "off", level_db: -30, frequency: 1000 },
      eq: defaultEq(),
    });
  }
  while (candidate.buses.length < auxes + 1)
    candidate.buses.push({
      name: `Monitor ${candidate.buses.length}`,
      format: $("new-aux-format").value,
      gain_db: -12,
      mute: false,
      output: null,
      inserts: [],
      fx: null,
      eq: defaultEq(),
    });
  for (const ch of candidate.channels) {
    ch.sends_db = ch.sends_db.slice(0, auxes);
    ch.pre = ch.pre.slice(0, auxes);
    while (ch.sends_db.length < auxes) ch.sends_db.push(-90);
    while (ch.pre.length < auxes) ch.pre.push(true);
  }
  if (candidate.pc_monitor >= candidate.buses.length) candidate.pc_monitor = 0;
  button.disabled = true;
  button.textContent = "Aplicando…";
  try {
    await queue;
    await api("config", candidate);
    cfg = candidate;
    meterStates.clear();
    render();
    $("counts-dialog").close();
  } catch (e) {
    $("counts-error").textContent = e.message;
  } finally {
    button.disabled = false;
    button.textContent = "Aplicar estructura";
  }
};
for (const [bank, bridge] of [
  ["mixer", "meterbridge"],
  ["buses", "outputbridge"],
]) {
  $(bank).addEventListener(
    "scroll",
    () => {
      $(bridge).scrollLeft = $(bank).scrollLeft;
    },
    { passive: true },
  );
}

Promise.all([
  fetch("/api/config").then((r) => r.json()),
  fetch("/api/limits").then((r) => r.json()),
])
  .then(([c, l]) => {
    limits = l;
    $("channel-count").max = l.channels;
    $("aux-count").max = l.auxes;
    $("inputs").max = l.inputs;
    cfg = normalise(c);
    render();
    poll();
    requestAnimationFrame(animateMeters);
  })
  .catch(error);
