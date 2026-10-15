# MDA native audio effects

Source: https://github.com/hollance/mda-plugins-juce
Revision: 552702479a9eac534f3abbf6e878a5eab473e0cb
Copyright: Matthijs Hollemans and Paul Kellett. License: MIT (LICENSE.txt).

Original processor sources/headers for the 20 audio effects are retained in
subdirectories. `native_mda.h` adapts their DSP to Monitor Mix: removes JUCE
processor/UI/state wrappers and formatting callbacks, uses our independently
implemented buffer/parameter utilities (`native/dsp_compat.h`), preserves
DSP state/helpers/defaults/ranges, and replaces global rand with per-instance
xorshift. Ambience releases its delay storage in its destructor; RezFilter's
maximum frequency coefficient is capped at 1.0 to keep the legacy filter
stable at high resonance. Out-of-range defaults are clamped by the parameter
adapter. No JUCE implementation is linked.

Piano, EPiano, DX10 and JX10 are MIDI instruments and are not integrated.
All 20 audio effects are available as native inserts, with generic numeric
parameters rather than the upstream plugin editors/preset system.
