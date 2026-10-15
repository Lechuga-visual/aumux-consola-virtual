"use strict";
const $ = (id) => document.getElementById(id);
const token = document.querySelector('meta[name="monitor-token"]').content;
let remoteNeedsRender = false;
let serverConfig,
  syncedRemoteRevision = -1,
  configRequests = 0,
  replaceScene = false;
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
let eqTarget,
  eqBandIndex = 0,
  eqKnobs = [],
  eqWriteTimer,
  eqWriteVersion = 0;
let structureBusy = false;
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
    ch.send_mute = Array.from(
      { length: c.buses.length - 1 },
      (_, a) => ch.send_mute?.[a] ?? false,
    );
    ch.compressor ||= defaultCompressor();
    ch.pin ||= "none";
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
    b.compressor ||= defaultCompressor();
    b.pin ||= b === c.buses[0] ? "right" : "none";
    if (legacy) b.format = "stereo";
  });
  if (legacy) c.version = 2;
  return c;
}
function error(e) {
  $("error").textContent = e.message || e;
  $("error").title = $("error").textContent;
}
async function api(route, body = {}, options = {}) {
  const configCall = route === "config";
  if (configCall) configRequests++;
  const payload = configCall
    ? {
        config: body,
        base: options.base || serverConfig || cfg,
        replace_scene: replaceScene,
      }
    : body;
  try {
    const r = await fetch("/api/" + route, {
      method: "POST",
      headers: { "Content-Type": "application/json", "X-Monitor-Token": token },
      body: JSON.stringify(payload),
    });
    if (r.status === 403 && location.pathname.startsWith("/console")) {
      location.reload();
      throw Error("Sesión de consola cerrada");
    }
    const v = await r.json();
    if (!r.ok) throw Error(v.error || r.statusText);
    if (configCall && v.config) {
      remoteNeedsRender = reconcileConfig(v.config, body) || remoteNeedsRender;
      serverConfig = structuredClone(v.config);
      syncedRemoteRevision = v.remote_revision;
    }
    return v;
  } finally {
    if (configCall) configRequests--;
  }
}
function reconcileConfig(incoming, base) {
  if (!cfg || !incoming || !base) return false;
  let changed = false;
  function walk(target, old, next) {
    for (const key of Object.keys(next)) {
      const a = target[key],
        b = old?.[key],
        n = next[key];
      if (
        n &&
        a &&
        b &&
        typeof n === "object" &&
        typeof a === "object" &&
        typeof b === "object" &&
        Array.isArray(a) === Array.isArray(n) &&
        (!Array.isArray(n) || (a.length === n.length && b.length === n.length))
      )
        walk(a, b, n);
      else if (
        JSON.stringify(a) === JSON.stringify(b) &&
        JSON.stringify(a) !== JSON.stringify(n)
      ) {
        target[key] = structuredClone(n);
        changed = true;
      }
    }
  }
  walk(cfg, base, incoming);
  return changed;
}
function reconcileSends(incoming, base) {
  if (
    !cfg ||
    cfg.channels.length !== incoming.channels.length ||
    base.channels.length !== cfg.channels.length
  )
    return false;
  let changed = false;
  cfg.channels.forEach((ch, i) => {
    for (const field of ["sends_db", "pre", "send_mute"]) {
      incoming.channels[i][field]?.forEach((v, a) => {
        if (
          ch[field]?.[a] === base.channels[i][field]?.[a] &&
          ch[field][a] !== v
        ) {
          ch[field][a] = v;
          changed = true;
        }
      });
    }
  });
  return changed;
}
let pendingPublish,
  publishRunning = false;
