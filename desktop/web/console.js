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
const meterStates = new Map(),
  lastSignal = Array(8).fill("white");
const signalNames = {
  off: "Entrada física",
  white: "Ruido blanco",
  pink: "Ruido rosa",
  sine: "Tono seno",
};
const colours = [
  "#51b7be",
  "#66a9d0",
  "#8c9acb",
  "#9d91bc",
  "#bc9389",
  "#baa26e",
  "#8eb493",
  "#6eabb4",
];

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}
function normalise(c) {
  c.pc_monitor ??= 0;
  c.channels.forEach((ch) => {
    ch.inserts ||= [];
    if (ch.fx) ch.inserts.unshift(ch.fx);
    ch.fx = null;
    ch.generator ||= { signal: "off", level_db: -30, frequency: 1000 };
  });
  c.buses.forEach((b) => {
    b.inserts ||= [];
    if (b.fx) b.inserts.unshift(b.fx);
    b.fx = null;
  });
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
      s.rms[i] += (s.rmsTarget[i] - s.rms[i]) * (1 - Math.exp(-dt / 0.3));
      if (s.target[i] >= s.hold[i]) {
        s.hold[i] = s.target[i];
        s.until[i] = now + 1400;
      } else if (now > s.until[i])
        s.hold[i] = Math.max(s.target[i], s.hold[i] - dt * 14);
      if (s.target[i] >= 0) s.clip = true;
    }
  const percent = (value) =>
    Math.max(0, Math.min(100, ((value + 60) / 66) * 100));
  for (const w of meterWidgets) {
    const s = peakState(w.key);
    w.widget.classList.toggle("clipped", s.clip);
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
        fx ? fxName(fx) : slot === target.inserts.length ? "＋ VST3" : "—",
      ),
    );
    button.title = fx
      ? `${fxName(fx)}\n${fx.path}${running ? "\nDetén el audio para editar." : ""}`
      : "Añadir un efecto VST3";
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
    type === "channel" ? colours[index] : index === 0 ? "#dd8d9f" : "#e4b666",
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
  for (const value of [6, 0, -6, -12, -24, -36, -60]) {
    const mark = el("span", "", String(value));
    mark.style.top = 100 - faderPosition(value) / 10 + "%";
    scale.append(mark);
  }
  const fader = el("div", "fader");
  const level = el(
    "div",
    "level",
    db(channel && view >= 0 ? target.sends_db[view] : target.gain_db),
  );
  level.id = "fader-value-" + key;
  const current = channel && view >= 0 ? target.sends_db[view] : target.gain_db;
  const input = slider(faderPosition(current), 0, 1000, 1, (value) => {
    const gain = faderDb(value);
    if (channel && view >= 0) target.sends_db[view] = gain;
    else target.gain_db = gain;
    level.textContent = db(gain);
    publish();
    updateSendLabels();
  });
  input.setAttribute(
    "aria-label",
    `${target.name} ${channel && view >= 0 ? "envío Monitor " + (view + 1) : "fader"}`,
  );
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
    ? "ENTRADA ASIO"
    : c.generator.signal === "sine"
      ? `${c.generator.frequency} Hz`
      : "RUIDO " + (c.generator.signal === "white" ? "BLANCO" : "ROSA");
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
  card.style.setProperty("--strip-color", colours[index]);
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
  pan.append(
    slider(c.pan, -1, 1, 0.01, (value) => {
      c.pan = value;
      panLabel.textContent =
        value === 0
          ? "PAN · C"
          : `PAN · ${value < 0 ? "L" : "R"} ${Math.round(Math.abs(value) * 100)}`;
      publish();
    }),
    panLabel,
  );
  card.append(pan);
  const sends = el("div", "send-grid");
  for (let a = 0; a < 4; a++) {
    const b = el("button");
    b.dataset.send = `${index}:${a}`;
    b.onclick = () => {
      view = a;
      selectStrip("channel", index);
      render();
    };
    sends.append(b);
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
    } else c.generator.signal = lastSignal[index];
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
  route.disabled = running;
  route.append(new Option("SIN ASIGNAR", ""));
  for (let p = 0; p < 64; p += 2)
    route.append(new Option(`OUT ${p + 1}–${p + 2}`, p));
  route.value = b.output === null ? "" : b.output;
  route.onchange = () => {
    const old = b.output;
    b.output = route.value === "" ? null : Number(route.value);
    if (
      b.output !== null &&
      cfg.buses.some((o) => o !== b && o.output === b.output)
    ) {
      b.output = old;
      route.value = old === null ? "" : old;
      error("Cada bus necesita un par físico distinto.");
      return;
    }
    publish();
    renderInspector();
  };
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
  return card;
}
function updateSendLabels() {
  document.querySelectorAll("[data-send]").forEach((e) => {
    const [i, a] = e.dataset.send.split(":").map(Number);
    const value = cfg.channels[i].sends_db[a];
    e.textContent = `M${a + 1} ${value <= -90 ? "−∞" : value.toFixed(0)}`;
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
  $("inputs").value = cfg.inputs;
  $("rate").value = cfg.sample_rate;
  $("buffer").value = cfg.buffer;
  $("bridge").checked = cfg.obs_bridge;
  $("pc-monitor").replaceChildren(
    ...cfg.buses.map((b, i) => new Option("Escucha PC: " + b.name, i)),
  );
  $("pc-monitor").value = cfg.pc_monitor;
  if (cfg.device !== null) $("device").value = cfg.device;
  $("meterbridge").replaceChildren(
    ...cfg.channels.map((_, i) => bridgeCell("channel", i)),
  );
  const outputOrder = [1, 2, 3, 4, 0];
  $("outputbridge").replaceChildren(
    ...outputOrder.map((i) => bridgeCell("bus", i)),
  );
  $("mixer").replaceChildren(...cfg.channels.map((_, i) => channelStrip(i)));
  $("buses").replaceChildren(...outputOrder.map((i) => busStrip(i)));
  $("layers").replaceChildren();
  for (let layer = -1; layer < 4; layer++) {
    const b = el(
      "button",
      view === layer ? "active" + (layer >= 0 ? " send-active" : "") : "",
      layer < 0 ? "MAIN" : "MON " + (layer + 1),
    );
    b.setAttribute(
      "aria-label",
      layer < 0
        ? "Faders de mezcla principal"
        : "Faders de Monitor " + (layer + 1),
    );
    b.onclick = () => {
      view = layer;
      render();
    };
    $("layers").append(b);
  }
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
        ? "Entrada mono · proceso estéreo"
        : "Bus estéreo · salida independiente",
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
          $("fader-value-c" + index).textContent = db(v);
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
          : `ASIO ${target.output + 1} / ${target.output + 2}`,
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
        "Sin efectos. Añade un VST3 desde la tira con el audio detenido.",
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
    selected.type === "channel" ? selected.index : 8 + selected.index;
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
    updateLatency();
  } catch (e) {
    $("status").textContent = "SIN CONEXIÓN";
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
  $("fx-title").textContent = fx ? fxName(fx) : "Añadir VST3";
  $("fx-location").textContent = `${target.name} · INSERT ${slot + 1}`;
  $("removefx").disabled = !fx;
  $("fxup").disabled = !fx || slot === 0;
  $("fxdown").disabled = !fx || slot === target.inserts.length - 1;
  $("fx").showModal();
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
$("inspect").onclick = async () => {
  try {
    $("fxerror").textContent = "Leyendo VST3…";
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
      "Primero pulsa Leer VST3 y selecciona un efecto.";
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
};
$("stop").onclick = () => api("stop").catch(error);
$("panic").onclick = () => api("panic").catch(error);
$("reset-peaks").onclick = resetPeaks;
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
fetch("/api/config")
  .then((r) => r.json())
  .then((c) => {
    cfg = normalise(c);
    render();
    poll();
    requestAnimationFrame(animateMeters);
  })
  .catch(error);
