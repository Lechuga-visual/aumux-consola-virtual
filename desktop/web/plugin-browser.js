"use strict";
// Both plugin browsers share metadata only. Audio processing stays in the engine.
const pluginGroups = [
  ["dynamics", "Dinámica"],
  ["eq", "EQ y filtros"],
  ["delay", "Delay y reverb"],
  ["modulation", "Modulación"],
  ["distortion", "Distorsión"],
  ["voice", "Voz y pitch"],
  ["tools", "Herramientas"],
  ["other", "Sin clasificar"],
];
const nativePluginGroups = {
  dynamics: "dynamics",
  limiter: "dynamics",
  envelope: "dynamics",
  loudness: "eq",
  rezfilter: "eq",
  ambience: "delay",
  delay: "delay",
  ringmod: "modulation",
  stereo: "modulation",
  image: "modulation",
  bandisto: "distortion",
  degrade: "distortion",
  overdrive: "distortion",
  detune: "voice",
  subsynth: "voice",
  dither: "tools",
  splitter: "tools",
  testtone: "tools",
  beatbox: "tools",
  shepard: "tools",
};
const pluginStorageKey = "aumux-plugin-library-v1";
let pluginPrefs = readPluginPrefs(),
  pluginEntries = [],
  nativePluginEntries = [],
  pluginCatalogueRequest,
  pluginBusy = false,
  quickFxContext,
  quickFxCategory = null,
  browserPluginCategory = "all",
  browserPluginKey = "",
  quickFxAnchor;
function readPluginPrefs() {
  try {
    const v = JSON.parse(localStorage.getItem(pluginStorageKey) || "{}");
    return {
      favorites: Array.isArray(v.favorites)
        ? v.favorites.filter((k) => typeof k === "string").slice(0, 1024)
        : [],
      recents: Array.isArray(v.recents)
        ? v.recents.filter((k) => typeof k === "string").slice(0, 12)
        : [],
      categories:
        v.categories && typeof v.categories === "object" ? v.categories : {},
      known: Array.isArray(v.known)
        ? v.known
            .filter(
              (p) =>
                typeof p.path === "string" &&
                typeof p.id === "string" &&
                typeof p.name === "string",
            )
            .slice(0, 1024)
        : [],
    };
  } catch {
    return { favorites: [], recents: [], categories: {}, known: [] };
  }
}
function savePluginPrefs() {
  try {
    localStorage.setItem(pluginStorageKey, JSON.stringify(pluginPrefs));
  } catch {}
}
function newPluginInstance() {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}
const pluginKey = (p) => `${p.path}\n${p.id || ""}`;
const pluginText = (value) =>
  String(value)
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase();
function pluginCategory(p) {
  const override =
    pluginPrefs.categories[pluginKey(p)] ||
    pluginPrefs.categories[`${p.path}\n`];
  if (pluginGroups.some(([id]) => id === override)) return override;
  if (p.format === "native")
    return p.id === "native/freevox8"
      ? "voice"
      : nativePluginGroups[p.id.split("/").pop()] || "other";
  const tags = (p.sub_categories || []).map(pluginText).join(" ");
  if (/dynamics|compress|limit|gate|expander|transient/.test(tags))
    return "dynamics";
  if (/\beq\b|equaliz|filter/.test(tags)) return "eq";
  if (/delay|reverb/.test(tags)) return "delay";
  if (/modulation|chorus|flanger|phaser|tremolo|spatial|surround/.test(tags))
    return "modulation";
  if (/distortion|saturat|overdrive/.test(tags)) return "distortion";
  if (/pitch|vocal|vocoder/.test(tags)) return "voice";
  if (/analy[sz]|meter|tools|restoration|generator|network/.test(tags))
    return "tools";
  return "other";
}
const pluginCategoryLabel = (p) =>
  pluginGroups.find(([id]) => id === pluginCategory(p))[1];
