# Hoja de ruta

Estado a partir de AuMux 0.11. Todos los caminos de audio seguirán a
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
LAN antes de considerarlos aptos para monitoreo de participantes. Desde 0.10 se
permiten objetivos de búfer de 40/80/120/200 ms en el navegador (80 ms inicialmente),
además de red y dispositivo;
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

## Mezclas y consola remotas por web · disponibles en 0.11

Ya implementado: superficie por auxiliar con faders y mute de envío, permiso
PRE/POST, límite de nivel, enlaces revocables, sincronización con la consola y
reconexión sin reproducir cambios antiguos. Enlaces con IP Tailscale/MagicDNS
para control y escucha. Se añaden vistas touch, enlaces unificados con contraseña
u abiertos, selector de auxiliar, consola principal remota y selección de interfaz
de red. En 0.11 el control de auxiliares usa WebSocket, agrupa los últimos cambios
cada 80 ms y recibe estado cada 100 ms; las confirmaciones llegan después de aplicar.
La reconexión descarta comandos pendientes antiguos. Ver [guía 0.11](interfaz-0.11.md).

Pendiente: optimizar y medir transporte de escucha para monitoreo de mínima
latencia, validar sesiones largas y varios dispositivos reales en LAN/tailnet,
y diseñar paneo independiente por envío estéreo. El control web y su respuesta
se miden separados del retardo del audio; la API de administración sigue local.

## Grabación multipista

Disponible desde 0.14: selección de canales PRE/POST y master/aux postfader,
WAV Float32 a 48 kHz, nombres y manifiesto, escritor separado del callback,
cola acotada, errores de disco explícitos y cabeceras RF64 actualizadas.
Pendiente: explorador y reproducción de tomas, edición y automatización.
[Alcance y puntos exactos de captura](interfaz-0.14.md).

## Herramientas de señal

Disponibles: diagnóstico de componentes de latencia y calculadora a 48 kHz.
Desde 0.13: loopback físico ASIO, barridos de buffers, comparación de rutas/EFX,
informes, detector de posibles acoples con cortes manuales en EQ, correlación
de fase y osciloscopio. Ver [alcance y pruebas](interfaz-0.13.md). Pendientes:
validación con hardware real y latencia hasta receptores OBS/móviles; el
loopback no mide automáticamente esos otros trayectos.

## Control y expansión

- OSC y modo controlador sin abrir motor de audio.
- Salida Mackie Control/Mackie HUI y perfiles de dispositivos verificados con documentación.
- Persistencia de estados binarios VST3 y evaluación de editores nativos.
- Compensación de latencia de plugins y pruebas prolongadas
  de estabilidad/latencia con ASIO y hardware real.

Ver [control remoto](control-remoto.md) para el diseño previo de OSC/Mackie y
[la versión actual](interfaz-0.11.md) para lo ya implementado.

## AuMux Audio 0.12

Implementado: flujos UDP propios identificados, master/aux, unicast a varios
destinos, broadcast LAN, receptor OBS con puerto compartido, búfer y delay por
fuente, editor Win32 y editor Qt6 probado con libobs Linux. Ver
[la guía 0.12](interfaz-0.12.md). Pendientes: validación física prolongada,
macOS nativo y distribución Flatpak. El control X32/M32 sigue pendiente.

## Base multipista disponible en 0.14

Grabación selectiva PRE/POST por canal y buses postfader, WAV Float32/RF64 a 48 kHz,
con escritor separado y manifiesto. Quedan para ampliar: explorador/reproducción
de tomas, edición y automatización; no se incorporan instrumentos virtuales.
[Alcance implementado](interfaz-0.14.md).
