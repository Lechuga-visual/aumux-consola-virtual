/*
 * Copyright (C) 2006 Robert Reif
 * Portions copyright (C) 2007 Ralf Beck
 * Portions copyright (C) 2007 Johnny Petrantoni
 * Portions copyright (C) 2007 Stephane Letz
 * Portions copyright (C) 2008 William Steidtmann
 * Portions copyright (C) 2010 Peter L Jones
 * Portions copyright (C) 2010 Torben Hohn
 * Portions copyright (C) 2010 Nedko Arnaudov
 * Portions copyright (C) 2013 Joakim Hernberg
 *
 * This library is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public
 * License as published by the Free Software Foundation; either
 * version 2.1 of the License, or (at your option) any later version.
 *
 * This library is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
 * Lesser General Public License for more details.
 *
 * You should have received a copy of the GNU Lesser General Public
 * License along with this library; if not, write to the Free Software
 * Foundation, Inc., 51 Franklin St, Fifth Floor, Boston, MA 02110-1301, USA
 */
// SPDX-License-Identifier: LGPL-2.1-or-later
// Open ASIO ABI binding. Interface slot order follows WineASIO's IWineASIO
// (Robert Reif and WineASIO contributors). No Steinberg SDK implementation is used.
#pragma once
#include <windows.h>
#include <unknwn.h>
using ASIOBool=long; using ASIOError=long; using ASIOSampleRate=double; using ASIOSampleType=long;
constexpr long ASIOFalse=0, ASIOTrue=1;
enum : long { ASE_OK=0, ASE_SUCCESS=0x3f4847a0, ASE_NotPresent=-1000, ASE_HWMalfunction=-999, ASE_InvalidParameter=-998, ASE_InvalidMode=-997, ASE_SPNotAdvancing=-996, ASE_NoClock=-995, ASE_NoMemory=-994 };
enum : long { ASIOSTInt16MSB=0,ASIOSTInt24MSB=1,ASIOSTInt32MSB=2,ASIOSTFloat32MSB=3,ASIOSTFloat64MSB=4,ASIOSTInt16LSB=16,ASIOSTInt24LSB=17,ASIOSTInt32LSB=18,ASIOSTFloat32LSB=19,ASIOSTFloat64LSB=20 };
enum : long { kAsioSelectorSupported=1,kAsioEngineVersion,kAsioResetRequest,kAsioBufferSizeChange,kAsioResyncRequest,kAsioLatenciesChanged,kAsioSupportsTimeInfo,kAsioSupportsTimeCode,kAsioMMCCommand,kAsioSupportsInputMonitor,kAsioSupportsInputGain,kAsioSupportsInputMeter,kAsioSupportsOutputGain,kAsioSupportsOutputMeter,kAsioOverload };
#pragma pack(push,4)
struct ASIOTime;
struct ASIOCallbacks {void (*bufferSwitch)(long,ASIOBool);void (*sampleRateDidChange)(ASIOSampleRate);long (*asioMessage)(long,long,void*,double*);ASIOTime* (*bufferSwitchTimeInfo)(ASIOTime*,long,ASIOBool);};
struct ASIODriverInfo {long asioVersion;long driverVersion;char name[32];char errorMessage[124];void* sysRef;};
struct ASIOBufferInfo {ASIOBool isInput;long channelNum;void* buffers[2];};
struct ASIOChannelInfo {long channel;ASIOBool isInput;ASIOBool isActive;long channelGroup;ASIOSampleType type;char name[32];};
struct AsioInt64 {unsigned long hi,lo;};
#pragma pack(pop)
static_assert(sizeof(void*)==8, "Windows x64 only");
static_assert(sizeof(ASIODriverInfo)==172 && sizeof(ASIOChannelInfo)==52 && sizeof(ASIOBufferInfo)==24, "ASIO ABI layout");
// COM method order is part of the public driver ABI, not a SDK dependency.
struct OpenAsio : IUnknown {
 virtual ASIOBool init(void*)=0; virtual void getDriverName(char*)=0; virtual long getDriverVersion()=0; virtual void getErrorMessage(char*)=0;
 virtual ASIOError start()=0; virtual ASIOError stop()=0;
 virtual ASIOError getChannels(long*,long*)=0; virtual ASIOError getLatencies(long*,long*)=0;
 virtual ASIOError getBufferSize(long*,long*,long*,long*)=0; virtual ASIOError canSampleRate(double)=0;virtual ASIOError getSampleRate(double*)=0;virtual ASIOError setSampleRate(double)=0;
 virtual ASIOError getClockSources(void*,long*)=0;virtual ASIOError setClockSource(long)=0;virtual ASIOError getSamplePosition(AsioInt64*,AsioInt64*)=0;
 virtual ASIOError getChannelInfo(ASIOChannelInfo*)=0;virtual ASIOError createBuffers(ASIOBufferInfo*,long,long,ASIOCallbacks*)=0;
 virtual ASIOError disposeBuffers()=0;virtual ASIOError controlPanel()=0;virtual ASIOError future(long,void*)=0;virtual ASIOError outputReady()=0;
};
ASIOError ASIOInit(ASIODriverInfo*);ASIOError ASIOExit();ASIOError ASIOStart();ASIOError ASIOStop();ASIOError ASIOGetChannels(long*,long*);ASIOError ASIOGetLatencies(long*,long*);ASIOError ASIOGetBufferSize(long*,long*,long*,long*);ASIOError ASIOCanSampleRate(double);ASIOError ASIOGetSampleRate(double*);ASIOError ASIOSetSampleRate(double);ASIOError ASIOGetChannelInfo(ASIOChannelInfo*);ASIOError ASIOCreateBuffers(ASIOBufferInfo*,long,long,ASIOCallbacks*);ASIOError ASIODisposeBuffers();ASIOError ASIOOutputReady();