function isFavoritePlugin(p) {
  return (
    pluginPrefs.favorites.includes(pluginKey(p)) ||
    pluginPrefs.favorites.includes(`${p.path}\n`)
  );
}
function pluginCatalogueUpdated() {
  const vst = vstCatalogue.flatMap((bundle) => {
    const known = pluginPrefs.known.filter((p) => p.path === bundle.path);
    const classes = known.length ? known : bundle.classes || [];
    return (classes.length ? classes : [bundle]).map((p) => ({
      ...p,
      path: bundle.path,
      vendor: p.vendor || "Fabricante sin declarar",
      format: "vst3",
    }));
  });
  pluginEntries = [...nativePluginEntries, ...vst].sort((a, b) =>
    a.name.localeCompare(b.name),
  );
  renderPluginLibrary();
  if (quickFx.open) renderQuickFx();
}
async function ensurePluginCatalogue(rescan = false) {
  if (pluginCatalogueRequest) return pluginCatalogueRequest;
  pluginCatalogueRequest = (async () => {
    const results = await Promise.allSettled([
      nativeCatalogue().then((list) => {
        nativePluginEntries = list.map((p) => ({
          ...p,
          path: `native:${p.id}`,
          format: "native",
          vendor: p.id === "native/freevox8" ? "FreeVox8" : "MDA",
        }));
        pluginCatalogueUpdated();
      }),
      rescan || !catalogueLoaded ? scanVst3() : Promise.resolve(),
    ]);
    pluginCatalogueUpdated();
    const failed = results.filter((r) => r.status === "rejected");
    if (failed.length)
      throw Error(failed.map((r) => r.reason.message || r.reason).join(" · "));
  })().finally(() => {
    pluginCatalogueRequest = undefined;
  });
  return pluginCatalogueRequest;
}
function rememberPluginDescriptors(path, list) {
  if (!path || path.startsWith("native:")) return;
  pluginPrefs.known = [
    ...pluginPrefs.known.filter((p) => p.path !== path),
    ...list.map((p) => ({
      path,
      id: p.id,
      name: p.name,
      vendor: p.vendor || "",
      sub_categories: p.sub_categories || [],
    })),
  ].slice(-1024);
  savePluginPrefs();
  pluginCatalogueUpdated();
}
function matchingPlugins(category, query, format = "all", vendor = "") {
  const words = pluginText(query).trim().split(/\s+/).filter(Boolean);
  let entries = pluginEntries.filter(
    (p) =>
      (format === "all" || p.format === format) &&
      (!vendor || p.vendor === vendor) &&
      words.every((w) =>
        pluginText(
          `${p.name} ${p.vendor} ${pluginCategoryLabel(p)} ${(p.sub_categories || []).join(" ")}`,
        ).includes(w),
      ),
  );
  if (category === "favorites") entries = entries.filter(isFavoritePlugin);
  else if (category === "recents")
    entries = entries
      .filter((p) => pluginPrefs.recents.includes(pluginKey(p)))
      .sort(
        (a, b) =>
          pluginPrefs.recents.indexOf(pluginKey(a)) -
          pluginPrefs.recents.indexOf(pluginKey(b)),
      );
  else if (category !== "all")
    entries = entries.filter((p) => pluginCategory(p) === category);
  return entries;
}
function pluginCategoryButtons(
  root,
  active,
  query,
  select,
  format = "all",
  vendor = "",
) {
  root.replaceChildren();
  for (const [id, name] of [
    ["all", "Todos"],
    ["favorites", "☆ Favoritos"],
    ["recents", "Recientes"],
    ...pluginGroups,
  ]) {
    const count = matchingPlugins(id, query, format, vendor).length;
    const button = el(
      "button",
      "plugin-category" + (id === active ? " active" : ""),
    );
    button.type = "button";
    button.dataset.category = id;
    button.setAttribute("aria-pressed", id === active);
    button.append(
      el("span", "", name),
      el("span", "plugin-count", String(count)),
    );
    button.onclick = () => select(id);
    root.append(button);
  }
}
function togglePluginFavorite(p) {
  const key = pluginKey(p);
  if (isFavoritePlugin(p))
    pluginPrefs.favorites = pluginPrefs.favorites.filter(
      (k) => k !== key && k !== `${p.path}\n`,
    );
  else pluginPrefs.favorites.push(key);
  savePluginPrefs();
  renderPluginLibrary();
  if (quickFx.open) renderQuickFx();
}
function pluginContext(type, index, slot) {
  const node = targetOf(type, index);
  if (!node || slot > node.inserts.length || slot >= (limits.inserts || 8))
    throw Error("La cadena no tiene un espacio disponible");
  return { type, index, slot, chain: structuredClone(node.inserts) };
}
function checkPluginContext(context) {
  const node = targetOf(context.type, context.index);
  if (!node || !configEqual(node.inserts, context.chain))
    throw Error(
      "La cadena cambió desde otra ventana. Vuelve a abrir el selector.",
    );
  return node;
}
async function insertCataloguePlugin(entry, context) {
  if (pluginBusy) return false;
  pluginBusy = true;
  try {
    checkPluginContext(context);
    const list =
      entry.format === "native"
        ? [
            {
              ...entry,
              params: entry.params.map((p) => ({ ...p, value: p.default })),
            },
          ]
        : await api("inspect", { path: entry.path });
    if (entry.format !== "native") rememberPluginDescriptors(entry.path, list);
    const d = entry.id
      ? list.find((p) => p.id === entry.id)
      : list.length === 1
        ? list[0]
        : null;
    if (!d) {
      if (!list.length)
        throw Error("El bundle no contiene efectos de audio compatibles");
      if (entry.id)
        throw Error("Ese efecto ya no está en el bundle. Actualiza la lista.");
      throw Error(
        "El bundle contiene varios efectos. Elige uno de la lista actualizada.",
      );
    }
    const node = checkPluginContext(context),
      before = structuredClone(node.inserts);
    node.inserts[context.slot] = {
      instance: newPluginInstance(),
      path: entry.path,
      id: d.id,
      name: d.name,
      bypass: false,
      params: Object.fromEntries(d.params.map((p) => [p.id, p.value])),
    };
    const sent = structuredClone(node.inserts);
    try {
      await publish();
    } catch (e) {
      if (configEqual(node.inserts, sent)) node.inserts = before;
      throw e;
    }
    const key = pluginKey({ path: entry.path, id: d.id });
    pluginPrefs.recents = [
      key,
      ...pluginPrefs.recents.filter((k) => k !== key),
    ].slice(0, 12);
    savePluginPrefs();
    fxMetadata.set(descriptorKey(entry.path, d.id), d);
    render();
    return true;
  } finally {
    pluginBusy = false;
  }
}

