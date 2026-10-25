"use strict";
// Each detached panel is another surface of the same service, never another audio process.
const panelDefinitions = {
  "eq-dialog": [980, 760],
  fx: [1120, 820],
  "compressor-dialog": [900, 740],
  "network-dialog": [1080, 850],
  "audio-settings": [700, 620],
  "latency-dialog": [880, 750],
  "calculator-dialog": [800, 640],
  "diagnostic-dialog": [1100, 850],
  "routing-dialog": [860, 720],
};
const panelQuery = new URLSearchParams(location.search);
const detachedPanelId = Object.hasOwn(panelDefinitions, panelQuery.get("panel"))
  ? panelQuery.get("panel")
  : null;
const panelWindows = new Map();
let panelMode = "windows",
  panelZ = 100,
  panelClosing = false;
try {
  panelMode =
    localStorage.getItem("aumux-panel-mode") === "floating"
      ? "floating"
      : "windows";
} catch {}
let detachedState = {};
if (detachedPanelId) {
  try {
    detachedState = JSON.parse(panelQuery.get("panel-state") || "{}");
  } catch {}
  document.body.classList.add("detached-panel");
}
function panelIdentity(id, state) {
  return [
    id,
    state.type || "",
    Number.isInteger(state.index) ? state.index : "",
    state.tool || "",
  ].join(":");
}
function detachedTitle() {
  const d = document.getElementById(detachedPanelId);
  if (d)
    document.title = `AuMux · ${d.querySelector("h2")?.textContent || "Panel"}`;
}
function openDetachedPanel(id, state = {}) {
  if (detachedPanelId === id || panelMode === "floating") return false;
  const key = panelIdentity(id, state),
    old = panelWindows.get(key);
  if (old && !old.closed) {
    old.focus();
    old.postMessage({ type: "aumux-panel-open", id, state }, location.origin);
    return true;
  }
  const url = new URL(location.href);
  url.search = "";
  url.hash = "";
  url.searchParams.set("panel", id);
  url.searchParams.set("panel-state", JSON.stringify(state));
  const [width, height] = panelDefinitions[id];
  const child = window.open(
    url.href,
    "_blank",
    `popup=yes,width=${Math.min(width, screen.availWidth || width)},height=${Math.min(height, screen.availHeight || height)},resizable=yes,scrollbars=yes`,
  );
  if (!child) {
    document.getElementById("error").textContent =
      "El navegador bloqueó la ventana. Se abrió como panel flotante; permite ventanas emergentes para este sitio si quieres separarla.";
    return false;
  }
  panelWindows.set(key, child);
  return true;
}
function showFloatingPanel(id) {
  const dialog = document.getElementById(id);
  if (dialog.open && dialog.matches(":modal")) dialog.close();
  dialog.classList.add("floating-panel");
  if (!dialog.open) dialog.show();
  dialog.style.zIndex = String(++panelZ);
  if (detachedPanelId === id) {
    dialog.classList.add("detached-active");
    Object.assign(dialog.style, {
      left: "0",
      top: "0",
      right: "auto",
      bottom: "auto",
      margin: "0",
      width: "100vw",
      maxWidth: "100vw",
      height: "100dvh",
      maxHeight: "100dvh",
      resize: "none",
    });
    detachedTitle();
  } else if (!dialog.dataset.positioned) {
    const rect = dialog.getBoundingClientRect();
    dialog.style.left = Math.max(8, (innerWidth - rect.width) / 2) + "px";
    dialog.style.top = Math.max(8, (innerHeight - rect.height) / 2) + "px";
    dialog.dataset.positioned = "true";
  }
}
function currentPanelState(id) {
  if (id === "eq-dialog") return { ...eqTarget, band: eqBandIndex };
  if (id === "fx") return { ...selectedFx };
  if (id === "compressor-dialog") return { ...compTarget };
  if (id === "routing-dialog")
    return { type: "channel", index: routingChannel };
  if (id === "diagnostic-dialog") return { tool: diagnosticTool };
  if (id === "network-dialog")
    return { tab: document.getElementById(id).dataset.category || "network" };
  return {};
}
function startDetachedPanel(id, state) {
  if (["eq-dialog", "fx", "compressor-dialog", "routing-dialog"].includes(id)) {
    if (
      !["channel", "bus"].includes(state.type) ||
      !Number.isInteger(state.index) ||
      state.index < 0 ||
      state.index >=
        (state.type === "channel" ? cfg.channels.length : cfg.buses.length)
    )
      throw Error(
        "El canal o bus ya no existe. Cierra este panel y vuelve a abrirlo desde la consola.",
      );
  }
  if (id === "eq-dialog") {
    openEq(state.type, state.index);
    if (Number.isInteger(state.band) && state.band >= 0 && state.band < 8)
      selectEqBand(state.band);
  } else if (id === "fx")
    openFx(
      state.type,
      state.index,
      Number.isInteger(state.slot) ? Math.max(0, Math.min(3, state.slot)) : 0,
    );
  else if (id === "compressor-dialog") openCompressor(state.type, state.index);
  else if (id === "routing-dialog") openRouting(state.index);
  else if (id === "network-dialog") {
    document.getElementById("network-settings").click();
    if (
      ["network", "console", "mix", "listen", "audio-network"].includes(
        state.tab,
      )
    )
      showPreference(state.tab);
  } else if (id === "latency-dialog") openLatency();
  else if (id === "calculator-dialog")
    document.getElementById("open-calculator").click();
  else if (id === "audio-settings") document.getElementById("settings").click();
  else if (id === "diagnostic-dialog") {
    if (
      ![
        "loopback",
        "stability",
        "comparison",
        "report",
        "feedback",
        "phase",
        "scope",
      ].includes(state.tool)
    )
      throw Error("Herramienta desconocida");
    openDiagnostic(state.tool);
  }
}
function refreshOpenPanels() {
  updatePluginLibrarySelection();
  if (quickFx.open) renderQuickFx();
  if (panelTextEditing()) return;
  if (document.getElementById("eq-dialog").open && eqTarget) {
    document.getElementById("eq-title").textContent =
      `FreeEQ8 · ${targetOf(eqTarget.type, eqTarget.index).name}`;
    refreshEq();
  }
  if (document.getElementById("compressor-dialog").open && compTarget) {
    const c = currentComp();
    document.getElementById("comp-bypass").checked = c.bypass;
    [
      "threshold",
      "ratio",
      "attack",
      "release",
      "knee",
      "makeup",
      "mix",
      "highpass",
    ].forEach((key, i) => compKnobs[i]?.set(c[key]));
    compGraph();
  }
  if (document.getElementById("fx").open && selectedFx && pendingFx) {
    const authoritative = (
      selectedFx.type === "channel" ? serverConfig.channels : serverConfig.buses
    )[selectedFx.index];
    const actual = authoritative?.inserts[selectedFx.slot];
    if (
      fxPanelBase &&
      (!actual ||
        actual.id !== fxPanelBase.id ||
        actual.path !== fxPanelBase.path)
    ) {
      clearTimeout(fxAutoTimer);
      fxAutoTimer = undefined;
      pendingFx = null;
      fxPanelBase = null;
      targetOf(selectedFx.type, selectedFx.index).inserts = structuredClone(
        authoritative?.inserts || [],
      );
      selectedFx.slot = Math.min(
        selectedFx.slot,
        targetOf(selectedFx.type, selectedFx.index).inserts.length,
      );
      document
        .getElementById("params")
        .replaceChildren(
          el(
            "p",
            "hint",
            "La cadena cambió desde otra ventana. Selecciona un insert de la lista para continuar.",
          ),
        );
      document.getElementById("fx-presets").replaceChildren();
      document.getElementById("applyfx").disabled = true;
      document.getElementById("fx-edit-status").textContent =
        "Cadena actualizada desde otra ventana";
      renderFxRail();
      return;
    }
    // Preserve unsaved local values; merge only fields untouched since the editor snapshot.
    if (
      actual &&
      actual.id === pendingFx.id &&
      actual.path === pendingFx.path &&
      fxPanelBase
    ) {
      function merge(target, base, next) {
        for (const [k, n] of Object.entries(next)) {
          if (n && typeof n === "object" && target[k] && base?.[k])
            merge(target[k], base[k], n);
          else if (JSON.stringify(target[k]) === JSON.stringify(base?.[k]))
            target[k] = structuredClone(n);
        }
      }
      const before = JSON.stringify(pendingFx);
      merge(pendingFx, fxPanelBase, actual);
      fxPanelBase = structuredClone(actual);
      if (before !== JSON.stringify(pendingFx)) {
        const descriptor = fxMetadata.get(
          descriptorKey(actual.path, actual.id),
        );
        if (descriptor) renderFxControls(descriptor);
      }
    }
    const native = pendingFx?.id.startsWith("native/");
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
      document.getElementById(id).disabled = running;
    if (!native) {
      for (const input of document
        .getElementById("params")
        .querySelectorAll("input"))
        input.disabled = running;
    }
    document.getElementById("applyfx").disabled =
      (running && !native) ||
      selectedFx.slot >
        targetOf(selectedFx.type, selectedFx.index).inserts.length;
    renderFxRail();
  }
  if (detachedPanelId) detachedTitle();
}
for (const id of Object.keys(panelDefinitions)) {
  const dialog = document.getElementById(id),
    heading = dialog.querySelector(".dialog-heading");
  if (!heading) continue;
  const button = document.createElement("button");
  button.type = "button";
  button.className = "panel-detach";
  button.textContent = "↗";
  button.title = "Abrir en ventana independiente";
  button.setAttribute("aria-label", "Abrir en ventana independiente");
  button.onclick = async () => {
    try {
      if (id === "fx") {
        await flushNativeFx();
        await savePendingFx();
      }
      if (id === "eq-dialog") flushEq();
      if (id === "compressor-dialog") flushComp();
      await queue;
      const mode = panelMode;
      panelMode = "windows";
      const opened = openDetachedPanel(id, currentPanelState(id));
      panelMode = mode;
      if (opened) dialog.close();
    } catch (e) {
      error(e);
    }
  };
  if (detachedPanelId === id) button.hidden = true;
  heading.insertBefore(button, heading.lastElementChild);
  dialog.addEventListener(
    "pointerdown",
    () => (dialog.style.zIndex = String(++panelZ)),
  );
  heading.addEventListener("pointerdown", (event) => {
    if (
      detachedPanelId === id ||
      event.button !== 0 ||
      event.target.closest("button,input,select,a")
    )
      return;
    const rect = dialog.getBoundingClientRect(),
      x = event.clientX,
      y = event.clientY;
    heading.setPointerCapture(event.pointerId);
    event.preventDefault();
    const move = (e) => {
      dialog.style.left =
        Math.max(0, Math.min(innerWidth - 80, rect.left + e.clientX - x)) +
        "px";
      dialog.style.top =
        Math.max(0, Math.min(innerHeight - 40, rect.top + e.clientY - y)) +
        "px";
    };
    const stop = () => {
      heading.removeEventListener("pointermove", move);
      heading.removeEventListener("pointerup", stop);
      heading.removeEventListener("pointercancel", stop);
    };
    heading.addEventListener("pointermove", move);
    heading.addEventListener("pointerup", stop);
    heading.addEventListener("pointercancel", stop);
  });
  dialog.addEventListener("close", () => {
    if (detachedPanelId === id && !panelClosing) {
      panelClosing = true;
      queueMicrotask(async () => {
        try {
          flushEq();
          flushComp();
          await flushNativeFx();
          await queue;
        } catch {}
        window.close();
      });
    }
  });
}
window.addEventListener("aumux-ready", () => {
  const select = document.getElementById("panel-mode");
  select.value = panelMode;
  select.onchange = () => {
    panelMode = select.value;
    try {
      localStorage.setItem("aumux-panel-mode", panelMode);
    } catch {}
  };
  if (detachedPanelId) {
    try {
      startDetachedPanel(detachedPanelId, detachedState);
    } catch (e) {
      document.body.classList.remove("detached-panel");
      error(e);
    }
  }
});
window.addEventListener("message", (event) => {
  if (
    event.origin !== location.origin ||
    event.source !== window.opener ||
    event.data?.type !== "aumux-panel-open" ||
    event.data.id !== detachedPanelId
  )
    return;
  try {
    if (detachedPanelId === "fx" && event.data.state.slot !== selectedFx?.slot)
      chooseFx(event.data.state.slot);
    detachedTitle();
  } catch (e) {
    error(e);
  }
});
window.addEventListener("beforeunload", () => {
  if (!detachedPanelId || !cfg) return;
  try {
    document.activeElement?.blur();
    flushEq();
    flushComp();
    flushNativeFx();
    if (document.getElementById("fx").open && pendingFx)
      savePendingFx().catch(() => {});
    const body = JSON.stringify({ config: cfg, base: serverConfig });
    if (
      body.length < 60000 &&
      JSON.stringify(cfg) !== JSON.stringify(serverConfig)
    )
      fetch("/api/config", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "X-Monitor-Token": token,
        },
        body,
        keepalive: true,
      }).catch(() => {});
  } catch {}
});
window.addEventListener("resize", () => {
  if (detachedPanelId) return;
  for (const dialog of document.querySelectorAll(
    "dialog.floating-panel[open]",
  )) {
    const r = dialog.getBoundingClientRect();
    dialog.style.left = Math.max(0, Math.min(r.left, innerWidth - 80)) + "px";
    dialog.style.top = Math.max(0, Math.min(r.top, innerHeight - 40)) + "px";
  }
});