function startPublishing() {
  if (publishRunning) return;
  publishRunning = true;
  const worker = queue.then(async () => {
    while (pendingPublish) {
      const batch = pendingPublish;
      pendingPublish = undefined;
      try {
        const result = await api("config", batch.snapshot, {
          base: batch.base,
        });
        for (const w of batch.waiters) w.resolve(result);
      } catch (e) {
        error(e);
        for (const w of batch.waiters) w.reject(e);
      }
    }
  });
  queue = worker.catch(error);
  worker
    .finally(() => {
      publishRunning = false;
      if (pendingPublish) startPublishing();
    })
    .catch(error);
}
function publish() {
  const snapshot = structuredClone(cfg),
    base = structuredClone(serverConfig || cfg);
  const task = new Promise((resolve, reject) => {
    if (pendingPublish) {
      pendingPublish.snapshot = snapshot;
      pendingPublish.base = base;
      pendingPublish.waiters.push({ resolve, reject });
    } else pendingPublish = { snapshot, base, waiters: [{ resolve, reject }] };
  });
  startPublishing();
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
    button.disabled = (!fx && running) || (!fx && slot > target.inserts.length);
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
      type === "channel" ? "button" : "div",
      "main-route",
      type === "channel" ? "Envíos ↗" : index === 0 ? "→ OBS" : "AUX " + index,
    ),
    buttons = card.querySelector(".strip-buttons"),
    level = card.querySelector(".level"),
    fader = card.querySelector(".fader-section"),
    peak = card.querySelector(".peak-readout"),
    footer = card.querySelector(".strip-footer");
  if (type === "channel") {
    main.setAttribute("aria-label", `Envíos canal ${index + 1}`);
    main.title = "Abrir envíos y nivel principal";
    main.onclick = () => openRouting(index);
  }
  card.tabIndex = 0;
  const actions = el("button", "touch-strip-menu", "⋮");
  actions.setAttribute("aria-label", `Acciones ${targetOf(type, index).name}`);
  actions.onclick = (e) => {
    e.stopPropagation();
    const r = card.getBoundingClientRect();
    openContext(card, r.left, r.top + 24);
  };
  header.append(actions);
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
    compressorButton(type, index),
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
    e.textContent = `A${a + 1} ${cfg.channels[i].send_mute[a] ? "M" : value <= -90 ? "−∞" : value.toFixed(0)}`;
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
  $("channel-total").textContent = `${cfg.channels.length} canales`;
  $("aux-total").textContent = `${cfg.buses.length - 1} aux`;
  $("add-channel").disabled =
    structureBusy || cfg.channels.length >= limits.channels;
  $("add-aux").disabled = structureBusy || cfg.buses.length - 1 >= limits.auxes;
  $("counts").disabled = structureBusy;
  if (
    selected.index >=
    (selected.type === "channel" ? cfg.channels.length : cfg.buses.length)
  )
    selected = { type: "channel", index: 0 };
  if (view >= cfg.buses.length - 1) view = -1;
  document
    .querySelector(".app")
    .style.setProperty(
      "--outputs",
      Math.min(5, Math.max(3, cfg.buses.length)) * 70 + "px",
    );
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
  layoutStrips();
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
      const sendMute = el(
        "button",
        "mini-toggle" + (target.send_mute[a] ? " on" : ""),
        "M",
      );
      sendMute.setAttribute(
        "aria-label",
        `Silenciar envío a ${cfg.buses[a + 1].name}`,
      );
      sendMute.onclick = () => {
        target.send_mute[a] = !target.send_mute[a];
        sendMute.classList.toggle("on", target.send_mute[a]);
        publish();
      };
      row.append(title, pre, sendMute, input);
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
    const statusStarted = performance.now();
    const r = await fetch("/api/status");
    if (r.status === 403 && location.pathname.startsWith("/console")) {
      location.reload();
      return;
    }
    if (!r.ok) throw Error("status");
    const s = await r.json();
    s.control_rtt_ms = performance.now() - statusStarted;
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
    updateWorkspaceStatus(s);
    updateLatencyTools(s);
    syncRemoteMix(s);
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
    if (key === "inputs") {
      for (const node of [...cfg.channels, ...cfg.buses]) {
        if (
          node.compressor.source.type === "input" &&
          node.compressor.source.index >= cfg.inputs
        )
          node.compressor.source = { type: "internal" };
      }
      cfg.channels.forEach((c) => {
        if (c.input >= cfg.inputs) c.input = null;
        if (c.input_right >= cfg.inputs) c.input_right = null;
      });
    }
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
$("save").onclick = async () => {
  try {
    await queue;
    const r = await fetch("/api/sync-state");
    if (!r.ok) throw Error("No se pudo sincronizar la mezcla para guardar");
    remoteNeedsRender =
      reconcileConfig((await r.json()).config, serverConfig) ||
      remoteNeedsRender;
    const a = el("a");
    a.href = URL.createObjectURL(
      new Blob([JSON.stringify(cfg, null, 2)], { type: "application/json" }),
    );
    a.download = "monitor-mix.json";
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  } catch (e) {
    error(e);
  }
};
$("load").onclick = () => $("scene").click();
$("scene").onchange = async () => {
  try {
    const file = $("scene").files[0];
    if (!file) return;
    const candidate = normalise(JSON.parse(await file.text()));
    await queue;
    replaceScene = true;
    try {
      await api("config", candidate);
    } finally {
      replaceScene = false;
    }
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
// Shared rotary control: pointer capture keeps a drag stable across UI updates.
function rotary({
  label,
  value,
  min,
  max,
  defaultValue = value,
  step = 0.01,
  log = false,
  unit = "",
  scale = 1,
  colour = "#53afd0",
  onChange,
  onCommit,
  inputLabel = label,
}) {
  const root = el("div", "rotary-control"),
    caption = el("span", "rotary-label", label),
    dial = el("div", "rotary-dial"),
    number = el("input", "rotary-value"),
    suffix = el("span", "rotary-unit", unit);
  root.style.setProperty("--knob-colour", colour);
  dial.tabIndex = 0;
  dial.setAttribute("role", "slider");
  dial.setAttribute("aria-label", `Perilla ${label}`);
  dial.setAttribute("aria-valuemin", min);
  dial.setAttribute("aria-valuemax", max);
  dial.title =
    "Arrastra arriba/abajo · Mayús: fino · doble clic: restablecer · flechas: ajustar";
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 64 64");
  svg.setAttribute("aria-hidden", "true");
  svg.innerHTML =
    '<path class="knob-track" d="M15.03 48.97 A24 24 0 1 1 48.97 48.97"/><path class="knob-arc"/><circle class="knob-face" cx="32" cy="32" r="18"/><path class="knob-pointer"/>';
  dial.append(svg);
  number.type = "number";
  number.min = Number((min * scale).toPrecision(7));
  number.max = Number((max * scale).toPrecision(7));
  number.step = "any";
  number.required = true;
  number.setAttribute("aria-label", inputLabel);
  const readout = el("div", "rotary-readout");
  readout.append(number, suffix);
  root.append(caption, dial, readout);
  let current = value,
    disabled = false,
    drag;
  const normalise = (v) =>
    log ? Math.log(v / min) / Math.log(max / min) : (v - min) / (max - min);
  const denormalise = (n) =>
    log ? min * (max / min) ** n : min + n * (max - min);
  const clamp = (v) => Math.max(min, Math.min(max, v));
  function set(v) {
    current = clamp(v);
    const n = Math.max(0, Math.min(1, normalise(current))),
      angle = ((135 + 270 * n) * Math.PI) / 180;
    const x = (r) => 32 + r * Math.cos(angle),
      y = (r) => 32 + r * Math.sin(angle);
    svg
      .querySelector(".knob-arc")
      .setAttribute(
        "d",
        `M15.03 48.97 A24 24 0 ${n > 2 / 3 ? 1 : 0} 1 ${x(24)} ${y(24)}`,
      );
    svg.querySelector(".knob-arc").style.opacity = n > 0.00001 ? 1 : 0;
    svg
      .querySelector(".knob-pointer")
      .setAttribute("d", `M${x(10)} ${y(10)} L${x(17)} ${y(17)}`);
    number.value = Number((current * scale).toPrecision(7));
    dial.setAttribute("aria-valuenow", current);
    dial.setAttribute("aria-valuetext", `${number.value} ${unit}`);
  }
  function change(v) {
    if (disabled || !Number.isFinite(v)) return;
    const rounded = clamp(Number((Math.round(v / step) * step).toFixed(6)));
    if (rounded === current) return;
    set(rounded);
    onChange(rounded);
  }
  dial.onpointerdown = (e) => {
    if (disabled || e.button !== 0) return;
    e.preventDefault();
    dial.focus();
    dial.setPointerCapture(e.pointerId);
    drag = { y: e.clientY, n: normalise(current) };
    root.classList.add("dragging");
  };
  dial.onpointermove = (e) => {
    if (!drag) return;
    const delta = (drag.y - e.clientY) / (e.shiftKey ? 1800 : 180),
      nextN = Math.max(0, Math.min(1, drag.n + delta));
    change(denormalise(nextN));
    drag = { y: e.clientY, n: nextN };
  };
  function release(e) {
    if (!drag) return;
    drag = null;
    root.classList.remove("dragging");
    if (dial.hasPointerCapture(e.pointerId))
      dial.releasePointerCapture(e.pointerId);
    onCommit?.();
  }
  dial.onpointerup = release;
  dial.onpointercancel = release;
  dial.onlostpointercapture = () => {
    drag = null;
    root.classList.remove("dragging");
  };
  dial.ondblclick = () => {
    change(defaultValue);
    onCommit?.();
  };
  dial.onkeydown = (e) => {
    let delta = 0;
    if (["ArrowUp", "ArrowRight"].includes(e.key)) delta = 1;
    if (["ArrowDown", "ArrowLeft"].includes(e.key)) delta = -1;
    if (delta) {
      e.preventDefault();
      change(
        log
          ? current * (max / min) ** (delta * (e.shiftKey ? 0.001 : 0.01))
          : current + delta * step * (e.shiftKey ? 1 : 10),
      );
      onCommit?.();
    }
    if (e.key === "Home") {
      e.preventDefault();
      change(defaultValue);
      onCommit?.();
    }
  };
  dial.addEventListener(
    "wheel",
    (e) => {
      if (document.activeElement !== dial || disabled) return;
      e.preventDefault();
      change(
        denormalise(
          Math.max(
            0,
            Math.min(
              1,
              normalise(current) +
                (e.deltaY < 0 ? 1 : -1) * (e.shiftKey ? 0.001 : 0.01),
            ),
          ),
        ),
      );
      onCommit?.();
    },
    { passive: false },
  );
  number.onchange = () => {
    if (!number.reportValidity()) return;
    change(Number(number.value) / scale);
    set(current);
    onCommit?.();
  };
  number.onkeydown = (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      number.dispatchEvent(new Event("change"));
    }
  };
  set(value);
  return {
    root,
    set,
    setDisabled(v) {
      disabled = v;
      number.disabled = v;
      dial.setAttribute("aria-disabled", v);
      dial.tabIndex = v ? -1 : 0;
      root.classList.toggle("disabled", v);
    },
  };
}
function renderFreevox(descriptor) {
  for (const p of descriptor.params) pendingFx.params[p.id] ??= p.value;
  const panel = el("section", "freevox-panel");
  panel.setAttribute("aria-label", "Editor FreeVox8");
  const heading = el("div", "plugin-panel-heading");
  heading.append(
    el("strong", "", "FreeVox8"),
    el("span", "", "VOCODER · FILTERBANK · 48 kHz"),
  );
  panel.append(heading);
  const modes = el("div", "vox-modes");
  modes.setAttribute("aria-label", "Modo FreeVox8");
  ["Vocoder", "Mask", "Ghost", "Morph"].forEach((name, i) => {
    const button = el("button", "", name);
    button.type = "button";
    button.dataset.mode = i;
    button.setAttribute(
      "aria-pressed",
      Math.round(pendingFx.params.Mode) === i,
    );
    button.onclick = () => {
      pendingFx.params.Mode = i;
      modes
        .querySelectorAll("button")
        .forEach((b, k) => b.setAttribute("aria-pressed", k === i));
      fxParameterChanged();
    };
    modes.append(button);
  });
  panel.append(modes);
  const names = {
    "Carrier blend": "Portadora",
    "Carrier note": "Nota portadora",
    "Formant shift": "Formante",
    "Mask depth": "Máscara",
    Clarity: "Claridad",
    Ghost: "Ghost",
    Air: "Aire",
    Freeze: "Freeze",
    "Transient protect": "Transitorios",
    Mix: "Mezcla",
    "Stereo width": "Anchura",
    Quality: "Calidad",
    "Output gain": "Salida",
  };
  const colours = ["#b79aeb", "#49bec8", "#daaa67"];
  for (const [g, [title, ids]] of [
    ["PORTADORA", ["Carrier blend", "Carrier note", "Formant shift"]],
    [
      "TEXTURA",
      ["Mask depth", "Clarity", "Ghost", "Air", "Freeze", "Transient protect"],
    ],
    ["SALIDA", ["Mix", "Stereo width", "Quality", "Output gain"]],
  ].entries()) {
    const group = el("div", "vox-group");
    group.append(el("h3", "", title));
    const knobs = el("div", "vox-knob-bank");
    ids.forEach((id) => {
      const p = descriptor.params.find((p) => p.id === id);
      if (!p) return;
      const percent = [
        "Carrier blend",
        "Mask depth",
        "Clarity",
        "Ghost",
        "Air",
        "Freeze",
        "Transient protect",
        "Mix",
        "Stereo width",
        "Quality",
      ].includes(id);
      const control = rotary({
        label: names[id],
        inputLabel: `FreeVox8 ${id}`,
        value: pendingFx.params[id],
        defaultValue: p.value,
        min: p.min,
        max: p.max,
        step: id === "Carrier note" ? 1 : percent ? 0.001 : 0.1,
        unit: percent
          ? "%"
          : id === "Formant shift"
            ? "st"
            : id === "Output gain"
              ? "dB"
              : "MIDI",
        scale: percent ? 100 : 1,
        colour: colours[g],
        onChange: (v) => {
          pendingFx.params[id] = v;
          fxParameterChanged();
        },
      });
      knobs.append(control.root);
    });
    group.append(knobs);
    panel.append(group);
  }
  panel.append(
    el(
      "p",
      "hint",
      "Arrastra las perillas · Mayús: fino · doble clic: valor inicial. Los efectos existentes se ajustan en vivo. Para añadir uno nuevo pulsa Aplicar insert. Los parámetros se guardan en la escena.",
    ),
  );
  $("params").append(panel);
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
  button.dataset.eq = keyOf(type, index);
  button.append(el("span", "", "EQ"), eqSvg(eq));
  button.onclick = () => openEq(type, index);
  return button;
}
const eqColours = [
  "#e45e74",
  "#d99c68",
  "#d5c45b",
  "#8fbf73",
  "#55bbca",
  "#75a8e4",
  "#bb85d7",
  "#d981b1",
];
const eqChart = {
  width: 900,
  height: 310,
  left: 42,
  right: 876,
  top: 18,
  bottom: 280,
};
function eqX(f) {
  return (
    eqChart.left +
    (Math.log(f / 20) / Math.log(1000)) * (eqChart.right - eqChart.left)
  );
}
function eqY(db) {
  return eqChart.top + ((24 - db) / 48) * (eqChart.bottom - eqChart.top);
}
function svgEl(tag, attrs = {}) {
  const e = document.createElementNS("http://www.w3.org/2000/svg", tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  return e;
}
function currentEq() {
  return targetOf(eqTarget.type, eqTarget.index).eq;
}
function openEq(type, index) {
  flushEq();
  eqTarget = { type, index };
  eqBandIndex = 0;
  $("eq-title").textContent = `FreeEQ8 · ${targetOf(type, index).name}`;
  $("eq-edit-status").textContent = "Cambios en vivo";
  const svg = svgEl("svg", {
    viewBox: "0 0 900 310",
    role: "group",
    "aria-label": "Curva paramétrica de ocho bandas",
  });
  for (const gain of [-24, -18, -12, -6, 0, 6, 12, 18, 24]) {
    svg.append(
      svgEl("line", {
        x1: eqChart.left,
        x2: eqChart.right,
        y1: eqY(gain),
        y2: eqY(gain),
        class: gain === 0 ? "eq-zero" : "eq-gridline",
      }),
    );
    const text = svgEl("text", {
      x: 32,
      y: eqY(gain) + 3,
      "text-anchor": "end",
      class: "eq-axis-text",
    });
    text.textContent = gain > 0 ? "+" + gain : String(gain);
    svg.append(text);
  }
  for (const f of [20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000]) {
    svg.append(
      svgEl("line", {
        x1: eqX(f),
        x2: eqX(f),
        y1: eqChart.top,
        y2: eqChart.bottom,
        class: "eq-gridline",
      }),
    );
    const text = svgEl("text", {
      x: eqX(f),
      y: 302,
      "text-anchor": "middle",
      class: "eq-axis-text",
    });
    text.textContent = f >= 1000 ? f / 1000 + "k" : String(f);
    svg.append(text);
  }
  svg.append(
    svgEl("path", { id: "eq-selected-fill", class: "eq-selected-fill" }),
    svgEl("path", { id: "eq-response", class: "eq-response" }),
  );
  currentEq().bands.forEach((band, i) => {
    const node = svgEl("g", {
      class: "eq-node",
      tabindex: 0,
      role: "slider",
      "data-band": i,
      "aria-label": `Punto EQ banda ${i + 1}`,
      "aria-valuemin": 20,
      "aria-valuemax": 20000,
    });
    node.style.setProperty("--band-colour", eqColours[i]);
    node.append(
      svgEl("circle", { r: 17, class: "eq-node-hit" }),
      svgEl("circle", { r: 9, class: "eq-node-dot" }),
    );
    const label = svgEl("text", { "text-anchor": "middle", y: 3 });
    label.textContent = i + 1;
    node.append(label);
    let drag;
    node.onpointerdown = (e) => {
      if (e.button !== 0) return;
      e.preventDefault();
      selectEqBand(i);
      node.focus();
      node.setPointerCapture(e.pointerId);
      drag = {
        x: e.clientX,
        y: e.clientY,
        frequency: currentEq().bands[i].frequency,
        gain: currentEq().bands[i].gain_db,
      };
    };
    node.onpointermove = (e) => {
      if (!drag) return;
      const rect = svg.getBoundingClientRect(),
        b = currentEq().bands[i],
        fine = e.shiftKey ? 0.1 : 1;
      b.frequency = Math.max(
        20,
        Math.min(
          20000,
          drag.frequency *
            1000 **
              (((((e.clientX - drag.x) / rect.width) * 900) /
                (eqChart.right - eqChart.left)) *
                fine),
        ),
      );
      const preciseFrequency = b.frequency;
      b.frequency = Math.round(b.frequency);
      if (b.kind < 3)
        b.gain_db = Math.max(
          -18,
          Math.min(
            18,
            Math.round(
              (drag.gain -
                ((((e.clientY - drag.y) / rect.height) * 310) /
                  (eqChart.bottom - eqChart.top)) *
                  48 *
                  fine) *
                10,
            ) / 10,
          ),
        );
      drag = {
        x: e.clientX,
        y: e.clientY,
        frequency: preciseFrequency,
        gain: Math.max(
          -18,
          Math.min(
            18,
            drag.gain -
              ((((e.clientY - drag.y) / rect.height) * 310) /
                (eqChart.bottom - eqChart.top)) *
                48 *
                fine,
          ),
        ),
      };
      editEq();
    };
    const release = (e) => {
      if (!drag) return;
      drag = null;
      if (node.hasPointerCapture(e.pointerId))
        node.releasePointerCapture(e.pointerId);
      flushEq();
    };
    node.onpointerup = release;
    node.onpointercancel = release;
    node.onlostpointercapture = () => (drag = null);
    node.ondblclick = () => {
      currentEq().bands[i].gain_db = 0;
      editEq();
      flushEq();
    };
    node.addEventListener(
      "wheel",
      (e) => {
        e.preventDefault();
        selectEqBand(i);
        const b = currentEq().bands[i];
        b.q = Math.max(
          0.1,
          Math.min(
            18,
            Number((b.q * (e.deltaY < 0 ? 1.1 : 1 / 1.1)).toFixed(3)),
          ),
        );
        editEq();
      },
      { passive: false },
    );
    node.onkeydown = (e) => {
      const b = currentEq().bands[i];
      if (
        !["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home"].includes(
          e.key,
        )
      )
        return;
      e.preventDefault();
      selectEqBand(i);
      if (e.key === "ArrowLeft" || e.key === "ArrowRight")
        b.frequency = Math.max(
          20,
          Math.min(
            20000,
            Math.round(
              b.frequency *
                2 **
                  ((e.key === "ArrowRight" ? 1 : -1) / (e.shiftKey ? 120 : 12)),
            ),
          ),
        );
      else if (e.key === "Home") b.gain_db = 0;
      else if (b.kind < 3)
        b.gain_db = Math.max(
          -18,
          Math.min(
            18,
            Number(
              (
                b.gain_db +
                (e.key === "ArrowUp" ? 1 : -1) * (e.shiftKey ? 0.1 : 0.5)
              ).toFixed(1),
            ),
          ),
        );
      else
        b.q = Math.max(
          0.1,
          Math.min(
            18,
            Number((b.q + (e.key === "ArrowUp" ? 0.1 : -0.1)).toFixed(3)),
          ),
        );
      editEq();
      flushEq();
    };
    svg.append(node);
  });
  $("eq-graph").replaceChildren(svg);
  $("eq-tabs").replaceChildren(
    ...currentEq().bands.map((_, i) => {
      const button = el("button", "eq-tab", String(i + 1));
      button.id = `eq-tab-${i}`;
      button.type = "button";
      button.setAttribute("role", "tab");
      button.setAttribute("aria-controls", "eq-bands");
      button.setAttribute("aria-label", `Banda EQ ${i + 1}`);
      button.style.setProperty("--band-colour", eqColours[i]);
      button.onclick = () => selectEqBand(i);
      button.onkeydown = (e) => {
        if (!["ArrowLeft", "ArrowRight"].includes(e.key)) return;
        e.preventDefault();
        const next = (i + (e.key === "ArrowRight" ? 1 : 7)) % 8;
        selectEqBand(next);
        $("eq-tabs").children[next].focus();
      };
      return button;
    }),
  );
  $("eq-kind").replaceChildren(
    ...[
      "Bell · Campana",
      "Low shelf",
      "High shelf",
      "High pass · Pasa altos",
      "Low pass · Pasa bajos",
      "Band pass · Pasa banda",
    ].map((name, i) => new Option(name, i)),
  );
  selectEqBand(0);
  renderEqPresets();
  $("eq-dialog").showModal();
}
function selectEqBand(index) {
  eqBandIndex = index;
  const band = currentEq().bands[index];
  $("eq-band-label").textContent = `BANDA ${index + 1}`;
  $("eq-band-label").style.color = eqColours[index];
  $("eq-kind").value = band.kind;
  eqKnobs = [
    ["frequency", "Frecuencia", 20, 20000, 1, true, "Hz"],
    ["gain_db", "Ganancia", -18, 18, 0.1, false, "dB"],
    ["q", "Q / pendiente", 0.1, 18, 0.001, true, ""],
  ].map(([key, label, min, max, step, log, unit]) =>
    rotary({
      label: `EQ banda ${index + 1} ${label.toLowerCase()}`,
      inputLabel: `EQ banda ${index + 1} ${key}`,
      value: band[key],
      defaultValue: defaultEq().bands[index][key],
      min,
      max,
      step,
      log,
      unit,
      colour: eqColours[index],
      onChange: (v) => {
        currentEq().bands[index][key] = v;
        editEq();
      },
      onCommit: flushEq,
    }),
  );
  // Keep short captions while preserving descriptive accessible labels.
  eqKnobs.forEach(
    (k, i) =>
      (k.root.querySelector(".rotary-label").textContent = [
        "Frecuencia",
        "Ganancia",
        "Q / pendiente",
      ][i]),
  );
  $("eq-bands").replaceChildren(...eqKnobs.map((k) => k.root));
  $("eq-bands").setAttribute("role", "tabpanel");
  $("eq-bands").setAttribute("aria-labelledby", `eq-tab-${index}`);
  refreshEq();
}
function refreshEq() {
  if (!eqTarget || !$("eq-response")) return;
  const eq = currentEq(),
    band = eq.bands[eqBandIndex];
  let path = "",
    selectedPath = `M${eqX(20)} ${eqY(0)} `;
  for (let i = 0; i <= 256; i++) {
    const f = 20 * 1000 ** (i / 256),
      x = eqX(f),
      y = (v) => Math.max(eqChart.top, Math.min(eqChart.bottom, eqY(v)));
    path += `${i ? "L" : "M"}${x} ${y(eqResponse(eq, f))} `;
    selectedPath += `L${x} ${y(eqResponse({ bypass: eq.bypass, bands: [band] }, f))} `;
  }
  $("eq-response").setAttribute("d", path);
  $("eq-response").classList.toggle("bypassed", eq.bypass);
  $("eq-selected-fill").setAttribute(
    "d",
    selectedPath + `L${eqX(20000)} ${eqY(0)} Z`,
  );
  $("eq-selected-fill").style.fill = eqColours[eqBandIndex];
  $("eq-graph")
    .querySelectorAll(".eq-node")
    .forEach((node, i) => {
      const b = eq.bands[i];
      node.setAttribute(
        "transform",
        `translate(${eqX(b.frequency)} ${eqY(b.kind < 3 ? b.gain_db : 0)})`,
      );
      node.setAttribute("aria-valuenow", b.frequency);
      node.setAttribute(
        "aria-valuetext",
        `${Math.round(b.frequency)} Hz, ${b.kind < 3 ? b.gain_db : 0} dB, Q ${b.q}`,
      );
      node.classList.toggle("selected", i === eqBandIndex);
    });
  $("eq-tabs")
    .querySelectorAll("button")
    .forEach((b, i) => {
      b.setAttribute("aria-selected", i === eqBandIndex);
      b.tabIndex = i === eqBandIndex ? 0 : -1;
    });
  [band.frequency, band.gain_db, band.q].forEach((v, i) => eqKnobs[i]?.set(v));
  eqKnobs[1]?.setDisabled(band.kind >= 3);
  $("eq-bypass").checked = eq.bypass;
  $("eq-kind").value = band.kind;
  const key = keyOf(eqTarget.type, eqTarget.index);
  document
    .querySelectorAll(`.eq-module[data-eq="${key}"]`)
    .forEach((button) => {
      button.classList.toggle("eq-off", eq.bypass);
      button.querySelector("svg")?.replaceWith(eqSvg(eq));
    });
}
function editEq() {
  eqWriteVersion++;
  refreshEq();
  $("eq-edit-status").textContent = "Aplicando…";
  if (!eqWriteTimer) eqWriteTimer = setTimeout(flushEq, 40);
}
function flushEq() {
  if (!eqWriteTimer) return;
  clearTimeout(eqWriteTimer);
  eqWriteTimer = undefined;
  const version = eqWriteVersion;
  publish()
    .then(() => {
      if (version === eqWriteVersion)
        $("eq-edit-status").textContent = "Aplicado · en vivo";
    })
    .catch((e) => {
      $("eq-edit-status").textContent = "No se pudo aplicar";
      error(e);
    });
}
$("eq-dialog").addEventListener("close", flushEq);
$("eq-kind").onchange = () => {
  currentEq().bands[eqBandIndex].kind = Number($("eq-kind").value);
  editEq();
  flushEq();
};
$("eq-bypass").onchange = () => {
  currentEq().bypass = $("eq-bypass").checked;
  editEq();
  flushEq();
};
$("eq-reset-band").onclick = () => {
  currentEq().bands[eqBandIndex] = defaultEq().bands[eqBandIndex];
  selectEqBand(eqBandIndex);
  editEq();
  flushEq();
};
$("eq-reset-all").onclick = () => {
  targetOf(eqTarget.type, eqTarget.index).eq = defaultEq();
  selectEqBand(eqBandIndex);
  editEq();
  flushEq();
};

function openCounts(focus = "channel-count") {
  $("channel-count").value = cfg.channels.length;
  $("aux-count").value = cfg.buses.length - 1;
  $("counts-error").textContent = "";
  $("counts-dialog").showModal();
  $(focus).focus();
  $(focus).select();
}
$("counts").onclick = () => openCounts();
$("channel-total").onclick = () => openCounts();
$("aux-total").onclick = () => openCounts("aux-count");
async function resizeStructure(
  channels,
  auxes,
  channelFormat = "mono",
  auxFormat = "mono",
) {
  if (structureBusy) throw Error("Hay un cambio de estructura en curso");
  if (
    !Number.isInteger(channels) ||
    !Number.isInteger(auxes) ||
    channels < 1 ||
    channels > limits.channels ||
    auxes < 0 ||
    auxes > limits.auxes
  )
    throw Error("Cantidad fuera de rango");
  structureBusy = true;
  render();
  try {
    await queue;
    const freshResponse = await fetch("/api/config");
    if (!freshResponse.ok)
      throw Error("No se pudo sincronizar antes de cambiar la estructura");
    const fresh = await freshResponse.json();
    reconcileConfig(fresh, serverConfig);
    serverConfig = structuredClone(fresh);
    const candidate = structuredClone(cfg);
    candidate.channels = candidate.channels.slice(0, channels);
    candidate.buses = candidate.buses.slice(0, auxes + 1);
    while (candidate.channels.length < channels) {
      const i = candidate.channels.length;
      candidate.channels.push({
        name: `Entrada ${i + 1}`,
        format: channelFormat,
        input: i < cfg.inputs ? i : null,
        input_right: i + 1 < cfg.inputs ? i + 1 : null,
        gain_db: 0,
        pan: 0,
        mute: false,
        sends_db: [],
        send_mute: [],
        pre: [],
        inserts: [],
        fx: null,
        generator: { signal: "off", level_db: -30, frequency: 1000 },
        eq: defaultEq(),
        compressor: defaultCompressor(),
        pin: "none",
      });
    }
    while (candidate.buses.length < auxes + 1)
      candidate.buses.push({
        name: `Monitor ${candidate.buses.length}`,
        format: auxFormat,
        gain_db: -12,
        mute: false,
        output: null,
        inserts: [],
        fx: null,
        eq: defaultEq(),
        compressor: defaultCompressor(),
        pin: "none",
      });
    for (const ch of candidate.channels) {
      ch.sends_db = ch.sends_db.slice(0, auxes);
      ch.send_mute = ch.send_mute.slice(0, auxes);
      while (ch.send_mute.length < auxes) ch.send_mute.push(false);
      ch.pre = ch.pre.slice(0, auxes);
      while (ch.sends_db.length < auxes) ch.sends_db.push(-90);
      while (ch.pre.length < auxes) ch.pre.push(true);
    }
    if (candidate.pc_monitor >= candidate.buses.length)
      candidate.pc_monitor = 0;
    for (const node of [...candidate.channels, ...candidate.buses]) {
      if (
        node.compressor?.source?.type === "channel" &&
        node.compressor.source.index >= channels
      )
        node.compressor.source = { type: "internal" };
    }
    await api("config", candidate);
    cfg = candidate;
    meterStates.clear();
  } finally {
    structureBusy = false;
    render();
  }
}
function revealStrip(type, index) {
  selectStrip(type, index);
  document
    .querySelector(`.strip[data-strip="${keyOf(type, index)}"]`)
    ?.scrollIntoView({ block: "nearest", inline: "center" });
}
$("add-channel").onclick = () =>
  resizeStructure(cfg.channels.length + 1, cfg.buses.length - 1)
    .then(() => revealStrip("channel", cfg.channels.length - 1))
    .catch(error);
$("add-aux").onclick = () =>
  resizeStructure(cfg.channels.length, cfg.buses.length)
    .then(() => revealStrip("bus", cfg.buses.length - 1))
    .catch(error);
$("apply-counts").onclick = async () => {
  if (!$("channel-count").reportValidity() || !$("aux-count").reportValidity())
    return;
  const button = $("apply-counts");
  button.disabled = true;
  button.textContent = "Aplicando…";
  try {
    await resizeStructure(
      Number($("channel-count").value),
      Number($("aux-count").value),
      $("new-channel-format").value,
      $("new-aux-format").value,
    );
    $("counts-dialog").close();
  } catch (e) {
    $("counts-error").textContent = e.message;
  } finally {
    button.disabled = false;
    button.textContent = "Aplicar estructura";
  }
};
document.addEventListener("DOMContentLoaded", () => {
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
      serverConfig = structuredClone(cfg);
      render();
      poll();
      requestAnimationFrame(animateMeters);
    })
    .catch(error);
});