// A: small, anchored menu. The browser top layer prevents clipping by mixer scroll areas.
const quickFx = el("div", "quick-fx");
quickFx.id = "quick-fx";
quickFx.setAttribute("popover", "auto");
quickFx.setAttribute("role", "dialog");
quickFx.setAttribute("aria-label", "Añadir plugin rápidamente");
quickFx.innerHTML =
  '<div class="quick-fx-heading"></div><input id="quick-fx-search" type="search" placeholder="Buscar plugin o función…" aria-label="Buscar plugin rápido"><div class="quick-fx-columns"><nav id="quick-fx-categories" aria-label="Categorías rápidas"></nav><div id="quick-fx-results" role="group" aria-label="Efectos rápidos"></div></div><p id="quick-fx-status" role="status"></p><button id="quick-fx-more">Más plugins… · navegador completo</button>';
document.body.append(quickFx);
Object.defineProperty(quickFx, "open", {
  get: () => quickFx.matches(":popover-open"),
});
function positionQuickFx() {
  if (!quickFx.open || !quickFxAnchor) return;
  const box = quickFx.getBoundingClientRect(),
    maxX = Math.max(4, innerWidth - box.width - 6);
  const below = quickFxAnchor.bottom + 4;
  quickFx.style.left = Math.max(4, Math.min(quickFxAnchor.left, maxX)) + "px";
  quickFx.style.top =
    Math.max(4, Math.min(below, innerHeight - box.height - 6)) + "px";
}
function openQuickFx(anchor, type, index, slot) {
  quickFxContext = pluginContext(type, index, slot);
  quickFxAnchor = anchor.getBoundingClientRect();
  quickFxCategory = null;
  $("quick-fx-search").value = "";
  quickFx.querySelector(".quick-fx-heading").textContent =
    `AÑADIR FX · ${targetOf(type, index).name}`;
  $("quick-fx-status").textContent = "Cargando catálogo…";
  if (!quickFx.open) quickFx.showPopover();
  renderQuickFx();
  positionQuickFx();
  $("quick-fx-search").focus();
  ensurePluginCatalogue()
    .then(() => {
      $("quick-fx-status").textContent =
        "Un clic para añadir · Esc para cerrar";
      positionQuickFx();
    })
    .catch((e) => {
      $("quick-fx-status").textContent = e.message;
    });
}
function renderQuickFx() {
  if (!quickFx.open) return;
  const query = $("quick-fx-search").value;
  pluginCategoryButtons(
    $("quick-fx-categories"),
    quickFxCategory,
    query,
    (category) => {
      quickFxCategory = category;
      renderQuickFx();
      positionQuickFx();
    },
  );
  for (const button of $("quick-fx-categories").querySelectorAll("button"))
    button.onpointerenter = (e) => {
      if (
        e.pointerType === "mouse" &&
        quickFxCategory !== button.dataset.category
      ) {
        quickFxCategory = button.dataset.category;
        renderQuickFx();
        positionQuickFx();
      }
    };
  const showResults = !!query || quickFxCategory !== null;
  quickFx.classList.toggle("has-results", showResults);
  const root = $("quick-fx-results"),
    entries = showResults
      ? matchingPlugins(quickFxCategory || "all", query)
      : [];
  root.hidden = !showResults;
  root.replaceChildren();
  if (showResults && !entries.length)
    root.append(el("p", "plugin-empty", "No hay plugins en esta selección"));
  for (const entry of entries) {
    const button = el("button", "quick-fx-plugin");
    button.type = "button";
    button.dataset.pluginId = entry.id || "";
    button.title = `${entry.name} · ${entry.vendor}`;
    button.append(
      el("span", "", (isFavoritePlugin(entry) ? "★ " : "") + entry.name),
      el("span", "plugin-tag", entry.format === "native" ? "NATIVO" : "VST3"),
    );
    button.disabled = pluginBusy;
    button.onclick = async () => {
      const context = quickFxContext;
      $("quick-fx-status").textContent = "Añadiendo…";
      try {
        if (await insertCataloguePlugin(entry, context)) {
          if (quickFxContext === context && quickFx.open) quickFx.hidePopover();
        }
      } catch (e) {
        $("quick-fx-status").textContent = e.message;
      }
    };
    root.append(button);
  }
}
$("quick-fx-search").oninput = () => {
  renderQuickFx();
  positionQuickFx();
};
$("quick-fx-more").onclick = () => {
  const context = quickFxContext;
  quickFx.hidePopover();
  openFx(context.type, context.index, context.slot);
};
quickFx.addEventListener("keydown", (e) => {
  const buttons = [...quickFx.querySelectorAll("button:not(:disabled), input")];
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    const index = buttons.indexOf(document.activeElement),
      delta = e.key === "ArrowDown" ? 1 : -1;
    buttons[(index + delta + buttons.length) % buttons.length]?.focus();
  }
  if (e.key === "Escape") {
    e.stopPropagation();
    quickFx.hidePopover();
  }
});
window.addEventListener("resize", positionQuickFx);
document.querySelector(".mixer-area")?.addEventListener(
  "scroll",
  () => {
    if (quickFx.open) quickFx.hidePopover();
  },
  true,
);

