// SPDX-License-Identifier: MIT
#pragma once
// Windows x64 has one calling convention; no x86 thiscall assembler thunk is needed.
static_assert(sizeof(void*)==8,"Monitor Mix supports Windows x64 only");
