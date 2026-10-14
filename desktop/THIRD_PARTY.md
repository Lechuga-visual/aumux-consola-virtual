# Componentes reutilizados

- **SoundCraft DSP y host VST3**, revisión
  `3a28a657692c1b98b28c276d8f61df9471074f0f` de
  https://github.com/storytold/soundcraft — MIT OR Apache-2.0.
  Se enlazan esos dos crates; no se ha bifurcado ni renombrado el DAW completo.
  La superficie y el ruteo de Monitor Mix son propios.
- **RtAudio 6.0.1**, revisión `b4f04903312e0e0efffbe77655172e0f060dc085`
  de https://github.com/thestk/rtaudio — licencia en `vendor/rtaudio/LICENSE`.
  Se conservan sin cambios los archivos y avisos de su directorio `include`,
  incluidos los archivos ASIO de Steinberg distribuidos por ese proyecto.
  Información del SDK en `vendor/rtaudio/include/asioinfo.txt`.
- **OBS Studio**: el receptor usa su API; el código del receptor es
  GPL-2.0-or-later. Los headers de compilación Windows corresponden a
  `5533a277e4400203b50114c993f89878d50a27b9` (32.1.0).
  No se distribuye el runtime de OBS.
- Las demás dependencias Rust están fijadas en Cargo.lock. El paquete
  portable incluye sus archivos de licencia/NOTICE disponibles en `licenses/`.

Monitor Mix tiene identidad propia y no usa marcas ni recursos gráficos de
Artcraft. Los efectos comerciales del usuario no se incluyen en la descarga.