// B: full catalogue in the FX window, separate from the active insert chain.
function renderPluginLibrary() {
  const root = $("plugin-results");
  if (!root) return;
  const oldVendor = $("plugin-vendor").value;
  $("plugin-vendor").replaceChildren(
    new Option("Todos los fabricantes", ""),
    ...[...new Set(pluginEntries.map((p) => p.vendor))]
      .sort()
      .map((v) => new Option(v, v)),
  );
  $("plugin-vendor").value = oldVendor;
  const format = $("plugin-format").value,
    vendor = $("plugin-vendor").value,
    query = $("plugin-search").value;
  pluginCategoryButtons(
    $("plugin-categories"),
    browserPluginCategory,
    query,
    (category) => {
      browserPluginCategory = category;
      renderPluginLibrary();
    },
    format,
    vendor,
  );
  const entries = matchingPlugins(browserPluginCategory, query, format, vendor);
  const scroll = root.scrollTop;
  root.replaceChildren();
  if (!entries.some((p) => pluginKey(p) === browserPluginKey))
    browserPluginKey = "";
  if (!entries.length)
    root.append(
      el("p", "plugin-empty", "No hay plugins que coincidan con los filtros"),
    );
  for (const entry of entries) {
    const row = el("div", "plugin-result"),
      button = el("button", "plugin-result-name"),
      star = el(
        "button",
        "plugin-favorite",
        isFavoritePlugin(entry) ? "★" : "☆",
      );
    button.type = star.type = "button";
    button.dataset.pluginId = entry.id || "";
    button.setAttribute("aria-pressed", pluginKey(entry) === browserPluginKey);
    button.append(
      el("strong", "", entry.name),
      el(
        "span",
        "",
        `${pluginCategoryLabel(entry)} · ${entry.vendor} · ${entry.format === "native" ? "Nativo" : "VST3"}`,
      ),
    );
    button.onclick = () => {
      browserPluginKey = pluginKey(entry);
      for (const other of root.querySelectorAll(".plugin-result-name"))
        other.setAttribute("aria-pressed", other === button);
      updatePluginLibrarySelection();
    };
    button.ondblclick = () => useBrowserPlugin();
    button.onkeydown = (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        useBrowserPlugin();
      }
    };
    star.setAttribute("aria-label", `Favorito: ${entry.name}`);
    star.setAttribute("aria-pressed", isFavoritePlugin(entry));
    star.onclick = () => togglePluginFavorite(entry);
    row.append(button, star);
    root.append(row);
  }
  root.scrollTop = scroll;
  updatePluginLibrarySelection();
  $("plugin-library-status").textContent =
    `${entries.length} efectos · selecciona y añade; doble clic o Enter para acceso rápido`;
}
function updatePluginLibrarySelection() {
  const selected = pluginEntries.find((p) => pluginKey(p) === browserPluginKey);
  $("plugin-use").disabled = !selected || pluginBusy;
  $("plugin-use").textContent =
    selectedFx &&
    targetOf(selectedFx.type, selectedFx.index)?.inserts[selectedFx.slot]
      ? "Reemplazar insert"
      : "Añadir";
  $("plugin-category-override").replaceChildren(
    new Option("Según el plugin", ""),
    ...pluginGroups.map(([id, label]) => new Option(label, id)),
  );
  $("plugin-category-override").disabled = !selected;
  $("plugin-category-override").value = selected
    ? pluginPrefs.categories[pluginKey(selected)] || ""
    : "";
}
async function useBrowserPlugin() {
  const entry = pluginEntries.find((p) => pluginKey(p) === browserPluginKey);
  if (!entry || !selectedFx || pluginBusy) return;
  const selection = { ...selectedFx };
  try {
    if (targetOf(selection.type, selection.index).inserts[selection.slot]) {
      await flushNativeFx();
      await savePendingFx();
    }
    if (
      selectedFx.type !== selection.type ||
      selectedFx.index !== selection.index ||
      selectedFx.slot !== selection.slot
    )
      throw Error("La selección cambió; vuelve a elegir el plugin");
    $("plugin-library-status").textContent = "Añadiendo…";
    const context = pluginContext(
      selection.type,
      selection.index,
      selection.slot,
    );
    if (await insertCataloguePlugin(entry, context)) {
      if (
        selectedFx.type === selection.type &&
        selectedFx.index === selection.index &&
        selectedFx.slot === selection.slot
      ) {
        pendingFx = null;
        openFx(selection.type, selection.index, selection.slot);
      }
    }
  } catch (e) {
    $("plugin-library-status").textContent = e.message;
  }
}
$("plugin-use").onclick = useBrowserPlugin;
$("plugin-search").oninput = renderPluginLibrary;
$("plugin-format").onchange = renderPluginLibrary;
$("plugin-vendor").onchange = renderPluginLibrary;
$("plugin-category-override").onchange = () => {
  if (!browserPluginKey) return;
  if ($("plugin-category-override").value)
    pluginPrefs.categories[browserPluginKey] = $(
      "plugin-category-override",
    ).value;
  else delete pluginPrefs.categories[browserPluginKey];
  savePluginPrefs();
  renderPluginLibrary();
};
$("plugin-rescan").onclick = () => {
  $("plugin-library-status").textContent = "Actualizando catálogo…";
  ensurePluginCatalogue(true).catch((e) => {
    $("plugin-library-status").textContent = e.message;
  });
};
$("fx-browser").addEventListener("toggle", () => {
  if ($("fx-browser").open)
    ensurePluginCatalogue().catch((e) => {
      $("plugin-library-status").textContent = e.message;
    });
});
window.addEventListener("storage", (e) => {
  if (e.key === pluginStorageKey) {
    pluginPrefs = readPluginPrefs();
    pluginCatalogueUpdated();
  }
});
