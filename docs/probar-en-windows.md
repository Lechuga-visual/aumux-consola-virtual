# Monitor Mix 0.3 · prueba local en Windows x64

Miniapp de mezcla con ocho canales, cuatro monitores y master. Reutiliza DSP
y host VST3 de SoundCraft de Artcraft, con una superficie y un ruteo propios.
Es un prototipo para pruebas; todavía no está validado para una sesión en vivo.

## Probar sonido sin ASIO ni interfaz externa

1. Extrae **todo** `MonitorMix-0.3.0-Windows-x64.zip` y ejecuta `MonitorMix.exe`.
   Cierra la versión anterior antes de abrir esta.
2. Pulsa **AUDIO PC**. Usa los parlantes/audífonos predeterminados de Windows
   mediante WASAPI, sin controlador ASIO ni dispositivo MOTU.
3. Pulsa **GEN** en Entrada 1. Se activa ruido blanco, inicialmente a −30 dBFS.
   Ahora deberías escuchar el master y ver los medidores de ese canal y master.
4. Selecciona el canal y cambia su generador a **Ruido rosa** o **Tono seno**
   desde el inspector izquierdo. Ajusta nivel y frecuencia allí.
5. Para probar otra mezcla, pulsa **MON 1** y sube el envío de Entrada 1.
   En **Escucha PC**, selecciona Monitor 1. Repite con los demás monitores.

AUDIO PC reproduce **un bus estéreo elegido** a la vez. Cambiar Escucha PC
no modifica el master, sus envíos ni las asignaciones ASIO guardadas. No
captura el micrófono del PC en esta versión; se prueban los generadores y VST3.
Para elegir otros parlantes/audífonos, cambia la salida predeterminada de
Windows con el audio detenido y vuelve a iniciar AUDIO PC.

El modo **DEMO** procesa las mismas rutas pero no reproduce por el PC ni abre
hardware. También empieza en silencio: activa el generador de los canales
que quieras probar. Puede enviar su master al receptor OBS.

**SILENCIO TOTAL** corta todos los buses y la escucha. Para reanudar, detén y
vuelve a iniciar el motor. **DETENER** libera el dispositivo. Cerrar la consola
de Monitor Mix termina la app; cerrar solo su ventana gráfica no detiene el
proceso ni el audio.

## La consola

- Tiras de canal y salida con faders de recorrido graduado, nombres editables,
  panorama, mute y cuatro inserts VST3 visibles.
- Inspector con generador y envíos pre/post del canal seleccionado.
- **MAIN** controla los faders principales; **MON 1–4** convierte los faders
  de entrada en controles de envío hacia el monitor seleccionado.
- Medidores estéreo **post-fader** por canal y salida, más puente de medidores.
  Muestran Peak/RMS en dBFS, con integración de potencia RMS de 300 ms en el motor, retención
  de picos y señal de clipping enclavada. No son VU analógicos calibrados.
- Clic sobre un medidor o **Restablecer picos** borra su retención visual.
- Doble clic en un fader lo devuelve a 0 dB. Las marcas se alinean con el centro
  de la perilla; puedes pulsar una marca o escribir el nivel bajo el fader
  (por ejemplo, −6 o −5.3) y pulsar Enter. La edición numérica conserva décimas.
- La escala de medidores es independiente de la escala del fader: −60 a 0 dBFS.
  Un seno de pico −20 dBFS tiene RMS −23.01 dBFS. Los medidores de salida
  miden la señal entregada después del recorte; el contador Clips registra sobrecargas.

El medidor del canal sigue su nivel post-fader principal incluso al editar
un envío pre-fader. Por eso un monitor pre-fader puede seguir recibiendo señal
con el fader principal abajo: compruébalo en el medidor del monitor.

Los generadores son independientes por canal y **sustituyen** la entrada de
ese canal antes de sus inserts, también en ASIO. **Entrada física** desactiva
el generador. No se inyecta ruido automáticamente en canales no activados.

## Ventana de aplicación

En Windows se intenta abrir una ventana de Edge/Chrome en modo aplicación,
sin pestañas ni barra de direcciones, con un perfil separado del navegador
habitual. Si no están disponibles, se abre el navegador predeterminado.
El motor de audio es un proceso Rust/C++ local; la superficie sigue siendo
HTML/CSS/JS. No se ha sustituido por un toolkit gráfico nativo.

La consola de texto permanece abierta para diagnóstico. El botón de pantalla
completa amplía la superficie. El escritorio está pensado para pantallas de
1280 píxeles o más; las entradas se desplazan horizontalmente si hace falta.

## Con la MOTU 896mk3 Hybrid

1. Instala el controlador oficial, conecta la interfaz y cierra otras apps
   que estén usando su ASIO.
2. Abre **MOTOR DE AUDIO**, pulsa **Actualizar dispositivos** y elige la MOTU.
3. Selecciona 1–8 entradas, frecuencia y buffer; por ejemplo 48 kHz / 256
   muestras si el controlador lo admite.
4. Desactiva los generadores de los canales que deban recibir audio físico.
5. Asigna pares de salida distintos a cada bus. La numeración comienza en 1
   y corresponde al orden del controlador; confirma sus conectores reales.
6. Cierra el panel de motor y pulsa **ASIO**. Comprueba cada mezcla por separado.

Entrada y salida se abren en un único stream dúplex del mismo dispositivo.
El ruteo CueMix existente puede duplicar el monitoreo: usa una sola ruta al
comprobar cada señal. Para cambiar de ASIO a AUDIO PC o DEMO, pulsa DETENER.

