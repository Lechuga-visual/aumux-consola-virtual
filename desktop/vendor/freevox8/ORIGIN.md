# FreeVox8 DSP integration

Source: https://github.com/GareBear99/FreeVox8
Revision: 02eedfe2ee38618d38718c3c47d8ba55a0cd6a10
Author: Gary Doman. License: GPL-3.0 (see LICENSE).

DSP headers copied from `Source/DSP`. Changes: JUCE includes removed,
`juce::` references replaced by the independently implemented `mm_dsp`
buffers, math, parameters, MIDI note and filter utilities in
`native/dsp_compat.h`. No JUCE implementation is linked.

Monitor Mix exposes the low-latency filterbank engine and its four creative
vocoder modes, with an internal carrier note. Spectral engine, external
sidechains, MIDI input, plugin UI and presets are not exposed. The spectral
header remains because the upstream DSP class references its definitions.
