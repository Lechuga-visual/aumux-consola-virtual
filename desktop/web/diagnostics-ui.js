"use strict";
const diagnosticOwner = `panel-${Date.now()}-${Math.random().toString(36).slice(2)}`;
let diagnosticTool = "",
  diagnosticGeneration = 0,
  diagnosticPromise = null,
  diagnosticPending = false;
function diagnosticField(label, input) {
  const item = el("label", "diagnostic-field", label);
  item.append(input);
  return item;
}
function diagnosticNumber(id, value, min, max) {
  const n = el("input");
  n.id = id;
  n.type = "number";
  n.min = min;
  n.max = max;
  n.value = value;
  return n;
}
function diagnosticButton(label, fn, id) {
  const b = el("button", "", label);
  if (id) b.id = id;
  b.onclick = async () => {
    diagnosticGeneration++;
    $("diagnostic-notice").textContent = "";
    b.disabled = true;
    try {
      await fn();
    } catch (e) {
      $("diagnostic-notice").textContent = e.message;
    } finally {
      b.disabled = false;
    }
  };
  return b;
}
function diagnosticTable(headers, rows) {
  const table = el("table", "diagnostic-table"),
    head = el("thead"),
    tr = el("tr");
  for (const h of headers) tr.append(el("th", "", h));
  head.append(tr);
  table.append(head);
  const body = el("tbody");
  for (const row of rows) {
    const tr = el("tr");
    for (const cell of row) tr.append(el("td", "", String(cell ?? "—")));
    body.append(tr);
  }
  table.append(body);
  return table;
}
function diagnosticDelta(row, history) {
  if (row.context?.kind !== "physical_route" || row.result.samples == null)
    return "—";
  if (row.context.bypass) return "Referencia";
  const keys = [
    "device",
    "buffer",
    "input",
    "output",
    "channel_index",
    "bus_index",
  ];
  const reference = history.findLast(
    (h) =>
      h.context?.kind === "physical_route" &&
      h.context.bypass &&
      h.result.samples != null &&
      keys.every((k) => h.context[k] === row.context[k]),
  );
  if (!reference) return "Sin referencia equivalente";
  const delta = row.result.samples - reference.result.samples;
  return `${delta >= 0 ? "+" : ""}${(delta / 48).toFixed(3)} ms (${delta} muestras)`;
}
function diagnosticMillis(n) {
  return typeof n === "number" ? n.toFixed(3) + " ms" : "Sin medición";
}
function openDiagnostic(tool) {
  if (openDetachedPanel("diagnostic-dialog", { tool })) {
    $("tools-menu").open = false;
    return;
  }
  diagnosticTool = tool;
  diagnosticGeneration++;
  $("tools-menu").open = false;
  $("diagnostic-notice").textContent = "";
  $("diagnostic-message").textContent = "";
  $("diagnostic-controls").replaceChildren();
  $("diagnostic-results").replaceChildren();
  delete $("diagnostic-results").dataset.feedbackSignature;
  $("diagnostic-title").textContent = {
    loopback: "Medidor de latencia loopback",
    stability: "Buffers y estabilidad",
    comparison: "Comparador de rutas y EFX",
    report: "Informe de pruebas",
    feedback: "Detector / eliminador de acoples",
    phase: "Detector de fase",
    scope: "Osciloscopio",
  }[tool];
  showFloatingPanel("diagnostic-dialog");
  if (tool === "loopback") {
    $("diagnostic-controls").append(
      el(
        "p",
        "hint",
        "Requiere ASIO activo a 48 kHz. Conecta con un cable la salida elegida a una entrada de línea. Durante cada captura se aíslan las demás rutas y se emite una ráfaga a −24 dBFS; la entrada de retorno no se mezcla. Evita el monitoreo directo del retorno en el hardware.",
      ),
      diagnosticField(
        "Salida física (desde 1)",
        diagnosticNumber("diag-output", 1, 1, 64),
      ),
      diagnosticField(
        "Entrada de retorno (desde 1)",
        diagnosticNumber("diag-input", 1, 1, cfg.inputs),
      ),
      diagnosticButton(
        "Medir ida y vuelta",
        async () => {
          const input = $("diag-input"),
            output = $("diag-output");
          if (!input.reportValidity() || !output.reportValidity()) return;
          await api("diagnostics/probe", {
            owner: diagnosticOwner,
            input: Number(input.value) - 1,
            output: Number(output.value) - 1,
            label: "Loopback físico",
          });
          await pollDiagnostic();
        },
        "diag-measure",
      ),
      diagnosticButton(
        "Cancelar captura",
        async () => {
          await api("diagnostics/cancel");
          await pollDiagnostic();
        },
        "diag-cancel",
      ),
    );
  }
  if (tool === "comparison") {
    const channel = el("select"),
      bus = el("select"),
      label = el("input");
    channel.id = "diag-route-channel";
    bus.id = "diag-route-bus";
    label.id = "diag-route-label";
    label.value = "Prueba de ruta";
    label.maxLength = 120;
    cfg.channels.forEach((c, i) => channel.append(new Option(c.name, i)));
    cfg.buses.forEach((b, i) => bus.append(new Option(b.name, i)));
    const measure = async (bypass) => {
      if (!$("diag-input").reportValidity()) return;
      const b = Number(bus.value),
        output = cfg.buses[b].output;
      if (output == null)
        throw Error("Asigna una salida física al bus antes de medir");
      await api("diagnostics/probe", {
        owner: diagnosticOwner,
        input: Number($("diag-input").value) - 1,
        output,
        route: { channel: Number(channel.value), bus: b },
        bypass,
        label:
          label.value +
          (bypass ? " · referencia sin EFX" : " · EFX activos según escena"),
      });
      await pollDiagnostic();
    };
    $("diagnostic-controls").append(
      el(
        "p",
        "hint",
        "Medición física por canal → master/aux → salida izquierda/mono del bus → cable → entrada de retorno. Se aíslan las demás rutas y los envíos de red. Se conservan nivel, paneo, mute y envío: una ruta cerrada no dará retorno. La referencia omite EQ, compresor e inserts solo durante la captura y luego los restaura. El retardo predominante se estima por correlación; efectos que transforman mucho la señal pueden impedir medir.",
      ),
      diagnosticField("Canal", channel),
      diagnosticField("Bus / salida asignada", bus),
      diagnosticField(
        "Entrada de retorno (desde 1)",
        diagnosticNumber("diag-input", 1, 1, cfg.inputs),
      ),
      diagnosticField("Nombre de la prueba", label),
      diagnosticButton(
        "Medir referencia sin procesamiento",
        () => measure(true),
        "diag-route-reference",
      ),
      diagnosticButton(
        "Medir ruta con EFX",
        () => measure(false),
        "diag-route-effects",
      ),
      diagnosticButton("Cancelar captura", async () => {
        await api("diagnostics/cancel");
        await pollDiagnostic();
      }),
    );
  }
  if (["feedback", "phase", "scope"].includes(tool)) buildMonitorControls(tool);
  if (tool === "scope") buildScopeControls();
  if (tool === "report") buildReportControls();
  if (tool === "stability") {
    const mode = el("select");
    mode.id = "diag-bench-mode";
    mode.append(
      new Option("ASIO · dispositivo físico", "asio"),
      new Option("AUDIO PC · salida del sistema", "system"),
      new Option("DEMO · solo procesamiento, sin hardware", "demo"),
    );
    const buffers = el("input");
    buffers.id = "diag-bench-buffers";
    buffers.value = "64,128,256,512";
    $("diagnostic-controls").append(
      el(
        "p",
        "hint",
        "Empieza con el motor y el control remoto detenidos. El barrido abre el motor con cada buffer solicitado y conserva el ruteo de la escena. Se guarda el buffer real aceptado. Al terminar o cancelar se restaura la configuración con el motor detenido. DEMO no prueba el controlador ni detecta cortes de hardware.",
      ),
      diagnosticField("Motor", mode),
      diagnosticField("Buffers separados por comas", buffers),
      diagnosticField(
        "Segundos por buffer",
        diagnosticNumber("diag-bench-seconds", 15, 1, 600),
      ),
      diagnosticButton(
        "Iniciar barrido",
        async () => {
          if (!$("diag-bench-seconds").reportValidity()) return;
          await api("diagnostics/bench/start", {
            mode: mode.value,
            buffers: buffers.value.split(",").map((v) => Number(v.trim())),
            seconds: Number($("diag-bench-seconds").value),
          });
          await pollDiagnostic();
        },
        "diag-bench-start",
      ),
      diagnosticButton(
        "Cancelar y restaurar",
        async () => {
          await api("diagnostics/bench/cancel");
          await pollDiagnostic();
        },
        "diag-bench-cancel",
      ),
    );
  }
  pollDiagnostic().catch(
    (e) => ($("diagnostic-message").textContent = e.message),
  );
}
for (const tool of [
  "loopback",
  "stability",
  "comparison",
  "report",
  "feedback",
  "phase",
  "scope",
])
  $("open-" + tool).onclick = () => openDiagnostic(tool);
