"use strict";
// FX chains, user presets, and routing share the scene model; audio stays in Rust/C++.
let fxPanelBase;
let fxSequence = 0,
  fxAutoTimer,
  nativeCataloguePromise;
const fxMetadata = new Map();
async function nativeCatalogue() {
  return (nativeCataloguePromise ??= webFetch("/api/native")
    .then(async (r) => {
      if (!r.ok) throw Error("No se pudo leer la lista nativa");
      return r.json();
    })
    .catch((e) => {
      nativeCataloguePromise = undefined;
      throw e;
    }));
}
function descriptorKey(path, id) {
  return path + "\n" + id;
}
function fxError(e) {
  $("fxerror").textContent = e.message || e;
}
function nativeEditable() {
  const old =
    selectedFx &&
    targetOf(selectedFx.type, selectedFx.index).inserts[selectedFx.slot];
  return !!(
    old &&
    pendingFx &&
    old.id === pendingFx.id &&
    old.id.startsWith("native/")
  );
}
async function savePendingFx() {
  if (!pendingFx) return;
  if (
    [...$("params").querySelectorAll("input")].some((i) => !i.reportValidity())
  )
    throw Error("Revisa los valores del efecto");
  const target = targetOf(selectedFx.type, selectedFx.index),
    old = target.inserts[selectedFx.slot],
    value = structuredClone(pendingFx);
  // VST controls are read-only while running; browsing their chain must remain possible.
  if (running && old && old.id === value.id && !old.id.startsWith("native/"))
    return;
  if (
    old &&
    old.path === value.path &&
    old.id === value.id &&
    old.bypass === value.bypass &&
    Object.keys(old.params).length === Object.keys(value.params).length &&
    Object.entries(old.params).every(([k, v]) => value.params[k] === v)
  )
    return;
  if (
    running &&
    (!old || old.id !== value.id || !value.id.startsWith("native/"))
  )
    throw Error(
      "Detén el motor para modificar la cadena o los parámetros VST3",
    );
  const before = structuredClone(target.inserts),
    sentValue = structuredClone(value);
  target.inserts[selectedFx.slot] = value;
  try {
    await publish();
    const actual = target.inserts[selectedFx.slot];
    if (actual?.id === value.id && pendingFx?.id === value.id) {
      for (const [key, next] of Object.entries(actual.params))
        if (pendingFx.params[key] === sentValue.params[key])
          pendingFx.params[key] = next;
      fxPanelBase = structuredClone(actual);
    }
    $("fx-edit-status").textContent = running
      ? "Aplicado · en vivo"
      : "Cambios guardados";
    renderFxRail();
  } catch (e) {
    target.inserts = before;
    throw e;
  }
}
function flushNativeFx() {
  if (!fxAutoTimer) return Promise.resolve();
  clearTimeout(fxAutoTimer);
  fxAutoTimer = undefined;
  return savePendingFx().catch(fxError);
}
function fxParameterChanged() {
  $("fx-edit-status").textContent = nativeEditable()
    ? "Aplicando…"
    : "Pulsa Aplicar o cambia de efecto para guardar";
  if (nativeEditable() && !fxAutoTimer)
    fxAutoTimer = setTimeout(flushNativeFx, 40);
}
async function chooseFx(slot) {
  try {
    await flushNativeFx();
    await savePendingFx();
    openFx(selectedFx.type, selectedFx.index, slot);
  } catch (e) {
    fxError(e);
  }
}
function renderFxRail() {
  if (!selectedFx) return;
  const target = targetOf(selectedFx.type, selectedFx.index);
  $("fx-chain-list").replaceChildren();
  target.inserts.forEach((fx, slot) => {
    const row = el("div", "fx-chain-row"),
      bypass = el("input"),
      button = el("button", "fx-chain-name", `${slot + 1}  ${fxName(fx)}`);
    bypass.type = "checkbox";
    bypass.checked = !fx.bypass;
    bypass.setAttribute("aria-label", `Activar insert ${slot + 1}`);
    bypass.onchange = () => {
      fx.bypass = !bypass.checked;
      if (selectedFx.slot === slot && pendingFx) pendingFx.bypass = fx.bypass;
      publish().catch(fxError);
      render();
    };
    button.setAttribute("role", "option");
    button.setAttribute("aria-selected", slot === selectedFx.slot);
    button.dataset.slot = slot;
    button.title = fxName(fx);
    button.onclick = () => chooseFx(slot);
    button.onkeydown = (e) => {
      if (e.key === "Delete") {
        e.preventDefault();
        if (!running) $("removefx").click();
      }
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        chooseFx(
          Math.max(
            0,
            Math.min(
              target.inserts.length - 1,
              slot + (e.key === "ArrowDown" ? 1 : -1),
            ),
          ),
        );
      }
    };
    row.append(bypass, button);
    $("fx-chain-list").append(row);
  });
  if (selectedFx.slot >= target.inserts.length)
    $("fx-chain-list").append(el("div", "fx-chain-pending", "+ Nuevo efecto"));
  $("fx-chain-count").textContent =
    `${target.inserts.length} / 4 inserts · Supr para quitar`;
  $("fx-add").disabled = running || target.inserts.length >= 4;
  $("removefx").disabled = running || selectedFx.slot >= target.inserts.length;
  $("fxup").disabled =
    running ||
    selectedFx.slot === 0 ||
    selectedFx.slot >= target.inserts.length;
  $("fxdown").disabled =
    running || selectedFx.slot >= target.inserts.length - 1;
}
function openFx(type, index, slot) {
  if (openDetachedPanel("fx", { type, index, slot })) return;
  clearTimeout(fxAutoTimer);
  fxAutoTimer = undefined;
  const sequence = ++fxSequence;
  selectedFx = { type, index, slot };
  const target = targetOf(type, index),
    fx = target.inserts[slot];
  pendingFx = fx ? structuredClone(fx) : null;
  fxPanelBase = fx ? structuredClone(fx) : null;
  descriptors = [];
  $("path").value = fx?.path || "";
  $("params").replaceChildren(
    el(
      "p",
      "hint",
      fx
        ? "Cargando controles…"
        : "Elige un efecto nativo o un VST3. Se añadirá al pulsar Aplicar.",
    ),
  );
  $("classes").replaceChildren();
  $("fx-presets").replaceChildren();
  $("fxerror").textContent = "";
  $("fx-edit-status").textContent = "";
  $("fx-title").textContent = fx ? fxName(fx) : "Añadir efecto";
  $("fx-location").textContent = `${target.name} · CADENA FX`;
  $("fx").classList.remove("vox-editor");
  $("fx-browser").open = !fx;
  renderFxRail();
  for (const id of [
    "builtin-fx",
    "path",
    "inspect",
    "classes",
    "vst-installed",
    "vst-rescan",
    "vst-directory",
    "vst-search",
  ])
    $(id).disabled = running;
  showFloatingPanel("fx");
  nativeCatalogue()
    .then((list) => {
      if (sequence !== fxSequence) return;
      $("builtin-fx").replaceChildren(
        new Option("Seleccionar efecto nativo…", ""),
        ...list.map((p) => new Option(p.name, p.id)),
      );
    })
    .catch(fxError);
  if (fx) {
    if (fx.id.startsWith("native/"))
      nativeCatalogue()
        .then((list) => {
          if (sequence !== fxSequence) return;
          const d = list.find((p) => p.id === fx.id);
          if (!d) throw Error("Efecto nativo no encontrado");
          setFxDescriptors([
            { ...d, params: d.params.map((p) => ({ ...p, value: p.default })) },
          ]);
        })
        .catch(fxError);
    else if (fxMetadata.has(descriptorKey(fx.path, fx.id)))
      setFxDescriptors([fxMetadata.get(descriptorKey(fx.path, fx.id))]);
    else if (!running) $("inspect").click();
    else
      setFxDescriptors([
        {
          id: fx.id,
          name: fxName(fx),
          params: Object.entries(fx.params).map(([id, value]) => ({
            id,
            name: id,
            min: Math.min(0, value),
            max: Math.max(1, value),
            value,
          })),
        },
      ]);
  }
  ensurePluginCatalogue().catch(
    (e) => ($("plugin-library-status").textContent = e.message),
  );
}
function setFxDescriptors(list) {
  descriptors = list;
  $("classes").replaceChildren(...list.map((p, i) => new Option(p.name, i)));
  if (!list.length) throw Error("Este bundle no contiene efectos de audio");
  for (const d of list) fxMetadata.set(descriptorKey($("path").value, d.id), d);
  rememberPluginDescriptors($("path").value, list);
  showParams();
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
  for (const p of d.params) pendingFx.params[p.id] ??= p.value;
  if (old?.id === d.id) fxPanelBase = structuredClone(pendingFx);
  $("fx-title").textContent = d.name;
  $("fx-browser").open = false;
  renderFxControls(d);
  $("fx-presets").replaceChildren(
    presetBar({
      id: d.id,
      defaults: Object.fromEntries(d.params.map((p) => [p.id, p.value])),
      read: () => pendingFx.params,
      load: (values) => {
        for (const p of d.params)
          pendingFx.params[p.id] = Math.max(
            p.min,
            Math.min(
              p.max,
              Number.isFinite(values[p.id]) ? values[p.id] : p.value,
            ),
          );
        renderFxControls(d);
        fxParameterChanged();
      },
      editable: !running || d.id.startsWith("native/"),
    }),
  );
}
function renderFxControls(d) {
  $("params").replaceChildren();
  $("fx").classList.toggle("vox-editor", d.id === "native/freevox8");
  if (d.id === "native/freevox8") {
    renderFreevox(d);
    $("applyfx").disabled = false;
    return;
  }
  if (d.id.startsWith("native/")) {
    const panel = el("section", "generic-native-panel"),
      heading = el("div", "plugin-panel-heading");
    heading.append(el("strong", "", d.name), el("span", "", "NATIVE AUDIO FX"));
    panel.append(heading);
    const bank = el("div", "generic-knob-bank");
    for (const p of d.params) {
      const range = p.max - p.min,
        control = rotary({
          label: p.name,
          inputLabel: p.name,
          value: pendingFx.params[p.id],
          defaultValue: p.value,
          min: p.min,
          max: p.max,
          step: range <= 2 ? 0.001 : range <= 200 ? 0.1 : 1,
          log:
            p.min > 0 &&
            p.max / p.min >= 100 &&
            /freq|attack|release|time/i.test(p.name),
          colour: "#7dacba",
          onChange: (v) => {
            pendingFx.params[p.id] = v;
            fxParameterChanged();
          },
        });
      bank.append(control.root);
    }
    panel.append(bank);
    $("params").append(panel);
  } else {
    const heading = el("h3", "generic-vst-title", `${d.name} · parámetros`);
    $("params").append(heading);
    for (const p of d.params) {
      const row = el("label", "generic-parameter"),
        name = el("span", "", p.name),
        range = slider(
          pendingFx.params[p.id],
          p.min,
          p.max,
          (p.max - p.min) / 1000,
          (v) => {
            pendingFx.params[p.id] = v;
            number.value = v;
            fxParameterChanged();
          },
        ),
        number = el("input");
      number.type = "number";
      number.min = p.min;
      number.max = p.max;
      number.step = "any";
      number.value = pendingFx.params[p.id];
      number.setAttribute("aria-label", p.name);
      range.setAttribute("aria-label", `Control ${p.name}`);
      range.disabled = number.disabled = running;
      number.onchange = () => {
        if (number.reportValidity()) {
          pendingFx.params[p.id] = Number(number.value);
          range.value = number.value;
          fxParameterChanged();
        }
      };
      row.append(name, range, number);
      $("params").append(row);
    }
  }
  $("applyfx").disabled = running && !d.id.startsWith("native/");
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
  const sequence = fxSequence;
  try {
    $("fxerror").textContent = "Leyendo efecto…";
    const list = await api("inspect", { path: $("path").value });
    if (sequence !== fxSequence) return;
    setFxDescriptors(list);
    $("fxerror").textContent = "";
  } catch (e) {
    fxError(e);
  }
};
$("path").oninput = () => {
  pendingFx = null;
};
$("classes").onchange = showParams;
async function closeFx() {
  try {
    await flushNativeFx();
    await savePendingFx();
    $("fx").close();
    render();
  } catch (e) {
    fxError(e);
  }
}
$("closefx").onclick = closeFx;
$("fx").addEventListener("cancel", (e) => {
  e.preventDefault();
  closeFx();
});
$("applyfx").onclick = () => savePendingFx().catch(fxError);
$("fx-add").onclick = () =>
  chooseFx(targetOf(selectedFx.type, selectedFx.index).inserts.length);
