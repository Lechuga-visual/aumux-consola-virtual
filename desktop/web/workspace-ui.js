"use strict";
function defaultCompressor() {
  return {
    bypass: true,
    threshold: -18,
    ratio: 4,
    attack: 10,
    release: 120,
    knee: 6,
    makeup: 0,
    mix: 1,
    highpass: 0,
    source: { type: "internal" },
  };
}
const compRanges = {
  threshold: [-60, 0],
  ratio: [1, 20],
  attack: [0.1, 200],
  release: [10, 2000],
  knee: [0, 24],
  makeup: [-12, 24],
  mix: [0, 1],
  highpass: [0, 1000],
};
let compTarget,
  compKnobs = [],
  compTimer;
function compressorButton(type, index) {
  const c = (targetOf(type, index).compressor ??= defaultCompressor()),
    button = el("button", "comp-module" + (c.bypass ? " off" : ""));
  button.dataset.comp = keyOf(type, index);
  button.setAttribute(
    "aria-label",
    `Compresor ${type === "channel" ? "canal" : "bus"} ${index + 1}`,
  );
  button.append(el("span", "", "DYN"), el("i", "comp-mini-gr"));
  button.title = "Compresor con sidechain · abrir";
  button.onclick = () => openCompressor(type, index);
  return button;
}
function currentComp() {
  return targetOf(compTarget.type, compTarget.index).compressor;
}
function compReduction(c, db) {
  const x = db - c.threshold,
    f = 1 - 1 / c.ratio;
  return c.knee > 0 && Math.abs(x) < c.knee / 2
    ? (f * (x + c.knee / 2) ** 2) / (2 * c.knee)
    : f * Math.max(x, 0);
}
function compGraph() {
  const c = currentComp(),
    svg = svgEl("svg", {
      viewBox: "0 0 400 220",
      role: "img",
      "aria-label": "Curva de compresión: entrada y salida en dBFS",
    });
  for (const db of [-60, -48, -36, -24, -12, 0]) {
    const x = 25 + ((db + 60) / 60) * 350,
      y = 195 - ((db + 60) / 60) * 175;
    svg.append(
      svgEl("path", { d: `M${x} 20 V195 M25 ${y} H375`, class: "eq-gridline" }),
    );
    const t = svgEl("text", { x, y: 212, class: "eq-axis-text" });
    t.textContent = db;
    svg.append(t);
  }
  svg.append(svgEl("path", { d: "M25 195 L375 20", class: "comp-unity" }));
  let path = "";
  for (let i = 0; i <= 120; i++) {
    const db = -60 + i * 0.5,
      out = db - compReduction(c, db) + c.makeup;
    path += `${i ? "L" : "M"}${25 + ((db + 60) / 60) * 350} ${Math.max(10, Math.min(195, 195 - ((out + 60) / 60) * 175))} `;
  }
  svg.append(svgEl("path", { d: path, class: "comp-response" }));
  $("comp-graph").replaceChildren(svg);
}
function openCompressor(type, index) {
  if (openDetachedPanel("compressor-dialog", { type, index })) return;
  flushComp();
  compTarget = { type, index };
  const c = currentComp();
  $("compressor-title").textContent =
    `Compresor · ${targetOf(type, index).name}`;
  $("comp-bypass").checked = c.bypass;
  $("comp-source").replaceChildren(
    new Option("Interno · señal del propio canal/bus", "internal"),
    ...cfg.channels.map(
      (ch, i) =>
        new Option(`Canal ${i + 1} · ${ch.name} · precomp`, `channel:${i}`),
    ),
    ...Array.from(
      { length: cfg.inputs },
      (_, i) => new Option(`Entrada física ${i + 1}`, `input:${i}`),
    ),
  );
  $("comp-source").value =
    c.source.type === "internal"
      ? "internal"
      : c.source.type + ":" + c.source.index;
  compKnobs = [
    ["threshold", "Umbral", -60, 0, 0.1, false, "dB"],
    ["ratio", "Ratio", 1, 20, 0.1, false, ":1"],
    ["attack", "Ataque", 0.1, 200, 0.1, true, "ms"],
    ["release", "Release", 10, 2000, 1, true, "ms"],
    ["knee", "Knee", 0, 24, 0.1, false, "dB"],
    ["makeup", "Makeup", -12, 24, 0.1, false, "dB"],
    ["mix", "Mezcla", 0, 1, 0.001, false, "%"],
    ["highpass", "Filtro detector", 0, 1000, 1, false, "Hz"],
  ].map(([key, label, min, max, step, log, unit]) =>
    rotary({
      label,
      inputLabel: `Compresor ${key}`,
      value: c[key],
      defaultValue: defaultCompressor()[key],
      min,
      max,
      step,
      log,
      unit,
      scale: key === "mix" ? 100 : 1,
      colour: "#d9af65",
      onChange: (v) => {
        currentComp()[key] = v;
        compChanged();
      },
    }),
  );
  $("comp-knobs").replaceChildren(...compKnobs.map((k) => k.root));
  $("comp-presets").replaceChildren(
    presetBar({
      id: "native/compressor",
      defaults: Object.fromEntries(
        Object.entries(defaultCompressor()).filter(
          ([k]) => !["source", "bypass"].includes(k),
        ),
      ),
      read: () =>
        Object.fromEntries(
          Object.entries(currentComp()).filter(
            ([k]) => !["source", "bypass"].includes(k),
          ),
        ),
      load: (values) => {
        const c = currentComp();
        for (const [k, v] of Object.entries(values)) {
          if (
            k in defaultCompressor() &&
            typeof c[k] === "number" &&
            Number.isFinite(v)
          )
            c[k] = Math.max(compRanges[k][0], Math.min(compRanges[k][1], v));
        }
        compKnobs.forEach((knob, i) =>
          knob.set(
            c[
              [
                "threshold",
                "ratio",
                "attack",
                "release",
                "knee",
                "makeup",
                "mix",
                "highpass",
              ][i]
            ],
          ),
        );
        compChanged();
      },
    }),
  );
  compGraph();
  $("comp-status").textContent = "Cambios en vivo";
  showFloatingPanel("compressor-dialog");
}
function compChanged() {
  compGraph();
  $("comp-status").textContent = "Aplicando…";
  if (!compTimer) compTimer = setTimeout(flushComp, 40);
  document
    .querySelectorAll(
      `.comp-module[data-comp="${keyOf(compTarget.type, compTarget.index)}"]`,
    )
    .forEach((b) => b.classList.toggle("off", currentComp().bypass));
}
function flushComp() {
  if (!compTimer) return;
  clearTimeout(compTimer);
  compTimer = undefined;
  publish()
    .then(() => ($("comp-status").textContent = "Aplicado · en vivo"))
    .catch((e) => {
      $("comp-status").textContent = e.message;
      error(e);
    });
}
$("comp-bypass").onchange = () => {
  currentComp().bypass = $("comp-bypass").checked;
  compChanged();
};
$("comp-source").onchange = () => {
  const [type, index] = $("comp-source").value.split(":");
  currentComp().source =
    type === "internal" ? { type } : { type, index: Number(index) };
  compChanged();
};
$("compressor-dialog").addEventListener("close", flushComp);

