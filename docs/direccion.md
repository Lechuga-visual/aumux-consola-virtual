# Dirección del proyecto

## Requisitos confirmados

- Windows de 64 bits y OBS actualizado (falta fijar versión exacta).
- Interfaz disponible: MOTU 896mk3 Hybrid.
- Preferencia: miniapp con una superficie de control mejorada para CueMix.
- Alternativa: plugin OBS con mezclador ASIO.
- Mezclas personales, escucha del programa y otras mezclas de monitoreo.
- Efectos VST3 por canal, auxiliar y master.
- Tras leer el manual aportado por el usuario, se prioriza emular una
  superficie Mackie Control por MIDI. Véase [control-cuemix.md](control-cuemix.md).

## Dos rutas de señal

Una app de control modifica el mezclador del dispositivo; el audio sigue
su ruta de hardware. No convierte al DSP de la MOTU en un host VST3.

Para aplicar VST3, el audio debe procesarse en un host en el PC y regresar
por salidas ASIO. Esa ruta incorpora latencia de buffers y efectos. Si se
combina con monitoreo directo, hay que evitar duplicar la misma señal.
Los canales que requieran efectos deben tener explícitamente seleccionada
su ruta de monitoreo. La ruta hacia OBS también debe validarse: no se asume
que el controlador permita acceso simultáneo desde varios clientes.

El motor portable existente sirve para la alternativa de mezcla en software.
No controla CueMix y no incluye aún un host VST3 ni un backend ASIO.

## Compatibilidad de control: investigación pendiente

Se leyó el manual Windows de la 896mk3 Hybrid, páginas 116–117. Documenta
Mackie Control, HUI y Baby HUI y configuración de puertos MIDI, no OSC.
La compatibilidad OSC asumida anteriormente no queda establecida por este
documento. Falta consultar la ayuda de superficies incluida en CueMix para
el mapa y la configuración detallados, y validar mensajes y realimentación
en Windows con la interfaz. No reutilizar mapas de otros modelos.

Se revisaron los README públicos de estos proyectos como referencias:

- https://github.com/RadekPilich/CueMix-Bridge — declara CueMix 5 / CueMix Pro.
- https://github.com/Silhm/motu-mcu-control — declara interfaces MOTU AVB.
- https://github.com/ixnas/Mackie-of-the-Unicorn — declara MOTU AVB, probado con 8M.

Estas referencias no acreditan compatibilidad con la 896mk3 Hybrid. No se
ha copiado ni ejecutado su código.

Inicialmente el proxy rechazó las consultas a motu.com con
`Tunnel connection failed: 403 Forbidden`. Se guardaron motu.com,
www.motu.com y cdn-data.motu.com en el borrador de dominios permitidos;
guardar el borrador no modifica por sí mismo la red de la máquina actual.
Posteriormente se descargó correctamente el PDF oficial desde el enlace
exacto aportado por el usuario. Esto acredita acceso a ese PDF; no se ha
revalidado el resto de destinos.

## Primeras comprobaciones con el PC del estudio

1. Registrar versión exacta de Windows, controlador MOTU, CueMix y OBS,
   conexión USB/FireWire usada, frecuencia y buffer.
2. Configurar Mackie Control y sus puertos MIDI según la ayuda de CueMix. Verificar un fader,
   mute y selección de mezcla, incluyendo lectura del estado y reconexión.
3. Enumerar entradas/salidas disponibles en la frecuencia elegida y definir
   cuántas mezclas estéreo/mono se necesitan y sus destinos físicos.
4. Probar un único trayecto ASIO con un VST3 conocido antes de extenderlo a
   inserciones por canal, auxiliar y master. Medir latencia real de retorno.
5. Validar aislamiento de mezclas, acceso simultáneo con OBS, reinicio,
   persistencia y ausencia de retornos duplicados.

Si se confirma control de CueMix, priorizar esa superficie y mantener el host
VST3 como componente separado. Si no existe una vía de control utilizable,
continuar por la alternativa OBS/ASIO sin automatizar la UI de CueMix como
base de un sistema destinado al uso en vivo.
