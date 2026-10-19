# AuMux 0.12 · prueba local en Windows x64

Miniapp de mezcla con canales y auxiliares configurables y master estéreo, a 48 kHz. Reutiliza DSP
y host VST3 de SoundCraft de Artcraft, con una superficie y un ruteo propios.
Es un prototipo para pruebas; todavía no está validado para una sesión en vivo.

[Ver capturas de la consola, efectos, preferencias y accesos remotos](capturas.md).

## Probar sonido sin ASIO ni interfaz externa

1. Extrae **todo** `AuMux-0.12.0-Windows-x64.zip` y ejecuta `AuMux.exe`.
   Cierra la versión anterior antes de abrir esta.
2. Pulsa **AUDIO PC**. Usa los parlantes/audífonos predeterminados de Windows
   mediante WASAPI, sin controlador ASIO ni dispositivo MOTU.
3. Pulsa **G** en Entrada 1. Se activa ruido blanco, inicialmente a −30 dBFS.
   Ahora deberías escuchar el master y ver los medidores de ese canal y master.
4. Selecciona el canal y cambia su generador a **Ruido rosa** o **Tono seno**
   desde el inspector derecho. Ajusta nivel y frecuencia allí.
5. Para probar otra mezcla, selecciona **Fader Flip → Aux 1** y sube el envío de Entrada 1.
   En **Control Room → Input**, selecciona Monitor 1. Repite con los demás monitores.

AUDIO PC reproduce **un bus estéreo elegido** a la vez. Cambiar Control Room → Input
no modifica el master, sus envíos ni las asignaciones ASIO guardadas. No
captura el micrófono del PC en esta versión; se prueban los generadores y VST3.
Para elegir otros parlantes/audífonos, cambia la salida predeterminada de
Windows con el audio detenido y vuelve a iniciar AUDIO PC.

El modo **DEMO** procesa las mismas rutas pero no reproduce por el PC ni abre
hardware. También empieza en silencio: activa el generador de los canales
que quieras probar. Puede enviar su master al receptor OBS.

**SILENCIO TOTAL** corta todos los buses y la escucha. El mismo botón cambia
a **REACTIVAR AUDIO**: púlsalo para recuperar el sonido sin detener el motor,
con una entrada gradual de un bloque. **DETENER** libera el dispositivo. Cerrar la consola
de AuMux no detiene el proceso ni el audio. Usa **Cerrar servicio** desde la bandeja
y confirma con **Sí** para terminar el motor y los servidores.

## Novedades 0.12 · AuMux Audio

1. Cierra OBS y la instancia anterior de AuMux. Extrae todo el ZIP nuevo.
2. Ejecuta `Instalar-receptor-OBS.cmd` como administrador para actualizar la DLL.
3. Abre `AuMux.exe`. La marca debe mostrar **0.12.0**.
4. En OBS, agrega **AuMux Audio**, o reutiliza tu fuente existente.
5. Abre sus propiedades y pulsa **Configurar AuMux Audio…**.

**Mismo PC:** el ajuste **Local OBS** conserva `127.0.0.1:17840` y el identificador
`AUMUX-MAIN`. En AuMux activa **MASTER → OBS**. Esta ruta sigue funcionando sin
crear envíos de red.

**Otro PC o auxiliares:** en AuMux abre **Preferencias → AuMux Audio · envíos UDP**,
agrega un flujo, selecciona la mezcla y escribe el identificador y los destinos.
Activa Transmitir y aplica. El puerto habitual es **17843**. En el receptor OBS usa
**LAN / Tailscale**, el mismo puerto e identificador y aplica. Puedes configurar
varias fuentes OBS con identificadores diferentes sobre el mismo puerto.

El editor permite búfer 0–100 ms y delay de OBS por fuente; 8 ms es el punto
inicial del preset de red, no una garantía para todas las conexiones. El delay
se comparte con el ajuste de sincronización nativo de OBS. Los modos estéreo,
mono e intercambio L/R se aplican por fuente.

