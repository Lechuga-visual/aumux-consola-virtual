# Probar Monitor Mix 0.1 en Windows x64

Este es un **prototipo para pruebas locales**, no una versión validada para
usar en una sesión del estudio. No requiere REAPER, Ardour, CueMix, Rust ni
Visual Studio para ejecutar el paquete portable.

## Sin la MOTU

1. Extrae **todo** `MonitorMix-0.1.0-Windows-x64.zip` en una carpeta.
2. Ejecuta `MonitorMix.exe`. Deja abierta su ventana de consola; la interfaz
   se abre en el navegador local. Si el navegador no se abre, copia la
   dirección que imprime esa ventana.
3. Pulsa **Probar DEMO sin hardware**. Genera señales de prueba y muestra el
   master. No abre un dispositivo ni reproduce por los altavoces del PC.
4. Selecciona **Monitor 1 — envíos** y sube el fader de Entrada 1. Comprueba
   que aparece señal en ese monitor. Repite con Monitor 2 y otro nivel.
5. Pulsa **SILENCIO TOTAL**: corta todos los buses. Para volver a sonar,
   detén el audio y vuelve a iniciar DEMO/ASIO.

La app sigue procesando si cierras la pestaña. **Detener** libera el audio;
cerrar la consola finaliza la app. Solo ejecutar una instancia a la vez.

También puedes ejecutar desde PowerShell:

```powershell
.\MonitorMix.exe --self-test
```

Debe informar `PASS` sobre mezcla independiente y silencio de emergencia.
Ese comando no abre hardware ni valida el controlador ASIO.

## Con la MOTU 896mk3 Hybrid

1. Instala el controlador oficial MOTU para tu sistema y conecta la interfaz.
   Cierra otras apps que puedan tener abierto el mismo controlador ASIO.
2. En Monitor Mix, pulsa **Actualizar dispositivos** y selecciona la MOTU.
3. Selecciona la cantidad de entradas a abrir (1–8), frecuencia y buffer.
   Empieza por 48 kHz / 256 muestras si el controlador lo admite.
4. Los canales representan las primeras entradas del controlador, en su
   orden ASIO. Confirma qué conectores corresponden a esos índices.
5. Asigna el master y los monitores a pares físicos distintos. La numeración
   de salida empieza en 1. `Sin asignar` no envía ese bus al hardware.
   Un par debe existir en el dispositivo y no puede compartirse entre buses.
6. Inicia ASIO con niveles de escucha bajos. Los envíos parten en −∞; abre
   cada mezcla desde el selector de faders y sube solo los canales necesarios.
7. Comprueba cada salida por separado. Cambiar el fader principal de un
   canal no afecta a su envío **pre-fader**. El envío post-fader sí lo sigue.
   El mute del canal corta todos sus envíos. El mute del master solo corta
   el master; los auxiliares siguen independientes.

El backend abre entrada y salida del **mismo dispositivo** en un único stream
dúplex RtAudio/ASIO. No usa el mezclador CueMix. El monitoreo directo que ya
exista en la MOTU puede sumarse al de la app: deja una sola ruta para la señal
que estés comprobando, para poder medir y escuchar el resultado correctamente.

## VST3

1. Detén el audio. Pulsa **VST3 +** en un canal, monitor o master.
2. Escribe la ruta completa al archivo/carpeta `.vst3` instalado en este PC.
3. Pulsa **Leer**, selecciona el efecto y ajusta sus parámetros numéricos.
4. Pulsa **Aplicar insert** y vuelve a iniciar DEMO/ASIO.

Esta versión tiene **un insert estéreo por canal y bus**, con editor genérico.
No abre la ventana nativa del efecto. Los canales mono se duplican a estéreo
antes del insert y conservan su salida estéreo. El pre-fader se toma después
del insert. Cambiar efectos o sus parámetros requiere detener/reiniciar.

El archivo de ajustes guarda selección y valores numéricos de los efectos;
todavía no guarda sus estados binarios, muestras ni presets internos. Los
plugins deben seguir instalados en la misma ruta. Cambiar a otra versión del
mismo plugin desde otra ruta requiere reiniciar la app.

**No hay compensación automática de latencia.** Se muestran las muestras
declaradas por cada insert (ocho canales, master y cuatro monitores). Un
lookahead o un efecto de alta latencia puede desalinear señales. El host VST3
de SoundCraft tiene limitaciones propias, incluidas sidechains no activas;
no se ha comprobado compatibilidad con tus plugins particulares.

## Master hacia OBS

1. Cierra OBS.
2. Ejecuta `Instalar-receptor-OBS.cmd`. Copia el receptor a la carpeta de
   plugins del usuario actual; no requiere modificar Program Files.
3. Abre **OBS de 64 bits 32.1 o posterior**. Añade una fuente
   **Monitor Mix — master local**. Crea una sola y reutilízala en otras escenas.
4. En Monitor Mix activa **Enviar master a OBS** y arranca DEMO o ASIO.
5. Comprueba señal en el mezclador de OBS y haz una grabación corta.

La comunicación usa audio estéreo float32 por UDP de loopback, puerto 17840.
Los sockets se usan fuera del callback ASIO. No requiere un cable virtual ni
que OBS abra el mismo controlador ASIO. El master puede llegar a OBS aunque
su salida física esté `Sin asignar`; los monitores continúan en sus salidas.
Los auxiliares no se envían como fuentes separadas a OBS en esta versión.

Mantén desactivado el monitoreo de esta fuente en OBS mientras verificas las
salidas de la miniapp. Evita añadir también una captura duplicada de la MOTU.
Si no aparece la fuente, comprueba arquitectura/versión de OBS y su registro.
Si el puerto está ocupado, el receptor lo informa en ese registro.

Para desinstalar, cierra OBS y elimina únicamente
`%APPDATA%\obs-studio\plugins\monitor-mix-receiver`.

## Ajustes y diagnóstico

**Descargar ajustes** exporta un JSON; **Cargar ajustes** lo restaura con el
audio detenido. Los índices de dispositivo pueden cambiar al reconectar:
vuelve a seleccionar la MOTU después de cargar un archivo.

Si falla ASIO, copia el error visible y registra dispositivo, número de
entradas, frecuencia, buffer solicitado y salidas asignadas. Para fallos de
VST3 registra su nombre, versión y el paso que falla. `XRuns` cuenta avisos
del driver; no es una medición de latencia de ida y vuelta. Los picos fuera
de ±1 se recortan y se contabilizan; no hay limitador de calidad de producción.

## Qué se validó aquí

- Compilación del ejecutable Windows x64 con ASIO y del receptor OBS x64.
- Lógica del motor en Linux: cuatro pruebas de ruteo, mute, límites y clipping.
- Prueba adicional con un VST3 binario real de test en canal, auxiliar y master.
- Prueba en Chromium de la interfaz, envíos, silencio, VST3 y exportación.
- Señal desde DEMO de la app, pasando por el puente, hasta callbacks reales
  de captura de libobs 30.2 en Linux.

No se ejecutó nativamente el paquete en Windows, ni se comprobó ASIO con la
MOTU, plugins comerciales, latencia, sincronía con video o estabilidad en
sesiones largas. El receptor Windows se compiló contra la API OBS 32.1; su
carga en OBS Windows es parte de tu prueba local.