window.addEventListener("storage", (event) => {
  if (event.key === "aumux-panel-mode") {
    panelMode = event.newValue === "floating" ? "floating" : "windows";
    document.getElementById("panel-mode").value = panelMode;
  }
});

document.addEventListener("keydown", (event) => {
  if (
    event.key !== "Escape" ||
    event.defaultPrevented ||
    document.querySelector("dialog:modal") ||
    document.getElementById("tools-menu").open
  )
    return;
  const dialogs = [
    ...document.querySelectorAll("dialog.floating-panel[open]"),
  ].sort((a, b) => Number(b.style.zIndex) - Number(a.style.zIndex));
  const top = dialogs[0];
  if (!top) return;
  event.preventDefault();
  if (top.id === "fx") closeFx();
  else top.close();
});

let panelDraftInput;
function panelTextEditing() {
  return !!(
    panelDraftInput &&
    panelDraftInput.isConnected &&
    document.activeElement === panelDraftInput
  );
}
document.addEventListener("input", (event) => {
  if (
    event.target.matches(
      'input[type="number"],input[type="text"],input[type="search"],textarea',
    )
  )
    panelDraftInput = event.target;
});
document.addEventListener("change", (event) => {
  if (event.target === panelDraftInput && event.target.checkValidity()) {
    panelDraftInput = undefined;
  }
});
document.addEventListener("focusout", (event) => {
  if (event.target === panelDraftInput) panelDraftInput = undefined;
});