[Detalles, protocolo y pruebas de 0.12](interfaz-0.12.md).

## Corrección 0.11.2

En consola local y consola remota, activa **Herramientas → Touch**. Los faders
solo responden al arrastrar su perilla blanca; el recorrido y sus marcas dejan
pasar el desplazamiento lateral sin cambiar niveles. La mezcla remota de
auxiliares también usa faders que se agarran por la perilla.

Cierra la versión anterior, extrae todo el ZIP y comprueba **0.11.2** en la consola.
Consulta [el comportamiento y las pruebas](interfaz-0.11.2.md).

## Corrección 0.11.1

Desde 0.11.1, la consola completa remota corrige la descarga incompleta de
estilos/scripts y respuestas HTTP en Windows. La corrección sigue incluida en
0.11.2. No mezcles el EXE nuevo con una copia anterior.

Consulta [la explicación del fallo y las pruebas](interfaz-0.11.1.md).

## Novedades 0.11

- El programa se llama **AuMux**, incorpora su logo y se ejecuta con **AuMux.exe**.
- **Preferencias → Redes y servidores** aparece primero. Consola remota,
  control de auxiliares y escucha tienen páginas propias. El bloque antiguo de
  control remoto se retira de la superficie principal.
- El control de auxiliares usa WebSocket: envía los últimos cambios de controles
  en tandas de 80 ms, recibe estado cada 100 ms y confirma después de aplicar.
  Si se corta la conexión, conserva la vista y se reconecta sin repetir movimientos
  pendientes de la conexión anterior. El audio usa su transporte separado.
- El pie de consola muestra CPU y RAM de este proceso y la latencia declarada por
  el driver; cuando no existe ese dato, muestra la duración de bloque identificada
  como tal. No es una medición física de ida y vuelta.
- La bandeja muestra **Abrir consola**, IP local, latencia, CPU, RAM y
  **Cerrar servicio**, en ese orden. Cerrar servicio requiere confirmación; **No**
  conserva la app y el audio activos. Cerrar la ventana del navegador tampoco
  detiene el servicio.
- El modo touch sigue siendo manual en **Herramientas → Interfaz touch**.
- Se conservan escucha con búfer 40/80/120/200 ms y reconexión automática,
  contraseñas por función y enlaces personales de auxiliares.
- El paquete sigue sin firma Authenticode: [opciones de firma](firma-windows.md).

Consulta [la guía 0.11](interfaz-0.11.md). La marca superior debe indicar AuMux 0.11.

## Control remoto web 0.8

**Preferencias → Redes y servidores → Iniciar control remoto**; después,
**Control de auxiliares → participante + auxiliar → Crear enlace**.
El participante ajusta solo sus envíos. Puedes permitir PRE/POST y limitar nivel.
Para Tailscale, usa la IP 100.x o MagicDNS en **Dirección para compartir** y aplica.
Control usa TCP 17842 y escucha TCP 17841; la escucha sigue teniendo búfer.
Consulta [la guía de control remoto y Tailscale](interfaz-0.8.md) para pasos,
permisos, reconexión y límites. Los permisos y enlaces siguen siendo compatibles; consulta la organización actual en la guía 0.11.

## Novedades 0.7

- **DYN**, debajo del EQ, abre el compresor. Desmarca Bypass y elige detector
  interno, otro canal o una entrada ASIO. Las perillas, curva y medidor GR
  funcionan en vivo, también en auxiliares y master.
- Clic derecho sobre una tira: EQ, compresor, FX, envíos, duplicar/eliminar
  y fijar a izquierda/derecha. Los campos conservan su menú de edición.
- Con pocas tiras la consola queda centrada. Al crecer, la rueda sobre el
  fondo desplaza horizontalmente y el master permanece visible a la derecha.