function pollDiagnostic() {
  if (diagnosticPromise) {
    diagnosticPending = true;
    return diagnosticPromise;
  }
  diagnosticPromise = readDiagnostic().finally(() => {
    diagnosticPromise = null;
    if (diagnosticPending) {
      diagnosticPending = false;
      pollDiagnostic().catch(
        (e) => ($("diagnostic-notice").textContent = e.message),
      );
    }
  });
  return diagnosticPromise;
}
async function readDiagnostic() {
  if (!$("diagnostic-dialog").open) return;
  const tool = diagnosticTool,
    generation = diagnosticGeneration;
  const r = await webFetch(
    ["feedback", "phase", "scope"].includes(tool)
      ? "/api/diagnostics/live"
      : "/api/diagnostics",
  );
  if (!r.ok) throw Error("No se pudo consultar el diagnóstico");
  const d = await r.json();
  if (tool !== diagnosticTool || generation !== diagnosticGeneration) return d;
  const sourceName = (s) =>
    s?.kind === 0
      ? `Entrada física ${s.index + 1}`
      : s?.kind === 1
        ? `${cfg.channels[s.index]?.name || "Canal"} ${s.side ? "R" : "L"}`
        : s?.kind === 2
          ? `${cfg.buses[s.index]?.name || "Bus"} ${s.side ? "R" : "L"}`
          : "—";
  $("diagnostic-sources").textContent =
    ["feedback", "phase", "scope"].includes(tool) && d.context?.a
      ? `Captura ${d.active ? "actual" : "anterior"}: A · ${sourceName(d.context.a)} / B · ${sourceName(d.context.b)}${d.context.owner && d.context.owner !== diagnosticOwner ? " · compartida con otra ventana" : ""}`
      : "";

  if (["loopback", "comparison"].includes(diagnosticTool)) {
    $("diagnostic-message").textContent = d.active
      ? `Capturando ${d.captured_frames} muestras…`
      : d.result?.error ||
        (d.result?.ms != null
          ? `Ida y vuelta medida: ${diagnosticMillis(d.result.ms)} · ${d.result.samples} muestras${d.result.inverted ? " · polaridad invertida" : ""}`
          : "Conecta el cable y pulsa Medir.");
    const rows = d.history.map((h) => [
      h.label +
        (h.context?.channel
          ? ` · ${h.context.channel.name} → ${h.context.bus.name}`
          : ""),
      h.context?.buffer,
      h.context?.input,
      h.context?.output,
      diagnosticMillis(h.result.ms),
      h.result.samples,
      diagnosticDelta(h, d.history),
      h.result.error ||
        `Correlación ${h.result.correlation.toFixed(3)}${h.result.inverted ? " · invertido" : ""}`,
    ]);
    $("diagnostic-results").replaceChildren(
      diagnosticTable(
        [
          "Prueba",
          "Buffer",
          "Entrada",
          "Salida",
          "Ida/vuelta",
          "Muestras",
          "Δ vs referencia",
          "Resultado",
        ],
        rows,
      ),
    );
    const valid = d.history
      .filter((h) => h.result.ms != null)
      .map((h) => h.result.ms);
    if (valid.length)
      $("diagnostic-results").prepend(
        el(
          "p",
          "hint",
          `Historial: ${valid.length} mediciones válidas. Mínimo ${diagnosticMillis(Math.min(...valid))}, promedio ${diagnosticMillis(valid.reduce((a, b) => a + b, 0) / valid.length)}, máximo ${diagnosticMillis(Math.max(...valid))}. Puede contener rutas/buffers diferentes; compara solo configuraciones equivalentes.`,
        ),
      );
  }
  if (diagnosticTool === "feedback") renderFeedback(d);
  if (diagnosticTool === "phase") renderPhase(d);
  if (diagnosticTool === "scope") renderScope(d);
  if (diagnosticTool === "stability") {
    $("diagnostic-message").textContent = d.benchmark
      ? `Probando ${d.benchmark.buffers[d.benchmark.index]} muestras · ${(d.benchmark.elapsed_seconds || 0).toFixed(1)} / ${d.benchmark.seconds} s`
      : "Motor detenido al finalizar. Resultados de esta sesión:";
    $("diagnostic-results").replaceChildren(
      diagnosticTable(
        [
          "Motor",
          "Solicitado",
          "Real",
          "Duración",
          "XRuns driver",
          "DSP máx.",
          "DSP fuera de presupuesto",
          "Resultado",
        ],
        d.bench_results.map((r) => [
          r.mode,
          r.requested_buffer,
          r.actual_buffer,
          r.seconds?.toFixed(1),
          r.xruns,
          diagnosticMillis(r.performance?.max_ms),
          r.performance?.over_budget_blocks,
          r.error ||
            (r.cancelled
              ? "Cancelada"
              : r.failed
                ? "Fallo del motor"
                : r.hardware
                  ? "Prueba física · revisar XRuns y escucha"
                  : "Solo DSP · no valida hardware"),
        ]),
      ),
    );
  }
  return d;
}
let diagnosticPollInFlight = false;
setInterval(async () => {
  if (!$("diagnostic-dialog").open || diagnosticPollInFlight) return;
  diagnosticPollInFlight = true;
  try {
    await pollDiagnostic();
  } catch (e) {
    $("diagnostic-notice").textContent = e.message;
  } finally {
    diagnosticPollInFlight = false;
  }
}, 250);
function diagnosticDownload(name, content, type) {
  const url = URL.createObjectURL(new Blob([content], { type })),
    a = el("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
const reportEscape = (v) =>
  String(v ?? "No informado").replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ],
  );
