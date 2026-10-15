"use strict";
function applyTouchMode() {
  const mode = $("touch-mode").value;
  const active = mode === "on";
  document.body.classList.toggle("touch-mode", active);
  document
    .querySelector(".app")
    .classList.toggle(
      "touch-compact",
      active &&
        (innerHeight < 900 ||
          document.querySelector(".app").classList.contains("small-console")),
    );
  try {
    localStorage.setItem("mm-touch-mode", mode);
  } catch {}
  layoutStrips();
  positionTools();
}
try {
  $("touch-mode").value =
    localStorage.getItem("mm-touch-mode") === "on" ? "on" : "off";
} catch {}
$("touch-mode").onchange = applyTouchMode;
window.addEventListener("resize", applyTouchMode);
applyTouchMode();
$("open-calculator").onclick = () => {
  $("tools-menu").open = false;
  $("calculator-dialog").showModal();
  calculateLatency();
};
function openLatency() {
  $("tools-menu").open = false;
  $("latency-dialog").showModal();
  if (lastStatus) updateLatencyTools(lastStatus);
}
$("open-diagnostics").onclick = openLatency;
$("latency-badge").onclick = openLatency;
$("system-latency").onclick = openLatency;
function calculateLatency() {
  const ids = [
    "calc-frames",
    "calc-blocks",
    "calc-plugins",
    "calc-network",
    "calc-extra",
  ];
  if (ids.some((id) => !$(id).checkValidity())) {
    $("calc-result").textContent = "Revisa los valores";
    return;
  }
  const frames = Number($("calc-frames").value),
    blocks = Number($("calc-blocks").value),
    plugins = Number($("calc-plugins").value),
    other = Number($("calc-network").value) + Number($("calc-extra").value);
  const total = (frames * blocks + plugins) / 48 + other;
  $("calc-result").textContent =
    `${total.toFixed(2)} ms estimados · ${(total * 48).toFixed(0)} muestras · un bloque: ${(frames / 48).toFixed(2)} ms`;
}
for (const id of [
  "calc-frames",
  "calc-blocks",
  "calc-plugins",
  "calc-network",
  "calc-extra",
])
  $(id).oninput = calculateLatency;
function latencyCard(label, value) {
  const card = el("div", "latency-card");
  card.append(el("small", "", label), el("strong", "", value));
  return card;
}
const fmtMs = (v) =>
  typeof v === "number" && Number.isFinite(v)
    ? v.toFixed(2) + " ms"
    : "Sin dato";