function layoutStrips() {
  if (!cfg) return;
  const stage = $("mixer-stage"),
    cards = [...document.querySelectorAll(".strip")],
    width =
      parseFloat(
        getComputedStyle(document.querySelector(".app")).getPropertyValue(
          "--strip",
        ),
      ) || 70;
  const ordered = cards.map((card) => {
    const key = card.dataset.strip,
      type = key[0] === "c" ? "channel" : "bus",
      index = Number(key.slice(1)),
      node = targetOf(type, index),
      pin = node.pin || "none",
      rank =
        type === "channel"
          ? index
          : index === 0
            ? 10000
            : cfg.channels.length + index;
    return { card, key, pin, rank };
  });
  // Reserve one visible strip when the window shrinks; retain pin choices in the scene.
  const pinBudget = Math.max(1, Math.floor(stage.clientWidth / width) - 1);
  const requested = ordered
    .filter((x) => x.pin !== "none")
    .sort((a, b) =>
      a.key === "b0" ? -1 : b.key === "b0" ? 1 : a.rank - b.rank,
    );
  if (cards.length * width > stage.clientWidth)
    requested.slice(pinBudget).forEach((x) => (x.pin = "none"));
  ordered.sort((a, b) => {
    const lane = (p) => (p === "left" ? 0 : p === "right" ? 2 : 1);
    return lane(a.pin) - lane(b.pin) || a.rank - b.rank;
  });
  const left = ordered.filter((x) => x.pin === "left"),
    right = ordered.filter((x) => x.pin === "right");
  ordered.forEach((item, order) => {
    for (const target of [
      item.card,
      document.querySelector(`.bridge-cell[data-strip="${item.key}"]`),
    ]) {
      if (!target) continue;
      target.style.order = order;
      target.dataset.pin = item.pin;
      target.style.left =
        item.pin === "left" ? left.indexOf(item) * width + "px" : "auto";
      target.style.right =
        item.pin === "right"
          ? (right.length - 1 - right.indexOf(item)) * width + "px"
          : "auto";
      target.classList.toggle("pinned", item.pin !== "none");
    }
    item.card.dataset.pin = item.pin;
  });
  const overflow = cards.length * width > stage.clientWidth;
  stage.classList.toggle("overflowing", overflow);
  $("meter-stage").classList.toggle("overflowing", overflow);
  $("meter-stage").scrollLeft = stage.scrollLeft;
}
new ResizeObserver(layoutStrips).observe($("mixer-stage"));
$("mixer-stage").addEventListener(
  "scroll",
  () => ($("meter-stage").scrollLeft = $("mixer-stage").scrollLeft),
  { passive: true },
);
$("mixer-stage").addEventListener(
  "wheel",
  (e) => {
    if (e.target.closest("input,select,[role=slider]")) return;
    if ($("mixer-stage").scrollWidth <= $("mixer-stage").clientWidth) return;
    e.preventDefault();
    $("mixer-stage").scrollLeft +=
      Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
  },
  { passive: false },
);
async function pinStrip(type, index, pin) {
  await setProperty(type, index, "pin", pin);
}
async function contextScene(candidate) {
  if (structureBusy) return;
  structureBusy = true;
  try {
    await queue;
    await api("config", candidate);
    cfg = candidate;
    render();
  } finally {
    structureBusy = false;
  }
}
async function duplicateStrip(type, index) {
  const candidate = structuredClone(cfg),
    original = structuredClone(targetOf(type, index));
  original.name += " copia";
  original.pin = "none";
  if (type === "channel") {
    if (candidate.channels.length >= limits.channels) return;
    candidate.channels.push(original);
  } else {
    if (candidate.buses.length > limits.auxes) return;
    original.output = null;
    candidate.buses.push(original);
    candidate.channels.forEach((c) => {
      c.sends_db.push(-90);
      c.send_mute.push(false);
      c.pre.push(true);
    });
  }
  await contextScene(candidate);
  revealStrip(
    type,
    type === "channel" ? cfg.channels.length - 1 : cfg.buses.length - 1,
  );
}
async function removeStrip(type, index) {
  const candidate = structuredClone(cfg);
  if (type === "channel") {
    if (candidate.channels.length === 1) return;
    candidate.channels.splice(index, 1);
    for (const node of [...candidate.channels, ...candidate.buses]) {
      const key = node.compressor?.source;
      if (key?.type === "channel") {
        if (key.index === index) node.compressor.source = { type: "internal" };
        else if (key.index > index) key.index--;
      }
    }
  } else {
    if (index === 0) return;
    candidate.buses.splice(index, 1);
    candidate.channels.forEach((c) => {
      c.sends_db.splice(index - 1, 1);
      c.send_mute.splice(index - 1, 1);
      c.pre.splice(index - 1, 1);
    });
    if (candidate.pc_monitor === index) candidate.pc_monitor = 0;
    else if (candidate.pc_monitor > index) candidate.pc_monitor--;
  }
  await contextScene(candidate);
}
function hideContext() {
  $("strip-context").hidden = true;
}
function openContext(card, x, y) {
  const key = card.dataset.strip,
    type = key[0] === "c" ? "channel" : "bus",
    index = Number(key.slice(1)),
    node = targetOf(type, index),
    menu = $("strip-context");
  menu.replaceChildren(el("strong", "", node.name));
  const entries = [
    [
      "Renombrar",
      () => {
        const field = card.querySelector(".name");
        field.focus();
        field.select();
      },
    ],
    ["EQ", () => openEq(type, index)],
    ["Compresor / sidechain", () => openCompressor(type, index)],
    ["Cadena FX", () => openFx(type, index, 0)],
    ["Fijar a la izquierda", () => pinStrip(type, index, "left")],
    ["Fijar a la derecha", () => pinStrip(type, index, "right")],
    ["Desfijar", () => pinStrip(type, index, "none")],
  ];
  if (type === "channel")
    entries.splice(3, 0, ["Envíos", () => openRouting(index)]);
  if (!(type === "bus" && index === 0)) {
    entries.push(["Duplicar", () => duplicateStrip(type, index)]);
    const last =
      index ===
      (type === "channel" ? cfg.channels.length : cfg.buses.length) - 1;
    entries.push([
      "Eliminar",
      () => removeStrip(type, index),
      (running && !last) || (type === "channel" && cfg.channels.length === 1),
    ]);
  }
  for (const [text, action, disabled] of entries) {
    const b = el("button", "", text);
    b.setAttribute("role", "menuitem");
    b.disabled = !!disabled;
    b.onclick = () => {
      hideContext();
      Promise.resolve().then(action).catch(error);
    };
    menu.append(b);
  }
  menu.hidden = false;
  menu.style.left =
    Math.max(0, Math.min(x, innerWidth - menu.offsetWidth - 6)) + "px";
  menu.style.top =
    Math.max(0, Math.min(y, innerHeight - menu.offsetHeight - 6)) + "px";
  menu.querySelector("button")?.focus();
}
document.addEventListener("contextmenu", (e) => {
  if (
    e.target.closest(
      'input,textarea,select,[contenteditable="true"],.selectable',
    )
  )
    return;
  e.preventDefault();
  const card = e.target.closest(".strip");
  if (card) openContext(card, e.clientX, e.clientY);
  else hideContext();
});
document.addEventListener("pointerdown", (e) => {
  if (!e.target.closest("#strip-context")) hideContext();
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") hideContext();
  if ((e.shiftKey && e.key === "F10") || e.key === "ContextMenu") {
    const card = e.target.closest(".strip");
    if (card && !e.target.closest("input,select,textarea")) {
      e.preventDefault();
      const r = card.getBoundingClientRect();
      openContext(card, r.left, r.top + 25);
    }
  }
  if (!$("strip-context").hidden && ["ArrowDown", "ArrowUp"].includes(e.key)) {
    e.preventDefault();
    const items = [
        ...$("strip-context").querySelectorAll("button:not(:disabled)"),
      ],
      i = items.indexOf(document.activeElement);
    items[
      (i + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length
    ]?.focus();
  }
});

function updateWorkspaceStatus(s) {
  document.querySelectorAll(".comp-module").forEach((b) => {
    const key = b.dataset.comp,
      index =
        key[0] === "c"
          ? Number(key.slice(1))
          : cfg.channels.length + Number(key.slice(1)),
      gr = s.gain_reduction_db?.[index] || 0;
    b.querySelector("i").style.width = Math.min(100, (gr / 30) * 100) + "%";
    b.title = `Compresor · reducción ${gr.toFixed(1)} dB`;
  });
  if (compTarget && $("compressor-dialog").open) {
    const i =
        compTarget.type === "channel"
          ? compTarget.index
          : cfg.channels.length + compTarget.index,
      gr = s.gain_reduction_db?.[i] || 0;
    $("comp-gr-value").textContent = gr.toFixed(1) + " dB";
    $("comp-gr-fill").style.height = Math.min(100, (gr / 30) * 100) + "%";
  }
  if ($("network-dialog").open) {
    updateRemoteAdmin(s.remote);
    updateAccessUI(s);
    updateAudioNetwork(s);
    $("preferences-services").textContent =
      `Control: ${s.remote.enabled ? "activo · " + s.remote.port : "detenido"} · Escucha: ${s.listen.enabled ? "activa · " + s.listen.port : "detenida"}`;
    const n = s.listen;
    $("network-toggle").textContent = n.enabled
      ? "Detener escucha LAN"
      : "Iniciar escucha LAN";
    $("network-port").disabled = n.enabled;
    if (n.enabled) $("network-port").value = n.port;
    $("network-url").value = n.enabled ? n.url : "";
    $("network-open").href = n.enabled ? n.url : "#";
    $("network-status").textContent = n.enabled
      ? `${n.clients} / 8 oyentes · paquetes descartados ${n.dropped}`
      : "Servidor detenido";
  }
}
$("network-settings").onclick = () => {
  if (openDetachedPanel("network-dialog", { tab: "network" })) return;
  showFloatingPanel("network-dialog");
  showPreference("network");
  $("network-host").value =
    lastStatus?.remote?.host || lastStatus?.listen?.host || "";
  if (lastStatus) updateWorkspaceStatus(lastStatus);
};
$("network-toggle").onclick = async () => {
  if (!$("network-port").reportValidity()) return;
  const button = $("network-toggle");
  button.disabled = true;
  try {
    const n = await api("listen", {
      enabled: !lastStatus.listen.enabled,
      port: Number($("network-port").value),
    });
    lastStatus.listen = n;
    updateWorkspaceStatus(lastStatus);
  } catch (e) {
    $("network-status").textContent = e.message;
    $("access-status").textContent = e.message;
    $("access-status").dataset.error = "true";
  } finally {
    button.disabled = false;
  }
};
$("network-copy").onclick = () => {
  const field = $("network-url");
  field.focus();
  field.select();
  try {
    document.execCommand("copy");
  } catch {}
};

let remoteSyncBusy = false,
  remotePointerDown = false,
  remoteListSignature = "",
  remoteAuxSignature = "";
document.addEventListener("pointerdown", () => (remotePointerDown = true));
document.addEventListener("pointerup", () => (remotePointerDown = false));
document.addEventListener("pointercancel", () => (remotePointerDown = false));
window.addEventListener("blur", () => (remotePointerDown = false));
async function syncRemoteMix(s) {
  if (
    remoteNeedsRender &&
    !configRequests &&
    !structureBusy &&
    !remotePointerDown &&
    !panelTextEditing()
  ) {
    remoteNeedsRender = false;
    render();
    if ($("routing-dialog").open) renderRouting();
    refreshOpenPanels();
  }
  if (
    s.remote.revision === syncedRemoteRevision ||
    remoteSyncBusy ||
    configRequests ||
    structureBusy ||
    remotePointerDown ||
    panelTextEditing()
  )
    return;
  remoteSyncBusy = true;
  try {
    const r = await webFetch("/api/sync-state");
    if (!r.ok) return;
    const incoming = await r.json();
    if (
      configRequests ||
      structureBusy ||
      remotePointerDown ||
      panelTextEditing() ||
      !serverConfig
    )
      return;
    const structure =
      incoming.config.channels.length !== cfg.channels.length ||
      incoming.config.buses.length !== cfg.buses.length;
    if (structure) {
      if (JSON.stringify(cfg) !== JSON.stringify(serverConfig))
        error(
          "La estructura cambió desde otra consola; se recuperó el estado del servidor y se descartaron ajustes sin publicar.",
        );
      for (const id of [
        "eq-dialog",
        "compressor-dialog",
        "routing-dialog",
        "fx",
      ])
        if ($(id).open) $(id).close();
      cfg = normalise(incoming.config);
      selected.index = Math.min(
        selected.index,
        (selected.type === "channel" ? cfg.channels.length : cfg.buses.length) -
          1,
      );
    }
    const changed = structure || reconcileConfig(incoming.config, serverConfig);
    serverConfig = structuredClone(incoming.config);
    syncedRemoteRevision = incoming.revision;
    if (changed) {
      render();
      if ($("routing-dialog").open) renderRouting();
    }
    refreshOpenPanels();
  } catch (e) {
    error(e);
  } finally {
    remoteSyncBusy = false;
  }
}
function updateRemoteAdmin(n) {
  if (!n) return;
  $("remote-toggle").textContent = n.enabled
    ? "Detener control remoto"
    : "Iniciar control remoto";
  $("remote-port").disabled = n.enabled;
  if (n.enabled) $("remote-port").value = n.port;
  $("remote-create").disabled = !n.enabled || cfg.buses.length <= 1;
  $("remote-status").textContent = n.enabled
    ? `${n.sessions.length} / 32 enlaces personales activos`
    : "Control remoto detenido";
  const auxSignature = JSON.stringify(cfg.buses.map((b) => b.name));
  if (auxSignature !== remoteAuxSignature) {
    const old = $("remote-aux").value;
    $("remote-aux").replaceChildren(
      ...cfg.buses
        .slice(1)
        .map((b, i) => new Option(`AUX ${i + 1} · ${b.name}`, i + 1)),
    );
    if ([...$("remote-aux").options].some((o) => o.value === old))
      $("remote-aux").value = old;
    remoteAuxSignature = auxSignature;
  }
  const signature = JSON.stringify(n.sessions);
  if (signature === remoteListSignature) return;
  remoteListSignature = signature;
  $("remote-links").replaceChildren(
    ...n.sessions.map((s) => {
      const row = el("div", "remote-link"),
        title = el(
          "strong",
          "",
          `${s.label} · AUX ${s.aux} · máximo ${s.max_db} dB`,
        ),
        link = el("input");
      link.type = "text";
      link.readOnly = true;
      link.value = s.url;
      link.setAttribute("aria-label", `Enlace de ${s.label}`);
      const copy = el("button", "", "Copiar");
      copy.onclick = () => {
        link.focus();
        link.select();
        document.execCommand("copy");
      };
      const open = el("a", "", "Abrir");
      open.href = s.url;
      open.target = "_blank";
      open.rel = "noreferrer";
      const revoke = el("button", "", "Revocar");
      revoke.onclick = async () => {
        try {
          const n = await api("remote", { action: "revoke", id: s.id });
          lastStatus.remote = n;
          updateRemoteAdmin(n);
        } catch (e) {
          error(e);
        }
      };
      const actions = el("div", "form-row");
      actions.append(copy, open, revoke);
      row.append(title, link, actions);
      return row;
    }),
  );
}
$("network-host-apply").onclick = async () => {
  try {
    const v = await api("network-host", { host: $("network-host").value });
    lastStatus.listen = v.listen;
    lastStatus.remote = v.remote;
    updateWorkspaceStatus(lastStatus);
  } catch (e) {
    error(e);
    $("remote-status").textContent = e.message;
  }
};
$("remote-toggle").onclick = async () => {
  if (!$("remote-port").reportValidity()) return;
  const button = $("remote-toggle");
  button.disabled = true;
  try {
    const n = await api("remote", {
      action: lastStatus.remote.enabled ? "stop" : "start",
      port: Number($("remote-port").value),
    });
    lastStatus.remote = n;
    updateRemoteAdmin(n);
  } catch (e) {
    error(e);
    $("remote-status").textContent = e.message;
    $("access-status").textContent = e.message;
    $("access-status").dataset.error = "true";
  } finally {
    button.disabled = false;
  }
};
$("remote-create").onclick = async () => {
  for (const id of ["remote-label", "remote-max"])
    if (!$(id).reportValidity()) return;
  const button = $("remote-create");
  button.disabled = true;
  try {
    const n = await api("remote", {
      action: "create",
      label: $("remote-label").value,
      aux: Number($("remote-aux").value),
      max_db: Number($("remote-max").value),
      allow_pre: $("remote-pre").checked,
    });
    lastStatus.remote = n;
    updateRemoteAdmin(n);
  } catch (e) {
    error(e);
    $("remote-status").textContent = e.message;
  } finally {
    button.disabled = !lastStatus.remote.enabled;
  }
};