function reportTree(v) {
  if (v === null || v === undefined) return "No informado";
  if (Array.isArray(v))
    return v.length
      ? "<ol>" +
          v.map((x) => "<li>" + reportTree(x) + "</li>").join("") +
          "</ol>"
      : "Sin registros";
  if (typeof v === "object")
    return (
      "<dl>" +
      Object.entries(v)
        .map(([k, x]) => `<dt>${reportEscape(k)}</dt><dd>${reportTree(x)}</dd>`)
        .join("") +
      "</dl>"
    );
  return reportEscape(typeof v === "boolean" ? (v ? "Sí" : "No") : v);
}
function reportHTML(r) {
  const s = r.status,
    m = s.system,
    diag = r.diagnostics;
  const humanAnalysis = structuredClone(diag.analysis);
  if (humanAnalysis?.scope) delete humanAnalysis.scope.points;
  const table = (headers, rows) =>
    `<table><thead><tr>${headers.map((h) => `<th>${reportEscape(h)}</th>`).join("")}</tr></thead><tbody>${rows.map((row) => `<tr>${row.map((c) => `<td>${reportEscape(c)}</td>`).join("")}</tr>`).join("")}</tbody></table>`;
  const bytes = (n) =>
    typeof n === "number" ? (n / 1048576).toFixed(1) + " MiB" : "No informado";
  const pct = (n) =>
    typeof n === "number" ? n.toFixed(2) + " %" : "No informado";
  const io = (n) =>
    typeof n === "number"
      ? (n / 1048576).toFixed(3) + " MiB/s"
      : "No informado";
  const body = `<h1>AuMux ${reportEscape(r.application.version)} · Informe de pruebas</h1><p>${reportEscape(new Date(r.generated_unix_seconds * 1000).toLocaleString())} · ${reportEscape(r.application.architecture)} · ${reportEscape(s.mode)}</p>
  <h2>Resumen</h2><p>Frecuencia: ${r.configuration.sample_rate} Hz. Buffer activo: ${s.actual_buffer} muestras. Canales: ${r.configuration.channels.length}. Auxiliares: ${r.configuration.buses.length - 1}.</p>
  <h2>Equipo y sistema operativo</h2>${reportTree(r.inventory)}
  <h2>Interfaz seleccionada</h2>${reportTree(r.interface)}<h3>Datos aportados por el operador</h3>${reportTree(r.operator)}
  <h2>Recursos del proceso AuMux</h2>${table(
    ["Recurso", "Actual / última muestra", "Máximo muestreado desde inicio"],
    [
      [
        "CPU (normalizada sobre todos los procesadores)",
        pct(m.cpu_percent),
        pct(m.peak_cpu_percent),
      ],
      ["RAM residente", bytes(m.ram_bytes), bytes(m.peak_ram_bytes)],
      [
        "E/S de lectura del proceso",
        io(m.io_read_bytes_per_second),
        io(m.peak_io_read_bytes_per_second),
      ],
      [
        "E/S de escritura del proceso",
        io(m.io_write_bytes_per_second),
        io(m.peak_io_write_bytes_per_second),
      ],
    ],
  )}<p>Los recursos excluyen el navegador. El máximo se obtiene por muestreo de ~1 s; E/S no representa porcentaje de ocupación del disco.</p>
  <h2>Ida y vuelta física / comparación de rutas</h2>${table(
    [
      "Prueba",
      "Buffer",
      "Entrada",
      "Salida",
      "Muestras",
      "Milisegundos",
      "Correlación",
      "Resultado",
    ],
    diag.history.map((h) => [
      h.label,
      h.context.buffer,
      h.context.input,
      h.context.output,
      h.result.samples,
      h.result.ms?.toFixed(4),
      h.result.correlation?.toFixed(4),
      h.result.error || "Retorno detectado",
    ]),
  )}
  <h2>Pruebas de buffers y estabilidad</h2>${table(
    [
      "Motor",
      "Solicitado",
      "Aceptado",
      "Segundos",
      "XRuns",
      "DSP máximo (ms)",
      "DSP fuera de presupuesto",
      "Estado",
    ],
    diag.bench_results.map((b) => [
      b.mode,
      b.requested_buffer,
      b.actual_buffer,
      b.seconds?.toFixed(2),
      b.xruns,
      b.performance?.max_ms?.toFixed(4),
      b.performance?.over_budget_blocks,
      b.error ||
        (b.cancelled
          ? "Cancelada"
          : b.hardware
            ? "Hardware"
            : "DEMO · solo DSP"),
    ]),
  )}
  <h2>Configuración de canales</h2>${table(
    ["Nombre", "Formato", "Nivel (dB)", "Mute", "EQ", "Compresor", "Inserts"],
    r.configuration.channels.map((c) => [
      c.name,
      c.format,
      c.gain_db,
      c.mute ? "Sí" : "No",
      c.eq.bypass ? "Bypass" : "Activo",
      c.compressor.bypass ? "Bypass" : "Activo",
      (c.inserts.length ? c.inserts : c.fx ? [c.fx] : [])
        .map((f) => (f.name || f.id) + (f.bypass ? " (bypass)" : ""))
        .join(", "),
    ]),
  )}
  <h2>Configuración de buses</h2>${table(
    [
      "Nombre",
      "Formato",
      "Salida física (desde 1)",
      "Nivel (dB)",
      "Mute",
      "Inserts",
    ],
    r.configuration.buses.map((b) => [
      b.name,
      b.format,
      b.output == null ? "Sin salida" : b.output + 1,
      b.gain_db,
      b.mute ? "Sí" : "No",
      (b.inserts.length ? b.inserts : b.fx ? [b.fx] : [])
        .map((f) => (f.name || f.id) + (f.bypass ? " (bypass)" : ""))
        .join(", "),
    ]),
  )}
  <h2>Latencia declarada y calculada</h2>${reportTree({ sample_rate: s.latency.sample_rate, block_frames: s.latency.block_frames, block_ms: s.latency.block_ms, driver_frames: s.latency.driver_frames, driver_ms: s.latency.driver_ms, end_to_end_measured_ms: s.latency.end_to_end_measured_ms })}<p>El mapa detallado de rutas y plugins se conserva en el JSON.</p><h2>Procesamiento DSP</h2>${reportTree(s.performance)}
  <h2>Último análisis de señal</h2>${reportTree(humanAnalysis)}
  <h2>Alcance de las mediciones</h2><ul>${r.measurement_notes.map((n) => `<li>${reportEscape(n)}</li>`).join("")}</ul>`;
  return `<!doctype html><html lang="es"><meta charset="utf-8"><title>AuMux · Informe de pruebas</title><style>body{font:15px/1.5 system-ui,sans-serif;max-width:1100px;margin:32px auto;padding:0 20px;color:#18242c}h1,h2{color:#126373}h2{margin-top:30px}table{border-collapse:collapse;width:100%;font-size:13px}td,th{padding:8px;border:1px solid #bcc9ce;text-align:left;overflow-wrap:anywhere}th{background:#e8f2f4}dt{font-weight:600}dd{margin:0 0 8px 20px}dl{margin:10px 0}li{margin:6px 0}@media print{body{margin:0}h2{break-after:avoid}tr{break-inside:avoid}}</style><body>${body}</body></html>`;
}
function buildReportControls() {
  $("diagnostic-controls").append(
    el(
      "p",
      "hint",
      "Informe HTML independiente, legible e imprimible, y JSON con los datos detallados. CPU/RAM/E/S pertenecen al proceso del motor y se muestrean cada ~1 s. Las versiones no expuestas por el sistema pueden completarse aquí; se identificarán como datos del operador. No se exportan enlaces con claves de acceso.",
    ),
  );
  for (const [id, label] of [
    ["interface_name", "Nombre de interfaz (si no se detecta)"],
    ["hardware_revision", "Revisión de hardware"],
    ["firmware", "Versión de firmware"],
    ["asio_driver_version", "Versión del controlador ASIO"],
    ["obs_version", "Versión de OBS"],
    ["notes", "Notas de la prueba"],
  ]) {
    const input = el("input");
    input.id = "diag-report-" + id;
    input.maxLength = 2000;
    $("diagnostic-controls").append(diagnosticField(label, input));
  }
  const data = async () => {
    const response = await webFetch("/api/diagnostics/report");
    if (!response.ok) throw Error("No se pudo generar el informe");
    const r = await response.json();
    r.operator = {};
    for (const input of document.querySelectorAll('[id^="diag-report-"]'))
      r.operator[input.id.replace("diag-report-", "")] = input.value || null;
    return r;
  };
  $("diagnostic-controls").append(
    diagnosticButton(
      "Descargar informe HTML",
      async () => {
        const r = await data();
        diagnosticDownload(
          `AuMux-${r.application.version}-informe.html`,
          reportHTML(r),
          "text/html;charset=utf-8",
        );
      },
      "diag-export-html",
    ),
    diagnosticButton(
      "Descargar datos JSON",
      async () => {
        const r = await data();
        diagnosticDownload(
          `AuMux-${r.application.version}-diagnostico.json`,
          JSON.stringify(r, null, 2),
          "application/json",
        );
      },
      "diag-export-json",
    ),
  );
}
function diagnosticSourceOptions(select) {
  for (let i = 0; i < cfg.inputs; i++)
    select.append(
      new Option(
        `Entrada física ${i + 1}`,
        JSON.stringify({ kind: 0, index: i, side: 0 }),
      ),
    );
  for (const [kind, nodes] of [
    [1, cfg.channels],
    [2, cfg.buses],
  ])
    nodes.forEach((node, index) => {
      for (let side = 0; side < 2; side++)
        select.append(
          new Option(
            `${kind === 1 ? "Canal" : "Bus"} · ${node.name} · ${side ? "R" : "L"}${kind === 1 ? " · post-FX/pre-fader" : ""}`,
            JSON.stringify({ kind, index, side }),
          ),
        );
    });
}
function buildMonitorControls(tool) {
  if (tool === "feedback") {
    const bus = el("select");
    bus.id = "diag-feedback-bus";
    cfg.buses.forEach((b, i) => bus.append(new Option(b.name, i)));
    $("diagnostic-controls").append(
      el(
        "p",
        "hint",
        "Analiza el bus post-fader. Marca tonos persistentes y prominentes como posibles acoples; una nota musical sostenida puede producir el mismo indicio. Los cortes son manuales, de −6 dB y Q 12, en bandas libres del EQ existente. No sustituye el control del nivel acústico.",
      ),
      diagnosticField("Bus que analizar y corregir", bus),
      diagnosticButton(
        "Iniciar detector",
        async () => {
          const index = Number(bus.value);
          await api("diagnostics/cancel");
          await api("diagnostics/monitor", {
            owner: diagnosticOwner,
            a: { kind: 2, index, side: 0 },
            b: { kind: 2, index, side: 1 },
          });
          await pollDiagnostic();
        },
        "diag-monitor-start",
      ),
    );
  } else {
    $("diagnostic-controls").append(
      el(
        "p",
        "hint",
        tool === "scope"
          ? "Entradas físicas, canales post-FX/pre-fader o buses post-fader. A se dibuja en cian y B en magenta. Las líneas unen muestras reales según el zoom; las barras conservan los mínimos/máximos intermedios. Congelar solo detiene la vista."
          : "Compara entradas físicas, canales post-FX/pre-fader o buses post-fader. La correlación se calcula sin componente continua: +1 indica señales coincidentes, −1 señales opuestas. El retardo y el contenido musical también influyen; esto no prueba por sí solo una inversión de cableado. La suma mono se calcula como (A+B)/2.",
      ),
    );
    for (const lane of ["a", "b"]) {
      const select = el("select");
      select.id = "diag-source-" + lane;
      diagnosticSourceOptions(select);
      select.value = JSON.stringify({
        kind: 2,
        index: 0,
        side: lane === "a" ? 0 : 1,
      });
      $("diagnostic-controls").append(
        diagnosticField("Señal " + lane.toUpperCase(), select),
      );
    }
    $("diagnostic-controls").append(
      diagnosticButton(
        "Iniciar análisis",
        async () => {
          await api("diagnostics/cancel");
          if (tool === "scope")
            await api("diagnostics/scope-window", {
              frames: Math.round(Number($("diag-scope-time").value) * 48),
              rising: $("diag-scope-trigger").value === "rising",
            });
          await api("diagnostics/monitor", {
            owner: diagnosticOwner,
            a: JSON.parse($("diag-source-a").value),
            b: JSON.parse($("diag-source-b").value),
          });
          await pollDiagnostic();
        },
        "diag-monitor-start",
      ),
    );
  }
  $("diagnostic-controls").append(
    diagnosticButton(
      "Detener análisis",
      async () => {
        await api("diagnostics/cancel");
        await pollDiagnostic();
      },
      "diag-monitor-stop",
    ),
  );
}
async function applyFeedbackCut(bus, frequency) {
  if (!cfg.buses[bus]) throw Error("El bus ya no existe");
  const eq = cfg.buses[bus].eq;
  if (
    eq.bands.some(
      (b) =>
        b.kind === 0 &&
        b.gain_db < 0 &&
        Math.abs(Math.log2(b.frequency / frequency)) < 1 / 12,
    )
  )
    throw Error("Ya hay un corte en esa frecuencia; revísalo desde el EQ.");
  const index = eq.bands.findIndex((b) => b.kind === 0 && b.gain_db === 0);
  if (index < 0) throw Error("No quedan bandas libres; revisa el EQ del bus.");
  if (eq.bypass && eq.bands.some((b) => b.kind !== 0 || b.gain_db !== 0))
    throw Error(
      "Activa primero el EQ del bus para conservar el control de sus ajustes existentes.",
    );
  eq.bands[index] = {
    kind: 0,
    frequency: Math.round(frequency),
    gain_db: -6,
    q: 12,
  };
  eq.bypass = false;
  await publish();
  $("diagnostic-notice").textContent =
    `Corte aplicado en ${cfg.buses[bus].name}: ${Math.round(frequency)} Hz, −6 dB, Q 12. Se guarda con la escena y se puede quitar desde su EQ.`;
}
function renderFeedback(d) {
  const analysis = d.analysis,
    items = analysis?.feedback_candidates || [];
  $("diagnostic-message").textContent = d.active
    ? `Detector activo · ${d.captured_frames} muestras · ${d.dropped_chunks} fragmentos descartados`
    : "Detector detenido · se conserva la última captura.";
  if (d.active && d.analysis_age_ms > 1000)
    $("diagnostic-message").textContent =
      "Sin muestras nuevas recientes; revisa el motor. Se muestra la última captura.";
  const pane = $("diagnostic-results");
  const bus = d.context?.a?.kind === 2 ? d.context.a.index : null;
  const signature = JSON.stringify([
    bus,
    ...items.map((item) => Math.round(item.frequency_hz / 5)),
  ]);
  if (pane.dataset.feedbackSignature !== signature) {
    pane.replaceChildren(el("p", "hint"));
    pane.dataset.feedbackSignature = signature;
    for (const item of items) {
      const card = el("div", "diagnostic-candidate");
      card.append(el("span"));
      if (bus !== null)
        card.append(
          diagnosticButton(
            `Aplicar corte en ${cfg.buses[bus]?.name || "bus"}`,
            () => applyFeedbackCut(bus, item.frequency_hz),
          ),
        );
      pane.append(card);
    }
  }
  pane.querySelector("p").textContent =
    `Ventana ${analysis?.window_frames || 4096} muestras · resolución ${analysis?.frequency_resolution_hz?.toFixed(2) || "11,72"} Hz. ${items.length ? "Tonos sospechosos detectados:" : "Sin tonos persistentes sospechosos en la captura actual."}`;
  [...pane.querySelectorAll(".diagnostic-candidate")].forEach((card, i) => {
    const item = items[i];
    card.querySelector("span").textContent =
      `${item.frequency_hz.toFixed(1)} Hz · ${item.dbfs.toFixed(1)} dBFS · prominencia ${item.prominence_db.toFixed(1)} dB · ${(item.sustained_ms / 1000).toFixed(1)} s`;
    const button = card.querySelector("button");
    if (button)
      button.disabled =
        !d.active ||
        d.analysis_age_ms > 1000 ||
        Number($("diag-feedback-bus").value) !== bus;
  });
}

