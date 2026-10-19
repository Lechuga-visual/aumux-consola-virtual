# Dirección del proyecto

## Requisitos confirmados

- Windows de 64 bits y OBS actualizado.
- Mezclador independiente con entradas y salidas ASIO.
- Mezclas personales, escucha del programa y otras mezclas de monitoreo.
- Canales y auxiliares configurables en cantidad y formato mono/estéreo.
- Efectos VST3 por canal, auxiliar y master.
- Procesamiento a 48 kHz y prioridad de baja latencia en tiempo real.
- Entrega de audio a OBS mediante el receptor AuMux Audio.
- Pruebas sin hardware mediante generadores y escucha AUDIO PC.

Consulta [la guía de Windows](probar-en-windows.md) y
[AuMux Audio 0.12](interfaz-0.12.md) para el funcionamiento disponible.

## Mezcla propia y control externo

AuMux procesa el audio y los efectos en el PC. La ruta ASIO incorpora latencia
por los buffers del dispositivo y los efectos. Si se combina con monitoreo
directo, hay que evitar duplicar la misma señal. Cada mezcla debe tener
seleccionado su destino de monitoreo.

Un modo de control externo modificaría el mezclador de otro dispositivo o
aplicación, mientras el audio seguiría su propia ruta. Ese modo no convertiría
el DSP del dispositivo en un host VST3. Los protocolos OSC, Mackie Control y
Mackie HUI y sus perfiles de dispositivos quedan pendientes; véase
[control remoto](control-remoto.md).

## Comprobaciones con hardware

1. Registrar versión de Windows, controlador de audio y OBS, conexión usada,
   frecuencia y tamaño de buffer.
2. Enumerar entradas y salidas disponibles a 48 kHz y definir las mezclas
   mono/estéreo y sus destinos físicos.
3. Probar un trayecto ASIO con un VST3 conocido antes de extenderlo a
   inserciones por canal, auxiliar y master. Medir latencia real de retorno.
4. Validar aislamiento de mezclas, entrega de audio a OBS, reinicio,
   persistencia y ausencia de retornos duplicados.
5. Comprobar estabilidad y carga de procesamiento durante sesiones largas.

Para cualquier futura integración de control externo se debe contrastar su
manual con mensajes reales. La prueba inicial debe cubrir un fader, mute,
selección de mezcla, realimentación y reconexión. No se asume que un mapa de
control de otro modelo sea compatible.
