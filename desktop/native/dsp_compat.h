// SPDX-License-Identifier: MIT
// Minimal Monitor Mix buffers/parameters/filter utilities. This is not JUCE code.
#pragma once
#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <memory>
#include <string>
#include <vector>
#include <cstdio>
#if defined(__SSE__) || defined(_M_X64)
#include <xmmintrin.h>
#endif
namespace mm_dsp {
using uint32=uint32_t;using String=std::string;using StringArray=std::vector<std::string>;
template<class T>T jlimit(T a,T b,T v){return std::clamp(v,a,b);}template<class T>T jmin(T a,T b){return std::min(a,b);}template<class T>T jmax(T a,T b){return std::max(a,b);}
template<class T>struct MathConstants{static constexpr T pi=T(3.14159265358979323846),twoPi=2*pi;};
struct Decibels{template<class T>static T decibelsToGain(T x){return std::pow(T(10),x/T(20));}};
struct ScopedNoDenormals{
#if defined(__SSE__) || defined(_M_X64)
 unsigned previous;ScopedNoDenormals():previous(_mm_getcsr()){_mm_setcsr(previous|0x8040u);}~ScopedNoDenormals(){_mm_setcsr(previous);}
#endif
};
template<class T>struct AudioBuffer{T* data[2];int frames;AudioBuffer(T*l,T*r,int n):data{l,r},frames(n){}int getNumChannels()const{return 2;}int getNumSamples()const{return frames;}T*getWritePointer(int c){return data[c];}const T*getReadPointer(int c)const{return data[c];}void clear(int c,int start,int n){std::fill(data[c]+start,data[c]+start+n,T(0));}void clear(){clear(0,0,frames);clear(1,0,frames);}};
struct MidiMessage{int note=48;bool isNoteOn()const{return true;}int getNoteNumber()const{return note;}};struct MidiMetadata{int note=48;MidiMessage getMessage()const{return {note};}};struct MidiBuffer{MidiMetadata item;const MidiMetadata*begin()const{return &item;}const MidiMetadata*end()const{return &item+1;}};
struct ParameterID{std::string value;ParameterID(const char*s,int):value(s){}};
template<class T>struct NormalisableRange{T start=0,end=1;NormalisableRange()=default;template<class...A>NormalisableRange(T a,T b,A...):start(a),end(b){}};
struct Parameter {std::string id,name;float min,max,def;std::atomic<float> value;Parameter(ParameterID i,std::string n,float lo,float hi,float d):id(i.value),name(n),min(lo),max(hi),def(std::clamp(d,lo,hi)),value(def){}float getValue()const{return (value.load()-min)/(max-min);}virtual~Parameter()=default;};
struct AudioParameterFloat:Parameter{template<class...A>AudioParameterFloat(ParameterID i,std::string n,NormalisableRange<float>r,float d,A...):Parameter(i,n,r.start,r.end,d){}};
struct AudioParameterInt:Parameter{template<class...A>AudioParameterInt(ParameterID i,std::string n,int a,int b,int d,A...):Parameter(i,n,float(a),float(b),float(d)){}};
struct AudioParameterBool:Parameter{AudioParameterBool(ParameterID i,std::string n,bool d):Parameter(i,n,0,1,d?1:0){}};
struct AudioParameterChoice:Parameter{template<class...A>AudioParameterChoice(ParameterID i,std::string n,StringArray choices,int d,A...):Parameter(i,n,0,float(choices.size()-1),float(d)){}};
struct AudioProcessorValueTreeState{
 struct ParameterLayout{std::vector<std::unique_ptr<Parameter>> params;void add(std::unique_ptr<Parameter>p){params.push_back(std::move(p));}};
 std::vector<std::unique_ptr<Parameter>> params;
 Parameter*getParameter(const char*id){for(auto&p:params)if(p->id==id)return p.get();std::abort();}
 std::atomic<float>*getRawParameterValue(const char*id){for(auto&p:params)if(p->id==id)return &p->value;std::abort();}
};
namespace dsp {
struct ProcessSpec{double sampleRate;uint32 maximumBlockSize,numChannels;};
namespace IIR {
template<class T>struct Filter{};
template<class T>struct Coefficients{
 std::array<T,5> c{1,0,0,0,0};
 static Coefficients make(double sr,double hz,double q,bool high){double w=2*MathConstants<double>::pi*hz/sr,co=std::cos(w),al=std::sin(w)/(2*q),a0=1+al;Coefficients r;r.c={T((1+(high?co:-co))/(2*a0)),T((high?-(1+co):(1-co))/a0),T((1+(high?co:-co))/(2*a0)),T(-2*co/a0),T((1-al)/a0)};return r;}
 static Coefficients makeHighPass(double s,T f,T q){return make(s,f,q,true);}static Coefficients makeLowPass(double s,T f,T q){return make(s,f,q,false);}
};}
template<class Filter,class Coeff>struct ProcessorDuplicator {Coeff state;std::array<std::array<double,2>,2>z{};void prepare(ProcessSpec){reset();}void reset(){z={};}float processSample(int ch,float x){auto&v=z[ch];auto&c=state.c;double y=c[0]*double(x)+v[0];v[0]=c[1]*double(x)-c[3]*y+v[1];v[1]=c[2]*double(x)-c[4]*y;return float(y);}};
}
}
struct MdaBase {
 mm_dsp::AudioProcessorValueTreeState apvts;double rate=48000;int block=8192;uint32_t randomState=0x973251ad;
 virtual ~MdaBase()=default;
 double getSampleRate()const{return rate;}int getBlockSize()const{return block;}int getTotalNumInputChannels()const{return 2;}int getTotalNumOutputChannels()const{return 2;}
 int randomInt(){randomState^=randomState<<13;randomState^=randomState>>17;randomState^=randomState<<5;return int(randomState&0x7fffffff);}
 virtual void prepareToPlay(double,int)=0;virtual void processBlock(mm_dsp::AudioBuffer<float>&,mm_dsp::MidiBuffer&)=0;
};