function renderPhase(d) {
  const p = d.analysis?.phase;
  $("diagnostic-message").textContent = d.active
    ? "Análisis activo · ventana de 85,33 ms"
    : "Análisis detenido · última captura";
  if (d.active && d.analysis_age_ms > 1000)
    $("diagnostic-message").textContent =
      "Sin muestras nuevas recientes; revisa el motor. Se muestra la última captura.";
  const pane = $("diagnostic-results");
  pane.replaceChildren();
  if (!p) {
    pane.append(el("p", "hint", "Esperando señal…"));
    return;
  }
  const correlation = p.correlation;
  const value =
    typeof correlation === "number"
      ? correlation.toFixed(4)
      : "Sin señal AC suficiente";
  const card = el("div", "phase-readout");
  card.append(el("strong", "", `Correlación: ${value}`));
  const meter = el("meter");
  meter.min = -1;
  meter.max = 1;
  meter.value = correlation ?? 0;
  meter.setAttribute("aria-label", "Correlación de fase");
  card.append(meter);
  pane.append(card);
  pane.append(
    diagnosticTable(
      ["Medida", "Resultado"],
      [
        [
          "Señal A",
          p.rms_a_dbfs == null
            ? "Silencio / bajo umbral"
            : p.rms_a_dbfs.toFixed(2) + " dBFS",
        ],
        [
          "Señal B",
          p.rms_b_dbfs == null
            ? "Silencio / bajo umbral"
            : p.rms_b_dbfs.toFixed(2) + " dBFS",
        ],
        [
          "Suma mono (A+B)/2",
          p.mono_dbfs == null
            ? "Silencio / bajo umbral"
            : p.mono_dbfs.toFixed(2) + " dBFS",
        ],
        [
          "Cambio al sumar a mono",
          p.mono_relative_db == null
            ? "Sin señal"
            : p.mono_relative_db.toFixed(2) + " dB",
        ],
        [
          "Desfase en frecuencia dominante",
          p.dominant_phase_deg == null
            ? "Sin señal suficiente"
            : p.dominant_phase_deg.toFixed(1) +
              "° a " +
              p.dominant_frequency_hz.toFixed(1) +
              " Hz",
        ],
      ],
    ),
  );
  if (correlation < -0.5)
    pane.append(
      el(
        "p",
        "hint",
        "Correlación negativa sostenida: revisa polaridad, alineación temporal y compatibilidad mono.",
      ),
    );
}

