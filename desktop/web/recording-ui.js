"use strict";
let recordChoices = { channels: [], buses: [0], directory: "" },
  recordSignature = "",
  recordBusy = false;
try {
  const saved = JSON.parse(
    localStorage.getItem("aumux-recording-choices") || "null",
  );
  if (saved && Array.isArray(saved.channels) && Array.isArray(saved.buses))
    recordChoices = saved;
} catch {}
function saveRecordChoices() {
  try {
    localStorage.setItem(
      "aumux-recording-choices",
      JSON.stringify(recordChoices),
    );
  } catch {}
}
function openRecording() {
  $("tools-menu").open = false;
  if (openDetachedPanel("recording-dialog")) return;
  recordSignature = "";
  $("recording-directory").value = recordChoices.directory || "";
  showFloatingPanel("recording-dialog");
  updateRecordingStatus(lastStatus?.recording);
}
function recordingRows(state) {
  const open = !!state?.session_open;
  const signature = JSON.stringify([
    cfg.channels.map((c) => [c.name, c.format]),
    cfg.buses.map((b) => [b.name, b.format]),
    open,
    open ? state.tracks : [],
  ]);
  if (signature === recordSignature) return;
  recordSignature = signature;
  const root = $("recording-tracks");
  root.replaceChildren();
  recordChoices.channels = cfg.channels.map(
    (c, i) => recordChoices.channels[i] || { enabled: true, point: "pre" },
  );
  for (const [kind, nodes] of [
    ["channel", cfg.channels],
    ["bus", cfg.buses],
  ])
    nodes.forEach((node, index) => {
      const track = open
        ? state.tracks?.find((t) => t.kind === kind && t.index === index)
        : null;
      const choice = kind === "channel" ? recordChoices.channels[index] : null;
      const checked = open
        ? !!track
        : kind === "channel"
          ? choice.enabled
          : recordChoices.buses.includes(index);
      const row = el("tr"),
        arm = el("input"),
        cell = el("td"),
        name = el(
          "td",
          "",
          kind === "bus" && index === 0 ? `${node.name} · MASTER` : node.name,
        );
      arm.type = "checkbox";
      arm.checked = checked;
      arm.disabled = open;
      arm.dataset.recordKind = kind;
      arm.dataset.recordIndex = index;
      arm.setAttribute(
        "aria-label",
        `Grabar ${kind === "channel" ? "canal" : "bus"} ${index + 1}: ${node.name}`,
      );
      arm.onchange = () => {
        if (kind === "channel") choice.enabled = arm.checked;
        else
          recordChoices.buses = arm.checked
            ? [...new Set([...recordChoices.buses, index])]
            : recordChoices.buses.filter((i) => i !== index);
        saveRecordChoices();
      };
      cell.append(arm);
      row.append(
        cell,
        name,
        el("td", "", node.format === "mono" ? "Mono" : "Estéreo"),
      );
      const pointCell = el("td");
      if (kind === "channel") {
        const point = el("select");
        point.append(
          new Option("PRE · entrada", "pre"),
          new Option("POST · fader", "post"),
        );
        point.value = open ? track?.point || choice.point : choice.point;
        point.disabled = open;
        point.dataset.recordPoint = index;
        point.setAttribute(
          "aria-label",
          `Punto de grabación canal ${index + 1}`,
        );
        point.onchange = () => {
          choice.point = point.value;
          saveRecordChoices();
        };
        pointCell.append(point);
      } else pointCell.textContent = "POST · procesado";
      row.append(pointCell);
      root.append(row);
    });
}
function updateRecordingStatus(state) {
  if (!cfg) return;
  const open = !!state?.session_open;
  const active = !!state?.active;
  $("recording-indicator").hidden = !open;
  $("recording-indicator").textContent = state?.error
    ? "⚠ REC · error"
    : `● REC · ${Math.floor((state?.duration_seconds || 0) / 60)}:${String(Math.floor((state?.duration_seconds || 0) % 60)).padStart(2, "0")}`;
  $("recording-indicator").classList.toggle("recording-failed", !!state?.error);
  $("open-recording").textContent = active
    ? `● Grabando · ${(state.duration_seconds || 0).toFixed(0)} s`
    : "Grabación multipista";
  if (!$("recording-dialog").open) return;
  recordingRows(state);
  for (const id of [
    "recording-directory",
    "recording-point",
    "recording-all",
    "recording-none",
  ])
    $(id).disabled = open || recordBusy;
  $("recording-start").disabled = open || recordBusy || !running;
  $("recording-stop").disabled = !open || recordBusy;
  $("recording-status").textContent = state?.error
    ? `Error: ${state.error}`
    : active
      ? `● REC · ${(state.duration_seconds || 0).toFixed(1)} s · ${state.tracks?.length || 0} pistas · cola ${state.queued_packets}/${state.queue_capacity}`
      : open
        ? "Finalizando grabación…"
        : state?.state === "completed"
          ? `Finalizada · ${(state.duration_seconds || 0).toFixed(1)} s · ${state.tracks?.length || 0} pistas`
          : running
            ? "Listo para grabar las pistas seleccionadas"
            : "Inicia AUDIO PC, ASIO o DEMO para grabar";
  $("recording-status").classList.toggle("recording-active", active);
  $("recording-path").textContent = state?.directory
    ? `Sesión: ${state.directory}`
    : `Destino predeterminado: ${state?.default_directory || "Music/AuMux"}`;
}
$("open-recording").onclick = openRecording;
$("recording-indicator").onclick = openRecording;
for (const [id, enabled] of [
  ["recording-all", true],
  ["recording-none", false],
])
  $(id).onclick = () => {
    recordChoices.channels = cfg.channels.map((c, i) => ({
      enabled,
      point: recordChoices.channels[i]?.point || "pre",
    }));
    recordChoices.buses = enabled ? cfg.buses.map((b, i) => i) : [];
    recordSignature = "";
    saveRecordChoices();
    updateRecordingStatus(lastStatus?.recording);
  };
$("recording-point").onchange = () => {
  for (const choice of recordChoices.channels)
    choice.point = $("recording-point").value;
  recordSignature = "";
  saveRecordChoices();
  updateRecordingStatus(lastStatus?.recording);
};
$("recording-directory").onchange = () => {
  recordChoices.directory = $("recording-directory").value;
  saveRecordChoices();
};
for (const [id, action] of [
  ["recording-start", "start"],
  ["recording-stop", "stop"],
])
  $(id).onclick = async () => {
    recordBusy = true;
    updateRecordingStatus(lastStatus?.recording);
    try {
      recordChoices.directory = $("recording-directory").value;
      saveRecordChoices();
      const state = await api("recording", {
        action,
        directory: recordChoices.directory,
        channels: recordChoices.channels,
        buses: recordChoices.buses.filter((i) => i < cfg.buses.length),
      });
      lastStatus.recording = state;
      recordSignature = "";
      recordBusy = false;
      updateRecordingStatus(state);
    } catch (e) {
      recordBusy = false;
      updateRecordingStatus(lastStatus?.recording);
      $("recording-status").textContent = e.message;
    }
  };
