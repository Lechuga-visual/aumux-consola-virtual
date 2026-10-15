# Dirección prevista: mezclador y superficie de control

Requisito acordado: AuMux debe poder funcionar como mezclador independiente
y, en una etapa posterior, como superficie de control OSC o MIDI Mackie Control /
Mackie HUI. Esta dirección queda documentada; **la versión 0.11 no implementa esos
protocolos ni controla CueMix/MOTU**. El trabajo actual es la superficie de mezcla.

El control de mezclas personales por web ya está disponible en 0.8, con acceso
limitado a un auxiliar por enlace y direcciones LAN/Tailscale/MagicDNS. En 0.9 se
añaden enlaces compartidos con selección de auxiliar y consola completa con
autorización independiente. En 0.11 el control de auxiliares pasa a WebSocket con tandas de cambios de 80 ms
y estado cada 100 ms. Consulta [la guía web actual](interfaz-0.11.md). OSC y Mackie
siguen pendientes.

## Modos que se quieren incorporar

| Modo | Motor de AuMux | Destino de los controles |
| --- | --- | --- |
| Independiente | ASIO, AUDIO PC o DEMO | Mezcla, VST3, auxiliares y master propios |
| Controlador Mackie Control | No requiere abrir audio | Aplicación/dispositivo externo mediante MIDI |
| Controlador Mackie HUI | No requiere abrir audio | Aplicación/dispositivo externo mediante su perfil HUI |

OSC permitiría controlar la mezcla propia desde otra superficie y publicar su
estado. La salida OSC hacia otros programas requerirá un perfil de direcciones
del destino: no existe un mapa universal que controle cualquier equipo.

## Separación para la implementación posterior

La superficie debe emitir acciones con identidad de canal/bus (nivel en dB,
panorama, mute, envío y selección de banco). Un adaptador local aplicará esas
acciones al motor y los adaptadores OSC/MCU/HUI las traducirán al destino elegido.
El estado recibido debe actualizar los controles sin reenviar el mismo cambio en
bucle. La selección del modo y los puertos deberán estar visibles, para distinguir
qué mezcla o dispositivo se está manipulando.

Los adaptadores de control no deben hacer operaciones de red/MIDI en el callback
de audio. Medidores y mensajes de realimentación necesitan una frecuencia de
actualización limitada y colas acotadas. Controlador puro debe poder iniciarse
sin ASIO, sin interfaz de audio y sin transmitir audio al puente de OBS.

## Qué falta comprobar con la MOTU 896mk3 Hybrid

- Qué modo de superficie admite la versión de CueMix FX instalada y cómo se
  seleccionan los puertos de entrada/salida MIDI en Windows.
- Si el control debe pasar por CueMix y un puerto MIDI virtual. No asumir que
  enviar MIDI directamente a una interfaz controla su DSP.
- Mensajes de inicialización y mantenimiento, faders, pan, mute, selección de
  mezcla, bancos y realimentación. MCU y HUI necesitan perfiles diferentes.
- Correspondencia de canales y buses, unidades de nivel y comportamiento de
  faders, además de guardar/restaurar la sesión.

Se debe contrastar el manual con mensajes reales y realizar primero una prueba
mínima de un fader, mute y selección de mezcla. Los valores y números de canal
no se inventarán a partir de la apariencia de la interfaz. Las notas anteriores
sobre CueMix siguen en [control-cuemix.md](control-cuemix.md); los experimentos
OSC anteriores no constituyen una integración activa con la MOTU.