let scopeFrozen = false,
  scopeLast = null;
function buildScopeControls() {
  scopeFrozen = false;
  scopeLast = null;
  const time = el("select"),
    scale = el("select"),
    trigger = el("select");
  time.id = "diag-scope-time";
  scale.id = "diag-scope-scale";
  trigger.id = "diag-scope-trigger";
  for (const ms of [5, 10, 20, 50, 85.333])
    time.append(
      new Option(`${ms.toFixed(ms === 85.333 ? 2 : 0)} ms de ventana`, ms),
    );
  time.value = "20";
  for (const n of [0.0625, 0.125, 0.25, 0.5, 1, 2])
    scale.append(new Option(`±${n} amplitud`, n));
  scale.value = "0.25";
  trigger.append(
    new Option("Ascendente · señal A", "rising"),
    new Option("Libre", "free"),
  );
  const freeze = diagnosticButton(
    "Congelar vista",
    () => {
      scopeFrozen = !scopeFrozen;
      freeze.textContent = scopeFrozen ? "Reanudar vista" : "Congelar vista";
      if (!scopeFrozen && scopeLast) renderScope(scopeLast);
    },
    "diag-scope-freeze",
  );
  $("diagnostic-controls").append(
    diagnosticField("Tiempo visible", time),
    diagnosticField("Escala vertical", scale),
    diagnosticField("Disparo", trigger),
    freeze,
  );
  scale.onchange = () => {
    if (scopeLast) drawScope(scopeLast);
  };
  for (const input of [time, trigger])
    input.onchange = async () => {
      try {
        await api("diagnostics/scope-window", {
          frames: Math.round(Number(time.value) * 48),
          rising: trigger.value === "rising",
        });
        await pollDiagnostic();
      } catch (e) {
        $("diagnostic-notice").textContent = e.message;
      }
    };
  const canvas = el("canvas", "diagnostic-canvas");
  canvas.id = "diag-scope-canvas";
  canvas.width = 1000;
  canvas.height = 340;
  canvas.setAttribute("aria-label", "Osciloscopio: A cian, B magenta");
  $("diagnostic-results").append(canvas);
}
function renderScope(d) {
  if (scopeFrozen) {
    $("diagnostic-message").textContent =
      "Vista congelada; el audio y el análisis continúan.";
    return;
  }
  scopeLast = d;
  drawScope(d);
}
function drawScope(d) {
  const canvas = $("diag-scope-canvas");
  if (!canvas) return;
  const ctx = canvas.getContext("2d"),
    w = canvas.width,
    h = canvas.height;
  ctx.fillStyle = "#111821";
  ctx.fillRect(0, 0, w, h);
  ctx.strokeStyle = "#2d3b4b";
  ctx.lineWidth = 1;
  for (let i = 0; i <= 10; i++) {
    ctx.beginPath();
    ctx.moveTo(45 + ((w - 65) * i) / 10, 25);
    ctx.lineTo(45 + ((w - 65) * i) / 10, h - 35);
    ctx.stroke();
  }
  for (let i = 0; i <= 8; i++) {
    ctx.beginPath();
    ctx.moveTo(45, 25 + ((h - 60) * i) / 8);
    ctx.lineTo(w - 20, 25 + ((h - 60) * i) / 8);
    ctx.stroke();
  }
  const data = d.analysis?.scope;
  if (!data) {
    $("diagnostic-message").textContent = "Esperando captura del motor…";
    return;
  }
  const frames = data.visible_frames,
    amplitude = Number($("diag-scope-scale").value),
    step = data.bucket_frames,
    start = 0;
  const x = (frame) => 45 + ((frame - start) / frames) * (w - 65),
    y = (value) =>
      (h - 10) / 2 -
      (Math.max(-1, Math.min(1, value / amplitude)) * (h - 60)) / 2;
  for (const [side, color] of [
    [0, "#4de2ee"],
    [2, "#ef9cd8"],
  ]) {
    ctx.strokeStyle = color;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    for (let i = 0; i < data.points.length; i++) {
      const frame = i * step;
      if (frame < start || frame > start + frames) continue;
      ctx.moveTo(x(frame), y(data.points[i][side]));
      ctx.lineTo(x(frame), y(data.points[i][side + 1]));
    }
    ctx.stroke();
    ctx.beginPath();
    let first = true;
    for (let i = 0; i < data.points.length; i++) {
      const frame = i * step;
      if (frame < start || frame > start + frames) continue;
      const value = data.points[i][side === 0 ? 4 : 5];
      if (typeof value !== "number") continue;
      if (first) {
        ctx.moveTo(x(frame), y(value));
        first = false;
      } else ctx.lineTo(x(frame), y(value));
    }
    ctx.stroke();
  }
  ctx.fillStyle = "#d2e3ee";
  ctx.font = "13px monospace";
  ctx.fillText(
    `A cian · B magenta · ±${amplitude} · captura min/max cada ${step} muestras`,
    48,
    17,
  );
  for (let i = 0; i <= 5; i++)
    ctx.fillText(
      `${(((frames / 48) * i) / 5).toFixed(1)} ms`,
      45 + ((w - 100) * i) / 5,
      h - 10,
    );
  const age = (d.captured_frames - (d.analysis.through_frame || 0)) / 48;
  $("diagnostic-message").textContent =
    `${d.active ? "Captura activa" : "Detenido · última captura"} · ventana analizada ${d.analysis.window_ms.toFixed(2)} ms · pico A ${data.peak_a.toFixed(4)}, B ${data.peak_b.toFixed(4)} · cola aprox. ${Math.max(0, age).toFixed(1)} ms · descartes ${d.dropped_chunks}`;
}
$("diagnostic-dialog").addEventListener("close", () => {
  if (["feedback", "phase", "scope"].includes(diagnosticTool))
    api("diagnostics/cancel", { owner: diagnosticOwner }).catch(error);
});

window.addEventListener("beforeunload", () => {
  if (diagnosticTool)
    fetch("/api/diagnostics/cancel", {
      method: "POST",
      headers: { "Content-Type": "application/json", "X-Monitor-Token": token },
      body: JSON.stringify({ owner: diagnosticOwner }),
      keepalive: true,
    }).catch(() => {});
});
