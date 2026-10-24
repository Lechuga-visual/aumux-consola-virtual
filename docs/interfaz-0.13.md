# AuMux 0.13 · herramientas de prueba y análisis

[Windows x64](../dist/AuMux-0.13.0-Windows-x64.zip) · [Instalación](probar-en-windows.md)

## Herramientas → Latencia y diagnóstico

El submenú agrupa **Datos en tiempo real**, **Medidor de latencia loopback**,
**Pruebas de buffers y estabilidad**, **Comparador de rutas y EFX** e
**Informe exportable**. La calculadora de latencia conserva su entrada propia.
Detector/eliminador de acoples, detector de fase y osciloscopio son entradas
separadas del menú Herramientas. Las herramientas también se sirven en la
consola remota autorizada; el barrido automático requiere detener ese servicio
y realizarse desde la consola local.

### Datos en tiempo real

Conserva los componentes calculados y declarados de la latencia. Añade duración
DSP del último bloque, promedio, máximo, bloques procesados y bloques cuyo DSP
excede la duración del bloque. Este contador no sustituye a los XRuns del driver:
solo mide el procesamiento del mezclador, sin la espera física del dispositivo.
Las estadísticas DSP se reinician al crear el motor o un nuevo grafo de mezcla.

### Medidor de latencia loopback

1. Trabaja a 48 kHz, asigna las entradas/salidas y activa ASIO.
2. Conecta mediante un cable la salida elegida a una **entrada de línea**.
   Desactiva el monitoreo directo del retorno en la interfaz.
3. Selecciona salida y entrada en la herramienta, numeradas desde 1.
4. Pulsa **Medir ida y vuelta**. Repite varias veces con la misma configuración.

La captura emite una ráfaga conocida a −24 dBFS, con extremos suavizados.
Dura unos 500 ms; el rango detectable de ida/vuelta es aproximadamente 470 ms.
Se aíslan el mezclador y las demás salidas durante la captura, y se transmite
silencio al puente OBS y a los envíos web/UDP. El retorno no entra en la mezcla.
Cancelar, DETENER o SILENCIO TOTAL interrumpen la medición.

El resultado es la ida y vuelta física en muestras y milisegundos a 48 kHz,
con correlación, nivel del retorno y aviso de polaridad invertida. No se divide
por dos para inventar latencias individuales de entrada y salida. Saturación,
XRuns durante la captura, pérdidas en la cola, retorno débil o timeout producen
un fallo de medición, no un valor de latencia válido. DEMO y AUDIO PC no ofrecen
loopback físico: esta versión abre entrada física únicamente mediante ASIO.

El historial conserva hasta cien capturas durante la sesión. Mínimo, promedio
y máximo solo deben compararse entre rutas y buffers equivalentes.

### Buffers y estabilidad

Con el motor y el servicio de control remoto detenidos, selecciona ASIO,
AUDIO PC o DEMO, una lista de hasta diez buffers (32–8192 muestras) y duración
por buffer (1–600 segundos). Una prueba útil empieza con 64, 128, 256 y 512,
15–60 segundos cada uno, antes de ensayos prolongados de la configuración elegida.

El barrido conserva el ruteo de la escena y abre/cierra el dispositivo entre
pruebas. Anota buffer solicitado y aceptado, duración, XRuns del controlador,
clipping, fallo del motor, estadísticas DSP y recursos. Un buffer no admitido
queda registrado como error y se continúa con el siguiente. **DEMO solo prueba
procesamiento; no valida estabilidad del hardware ni cuenta cortes físicos.**

Al terminar, cancelar, pulsar DETENER o SILENCIO TOTAL se restaura la configuración
original y el motor queda detenido. La consola no permite editar la configuración
durante el barrido. Los recursos son máximos muestreados del proceso desde su inicio,
no máximos exclusivos de cada fila ni garantía de carga futura con otros efectos.

### Comparador de rutas y EFX

Conecta la salida izquierda/mono del bus elegido a la entrada de retorno.
Selecciona canal y bus. La prueba inyecta la ráfaga **antes del EQ del canal**,
pasa por su procesamiento y envío al master/aux elegido, por el procesamiento
del bus, y sale por su ruta física. Se aíslan otros canales/buses y las
publicaciones de audio hacia OBS y la red durante la medición.

- **Medir referencia sin procesamiento** omite temporalmente EQ, compresor e
  inserts del canal y del bus. Sus ajustes se restauran al terminar o cancelar.
- **Medir ruta con EFX** respeta sus estados actuales, incluidos bypass.

Los niveles, paneo, mutes y envíos se conservan: abre la ruta que quieres medir.
Una ruta silenciada no produce una medición fiable. El historial guarda nombre,
configuración de canal/bus, buffer, entrada/salida y resultado. Compara varias
capturas equivalentes de referencia y EFX; el informe JSON conserva los ajustes.
La tabla muestra Δ respecto a la última referencia con iguales dispositivo,
buffer, entrada/salida, canal y bus. Ese diferencial incluye variación del
trayecto físico; no aísla por sí solo la latencia de un plugin.
La correlación mide el retardo predominante: modulaciones, cambios de fase,
reverberaciones y otros efectos pueden debilitarla o cambiar el pico detectado.
No es compensación automática de latencia ni una medida aislada de un plugin.