- **Escucha LAN → Iniciar escucha LAN** abre por defecto el puerto TCP 17841.
  Copia el enlace completo a un dispositivo de la misma red; allí selecciona
  main o auxiliar y pulsa Escuchar. Hasta ocho oyentes independientes.
  Si Windows solicita acceso, permite la red privada para esta prueba.
  Si la IP sugerida pertenece a otra tarjeta de red, sustituye solo esa IP
  por la del PC en la red compartida, conservando puerto y clave.
  Es escucha de referencia con búfer, no monitoreo de escenario.

Más detalles y capturas en [interfaz-0.7.md](interfaz-0.7.md).

## La consola

- Tiras de canal y salida con faders de recorrido graduado, nombres editables,
  panorama, mute, EQ nativo y cuatro inserts nativos/VST3 visibles.
- Inspector a la derecha con generador y envíos pre/post del canal seleccionado.
- **Fader Flip → Main Level** controla los faders principales; **Aux 1…N**
  convierte los faders de entrada en controles de envío al monitor elegido.
  Pulsa **MAIN** para volver a la mezcla principal. Con muchos auxiliares, el
  selector de envío sustituye los botones A1–A4.
- Módulos alineados por filas: entrada, efectos, auxiliares, pan, destino, nombre,
  mute/generador, nivel y fader. Verde para entradas, azul para aux y rojo para master.
- **Consola completa / Consola reducida** muestra u oculta efectos, envíos y pan
  para dar más recorrido visible al fader; no cambia el procesamiento de audio.
- **Medidores** oculta/muestra el puente superior; **Inspector** oculta/muestra
  el panel derecho. Allí, **Control Room → Input** elige el bus para AUDIO PC
  y muestra el medidor de ese bus, aunque otro modo esté procesando audio.
  El texto indica si el PC reproduce o si se está usando DEMO/ASIO.
- Medidores estéreo **post-fader** por canal y salida, más puente de medidores.
  Muestran Peak/RMS en dBFS, con integración de potencia RMS de 300 ms en el motor, retención
  de picos y señal de clipping enclavada. No son VU analógicos calibrados.
- Clic sobre un medidor o **Restablecer picos** borra su retención visual.
- Doble clic en pan lo centra. Doble clic en un fader lo devuelve a 0 dB. Las marcas se alinean con el centro
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

## Estructura y EQ en vivo

**+ Canal** y **+ Aux** están sobre los bancos de entradas y salidas: agregan
una tira mono y la seleccionan sin detener el audio. El contador a su lado abre
la configuración. **Canales y auxiliares**, en la barra superior, permite elegir 1–64 canales y 0–32 auxiliares, incluso con
AUDIO PC, DEMO o ASIO activo. Se conservan las primeras tiras; reducir retira
las últimas y sus inserts/envíos. Guarda una escena antes de quitarlas si las
necesitarás después. Las tiras nuevas empiezan sin generador ni envíos.

El botón **M / ST** de cada tira cambia mono/estéreo en vivo. El inspector
permite asignar entrada física L/mono y R; no depende del número de tiras.
Puedes reutilizar entradas o elegir ninguna. Un canal mono usa L y una salida
aux mono mezcla ambos lados en una única señal, reproducida al centro en PC.
El master permanece estéreo. Al ampliar un bus a estéreo, se valida que el
segundo conector no pise otra salida. En ASIO se abren todas las salidas
anunciadas por el driver (hasta 64) al arrancar, para permitir estos cambios.
Cambiar el dispositivo, buffer o entradas físicas abiertas requiere detener.

