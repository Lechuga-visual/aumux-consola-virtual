# FreeEQ8 DSP core

Source: https://github.com/GareBear99/FreeEQ8
Revision: 11376c1c496569c975e4d195c3fe5fd44b53d415
Author: Gary Doman. License: GPL-3.0 (see LICENSE).

`Biquad.h` is the upstream `Source/DSP/Biquad.h`, unmodified. Monitor Mix
uses eight instances in `native/effects.cpp`, with smoothed parameters and
bypass, before the insert chain. Six filter types, minimum phase, 48 kHz.
The FreeEQ8 UI, matching, dynamics, drive, analyzer and linear-phase engine
are not integrated. This is a DSP-core integration, not the complete plugin.
