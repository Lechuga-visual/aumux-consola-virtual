# Transporte OSC: primer paso para la miniapp

**Actualización tras leer el manual:** las páginas 116–117 documentan
superficies Mackie por MIDI, no OSC. Este transporte se conserva como utilidad
genérica probada; no es el backend seleccionado para CueMix ni demuestra su
compatibilidad. Véase [control-cuemix.md](control-cuemix.md).

`tools/cuemix_osc.py` implementa mensajes OSC por UDP con int32, float32,
strings ASCII y booleanos T/F. Recibe también bundles (incluyendo su timetag),
pero no ejecuta mensajes programados ni soporta todos los tipos opcionales
de OSC. No incluye descubrimiento, handshake, suscripciones, mapa de CueMix,
conversión dB/fader, reintentos ni interfaz gráfica.

No envía nada al inicializarse o escuchar. Cada envío requiere destino,
puerto, dirección OSC y argumentos explícitos. Un envío UDP exitoso no
confirma conexión, compatibilidad o aceptación por CueMix. La recepción
muestra el origen; no autentica el emisor ni modifica controles.

## Prueba local, sin MOTU

Desde la raíz, en una terminal:

```sh
python tools/cuemix_osc.py --local-port 19000 listen
```

En otra terminal:

```sh
python tools/cuemix_osc.py send --remote-ip 127.0.0.1 --remote-port 19000 --address /test --arg f:0.5
```

La primera terminal debe imprimir un `Message` con dirección `/test` y
argumento `0.5`. `/test` es solo una dirección de prueba local y no se
afirma que exista en CueMix. Ctrl+C cierra la escucha.

El comando `listen` escucha en loopback por defecto. Para acceso remoto se
deben configurar una IP local concreta y los puertos establecidos por el
manual; no se abren reglas de firewall automáticamente. CueMix puede
requerir que el puerto de realimentación sea fijo o registrar al cliente:
el transporte no inventa esa negociación.

## Validación automatizada

```sh
python -m unittest discover -s tests -p test_osc.py -v
```

También se ejecuta desde CTest como `osc_transport`. Se comprueban fixtures
binarios OSC, recepción de bundles, rechazo de paquetes inválidos y un
intercambio UDP real de ida y vuelta en loopback. No se contacta a la MOTU.

## Datos que faltan del manual

- Quién escucha OSC: CueMix en Windows o el dispositivo directamente.
- Activación del servicio y puertos de envío/realimentación.
- Direcciones, tipos y escalas de faders, mute, panorama y selección de mezcla.
- Identificadores de canales/buses y forma de consultar el estado inicial.
- Suscripción, handshake, feedback y cualquier limitación documentada.

Con esos datos se puede enlazar una superficie sin asumir direcciones de
MOTU AVB o CueMix 5. El host VST3 sigue siendo una función separada pendiente.
