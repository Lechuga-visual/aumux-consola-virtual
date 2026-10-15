# Componentes reutilizados

- **SoundCraft DSP y host VST3**, revisión
  `3a28a657692c1b98b28c276d8f61df9471074f0f` de
  https://github.com/storytold/soundcraft — MIT OR Apache-2.0.
  Se enlazan esos dos crates; no se ha bifurcado ni renombrado el DAW completo.
  La superficie y el ruteo de Monitor Mix son propios.
- **RtAudio 6.0.1**, revisión `b4f04903312e0e0efffbe77655172e0f060dc085`
  de https://github.com/thestk/rtaudio — licencia en `vendor/rtaudio/LICENSE`.
  Se reemplazaron los archivos del SDK ASIO por un binding abierto x64,
  basado en la ABI de WineASIO (LGPL-2.1-or-later). Procedencia y licencia:
  `native/asio_abi/ORIGIN.md`, `native/asio_abi/COPYING.LIB`.
- **FreeEQ8** y **FreeVox8**, Gary Doman — GPL-3.0. Se integran sus núcleos
  DSP, no los plugins completos: EQ de ocho bandas de fase mínima y vocoder
  filterbank. Revisiones, modificaciones y licencias en `vendor/freeeq8/`
  y `vendor/freevox8/` (ORIGIN.md y LICENSE).
- **MDA**, Matthijs Hollemans / Paul Kellett — MIT. Se adaptaron los 20
  efectos de audio de `hollance/mda-plugins-juce` sin enlazar JUCE.
  `vendor/mda/ORIGIN.md` explica revisión y modificaciones; `LICENSE.txt`
  conserva los avisos originales. Los cuatro instrumentos MIDI no se incluyen.
- **OBS Studio**: el receptor usa su API; el código del receptor es
  GPL-2.0-or-later. Los headers de compilación Windows corresponden a
  `5533a277e4400203b50114c993f89878d50a27b9` (32.1.0).
  No se distribuye el runtime de OBS.
- **PBKDF2 / SHA-2 (RustCrypto)** — MIT OR Apache-2.0; derivación de
  contraseñas para acceso web, con versiones fijadas en Cargo.lock.
- **if-addrs 0.13.4** — MIT OR BSD-3-Clause; enumeración de interfaces de red.
- Las demás dependencias Rust están fijadas en Cargo.lock. El paquete
  portable incluye sus archivos de licencia/NOTICE disponibles en `licenses/`.

Monitor Mix tiene identidad propia y no usa marcas ni recursos gráficos de
Artcraft. Los efectos comerciales del usuario no se incluyen en la descarga.

La aplicación combinada 0.5 se distribuye bajo GPL-3.0-only, con fuente
completa del DSP y scripts de compilación en el ZIP. Ver `COPYING.GPL-3.0`.
