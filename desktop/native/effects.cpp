// SPDX-License-Identifier: GPL-3.0-only
#include "../vendor/freeeq8/Biquad.h"
#include "../vendor/freevox8/FreeVox8DSP.h"
#include "../vendor/mda/native_mda.h"
struct EqBandParams {float frequency,gain_db,q;unsigned type;};
struct NativeEq {std::array<Biquad,8> filters;std::array<EqBandParams,8> previous{};bool ready=false;float wet=1;};
extern "C" void* mm_eq_create(){try{return new NativeEq;}catch(...){return nullptr;}}
extern "C" void mm_eq_destroy(void*p){delete static_cast<NativeEq*>(p);}
extern "C" void mm_eq_process(void*p,float*l,float*r,unsigned frames,bool bypass,const EqBandParams*params){
 mm_dsp::ScopedNoDenormals noDenormals; auto&e=*static_cast<NativeEq*>(p);
 std::array<bool,8> changed{};
 for(unsigned b=0;b<8;b++) {auto&a=e.previous[b];auto&z=params[b];changed[b]=!e.ready||a.frequency!=z.frequency||a.gain_db!=z.gain_db||a.q!=z.q||a.type!=z.type;}
 if(!e.ready){std::copy(params,params+8,e.previous.begin());e.ready=true;}
 const float target=bypass?0.f:1.f;
 for(unsigned f=0;f<frames;f++){
  const float t=float(f+1)/frames;
  if(f%16==0||f+1==frames)for(unsigned b=0;b<8;b++){
   if(!changed[b])continue; auto a=e.previous[b];auto z=params[b]; auto type=static_cast<Biquad::Type>(z.type);
   e.filters[b].set(type,48000,a.frequency+(z.frequency-a.frequency)*t,a.q+(z.q-a.q)*t,a.gain_db+(z.gain_db-a.gain_db)*t);
  }
  float left=l[f],right=r[f];for(auto&b:e.filters){left=b.processL(left);right=b.processR(right);}
  const float wet=e.wet+(target-e.wet)*t;l[f]+=(left-l[f])*wet;r[f]+=(right-r[f])*wet;
 }
 e.wet=target;std::copy(params,params+8,e.previous.begin());
 for(auto&b:e.filters){if(std::abs(b.z1L)<1e-30)b.z1L=0;if(std::abs(b.z2L)<1e-30)b.z2L=0;if(std::abs(b.z1R)<1e-30)b.z1R=0;if(std::abs(b.z2R)<1e-30)b.z2R=0;}
}
struct NativeInsert {
 std::unique_ptr<MdaBase> mda;
 std::unique_ptr<FreeVox8DSP> vox;
 std::vector<std::unique_ptr<mm_dsp::Parameter>> voxParams;
 explicit NativeInsert(int index){if(index<20)mda=createMda(index);else{vox=std::make_unique<FreeVox8DSP>();
 auto add=[&](const char*id,float lo,float hi,float d){voxParams.push_back(std::make_unique<mm_dsp::Parameter>(mm_dsp::ParameterID(id,1),id,lo,hi,d));};
 add("Mode",0,3,0);add("Mix",0,1,1);add("Carrier blend",0,1,.65f);add("Mask depth",0,1,.5f);add("Clarity",0,1,.55f);add("Formant shift",-24,24,0);add("Ghost",0,1,0);add("Air",0,1,0);add("Freeze",0,1,0);add("Transient protect",0,1,.35f);add("Stereo width",0,2,1);add("Quality",0,1,.75f);add("Output gain",-24,6,0);add("Carrier note",0,127,48);
 }prepare();}
 auto&parameters(){return mda?mda->apvts.params:voxParams;}
 void prepare(){if(mda){mda->rate=48000;mda->block=8192;mda->prepareToPlay(48000,8192);}else vox->prepare(48000,8192,2);}
 void process(float*l,float*r,int frames){mm_dsp::ScopedNoDenormals noDenormals;mm_dsp::AudioBuffer<float>b(l,r,frames);mm_dsp::MidiBuffer midi;
 if(mda)mda->processBlock(b,midi);else{FreeVox8DSP::Parameters p;auto v=[&](int i){return voxParams[i]->value.load(std::memory_order_relaxed);};
 p.mode=static_cast<FreeVox8DSP::Mode>(int(std::round(v(0))));p.mix=v(1);p.carrierBlend=v(2);p.maskDepth=v(3);p.clarity=v(4);p.formantShift=v(5);p.ghost=v(6);p.air=v(7);p.freeze=v(8);p.transientProtect=v(9);p.stereoWidth=v(10);p.quality=v(11);p.outputGainDb=v(12);p.engineMode=0;midi.item.note=int(std::round(v(13)));vox->setParameters(p);vox->process(b,nullptr,midi);}}
};
extern "C" int mm_fx_count(){return 21;}
extern "C" const char* mm_fx_name(int i){return i>=0&&i<20?mdaNames[i]:i==20?"FreeVox8 (filterbank)":"Unknown";}
extern "C" void* mm_fx_create(int i){try{if(i<0||i>=21)return nullptr;return new NativeInsert(i);}catch(...){return nullptr;}}
extern "C" void mm_fx_destroy(void*p){delete static_cast<NativeInsert*>(p);}
extern "C" void mm_fx_process(void*p,float*l,float*r,unsigned n){static_cast<NativeInsert*>(p)->process(l,r,int(n));}
extern "C" int mm_fx_params(void*p){return int(static_cast<NativeInsert*>(p)->parameters().size());}
struct RawParam {const char*id;const char*name;float min,max,value;};
extern "C" bool mm_fx_param(void*p,int i,RawParam*out){auto&v=static_cast<NativeInsert*>(p)->parameters();if(i<0||size_t(i)>=v.size())return false;auto&x=*v[i];*out={x.id.c_str(),x.name.c_str(),x.min,x.max,x.value.load()};return true;}
extern "C" void mm_fx_set(void*p,int i,float value){auto&v=static_cast<NativeInsert*>(p)->parameters();if(i>=0&&size_t(i)<v.size())v[i]->value.store(std::clamp(value,v[i]->min,v[i]->max));}