let lastSystemMetricsUpdate = -Infinity;
function updateSystemMetrics(s) {
  const now = performance.now();
  if (now - lastSystemMetricsUpdate < 1000) return;
  lastSystemMetricsUpdate = now;
  const cpu = s.system?.cpu_percent;
  const ram = s.system?.ram_bytes;
  $("system-cpu").textContent =
    typeof cpu === "number" && Number.isFinite(cpu)
      ? `CPU: ${cpu.toFixed(1)} %`
      : "CPU: —";
  $("system-ram").textContent =
    typeof ram === "number" && Number.isFinite(ram)
      ? `RAM: ${(ram / 1048576).toFixed(0)} MiB`
      : "RAM: —";
  const driver = s.latency?.driver_ms;
  const block = s.latency?.block_ms;
  const measuredDriver = typeof driver === "number" && Number.isFinite(driver);
  const value = measuredDriver ? driver : block;
  $("system-latency").textContent =
    `${measuredDriver ? "Driver" : "Bloque"}: ${typeof value === "number" && Number.isFinite(value) ? value.toFixed(2) + " ms" : "—"}`;
  $("system-latency").title = measuredDriver
    ? "Latencia total declarada por el driver de audio. No incluye todos los efectos, OBS ni la red; no es una medición de extremo a extremo. Pulsa para ver los componentes."
    : "Duración calculada de un bloque del motor. El driver no informa latencia; este dato no es el retardo total de escucha. Pulsa para ver los componentes.";
}
let latencyRouteSignature = "";
function updateLatencyTools(s) {
  const l = s.latency;
  if (!l) return;
  $("latency-badge").textContent = `Bloque ${l.block_ms.toFixed(2)} ms`;
  if (!$("latency-dialog").open) return;
  const signature = JSON.stringify(l.buses.map((b) => [b.index, b.name]));
  if (signature !== latencyRouteSignature) {
    const old = $("latency-route").value;
    $("latency-route").replaceChildren(
      ...l.buses.map((b) => new Option(b.name, b.index)),
    );
    if ([...$("latency-route").options].some((o) => o.value === old))
      $("latency-route").value = old;
    latencyRouteSignature = signature;
  }
  const bus = l.buses[Number($("latency-route").value)] || l.buses[0];
  $("latency-details").replaceChildren(
    latencyCard(
      l.running ? "Bloque activo · calculado" : "Bloque DEMO · motor detenido",
      `${l.block_frames} muestras · ${fmtMs(l.block_ms)}`,
    ),
    latencyCard("Driver · latencia total declarada", fmtMs(l.driver_ms)),
    latencyCard(
      `Plugins del bus ${bus?.name || ""} · declarado`,
      fmtMs(bus?.plugin_ms),
    ),
    latencyCard(
      "Cola emisora OBS · cota actual",
      fmtMs(l.obs_queue_ms_upper_bound),
    ),
    latencyCard("Receptor OBS / sincronización", "Sin medición"),
    latencyCard("Consulta de control · RTT medido", fmtMs(s.control_rtt_ms)),
    latencyCard("Audio de red · ida", "Sin medición"),
    latencyCard("Extremo a extremo · audio", "Sin medición"),
  );
  const paths = el("div", "latency-paths");
  paths.append(el("strong", "", "Plugins por ruta hacia este bus"));
  for (const path of bus?.paths || [])
    paths.append(
      el(
        "div",
        "",
        `${path.name} → ${bus.name}: ${fmtMs(path.plugin_ms)} (${path.plugin_samples} muestras declaradas)`,
      ),
    );
  $("latency-details").append(paths);
}
$("latency-route").onchange = () => {
  if (lastStatus) updateLatencyTools(lastStatus);
};
let accessInitialized = false;
function updateAccessUI(s) {
  if (!s.remote) return;
  if (!accessInitialized) {
    for (const scope of ["mix", "console"])
      $(scope + "-access-mode").value =
        s.remote[scope + "_access"]?.mode || "disabled";
    $("listen-access-mode").value = s.listen.access?.mode || "disabled";
    $("mix-access-max").value = s.remote.shared_max_db ?? 0;
    $("mix-access-pre").checked = s.remote.shared_allow_pre ?? false;
    accessInitialized = true;
    updateAccessFields();
  }
  $("mix-generic-url").value =
    s.remote.enabled && s.remote.mix_access.mode !== "disabled"
      ? s.remote.generic_url
      : "";
  $("console-remote-url").value =
    s.remote.enabled && s.remote.console_access.mode !== "disabled"
      ? s.remote.console_url
      : "";
}
for (const scope of ["mix", "console", "listen"])
  $(scope + "-access-save").onclick = async () => {
    const mode = $(scope + "-access-mode").value,
      password = $(scope + "-password").value;
    const button = $(scope + "-access-save");
    button.disabled = true;
    try {
      const body = { mode, password };
      if (scope === "mix") {
        if (!$("mix-access-max").reportValidity()) return;
        body.max_db = Number($("mix-access-max").value);
        body.allow_pre = $("mix-access-pre").checked;
      }
      if (scope === "listen")
        lastStatus.listen = await api("listen-access", body);
      else
        lastStatus.remote = await api("remote", {
          action: "access",
          scope,
          ...body,
        });
      $(scope + "-password").value = "";
      updateWorkspaceStatus(lastStatus);
      $("access-status").dataset.error = "false";
      $("access-status").textContent =
        "Acceso actualizado; las sesiones por contraseña anteriores se cerraron.";
    } catch (e) {
      $("access-status").textContent = e.message;
      $("access-status").dataset.error = "true";
    } finally {
      button.disabled = false;
    }
  };
