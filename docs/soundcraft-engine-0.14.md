# SoundCraft · revisión del motor para AuMux 0.14

Se revisó el repositorio upstream `storytold/soundcraft`, utilizado por SoundCraft
de Artcraft, comparando el pin anterior `3a28a657` con el commit
[`cf0f4d2f6a2629b48a1dfbed858ac8c5f227bc42`](https://github.com/storytold/soundcraft/commit/cf0f4d2f6a2629b48a1dfbed858ac8c5f227bc42).
La comparación reportaba 434 commits de diferencia; no todos afectan el audio de
AuMux. La interfaz egui, instrumentos MIDI, edición de clips y actualización
automática de la DAW upstream no se incorporan a AuMux.

## Incorporado

- **Host VST3 actualizado:** el host proporciona `IMessage` e `IAttributeList`,
  usados por plugins para comunicar procesador y controlador. Upstream documenta
  fallos de carga de ciertos plugins sin esos objetos. AuMux no declara haber
  probado esos productos comerciales concretos.
- **Ciclo de vida de módulos:** la versión revisada gestiona `ModuleExit` y
  equivalentes, su hilo de entrada/salida y las instancias vivas. El código del
  módulo permanece mapeado hasta terminar el proceso, evitando reentradas o
  descargar código que aún pudiera estar en uso.
- **DSP y audio I/O del mismo pin:** no se mezclan revisiones incompatibles de los
  tres crates. Se añade `soundcraft-audio-io` para usar su función pública
  `wav_float_header`, con cabecera Float32 y paso a RF64 para archivos grandes.
  La escritura periódica de cabeceras y la separación captura/disco también
  siguen el criterio de grabación del upstream.

El ruteo, intercambio de grafos, colas de OBS/red, compresor y selección de puntos
de captura siguen siendo propios de AuMux. No se añade una capa de procesamiento
ni un buffer de reproducción por actualizar estas dependencias. La actualización
no equivale por sí sola a una reducción medida de latencia física.

## Comprobaciones

Se compila AuMux contra los tres crates fijados al hash completo. Pasan las
**15 pruebas de biblioteca del host VST3 upstream**, incluidas las de creación de
mensajes/atributos, transporte y carga inválida. Las pruebas de AuMux usan un
VST3 de fixture real, procesamiento de canales/aux/master, intercambios sin
heap Rust en el callback y PCM de escucha/OBS. La grabación se decodifica de
nuevo para comprobar muestras PRE y POST, formato y alineación.

La versión de SoundCraft incorporada declara licencia MIT OR Apache-2.0. Se conservan
sus fuentes/licencias en el paquete Windows junto a las atribuciones existentes.
Faltan pruebas nativas Windows con los plugins y driver del estudio; no se
presentan estas comprobaciones Linux como certificación de todos los VST3.
