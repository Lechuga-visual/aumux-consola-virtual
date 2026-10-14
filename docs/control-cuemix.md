# Miniapp CueMix: vía de control documentada

Fuente oficial inspeccionada:
https://cdn-data.motu.com/manuals/firewire-usb-audio/896mk3_Hybrid_Manual_Win.pdf

Es el manual Windows de 2011; acredita las funciones descritas en esa
edición, no una prueba del controlador/software actual del estudio.

## Evidencia

- Página 73: ocho buses de mezcla estéreo. Cada bus puede mezclar entradas
  hacia un par de salidas seleccionable. La existencia del bus no garantiza
  su control completo por un protocolo de superficie concreto.
- Página 115: `Phones > Follow Active Mix` hace que los auriculares sigan la
  mezcla visualizada. Cambiar de mezcla puede por tanto cambiar la escucha.
- Página 116: `Control Surfaces` admite Mackie Control, Mackie HUI y Baby HUI.
  `Enabled` activa el control y `Configure…` abre su configuración.
- Página 116: con `Share surfaces with other applications` activado, CueMix
  cede la superficie al cambiar de aplicación. Para una miniapp separada
  conviene probar con esta opción desactivada, para que CueMix mantenga el
  control mientras la miniapp tiene el foco.
- Página 117, figura 9-58: configuración de tipo de superficie, entrada MIDI,
  salida MIDI, grupo y posición. El botón `Help…` abre la ayuda detallada.
- Página 97: `Mix1 Return Includes Computer` no tiene efecto en la 896mk3.
  No usar esa opción como solución de retorno a OBS para este modelo.

No se encontró una especificación de control OSC en este PDF. Esto no prueba
que ninguna versión de CueMix pueda tener OSC; impide tratarlo como soporte
documentado de este modelo a partir de esta fuente.

## Diseño seleccionado

Miniapp Windows x64 que emula una superficie Mackie Control. Para comunicar
dos aplicaciones en el mismo PC harán falta puertos MIDI virtuales de un
proveedor compatible, o una implementación propia de ese transporte.
No se incluye ni instala un controlador MIDI virtual desde este entorno.

Ruta propuesta, pendiente de validación:

`Miniapp -> puerto MIDI virtual A -> entrada de superficie de CueMix`

`Miniapp <- puerto MIDI virtual B <- salida de superficie de CueMix`

Separar los puertos evita devolver automáticamente feedback a la entrada.
La miniapp deberá mantener estado de controles y bancos y distinguir una
acción del usuario de una actualización procedente de CueMix. No dar por
implementadas funciones cuyo mapa aún no se haya comprobado.

## Prueba mínima que desbloquea la superficie

En Windows, abrir `Control Surfaces > CueMix Control Surfaces > Configure…`
(los nombres concretos pueden variar con la versión), seleccionar Mackie
Control y revisar `Help…`. Obtener de esa ayuda la configuración de puertos,
controles, bancos y selección de mezcla. Confirmar cualquier negociación
necesaria antes de enviar mensajes.

Con el dispositivo conectado, probar un fader y un mute de un canal conocido
con sus niveles de escucha controlados. Cambiar también el valor desde
CueMix y verificar el feedback. Después comprobar cambio de banco, selección
de mezcla, desconexión y recuperación. Aún no se ha ejecutado esta prueba.

Solo tras validarlo construir faders, nombres, mute/solo, panorama, selección
de mezcla y escenas hasta el alcance realmente soportado. No prometer que
Mackie Control exponga todos los ajustes de DSP o el ruteo físico.

## VST3 y OBS

El controlador de superficie modifica CueMix; no procesa audio ni hospeda
VST3 en el DSP de la MOTU. Los efectos requeridos por canal, auxiliar y master
necesitan un host en el PC y una ruta ASIO validada. El acceso simultáneo
desde OBS, la forma de entregar el master y la latencia quedan pendientes.