$("network-settings").addEventListener("click", async () => {
  accessInitialized = false;
  if (lastStatus) updateAccessUI(lastStatus);
  try {
    const r = await webFetch("/api/addresses");
    const data = await r.json();
    const current = $("network-host").value;
    $("network-interface").replaceChildren(
      new Option("Manual / MagicDNS", ""),
      ...data.interfaces.map((n) => {
        const option = new Option(
          `${n.name} · ${n.ip}${n.tailscale_candidate ? " · Tailscale/100.x" : ""}${n.loopback ? " · solo este PC" : ""}${n.family === "IPv6" ? " · requiere escucha IPv6" : ""}`,
          n.ip,
        );
        option.disabled = n.family === "IPv6";
        return option;
      }),
    );
    if ([...$("network-interface").options].some((o) => o.value === current))
      $("network-interface").value = current;
  } catch (e) {
    $("access-status").textContent =
      "No se pudieron enumerar las interfaces: " + e.message;
  }
});
$("network-interface").onchange = () => {
  if ($("network-interface").value)
    $("network-host").value = $("network-interface").value;
  else $("network-host").focus();
};
const desktopInspectorToggle = $("toggle-inspector").onclick;
$("toggle-inspector").onclick = () => {
  if (document.body.classList.contains("touch-mode")) {
    const open = document.body.classList.toggle("touch-inspector-open");
    $("toggle-inspector").setAttribute("aria-pressed", open);
  } else desktopInspectorToggle();
};

$("mixer-layout").addEventListener("change", applyTouchMode);

function positionTools() {
  const menu = $("tools-menu"),
    panel = menu.querySelector(".tools-dropdown");
  if (!menu.open) return;
  const rect = menu.querySelector("summary").getBoundingClientRect();
  panel.style.width = Math.min(320, innerWidth - 16) + "px";
  panel.style.maxHeight = Math.max(80, innerHeight - 16) + "px";
  const height = panel.getBoundingClientRect().height;
  const below = innerHeight - rect.bottom - 8;
  const top =
    below >= height || below >= rect.top - 8
      ? rect.bottom + 4
      : rect.top - height - 4;
  panel.style.left =
    Math.max(
      8,
      Math.min(rect.left, innerWidth - panel.getBoundingClientRect().width - 8),
    ) + "px";
  panel.style.top = Math.max(8, Math.min(top, innerHeight - height - 8)) + "px";
}
const toolsPanel = $("tools-menu").querySelector(".tools-dropdown");
toolsPanel.setAttribute("popover", "manual");
$("tools-menu").addEventListener("toggle", () => {
  if ($("tools-menu").open) {
    toolsPanel.showPopover();
    positionTools();
  } else toolsPanel.hidePopover();
});
window.addEventListener("resize", positionTools);
window.addEventListener("scroll", positionTools, true);
document.addEventListener("pointerdown", (e) => {
  if (!$("tools-menu").contains(e.target)) $("tools-menu").open = false;
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") $("tools-menu").open = false;
});
function showPreference(name) {
  for (const button of document.querySelectorAll(
    '.preferences-nav [role="tab"]',
  )) {
    const active = button.dataset.preference === name;
    button.setAttribute("aria-selected", active);
    button.tabIndex = active ? 0 : -1;
    $("pref-" + button.dataset.preference).hidden = !active;
  }
  $("network-dialog").dataset.category = name;
  $("network-dialog").querySelector(".preferences-content").scrollTop = 0;
}
for (const button of document.querySelectorAll("[data-preference]"))
  button.addEventListener("click", () =>
    showPreference(button.dataset.preference),
  );
document.querySelector(".preferences-nav").addEventListener("keydown", (e) => {
  const tabs = [...document.querySelectorAll('.preferences-nav [role="tab"]')];
  let index = tabs.indexOf(document.activeElement);
  if (index < 0) return;
  if (["ArrowDown", "ArrowRight"].includes(e.key))
    index = (index + 1) % tabs.length;
  else if (["ArrowUp", "ArrowLeft"].includes(e.key))
    index = (index + tabs.length - 1) % tabs.length;
  else if (e.key === "Home") index = 0;
  else if (e.key === "End") index = tabs.length - 1;
  else return;
  e.preventDefault();
  tabs[index].focus();
  tabs[index].click();
});

function updateAccessFields() {
  for (const scope of ["mix", "console", "listen"])
    $(scope + "-password").closest("label").hidden =
      $(scope + "-access-mode").value !== "password";
}
for (const scope of ["mix", "console", "listen"])
  $(scope + "-access-mode").addEventListener("change", updateAccessFields);
