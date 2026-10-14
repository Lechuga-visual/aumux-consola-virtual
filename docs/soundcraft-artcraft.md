# Evaluación de SoundCraft de Artcraft

El usuario se refiere a https://getartcraft.com/apps/soundcraft, no a las
consolas Soundcraft ni a Mixing Station. Busca un mezclador Windows x64 para
MOTU 896mk3 Hybrid, mezclas de monitoreo y VST3 por canal, auxiliar y master,
con entrega de audio a OBS. No tiene acceso permanente al PC del estudio;
no bloquear el desarrollo por información que solo pueda obtener allí.

## Fuentes inspeccionadas

La consulta directa al sitio devolvió proxy HTTP 403. Se inspeccionaron el
README y archivos públicos del repositorio que el proyecto identifica:
https://github.com/storytold/soundcraft

Revisión examinada: `3a28a657692c1b98b28c276d8f61df9471074f0f`.
Versión declarada en Cargo.toml: 0.7.0.

- Cargo.toml: Rust, workspace modular y licencia MIT OR Apache-2.0.
- crates/playback/Cargo.toml: CPAL 0.16 y feature opcional `asio` (desactivada
  por defecto); requiere SDK ASIO y libclang para generar bindings.
- crates/playback/src/lib.rs: salida del main a canales físicos en orden y
  downmix cuando corresponde; no es un mapa independiente de auxiliares.
- crates/vst3-host/Cargo.toml: host VST3 separado de la interfaz.
- docs/hardware-parity.md: estado de hardware y ruteo.
- docs/gaps.md y ROADMAP.md: trabajo pendiente y correcciones recientes.

Las fuentes se leyeron; no se compiló SoundCraft ni se ejecutaron sus pruebas.
Sus afirmaciones de pruebas con hardware son resultados del proyecto, no
validaciones realizadas por nosotros. El README contiene estado más antiguo
que algunas actualizaciones de hardware: no basarse solo en su resumen.

## A favor

- Motor de mezcla, modelo de sesión e interfaz en crates separados.
- Buses, auxiliares, envíos pre/post y automatización descritos en el proyecto.
- Host VST3 y persistencia de estado; editores Windows descritos en su roadmap.
- Licencia permisiva para reutilizar código, con avisos correspondientes.
  La marca y sus recursos gráficos tienen condiciones separadas; una app
  derivada debe tener identidad propia según las instrucciones del proyecto.

## Lo que afecta directamente a este objetivo

Según docs/hardware-parity.md de la revisión examinada:

- ASIO es opcional; el proyecto informa una prueba con SSL 2+, no con MOTU.
- Entrada/salida aún usan streams separados; no hay dúplex con un solo reloj.
- La asignación de canales físicos no está terminada.
- El monitoreo de baja latencia figura como pendiente.

Por tanto, tener envíos y auxiliares en la sesión no demuestra que puedan
salir como varias mezclas de monitoreo a distintos pares físicos de la MOTU.
Tampoco se verificó un puente hacia OBS.

## Evaluación para elegir base

SoundCraft es candidato para reutilizar un motor abierto y construir una
superficie centrada en mezcla, evitando traer toda la edición de un DAW.
Antes de elegirlo para uso en vivo, la prueba decisiva debe cubrir:

1. Abrir entradas y salidas ASIO en un único callback/reloj.
2. Llevar dos mezclas independientes a pares de salidas diferentes.
3. Procesar un VST3 en canal, auxiliar y master, preservando su estado.
4. Entregar el master a OBS por una ruta de audio comprobada.

El desarrollo de la lógica y pruebas sintéticas no requiere la MOTU; la
validación final del controlador, latencia y estabilidad sí requiere Windows
y hardware. REAPER sigue siendo la vía de menor implementación de motor;
Ardour ofrece una base abierta más madura. Esta inspección no selecciona ni
instala otra base, ni reemplaza el código C++ ya creado.
