"use strict";
// Visual assistance only: no requests, audio processing or scene changes.
const commonParameterHelp = {
  Output: "Nivel de salida del efecto; permite compensar cambios de volumen.",
  Mix: "Mezcla la señal original con la procesada por el efecto.",
  Attack:
    "Tiempo de reacción al aumentar la señal: valores cortos responden más rápido.",
  Release:
    "Tiempo de recuperación al bajar la señal: valores largos producen una respuesta más suave.",
  Feedback:
    "Realimenta la salida hacia el efecto; valores altos prolongan las repeticiones o la resonancia.",
};
const nativeParameterHelp = {
  ambience: {
    Size: "Tamaño aparente de la sala reverberante.",
    "HF Damp":
      "Amortiguación de agudos en la reverberación; más amortiguación oscurece la cola.",
  },
  bandisto: {
    Listen:
      "Selecciona qué banda escuchar para ajustar la distorsión por separado.",
    "L <> M": "Frecuencia que separa las bandas de graves y medios.",
    "M <> H": "Frecuencia que separa las bandas de medios y agudos.",
    "L Dist": "Cantidad de distorsión en la banda de graves.",
    "M Dist": "Cantidad de distorsión en la banda de medios.",
    "H Dist": "Cantidad de distorsión en la banda de agudos.",
    "L Out": "Nivel de salida de la banda de graves.",
    "M Out": "Nivel de salida de la banda de medios.",
    "H Out": "Nivel de salida de la banda de agudos.",
    Mode: "Selecciona la modalidad de distorsión de las bandas.",
  },
  beatbox: {
    "Hat Thr": "Umbral que dispara el sonido de hi-hat a partir de la entrada.",
    "Hat Rate": "Intervalo mínimo entre disparos del hi-hat.",
    "Hat Mix": "Nivel del hi-hat sintetizado.",
    "Kik Thr": "Umbral de detección para disparar el bombo.",
    "Kik Trig":
      "Sintoniza el detector del bombo para reconocer la señal de entrada.",
    "Kik Mix": "Nivel del bombo sintetizado.",
    "Snr Thr": "Umbral de detección para disparar la caja.",
    "Snr Trig":
      "Sintoniza el detector de caja para reconocer la señal de entrada.",
    "Snr Mix": "Nivel de la caja sintetizada.",
    Dynamics:
      "Cuánto sigue la intensidad de la percusión sintetizada a la señal de entrada.",
    "Thru Mix": "Nivel de la señal original que se conserva en la salida.",
  },
  degrade: {
    Headroom:
      "Margen antes de la saturación; modifica cuánto se recorta la señal.",
    Quantize:
      "Resolución de cuantización; menos bits aumentan el carácter digital y el ruido.",
    "Sample Rate":
      "Frecuencia de muestreo simulada; reducirla introduce aliasing.",
    "Sample Mode": "Modalidad de reducción de frecuencia de muestreo.",
    PostFilter: "Filtro posterior que atenúa agudos y suaviza los artefactos.",
    "Non-Linearity": "Intensidad de la deformación no lineal de la señal.",
    "Non-Lin Mode": "Tipo de deformación no lineal aplicada.",
  },
  delay: {
    "L Delay": "Tiempo de retardo del canal izquierdo.",
    "R Delay":
      "Retardo derecho relativo al izquierdo; define la separación de los ecos estéreo.",
    "Fb Tone": "Color tonal de las repeticiones realimentadas.",
    "FX Mix": "Proporción de señal retardada respecto de la señal original.",
  },
  detune: {
    Detune: "Desafinación entre las copias para crear un efecto de doblaje.",
    Latency:
      "Tamaño de la ventana de procesamiento: cambia el carácter y el retardo del efecto.",
  },
  dither: {
    "Word Len": "Resolución de bits de la cuantización de salida.",
    Dither: "Tipo de ruido añadido para suavizar los errores de cuantización.",
    "Dith Amp": "Amplitud del ruido de dither añadido.",
    "DC Trim":
      "Pequeño desplazamiento de continua usado para examinar la cuantización.",
    "Zoom...":
      "Modo de inspección: reduce la señal y usa una cuantización de 6 bits para hacer audible el dither.",
  },
  dynamics: {
    Thresh: "Nivel a partir del cual actúa el compresor.",
    Ratio: "Proporción de compresión por encima del umbral.",
    Limiter: "Umbral del limitador que contiene los picos.",
    "Gate Thr": "Umbral por debajo del cual la puerta atenúa la señal.",
    "Gate Att": "Velocidad de apertura de la puerta al superar su umbral.",
    "Gate Rel": "Tiempo de cierre de la puerta al caer la señal.",
  },
  envelope: {
    Output:
      "Selecciona el modo de salida: envolvente o audio controlado por ella.",
    Gain: "Ganancia de la salida del seguidor de envolvente.",
  },
  image: {
    Mode: "Modalidad de conversión entre estéreo izquierdo/derecho y medio/lateral.",
    "S Width":
      "Nivel de la componente lateral, que determina la anchura estéreo.",
    "S Pan": "Distribución izquierda/derecha de la componente lateral.",
    "M Level": "Nivel de la componente central o mono.",
    "M Pan": "Posición estéreo de la componente central.",
  },
  limiter: {
    Thresh: "Nivel a partir del cual se limita la señal.",
    Knee: "Transición entre señal sin limitar y limitada: suave o abrupta.",
  },
  loudness: {
    Loudness:
      "Cantidad y sentido de la compensación tonal asociada al nivel de escucha.",
    Link: "Enlaza la compensación tonal con el nivel de salida.",
  },
  overdrive: {
    Drive: "Cantidad de saturación: más drive añade armónicos y distorsión.",
    Muffle: "Atenúa los agudos para oscurecer y suavizar la distorsión.",
  },
  rezfilter: {
    Freq: "Frecuencia de corte del filtro resonante.",
    Res: "Resonancia: realza las frecuencias cercanas al corte.",
    "Env->VCF":
      "Intensidad y dirección de la modulación del filtro por la envolvente de entrada.",
    "LFO->VCF": "Profundidad con que el oscilador lento mueve el filtro.",
    "LFO Rate": "Velocidad del oscilador lento que modula el filtro.",
    Trigger: "Modo y umbral de disparo de la modulación.",
    "Max Freq": "Límite superior del recorrido de frecuencia del filtro.",
  },
  ringmod: {
    Frequency:
      "Frecuencia de la portadora de modulación en anillo; produce nuevos parciales.",
    "Fine-tune": "Ajuste fino de la frecuencia de la portadora.",
    Level: "Nivel de salida de la señal modulada.",
  },
  shepard: {
    Mode: "Selecciona la salida del efecto Shepard o su combinación con la entrada.",
    Rate: "Velocidad y dirección del movimiento tonal continuo.",
  },
  splitter: {
    Mode: "Selecciona cómo se distribuyen las señales separadas entre las salidas.",
    Freq: "Frecuencia de separación de la señal.",
    "Freq SW":
      "Selecciona qué parte del espectro pasa por el filtro de separación.",
    Level: "Umbral de la separación por nivel.",
    "Level SW": "Selecciona el comportamiento de la separación por nivel.",
    Envelope: "Respuesta temporal del detector de nivel.",
  },
  stereo: {
    Width: "Anchura de la imagen estéreo creada a partir de la entrada.",
    Balance: "Equilibrio de nivel entre izquierda y derecha.",
    Delay: "Retardo que produce diferencias entre los canales estéreo.",
    Mod: "Profundidad de la modulación del retardo.",
    Rate: "Velocidad de la modulación del retardo.",
  },
  subsynth: {
    Type: "Método usado para generar el refuerzo de subgraves.",
    Level: "Nivel de los subgraves sintetizados.",
    Tune: "Sintonía del procesamiento que genera los subgraves.",
    "Dry Mix": "Cantidad de señal original que se conserva.",
    Thresh:
      "Umbral de señal necesario para activar la generación de subgraves.",
  },
  testtone: {
    Mode: "Tipo de señal de prueba: tonos, ruido o barridos según el modo.",
    Level: "Nivel de la señal de prueba generada.",
    Channel: "Canales de salida que reciben la señal de prueba.",
    F1: "Primera frecuencia de prueba; su función depende del modo elegido.",
    F2: "Segunda frecuencia de prueba; su función depende del modo elegido.",
    Sweep: "Duración o comportamiento del barrido de frecuencias.",
    Thru: "Nivel de la entrada original que acompaña al generador.",
    "0dB =": "Referencia de nivel usada para calibrar los tonos de prueba.",
  },
  freevox8: {
    Mode: "Selecciona el algoritmo de textura: Vocoder, Mask, Ghost o Morph.",
    "Carrier blend": "Cantidad de la portadora interna que excita el vocoder.",
    "Carrier note": "Nota de la portadora interna, expresada como número MIDI.",
    "Formant shift":
      "Desplaza la envolvente de formantes para cambiar el carácter de la voz.",
    "Mask depth": "Intensidad de la máscara espectral aplicada a la señal.",
    Clarity: "Realce de la definición de la envolvente vocal.",
    Ghost: "Cantidad de la textura espectral Ghost.",
    Air: "Cantidad de componente de aire y altas frecuencias.",
    Freeze:
      "Retiene la envolvente espectral al superar el 50 %; por debajo vuelve a seguir la entrada.",
    "Transient protect":
      "Conservación de ataques y transitorios de la entrada.",
    "Stereo width": "Anchura de la imagen estéreo procesada.",
    Quality:
      "Ajusta la intensidad y el carácter del procesamiento interno del banco de filtros.",
    "Output gain": commonParameterHelp.Output,
  },
};
const builtInHelp = {
  eq: {
    frequency: [
      "Frecuencia",
      "Frecuencia central de la banda o frecuencia de corte, según el tipo de filtro.",
    ],
    gain_db: [
      "Ganancia",
      "Realce o atenuación de la banda en dB. Los filtros de corte no usan esta ganancia.",
    ],
    q: [
      "Q / pendiente",
      "Q controla la anchura o resonancia de la banda: una Q mayor concentra el efecto. No es un selector de pendiente en dB/octava.",
    ],
    kind: [
      "Tipo de filtro",
      "Campana realza/atenúa una zona; shelf cambia graves o agudos; los filtros de paso dejan pasar solo parte del espectro.",
    ],
    bypass: [
      "Bypass",
      "Omite el EQ sin borrar los ajustes de sus ocho bandas.",
    ],
  },
  compressor: {
    threshold: [
      "Umbral",
      "Nivel a partir del cual se comprime la señal detectada.",
    ],
    ratio: [
      "Ratio",
      "Proporción de compresión por encima del umbral; 4:1 convierte 4 dB de exceso en aproximadamente 1 dB.",
    ],
    attack: [
      "Ataque",
      "Tiempo de reacción al superar el umbral; un ataque corto controla antes los transitorios.",
    ],
    release: [
      "Release",
      "Tiempo que tarda en recuperarse la ganancia al bajar la señal del detector.",
    ],
    knee: [
      "Knee",
      "Anchura de la transición alrededor del umbral; más dB hacen que la compresión entre gradualmente.",
    ],
    makeup: [
      "Makeup",
      "Ganancia aplicada a la rama comprimida para compensar la reducción de nivel.",
    ],
    mix: [
      "Mezcla",
      "Proporción entre la señal original y la rama comprimida, para compresión paralela.",
    ],
    highpass: [
      "Filtro detector",
      "Atenúa graves solo en el detector para reducir su influencia en la compresión; no filtra directamente el audio de salida.",
    ],
    source: [
      "Detector / sidechain",
      "Señal que activa el compresor. Otra entrada o canal puede controlar la reducción sin mezclarse con el audio procesado.",
    ],
    bypass: ["Bypass", "Omite la compresión y conserva sus ajustes."],
  },
};
function parameterHelpText(descriptor, param) {
  if (descriptor.id?.startsWith("native/")) {
    const plugin = descriptor.id.split("/").at(-1);
    return (
      nativeParameterHelp[plugin]?.[param.id] ||
      commonParameterHelp[param.name] ||
      "Control propio del efecto; consulta su documentación para el detalle del algoritmo."
    );
  }
  // VST3 parameter enumeration does not supply a manual or semantic descriptions.
  return `Control expuesto por el plugin. Rango comunicado: ${param.min}–${param.max}${param.units ? " " + param.units : ""}. El fabricante no proporciona una descripción mediante esta interfaz; consulta su manual. Una escala 0–1 puede ser normalizada.`;
}
let pluginHelpEnabled = true;
try {
  pluginHelpEnabled = localStorage.getItem("aumux-plugin-help") !== "off";
} catch {}
const pluginHelpPanels = new Map();
function applyPluginHelpPreference(enabled) {
  pluginHelpEnabled = enabled;
  document.getElementById("show-plugin-help").checked = enabled;
  for (const { footer } of pluginHelpPanels.values()) footer.hidden = !enabled;
}
function bindParameterHelp(element, id) {
  element.dataset.helpParameter = id;
  return element;
}
function setPluginHelp(dialogId, descriptor) {
  const dialog = document.getElementById(dialogId);
  let panel = pluginHelpPanels.get(dialogId);
  if (!panel) {
    const footer = document.createElement("aside"),
      title = document.createElement("strong"),
      text = document.createElement("p"),
      details = document.createElement("details"),
      summary = document.createElement("summary"),
      list = document.createElement("dl");
    footer.className = "plugin-help";
    footer.setAttribute("aria-label", "Ayuda de parámetros");
    text.setAttribute("aria-live", "polite");
    summary.textContent = "Ver todos los parámetros";
    details.append(summary, list);
    footer.append(title, text, details);
    dialog.append(footer);
    panel = { footer, title, text, list, descriptor: null };
    pluginHelpPanels.set(dialogId, panel);
    const show = (event) => {
      const id = event.target.closest("[data-help-parameter]")?.dataset
        .helpParameter;
      const p = panel.descriptor?.params.find((p) => p.id === id);
      if (!p || !pluginHelpEnabled) return;
      title.textContent = `${panel.descriptor.name} · ${p.name}`;
      text.textContent = p.help || parameterHelpText(panel.descriptor, p);
    };
    for (const type of ["pointerover", "pointerdown", "focusin"])
      dialog.addEventListener(type, show);
  }
  panel.descriptor = descriptor;
  panel.title.textContent = `${descriptor.name} · ayuda`;
  panel.text.textContent =
    "Toca o enfoca un parámetro para ver qué hace. También puedes consultar la lista completa.";
  panel.list.replaceChildren(
    ...descriptor.params.flatMap((p) => {
      const term = document.createElement("dt"),
        explanation = document.createElement("dd");
      term.textContent = p.name;
      explanation.textContent = p.help || parameterHelpText(descriptor, p);
      return [term, explanation];
    }),
  );
  panel.footer.hidden = !pluginHelpEnabled;
}
function setBuiltInHelp(dialogId, type, name) {
  setPluginHelp(dialogId, {
    id: `builtin/${type}`,
    name,
    params: Object.entries(builtInHelp[type]).map(([id, [name, help]]) => ({
      id,
      name,
      help,
    })),
  });
}
document
  .getElementById("show-plugin-help")
  .addEventListener("change", (event) => {
    applyPluginHelpPreference(event.target.checked);
    try {
      localStorage.setItem(
        "aumux-plugin-help",
        pluginHelpEnabled ? "on" : "off",
      );
    } catch {}
  });
window.addEventListener("storage", (event) => {
  if (event.key === "aumux-plugin-help" || event.key === null)
    applyPluginHelpPreference(event.key === null || event.newValue !== "off");
});
applyPluginHelpPreference(pluginHelpEnabled);