Los envíos pre-fader no siguen el fader principal; los post-fader sí. Mute
corta todos los envíos del canal. El mute del master no silencia auxiliares.

## VST3 y escenas

Con el audio detenido, pulsa **＋ VST3**. Aparece una lista de bundles instalados,
con búsqueda por nombre/ruta. Selecciona uno: se leen sus efectos y parámetros;
selecciona la clase si contiene varias, ajusta parámetros y aplica el insert.
Se muestra su nombre real en la tira.

La lista busca recursivamente en `%COMMONPROGRAMFILES%\VST3` (normalmente
`C:\Program Files\Common Files\VST3`) y `%LOCALAPPDATA%\Programs\Common\VST3`.
También admite `VST3_PATH` y **Carpeta adicional → Actualizar lista**.
La búsqueda enumera archivos sin ejecutar todos los plugins: solo carga el
seleccionado. El nombre inicial es el del bundle; al leerlo se muestran los
nombres reales de sus clases. Se mantiene la entrada manual para ubicaciones
especiales. Los VST2 no aparecen ni se alojan en esta versión.

Hay hasta **cuatro inserts en serie por canal, auxiliar y master**. Los botones
↑/↓ del panel cambian el orden. El botón de encendido junto a cada insert
permite bypass en vivo sin modificar el resto de la cadena. El editor sigue
siendo genérico; aún no se abre la ventana gráfica propia del plugin.

**Guardar escena** exporta un JSON; **Abrir escena** lo carga con el audio
detenido. Se migran los archivos 0.1 y sus inserts únicos. Se guardan selección,
bypass y parámetros numéricos, no el estado binario completo ni presets internos.
Los VST3 deben seguir instalados en su ruta. Los índices de dispositivo pueden
cambiar: vuelve a seleccionar la MOTU si corresponde.

No hay compensación automática de latencia. El inspector informa la suma de
latencias declaradas por los inserts, incluidos los que estén en bypass.
Plugins con lookahead pueden desalinear señales. El host de SoundCraft tampoco
activa sidechains; la compatibilidad con tus plugins necesita prueba local.

## Master hacia OBS

1. Cierra OBS y ejecuta el nuevo `Instalar-receptor-OBS.cmd` del ZIP **como
   administrador**. Corrige la ruta de 0.2: instala en
   `%PROGRAMDATA%\obs-studio\plugins\monitor-mix-receiver\bin\64bit`,
   verifica la copia y conserva un respaldo si ya existía el receptor.
2. Abre OBS x64 **32.1 o posterior** y añade **Monitor Mix — master local**.
   Crea una sola fuente y reutilízala en las demás escenas.
3. Activa **MASTER → OBS** y arranca AUDIO PC, DEMO o ASIO.
4. Monitor Mix debe mostrar **OBS recibe** cuando la fuente confirma recepción.
   Comprueba el medidor en OBS y realiza una grabación corta. Si indica
   **OBS sin receptor**, ejecuta `Diagnosticar-OBS.cmd`: imprime rutas y las
   líneas relevantes del último registro sin modificar OBS.

OBS 32.2.2 x64 instalado es compatible. Para OBS portable, usa
`Instalar-receptor-OBS.cmd -ObsRoot "D:\OBS"`, indicando la carpeta que contiene
`bin\64bit\obs64.exe`. La instalación portable tiene un destino diferente.
El instalador verifica archivos; el registro y la confirmación en vivo verifican
carga y recepción. OBS en modo seguro no carga este plugin.

El puente usa UDP de loopback en el puerto 17840 y audio float32 estéreo.
No exige que OBS abra ASIO ni instalar un cable de audio virtual. Solo envía
el master; los auxiliares no aparecen como fuentes separadas de OBS.
Con AUDIO PC ya tienes escucha local: mantén desactivado el monitoreo de la
fuente en OBS para evitar oír la misma señal dos veces.

Para desinstalar el receptor, cierra OBS y elimina únicamente
`%PROGRAMDATA%\obs-studio\plugins\monitor-mix-receiver`.
La copia 0.2 en `%APPDATA%` no se utiliza en Windows normal; puedes eliminar
únicamente esa carpeta `monitor-mix-receiver` antigua con OBS cerrado.

[Cómo funciona el puente, latencia y alternativa ReaStream](puente-obs.md).

## Validación y diagnóstico

En este entorno se comprobaron once pruebas del motor, incluidas medición
Peak/RMS de un seno conocido, ruido independiente y su espectro, selección de
bus para escucha PC, migración de escenas y cadenas/bypass con un VST3 binario
real de prueba. También se probaron la interfaz en Chromium y el puente desde
la app hasta captura real de libobs en Linux.

Se compilaron los binarios Windows con ASIO **y WASAPI**. No se reprodujo audio
nativamente en Windows aquí: faltan tu prueba de la salida normal del PC, la
MOTU, efectos particulares, latencia, sincronía de video y sesiones largas.
El recorte a ±1 evita valores fuera de escala; no sustituye un limitador.

Si falla AUDIO PC, indica la salida predeterminada de Windows, frecuencia,
buffer y el error de la app/consola. Si falla ASIO, añade controlador y canales.
`XRuns` cuenta avisos del driver; no mide latencia. El contador UDP cubre cola y errores de envío. **OBS recibe** confirma que
la fuente recibió paquetes y los entregó a libobs; no verifica su mute, fader,
selección de pistas o presencia en la escena de grabación.

Prueba de lógica sin hardware desde PowerShell:

```powershell
.\MonitorMix.exe --self-test
```
