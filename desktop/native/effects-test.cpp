// SPDX-License-Identifier: MIT
// Exercise the actual C++ audio path, including its allocator and DSP parameter limits.
#include <array>
#include <cassert>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <new>
static thread_local bool audioThread=false;
static thread_local unsigned heapOps=0;
void* operator new(size_t n){if(audioThread)++heapOps;if(auto p=std::malloc(n?n:1))return p;throw std::bad_alloc();}
void* operator new[](size_t n){return ::operator new(n);}
void operator delete(void*p)noexcept{if(audioThread)++heapOps;std::free(p);}
void operator delete[](void*p)noexcept{::operator delete(p);}
void operator delete(void*p,size_t)noexcept{::operator delete(p);}
void operator delete[](void*p,size_t)noexcept{::operator delete(p);}
extern "C" {int mm_fx_count();const char*mm_fx_name(int);void*mm_fx_create(int);void mm_fx_destroy(void*);void mm_fx_process(void*,float*,float*,unsigned);int mm_fx_params(void*);struct RawParam{const char*id;const char*name;float min,max,value;};bool mm_fx_param(void*,int,RawParam*);void mm_fx_set(void*,int,float);void*mm_eq_create();void mm_eq_destroy(void*);struct Band{float frequency,gain,q;unsigned type;};void mm_eq_process(void*,float*,float*,unsigned,bool,const Band*);}
int main(){
 std::array<float,8192>l{},r{};
 for(int i=0;i<mm_fx_count();i++) {
  auto*p=mm_fx_create(i);assert(p);
  for(int mode=0;mode<4;mode++) {
   for(int j=0;j<mm_fx_params(p);j++){RawParam v{};assert(mm_fx_param(p,j,&v));assert(v.min<=v.value&&v.value<=v.max);mm_fx_set(p,j,mode==0?v.value:mode==1?v.min:mode==2?v.max:(v.min+v.max)*.5f);}
   for(unsigned block: {1u,64u,257u,8192u}) {
    for(unsigned f=0;f<block;f++){l[f]=std::sin(float(f)*.131f)*.001f;r[f]=std::cos(float(f)*.113f)*.001f;}
    heapOps=0;audioThread=true;mm_fx_process(p,l.data(),r.data(),block);audioThread=false;
    if(heapOps){std::fprintf(stderr,"%s: %u heap operations\n",mm_fx_name(i),heapOps);return 1;}
    for(unsigned f=0;f<block;f++)if(!std::isfinite(l[f])||!std::isfinite(r[f])){std::fprintf(stderr,"%s nonfinite mode=%d block=%u frame=%u\n",mm_fx_name(i),mode,block,f);return 1;}
   }
  }
  mm_fx_destroy(p);
 }
 auto*eq=mm_eq_create();assert(eq);std::array<Band,8>bands{};
 for(unsigned kind=0;kind<6;kind++)for(float q: {.1f,.707f,2.f,18.f})for(float gain:{-18.f,0.f,18.f}){
  for(unsigned b=0;b<8;b++)bands[b]={20.f*std::pow(1000.f,float(b)/7),gain,q,kind};
  for(unsigned f=0;f<l.size();f++){l[f]=std::sin(float(f)*.131f)*.001f;r[f]=l[f];}
  heapOps=0;audioThread=true;mm_eq_process(eq,l.data(),r.data(),l.size(),false,bands.data());audioThread=false;assert(heapOps==0);
  for(unsigned f=0;f<l.size();f++)assert(std::isfinite(l[f])&&std::isfinite(r[f]));
 }
 mm_eq_destroy(eq);std::puts("PASS: 21 native inserts and eight-band EQ; parameter boundaries, odd/maximum blocks, finite stereo output and no C++ heap work during processing");
}
