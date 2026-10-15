// SPDX-License-Identifier: MIT
#include "asio_abi/asiodrivers.h"
#include <cstring>
#include <algorithm>
namespace {OpenAsio* active=nullptr;}
long AsioDrivers::asioGetNumDev() {
 entries.clear();HKEY root;
 if(RegOpenKeyExA(HKEY_LOCAL_MACHINE,"SOFTWARE\\ASIO",0,KEY_READ,&root)!=ERROR_SUCCESS)return 0;
 for(DWORD i=0;;++i) {char name[256];DWORD n=sizeof(name);if(RegEnumKeyExA(root,i,name,&n,nullptr,nullptr,nullptr,nullptr)!=ERROR_SUCCESS)break;
 HKEY key;if(RegOpenKeyExA(root,name,0,KEY_READ,&key)!=ERROR_SUCCESS)continue;
 wchar_t id[128]{};DWORD bytes=sizeof(id),type=0;
 if(RegQueryValueExW(key,L"CLSID",nullptr,&type,reinterpret_cast<BYTE*>(id),&bytes)==ERROR_SUCCESS && type==REG_SZ) {id[127]=0;CLSID clsid;if(SUCCEEDED(CLSIDFromString(id,&clsid)))entries.push_back({name,clsid});} RegCloseKey(key); }
 RegCloseKey(root);return static_cast<long>(entries.size());
}
ASIOError AsioDrivers::asioGetDriverName(int i,char* out,int length){if(i<0||size_t(i)>=entries.size()||length<1)return ASE_InvalidParameter;std::strncpy(out,entries[i].name.c_str(),length-1);out[length-1]=0;return ASE_OK;}
bool AsioDrivers::loadDriver(char* name) {removeCurrentDriver();for(auto&e:entries)if(e.name.compare(0,31,name)==0) return SUCCEEDED(CoCreateInstance(e.clsid,nullptr,CLSCTX_INPROC_SERVER,e.clsid,reinterpret_cast<void**>(&active)));return false;}
void AsioDrivers::removeCurrentDriver(){if(active){active->Release();active=nullptr;}}
ASIOError ASIOInit(ASIODriverInfo* i){if(!active||!active->init(i->sysRef))return ASE_NotPresent;active->getDriverName(i->name);i->driverVersion=active->getDriverVersion();active->getErrorMessage(i->errorMessage);return ASE_OK;}
ASIOError ASIOExit(){return ASE_OK;}
ASIOError ASIOStart(){return active?active->start():ASE_NotPresent;}ASIOError ASIOStop(){return active?active->stop():ASE_NotPresent;}
ASIOError ASIOGetChannels(long*a,long*b){return active?active->getChannels(a,b):ASE_NotPresent;}ASIOError ASIOGetLatencies(long*a,long*b){return active?active->getLatencies(a,b):ASE_NotPresent;}
ASIOError ASIOGetBufferSize(long*a,long*b,long*c,long*d){return active?active->getBufferSize(a,b,c,d):ASE_NotPresent;}
ASIOError ASIOCanSampleRate(double r){return active?active->canSampleRate(r):ASE_NotPresent;}ASIOError ASIOGetSampleRate(double*r){return active?active->getSampleRate(r):ASE_NotPresent;}ASIOError ASIOSetSampleRate(double r){return active?active->setSampleRate(r):ASE_NotPresent;}
ASIOError ASIOGetChannelInfo(ASIOChannelInfo*i){return active?active->getChannelInfo(i):ASE_NotPresent;}
ASIOError ASIOCreateBuffers(ASIOBufferInfo*i,long n,long f,ASIOCallbacks*c){return active?active->createBuffers(i,n,f,c):ASE_NotPresent;}
ASIOError ASIODisposeBuffers(){return active?active->disposeBuffers():ASE_NotPresent;}ASIOError ASIOOutputReady(){return active?active->outputReady():ASE_NotPresent;}
