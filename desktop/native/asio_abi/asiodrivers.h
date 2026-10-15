// SPDX-License-Identifier: MIT
#pragma once
#include "asio.h"
#include <vector>
#include <string>
class AsioDrivers {
 struct Entry {std::string name;CLSID clsid;};std::vector<Entry> entries;
 public: long asioGetNumDev(); ASIOError asioGetDriverName(int,char*,int); bool loadDriver(char*); void removeCurrentDriver();
};
