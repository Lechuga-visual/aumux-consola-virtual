# AuMux 0.11.2 · faders touch por la perilla

[Descargar Windows x64](../dist/AuMux-0.11.2-Windows-x64.zip).

[Ver la galería de capturas de esta versión](capturas.md).

En la consola local y en la consola completa remota, con el modo **Touch**
activado, solo la perilla blanca del fader recibe gestos de arrastre. La mezcla
remota de auxiliares usa el mismo comportamiento. El recorrido y las marcas de
la escala no cambian el nivel al tocarlos; desde ellos se puede desplazar el
banco de canales hacia los lados.

El comportamiento de los faders de la consola con Touch desactivado se conserva.
Los campos numéricos y el teclado permiten seguir ajustando niveles exactos.
El doble clic para volver a 0 dB también se realiza sobre la perilla.
La protección se aplica a canales, envíos y faders de buses, incluido el master.

## Pruebas

`desktop/tests/touch-access-ui.cjs` utiliza gestos táctiles reales del protocolo
DevTools de Chromium contra la aplicación en ejecución. En la consola local,
la consola remota y el control auxiliar comprueba:

- Tocar el recorrido o arrastrarlo verticalmente mantiene los niveles.
- Un gesto lateral iniciado en el recorrido desplaza el banco sin cambiar faders.
- Arrastrar la perilla sí cambia el nivel y actualiza la configuración.
- La página no se desplaza verticalmente durante esos gestos.

Estas pruebas se ejecutan en Chromium sobre Linux; la compilación Windows se
incluye para comprobar el resultado en el equipo y los navegadores del usuario.
No hay cambios en el motor de audio, los búferes ni los protocolos de red.

## Actualizar

1. Cierra el servicio anterior desde la bandeja de Windows.
2. Extrae todo el ZIP y ejecuta **AuMux.exe**.
3. Recarga las páginas remotas; la consola debe mostrar **0.11.2**.
4. En la consola local o remota, activa Touch manualmente en Herramientas.
5. Desliza sobre el recorrido para navegar y toma la perilla para mezclar.
