# Hoja de ruta

Estado a partir de Monitor Mix 0.7. Todos los caminos de audio seguirán a
**48 kHz** en esta etapa. Esta lista registra trabajo futuro, no funciones
ya disponibles ni fechas comprometidas. Los instrumentos virtuales quedan
fuera del alcance; el proyecto se centra en mezcla y procesamiento de audio.

## Dispositivos y subsistemas de Windows

Agregar selección explícita de entradas y salidas fuera de ASIO: empezar por
WASAPI compartido/exclusivo y evaluar DirectSound/MME según soporte del backend.
Mostrar dispositivos, canales, formatos y latencia disponible; gestionar cambios
del dispositivo predeterminado, desconexión y errores. Capturar micrófonos y
entradas del PC, no solo reproducir por la salida predeterminada como ahora.
Convertir frecuencia cuando el dispositivo no abra a 48 kHz y definir el
tratamiento de relojes independientes antes de habilitar rutas combinadas.

La agregación de varios dispositivos ASIO queda **fuera del alcance acordado**.

## Mezclas personales remotas por web

Crear una superficie web con copia de las tiras de entrada y niveles de envío.
Cada participante tendrá asignado **un auxiliar** y podrá ajustar su propia
mezcla, sin acceso al main ni a los auxiliares de otras personas. Prever niveles,
mutes de envío y estado PRE/POST según permisos del operador; definir qué
controles se comparten entre clientes y cómo se resuelven cambios simultáneos.

La asignación, revocación y permisos de cada enlace se validarán en el servidor.
Prever reconexión, estado sincronizado, límites de nivel y autorización para
estéreo/pan de envío. La escucha LAN de 0.7 será un punto de partida para
comprobar audio; el retardo del navegador seguirá siendo independiente del
control remoto. La API local de administración no se expondrá directamente.

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
[la versión actual](interfaz-0.7.md) para lo ya implementado.