$("removefx").onclick = async () => {
  if (running) return;
  clearTimeout(fxAutoTimer);
  fxAutoTimer = undefined;
  const target = targetOf(selectedFx.type, selectedFx.index),
    before = structuredClone(target.inserts);
  target.inserts.splice(selectedFx.slot, 1);
  pendingFx = null;
  try {
    await publish();
    openFx(
      selectedFx.type,
      selectedFx.index,
      Math.max(0, Math.min(selectedFx.slot, target.inserts.length - 1)),
    );
    render();
  } catch (e) {
    target.inserts = before;
    fxError(e);
  }
};
for (const [id, delta] of [
  ["fxup", -1],
  ["fxdown", 1],
])
  $(id).onclick = async () => {
    if (running) return;
    let target, before;
    try {
      await savePendingFx();
      target = targetOf(selectedFx.type, selectedFx.index);
      before = structuredClone(target.inserts);
      const slot = selectedFx.slot;
      [target.inserts[slot], target.inserts[slot + delta]] = [
        target.inserts[slot + delta],
        target.inserts[slot],
      ];
      await publish();
      openFx(selectedFx.type, selectedFx.index, slot + delta);
      render();
    } catch (e) {
      if (target && before) target.inserts = before;
      fxError(e);
    }
  };

function downloadPreset(preset) {
  const a = document.createElement("a");
  a.href = URL.createObjectURL(
    new Blob([JSON.stringify(preset, null, 2)], { type: "application/json" }),
  );
  a.download =
    (preset.name || "Preset").replace(/[^a-z0-9_.-]/gi, "_") +
    ".aumuxpreset.json";
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}
function presetBar({ id, defaults, read, load, editable = true }) {
  const root = el("section", "preset-bar"),
    select = el("select"),
    name = el("input"),
    save = el("button", "", "Guardar"),
    remove = el("button", "", "Borrar"),
    importButton = el("button", "", "Importar"),
    exportButton = el("button", "", "Exportar"),
    file = el("input"),
    status = el("span", "preset-status");
  root.setAttribute("aria-label", "Presets");
  select.setAttribute("aria-label", "Seleccionar preset");
  name.type = "text";
  name.maxLength = 80;
  name.placeholder = "Nombre del preset";
  name.setAttribute("aria-label", "Nombre del preset");
  file.type = "file";
  file.accept = ".json";
  file.hidden = true;
  file.setAttribute("aria-label", "Importar archivo de preset");
  let entries = [];
  function populate(all, selected) {
    entries = all.filter((p) => p.plugin_id === id);
    select.replaceChildren(
      new Option("Presets…", ""),
      new Option("Inicial / predeterminado", "default"),
      ...entries.map((p, i) => new Option(p.name, "user:" + i)),
    );
    if (selected) {
      const index = entries.findIndex((p) => p.name === selected);
      select.value = index >= 0 ? "user:" + index : "";
    }
    remove.disabled = !select.value.startsWith("user:");
  }
  fetch("/api/presets")
    .then(async (r) => {
      const v = await r.json();
      if (!r.ok) throw Error(v.error || "No se pudieron leer presets");
      if (root.isConnected) populate(v);
    })
    .catch((e) => (status.textContent = e.message));
  select.disabled = !editable;
  select.onchange = () => {
    remove.disabled = !select.value.startsWith("user:");
    if (select.value === "default") {
      load(structuredClone(defaults));
      name.value = "";
    } else if (select.value.startsWith("user:")) {
      const p = entries[Number(select.value.slice(5))];
      name.value = p.name;
      load(structuredClone(p.params));
    }
  };
  save.onclick = async () => {
    const title = name.value.trim();
    if (!title) {
      status.textContent = "Escribe un nombre";
      name.focus();
      return;
    }
    save.disabled = true;
    try {
      const all = await api("presets", {
        action: "save",
        preset: { plugin_id: id, name: title, params: structuredClone(read()) },
      });
      populate(all, title);
      status.textContent = "Preset guardado";
    } catch (e) {
      status.textContent = e.message;
    } finally {
      save.disabled = false;
    }
  };
  remove.onclick = async () => {
    const p = entries[Number(select.value.slice(5))];
    if (!p) return;
    try {
      populate(
        await api("presets", { action: "delete", plugin_id: id, name: p.name }),
      );
      status.textContent = "Preset eliminado";
    } catch (e) {
      status.textContent = e.message;
    }
  };
  importButton.onclick = () => file.click();
  file.onchange = async () => {
    try {
      const p = JSON.parse(await file.files[0].text());
      if (p.plugin_id !== id)
        throw Error("Este preset pertenece a otro efecto");
      if (
        !p.params ||
        typeof p.params !== "object" ||
        Object.values(p.params).some((v) => !Number.isFinite(v))
      )
        throw Error("Preset inválido");
      populate(await api("presets", { action: "save", preset: p }), p.name);
      name.value = p.name;
      if (editable) load(p.params);
      status.textContent = "Preset importado";
    } catch (e) {
      status.textContent = e.message;
    }
    file.value = "";
  };
  exportButton.onclick = () =>
    downloadPreset({
      plugin_id: id,
      name: name.value.trim() || "Preset",
      params: structuredClone(read()),
    });
  root.append(
    select,
    name,
    save,
    remove,
    importButton,
    exportButton,
    file,
    status,
  );
  return root;
}
function renderEqPresets() {
  const pack = (eq) =>
    Object.fromEntries(
      eq.bands.flatMap((b, i) =>
        Object.entries(b).map(([k, v]) => [i + "." + k, v]),
      ),
    );
  $("eq-presets").replaceChildren(
    presetBar({
      id: "native/freeeq8",
      defaults: pack(defaultEq()),
      read: () => pack(currentEq()),
      load: (values) => {
        const eq = currentEq();
        eq.bands = defaultEq().bands.map((b, i) => {
          for (const [key, min, max] of [
            ["frequency", 20, 20000],
            ["gain_db", -18, 18],
            ["q", 0.1, 18],
            ["kind", 0, 5],
          ]) {
            const v = values[i + "." + key];
            if (Number.isFinite(v))
              b[key] = Math.max(
                min,
                Math.min(max, key === "kind" ? Math.round(v) : v),
              );
          }
          return b;
        });
        selectEqBand(eqBandIndex);
        editEq();
        flushEq();
      },
    }),
  );
}