Pulsa el módulo **EQ** para abrir la curva interactiva y ocho bandas FreeEQ8: campana, low/high
shelf, pasa altos/bajos y pasa banda, ±18 dB y Q 0.1–18. En shelves, Q controla
pendiente y satura en Q=2; ganancia se ignora en filtros de corte/pasa banda.
Arrastra un punto horizontalmente para frecuencia y verticalmente para
ganancia; la rueda sobre el punto modifica Q. Selecciona una banda para usar
sus perillas: arrastra arriba/abajo, Mayús para ajuste fino y doble clic para
restablecer. Los valores bajo las perillas también admiten edición exacta.
Está antes de los inserts, también en aux/master. Curva, tipos, ajustes y
bypass se guardan en la escena y funcionan en vivo. Se integra su núcleo de
fase mínima; no el analizador, fase lineal ni todas las funciones del plugin.

## Ventana de aplicación

En Windows se intenta abrir una ventana de Edge/Chrome en modo aplicación,
sin pestañas ni barra de direcciones, con un perfil separado del navegador
habitual. Si no están disponibles, se abre el navegador predeterminado.
El motor de audio es un proceso Rust/C++ local; la superficie sigue siendo
HTML/CSS/JS. No se ha sustituido por un toolkit gráfico nativo.

El inicio normal no abre una terminal. Para diagnóstico desde PowerShell puedes
usar `AuMux.exe --no-browser`; las opciones de prueba también admiten ejecución
sin bandeja. El botón de pantalla
completa amplía la superficie. El escritorio está pensado para pantallas de
1280 píxeles o más; las entradas se desplazan horizontalmente si hace falta.

## Con la MOTU 896mk3 Hybrid

1. Instala el controlador oficial, conecta la interfaz y cierra otras apps
   que estén usando su ASIO.
2. Abre **MOTOR DE AUDIO**, pulsa **Actualizar dispositivos** y elige la MOTU.
3. Selecciona las entradas físicas que abrirá el driver (hasta 64) y buffer; por ejemplo 48 kHz / 256
   muestras si el controlador lo admite.
4. Desactiva los generadores de los canales que deban recibir audio físico.
5. Asigna una salida por bus mono o dos contiguas por bus estéreo, sin solaparlas. La numeración comienza en 1
   y corresponde al orden del controlador; confirma sus conectores reales.
6. Cierra el panel de motor y pulsa **ASIO**. Comprueba cada mezcla por separado.

Entrada y salida se abren en un único stream dúplex del mismo dispositivo.
El ruteo CueMix existente puede duplicar el monitoreo: usa una sola ruta al
comprobar cada señal. Para cambiar de ASIO a AUDIO PC o DEMO, pulsa DETENER.

Los envíos pre-fader no siguen el fader principal; los post-fader sí. Mute
corta todos los envíos del canal. El mute del master no silencia auxiliares.

## VST3 y escenas

Con el audio detenido, pulsa un insert vacío **＋ FX**. Aparece una lista de bundles instalados,
con búsqueda por nombre/ruta. Selecciona uno: se leen sus efectos y parámetros;
selecciona la clase si contiene varias, ajusta parámetros y aplica el insert.
Se muestra su nombre real en la tira.

La primera lista del panel ofrece **FreeVox8 (filterbank)** y los **20 efectos
MDA** integrados. No necesitas instalar sus VST3. Comparten los cuatro slots
con VST3 externos y se procesan en serie; el EQ no consume slots. El panel
ofrece perillas para MDA y un panel propio para FreeVox8. Los parámetros
nativos se ajustan en vivo. FreeVox8 es un vocoder creativo con
portadora interna y cuatro modos; no es un compresor vocal. No se exponen
su motor espectral, sidechain externo o entrada MIDI. Los instrumentos
virtuales quedan fuera del alcance del proyecto.

La lista busca recursivamente en `%COMMONPROGRAMFILES%\VST3` (normalmente
`C:\Program Files\Common Files\VST3`) y `%LOCALAPPDATA%\Programs\Common\VST3`.
También admite `VST3_PATH` y **Carpeta adicional → Actualizar lista**.
La búsqueda enumera archivos sin ejecutar todos los plugins: solo carga el
seleccionado. El nombre inicial es el del bundle; al leerlo se muestran los
nombres reales de sus clases. Se mantiene la entrada manual para ubicaciones
especiales. Los VST2 no aparecen ni se alojan en esta versión.

