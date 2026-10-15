# Open ASIO ABI binding

The COM interface slot order/types are based on WineASIO's `IWineASIO`:
https://github.com/wineasio/wineasio
Revision: b5e668103ad13e6f51f4118ed7090592213e5ca2
Copyright Robert Reif and WineASIO contributors, LGPL-2.1-or-later
(see COPYING.LIB). `asio.h` provides the ABI declarations needed by RtAudio.
The registry loader and wrapper implementation in `../asio_abi.cpp` are
Monitor Mix code (MIT). No Steinberg SDK implementation is included.

Windows x64 only. Structure sizes are checked at compile time. The ZIP
includes these sources so this binding can be rebuilt/relinked. Native
Windows ASIO-driver compatibility still requires a local hardware test.
