# Hoja de ruta

Estado a partir de Monitor Mix 0.8. Todos los caminos de audio seguirán a
**48 kHz** en esta etapa. Esta lista registra tareas pendientes y avances,
sin fechas comprometidas. Los instrumentos virtuales quedan
fuera del alcance; el proyecto se centra en mezcla y procesamiento de audio.

## Prioridad transversal: tiempo real y mínima latencia

Requisito principal acordado: mezcla y monitoreo en tiempo real, con la menor
latencia estable de extremo a extremo en entradas, canales, auxiliares, master,
salidas físicas, puente OBS y escucha por red. Nuevas funciones no deben añadir
esperas al camino de audio; calidad y continuidad de la señal también forman
parte del criterio de aceptación.

- Medir cada ruta completa a 48 kHz, incluyendo conversión, driver, buffers,
  plugins, transporte y reproducción. Registrar configuración, latencia medida,
  variación, cortes y XRuns; el tamaño de bloque por sí solo no mide la latencia.
- Usar el menor buffer que resulte estable con el hardware y la carga reales,
  evitar bloques y colas adicionales innecesarios, y mantener trabajo acotado.
- Mantener disco, red, interfaz, asignación/liberación de memoria y bloqueos
  fuera del callback. Grabación, medidores y control remoto no deben frenar audio.
- Mostrar la latencia declarada por los plugins y advertir cuando lookahead,
  fase lineal u otros procesos añadan retardo. La futura compensación alinea
  rutas añadiendo espera; no elimina la latencia de los efectos. Hacer explícito
  ese coste y permitir elegir qué efectos usar para monitoreo.
- Medir por separado respuesta de controles y transporte de audio remoto:
  mover un fader por web no debe agregar un buffer al audio local.

**Siguiente trabajo prioritario de audio:** revisar el transporte y la reproducción
LAN antes de considerarlos aptos para monitoreo de participantes. La versión 0.7
programa unos 120 ms de búfer en el navegador, además de red y dispositivo;
es una implementación inicial de escucha y todavía no cumple ese objetivo.
Evaluar transporte, paquetes y búfer adaptativo para evitar acumulación de
retraso; comparar alternativas como WebRTC y, si el navegador no permite el
resultado requerido, un receptor nativo. Validar latencia y estabilidad con
mediciones reales antes de elegir o prometer cifras.

## Dispositivos y subsistemas de Windows

Agregar selección explícita de entradas y salidas fuera de ASIO: empezar por
WASAPI compartido/exclusivo y evaluar DirectSound/MME según soporte del backend.
Priorizar rutas de baja latencia; documentar los límites de cada modo sin
prometer la misma respuesta para todos los dispositivos y subsistemas.
Mostrar dispositivos, canales, formatos y latencia disponible; gestionar cambios
del dispositivo predeterminado, desconexión y errores. Capturar micrófonos y
entradas del PC, no solo reproducir por la salida predeterminada como ahora.
Convertir frecuencia cuando el dispositivo no abra a 48 kHz y definir el
tratamiento de relojes independientes antes de habilitar rutas combinadas.

La agregación de varios dispositivos ASIO queda **fuera del alcance acordado**.

## Mezclas personales remotas por web · base disponible en 0.8

Ya implementado: superficie por auxiliar con faders y mute de envío, permiso
PRE/POST, límite de nivel, enlaces revocables, sincronización con la consola y
reconexión sin reproducir cambios antiguos. Enlaces con IP Tailscale/MagicDNS
para control y escucha. Ver [guía 0.8](interfaz-0.8.md).

Pendiente: optimizar y medir transporte de escucha para monitoreo de mínima
latencia, validar sesiones largas y varios dispositivos reales en LAN/tailnet,
y diseñar paneo independiente por envío estéreo. El control web y su respuesta
se miden separados del retardo del audio; la API de administración sigue local.

## Grabación multipista

Grabar todos los canales y, opcionalmente, main/aux en WAV a 48 kHz, con elección
pre/post procesamiento, nombres y selección de pistas. Escritura fuera del
callback, cola acotada y avisos claros de disco lleno o pérdida de bloques.
Definir segmentación/RF64 para sesiones largas y recuperación de archivos.

## Control y expansión

- OSC y modo controlador sin abrir motor de audio.
- Salida Mackie Control/Mackie HUI y mapa MOTU/CueMix verificado con documentación.
- Persistencia de estados binarios VST3 y evaluación de editores nativos.
- Compensación de latencia de plugins y pruebas prolongadas
  de estabilidad/latencia con ASIO y hardware real.

Ver [control remoto](control-remoto.md) para el diseño previo de OSC/Mackie y
[la versión actual](interfaz-0.8.md) para lo ya implementado.