let routingChannel = 0,
  routingTimer;
function routingChanged() {
  if (!routingTimer) routingTimer = setTimeout(flushRouting, 40);
  $("routing-status").textContent = "Aplicando…";
  updateSendLabels();
  const c = cfg.channels[routingChannel],
    f = $("mixer").children[routingChannel]?.querySelector(".fader input"),
    value = view < 0 ? c.gain_db : c.sends_db[view];
  if (f) {
    f.value = faderPosition(value);
    $("fader-value-c" + routingChannel).value = Number(value).toFixed(1);
  }
}
function flushRouting() {
  if (!routingTimer) return;
  clearTimeout(routingTimer);
  routingTimer = undefined;
  publish()
    .then(() => ($("routing-status").textContent = "Aplicado · en vivo"))
    .catch((e) => {
      $("routing-status").textContent = e.message;
      error(e);
    });
}
function routingLevel(value, label, change) {
  const row = el("div", "routing-level"),
    range = slider(faderPosition(value), 0, 1000, 0.1, (v) => {
      const gain = faderDb(v);
      number.value = Number(gain.toFixed(1));
      change(Number(gain.toFixed(1)));
      routingChanged();
    }),
    number = el("input");
  number.type = "number";
  number.min = -90;
  number.max = 6;
  number.step = 0.1;
  number.value = value;
  number.setAttribute("aria-label", label + " dB");
  range.setAttribute("aria-label", label);
  range.ondblclick = () => {
    range.value = faderPosition(0);
    number.value = 0;
    change(0);
    routingChanged();
  };
  number.onchange = () => {
    if (number.reportValidity()) {
      change(Number(number.value));
      range.value = faderPosition(Number(number.value));
      routingChanged();
    }
  };
  row.append(range, number, el("span", "", "dB"));
  return row;
}
function openRouting(index) {
  if (openDetachedPanel("routing-dialog", { type: "channel", index })) return;
  routingChannel = index;
  $("routing-title").textContent = `Envíos · ${cfg.channels[index].name}`;
  $("routing-status").textContent = "Cambios en vivo";
  renderRouting();
  showFloatingPanel("routing-dialog");
}
function renderRouting() {
  const c = cfg.channels[routingChannel];
  const main = el("section", "routing-main-strip"),
    info = el("div");
  info.append(
    el("h3", "", "MAIN / MASTER"),
    el("span", "hint", c.format === "mono" ? "Canal mono" : "Canal estéreo"),
  );
  const pan = rotary({
    label: "Paneo del canal",
    inputLabel: "Paneo en envíos",
    value: c.pan,
    min: -1,
    max: 1,
    defaultValue: 0,
    step: 0.01,
    colour: "#69b9c5",
    onChange: (v) => {
      c.pan = v;
      routingChanged();
    },
  });
  main.append(
    info,
    routingLevel(c.gain_db, "Nivel principal", (v) => (c.gain_db = v)),
    pan.root,
  );
  $("routing-main").replaceChildren(main);
  const available = c.sends_db.flatMap((v, i) =>
    v <= -90 ? [new Option(cfg.buses[i + 1].name, i)] : [],
  );
  $("routing-add-send").replaceChildren(
    new Option("Agregar envío…", ""),
    ...available,
  );
  $("routing-add-send").disabled = !available.length;
  $("routing-add-aux").disabled = cfg.buses.length - 1 >= limits.auxes;
  const rows = c.sends_db.flatMap((value, a) => {
    if (value <= -90) return [];
    const row = el("div", "routing-send"),
      destination = el("div", "routing-destination"),
      mode = el("select"),
      remove = el("button", "", "Quitar");
    row.dataset.aux = a;
    const mute = el(
      "button",
      "mini-toggle" + (c.send_mute[a] ? " on" : ""),
      "M",
    );
    mute.setAttribute("aria-label", `Silenciar envío ${a + 1}`);
    mute.onclick = () => {
      c.send_mute[a] = !c.send_mute[a];
      mute.classList.toggle("on", c.send_mute[a]);
      routingChanged();
    };
    destination.append(
      mute,
      el("strong", "", cfg.buses[a + 1].name),
      el(
        "span",
        "hint",
        cfg.buses[a + 1].format === "mono" ? "Mono" : "Estéreo",
      ),
    );
    mode.append(
      new Option("PRE · antes del fader", "pre"),
      new Option("POST · sigue el fader", "post"),
    );
    mode.value = c.pre[a] ? "pre" : "post";
    mode.setAttribute("aria-label", `Modo envío ${a + 1}`);
    mode.onchange = () => {
      c.pre[a] = mode.value === "pre";
      routingChanged();
    };
    remove.setAttribute("aria-label", `Quitar envío ${a + 1}`);
    remove.onclick = () => {
      c.sends_db[a] = -90;
      routingChanged();
      renderRouting();
    };
    row.append(
      destination,
      routingLevel(value, `Nivel envío ${a + 1}`, (v) => (c.sends_db[a] = v)),
      mode,
      remove,
    );
    return [row];
  });
  $("routing-sends").replaceChildren(
    ...(rows.length
      ? rows
      : [
          el(
            "p",
            "routing-empty",
            "Sin envíos auxiliares activos. Elige un destino en «Agregar envío». El canal sigue llegando al master.",
          ),
        ]),
  );
}
$("routing-add-send").onchange = () => {
  if ($("routing-add-send").value === "") return;
  cfg.channels[routingChannel].sends_db[Number($("routing-add-send").value)] =
    -12;
  routingChanged();
  renderRouting();
};
$("routing-add-aux").onclick = async () => {
  const button = $("routing-add-aux");
  button.disabled = true;
  try {
    flushRouting();
    await resizeStructure(cfg.channels.length, cfg.buses.length);
    cfg.channels[routingChannel].sends_db[cfg.buses.length - 2] = -12;
    routingChanged();
    renderRouting();
  } catch (e) {
    $("routing-status").textContent = e.message;
  } finally {
    button.disabled = false;
  }
};
$("routing-dialog").addEventListener("close", () => {
  flushRouting();
  render();
});