### Informe exportable

**HTML** independiente, legible e imprimible, y **JSON** con los datos detallados.
Incluyen versión/arquitectura de AuMux, fecha, modo, configuración, historial
físico, barridos, último análisis, latencias declaradas/calculadas y estadísticas
DSP. El HTML resume canales, buses, efectos y recursos; JSON conserva más detalle.

El inventario se obtiene en un hilo separado. En Windows consulta versión y
compilación del sistema, CPU, fabricante/modelo del equipo, RAM instalada,
capacidad/espacio libre de discos y controladores de audio que Windows enumera.
El nombre seleccionado de la interfaz se conserva cuando se enumera o abre.
El inventario puede no estar disponible si CIM/PowerShell no responde; no bloquea
el motor. No se consulta en el callback ni en cada petición de estado.

Firmware, revisión de hardware, versión específica de ASIO y OBS pueden
completarse manualmente. Se identifican como datos del operador. La lista de
controladores Windows no se presenta como identificación garantizada del driver
ASIO utilizado. Un dato no expuesto queda como **No informado**.

CPU se normaliza sobre todos los procesadores lógicos. RAM es el conjunto
residente del motor; no incluye el navegador. Los máximos se muestrean cada
segundo aproximadamente desde que arranca AuMux y pueden omitir picos más
breves. E/S muestra caudales y máximos de lectura/escritura del proceso, **no
porcentaje de actividad ni máximo instantáneo de un disco**. El espacio libre
del inventario es una instantánea. Los enlaces con claves de acceso y las rutas
locales de VST3 se excluyen del informe compartible.

## Detector / eliminador de acoples

Selecciona un bus e inicia el detector. Una FFT Hann de 4096 muestras, con salto
de 2048, busca tonos persistentes de elevada prominencia espectral. Señala como
**posibles acoples** los picos que superan los criterios de nivel/prominencia y
persisten al menos unos 640 ms. Una nota musical sostenida puede cumplir esos
criterios: el detector no demuestra que exista realimentación acústica.

Cada candidato muestra frecuencia, dBFS, prominencia y persistencia. **Aplicar
corte** añade un filtro bell de −6 dB y Q 12 en una banda libre del EQ de ese
bus. No reemplaza bandas ocupadas ni suma otro corte casi en la misma frecuencia.
Si el EQ está en bypass y contiene otros ajustes, actívalo primero para decidir
conscientemente si deben entrar también. Los cortes son manuales, se editan o
quitan en el EQ y se guardan con la escena. No hay cancelación automática ni
protección garantizada de micrófonos/parlantes.

## Detector de fase

Selecciona dos entradas físicas, señales de canal post-FX/pre-fader o lados de
buses post-fader. Muestra correlación AC (sin componente continua), niveles RMS,
suma mono (A+B)/2, cambio relativo de nivel al sumar y desfase en la frecuencia
dominante. No modifica el audio. Señales iguales dan aproximadamente +1;
invertidas, −1; sin señal AC suficiente no se inventa correlación. Retardos y
contenido musical también causan correlación negativa: no es un diagnóstico
automático de cableado ni una medida del retardo total.

## Osciloscopio

Dos fuentes seleccionables, colores A/B, ventanas visibles de 5–85,33 ms,
escala de amplitud, disparo ascendente en A/libre y congelación de la vista sin
detener el audio. La representación conserva mínimos y máximos de cada grupo
de hasta ocho muestras, ajustado al zoom para mostrar picos breves. No reconstruye puntos perdidos ni
es un osciloscopio de laboratorio de resolución ilimitada. La cola y los
fragmentos descartados se muestran junto al gráfico. Al cerrar las ventanas de
análisis se detiene la captura; una vista congelada no detiene el motor.

## Arquitectura y validación

En el callback solo hay temporización monotónica, atómicos, generación de la
ráfaga y copias acotadas a una cola de 256 fragmentos de 128 muestras como máximo.
FFT, correlación, detección y generación del inventario se realizan fuera del
callback. La cola no crece con una red lenta; sus descartes son visibles. No se
agrega buffer al audio normal por abrir un analizador. La vista se consulta a
4 Hz mientras está abierta, sin superponer peticiones. Los analizadores usan
un estado compacto que no vuelve a enviar el historial completo de pruebas.

Pruebas: correlación con retardos conocidos y polaridad invertida; rechazo de
silencio, saturación y pérdidas; aislamiento de salidas y fuentes; ruta real por
inserts con retardo conocido y bypass; ausencia de trabajo de heap en captura;
tono persistente frente a ruido/transitorio; fase 0/90/180°, silencio/DC;
preservación de picos; barridos/restauración/cancelación en DEMO; informes sin
inyección HTML ni claves de acceso; interfaz en Chromium y regresión del motor,
OBS y control remoto. Windows se compila de forma cruzada; el inventario CIM,
los controladores ASIO, tiempos físicos y sesiones largas requieren prueba nativa.