Hay hasta **cuatro inserts en serie por canal, auxiliar y master**. La lista
lateral selecciona el efecto mostrado. **+ FX** añade otro desde esa ventana;
**Quitar** o Supr con la lista enfocada lo elimina. ↑/↓ cambia el orden.
Los cambios de parámetros se guardan al aplicar, cambiar de efecto o cerrar.
Los parámetros nativos existentes se actualizan automáticamente en vivo;
los VST3 requieren detener el audio para editarse y usan un panel genérico de
deslizadores y valores, sin abrir su ventana gráfica propia. Las casillas de
la lista controlan bypass en vivo. Agregar, quitar y reordenar requiere detener.

El selector **Presets** ofrece los valores iniciales y tus presets guardados.
Escribe un nombre y pulsa Guardar; elige uno para recuperarlo. También puedes
borrarlo, exportarlo o importar un JSON del mismo efecto. La biblioteca se
conserva en `%LOCALAPPDATA%\MonitorMix\presets.json`; puedes respaldarla.
La ruta interna anterior se mantiene para no perder las bibliotecas existentes.
Se guardan parámetros numéricos, no bancos binarios ni presets internos VST3.

Pulsa **Envíos ↗** en la tira de entrada para abrir el nivel principal, paneo
y envíos PRE/POST. **Agregar envío** elige un aux disponible; **+ Aux nuevo**
crea otro aux y lo añade. Quitar apaga ese envío (−∞). Todo funciona en vivo.
[Capturas y guía de los paneles 0.6](interfaz-0.6.md).

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
   AuMux conserva ese nombre, el ID de fuente y `monitor-mix-receiver.dll` para
   mantener las escenas y el receptor existentes.
   Crea una sola fuente y reutilízala en las demás escenas.
3. Activa **MASTER → OBS** y arranca AUDIO PC, DEMO o ASIO.
4. AuMux debe mostrar **OBS recibe** cuando la fuente confirma recepción.
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

La validación automática del motor incluye medición
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
.\AuMux.exe --self-test
```

## OSC y superficies Mackie

Se dejó anotada la dirección futura: mezclador independiente o controlador MIDI
Mackie Control/HUI, además de OSC. No están implementados en 0.11. El mapa para
CueMix/MOTU necesita comprobar documentación y mensajes con el equipo.

Las pruebas de C++ nativo comprueban los 21 inserts y el EQ con ASan/UBSan,
valores límite y bloques de 1–8192 muestras, y verifican que el procesamiento
no asigna ni libera memoria C++. La prueba del callback Rust comprueba crecer
hasta 64/32 y reducir sin reiniciar ni asignar/liberar memoria Rust. Esto no
certifica latencia ni carga admisible con plugins reales: depende del PC y del
buffer. La conexión ASIO usa ahora una ABI abierta basada en WineASIO, sin SDK
propietario; compilada para Windows x64, pendiente de prueba con tu driver.

La aplicación combinada desde 0.5 es GPL-3.0-only; fuentes y licencias incluidas.
Grabación multipista y OSC/Mackie pendientes.

Si no aparecen **+ Canal**, **+ Aux** y **Canales y auxiliares**, cierra el
proceso anterior y abre el ejecutable del ZIP nuevo: la marca superior debe
mostrar **AuMux 0.12.0**. Se prueba en Windows extrayendo todo el ZIP; no mezcles
archivos de varias versiones.

Las novedades de dinámica, superficie adaptativa y escucha LAN se describen en
[interfaz-0.7.md](interfaz-0.7.md). Otros dispositivos Windows y optimización de escucha de baja
latencia quedan en [la hoja de ruta](hoja-de-ruta.md).
