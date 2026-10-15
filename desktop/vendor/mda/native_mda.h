

static constexpr float twopi = 6.2831853f;



// Lookup table of filter coefficients.
static float loudness[14][3] =
{
    {402.f,  0.0025f,  0.00f},  // -60 dB
    {334.f,  0.0121f,  0.00f},
    {256.f,  0.0353f,  0.00f},
    {192.f,  0.0900f,  0.00f},
    {150.f,  0.2116f,  0.00f},
    {150.f,  0.5185f,  0.00f},
    { 1.0f,     0.0f,  0.00f},  // 0 dB
    {33.7f,     5.5f,  1.00f},
    {92.0f,     8.7f,  0.62f},
    {63.7f,    18.4f,  0.44f},
    {42.9f,    48.2f,  0.30f},
    {37.6f,   116.2f,  0.18f},
    {22.9f,   428.7f,  0.09f},  // +60 dB
    { 0.0f,     0.0f,  0.00f}
};

// SPDX-License-Identifier: MIT
// Adapted DSP from hollance/mda-plugins-juce; see LICENSE.txt and ORIGIN.md.
#pragma once
#include "../../native/dsp_compat.h"


class MDAAmbienceAudioProcessor:public MdaBase { public:
~MDAAmbienceAudioProcessor() override {releaseResources();}
MDAAmbienceAudioProcessor(){apvts.params=std::move(createParameterLayout().params);}
void prepareToPlay(double,int) override;void processBlock(mm_dsp::AudioBuffer<float>&,mm_dsp::MidiBuffer&) override;void reset();void releaseResources();
private:

    mm_dsp::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();

    void update();
    void resetState();

    void flushBuffers();

    // Delay lines. The maximum length of these is hardcoded to 1024 samples.
    float *_buf1, *_buf2, *_buf3, *_buf4;

    // Read position in the delay buffers.
    int _pos;

    // This sets the length of the delays.
    float _size;

    // Feedback coefficient for the allpass filters.
    float _feedback;

    // Low-pass filter coefficient for HF damping.
    float _damp;

    // Low-pass filter state value.
    float _filter;

    // Wet/dry mix.
    float _wet, _dry;

    
};

void MDAAmbienceAudioProcessor::prepareToPlay(double sampleRate, int samplesPerBlock)
{
    _buf1 = new float[1024];
    _buf2 = new float[1024];
    _buf3 = new float[1024];
    _buf4 = new float[1024];

    resetState();
}
void MDAAmbienceAudioProcessor::releaseResources()
{
    delete [] _buf1; _buf1 = nullptr;
    delete [] _buf2; _buf2 = nullptr;
    delete [] _buf3; _buf3 = nullptr;
    delete [] _buf4; _buf4 = nullptr;
}
void MDAAmbienceAudioProcessor::reset()
{
    resetState();
}
void MDAAmbienceAudioProcessor::resetState()
{
    flushBuffers();
    _pos = 0;
    _filter = 0.0f;
}
void MDAAmbienceAudioProcessor::flushBuffers()
{
    memset(_buf1, 0, 1024 * sizeof(float));
    memset(_buf2, 0, 1024 * sizeof(float));
    memset(_buf3, 0, 1024 * sizeof(float));
    memset(_buf4, 0, 1024 * sizeof(float));
}
void MDAAmbienceAudioProcessor::update()
{
    _feedback = 0.8f;

    // Convert the percentage to a filter coefficient between 0.05 - 0.95.
    // The higher HF Damp, the *less* filtering!
    float fParam1 = apvts.getRawParameterValue("HF Damp")->load() / 100.0f;
    _damp = 0.05f + 0.9f * fParam1;

    // Convert the output from [-20, +20] dB into a gain of [0.1, 10.0].
    float fParam3 = apvts.getRawParameterValue("Output")->load();
    float tmp = mm_dsp::Decibels::decibelsToGain(fParam3);

    // Calculate a dry/wet balance based on the mix setting. If mix = 0%, only
    // the dry signal is used. At mix = 68%, dry and wet are both approx 0.54.
    // For mix = 100%, wet is 0.8 and dry is 0. So this is slightly different
    // from a regular dry/wet mix that does dry = 100% - wet.
    float fParam2 = apvts.getRawParameterValue("Mix")->load() / 100.0f;
    _dry = tmp - fParam2 * fParam2 * tmp;
    _wet = (0.4f + 0.4f) * fParam2 * tmp;

    // Convert the size from 0 - 10 meters to a value between 0.025 - 2.69.
    // This size is used to set the delay times on the different delay lines:
    // a larger size means a longer delay. The sample rate is not taken into
    // consideration here, so the units (meters) seem to be kind of pointless,
    // and running the plugin at different sample rates will give different
    // results for the same size setting.
    float fParam0 = apvts.getRawParameterValue("Size")->load() / 10.0f;
    tmp = 0.025f + 2.665f * fParam0;
    if (_size != tmp) { flushBuffers(); }  // need to flush delay lines
    _size = tmp;
}
void MDAAmbienceAudioProcessor::processBlock(mm_dsp::AudioBuffer<float> &buffer, mm_dsp::MidiBuffer &midiMessages)
{
    mm_dsp::ScopedNoDenormals noDenormals;
    auto totalNumInputChannels = getTotalNumInputChannels();
    auto totalNumOutputChannels = getTotalNumOutputChannels();

    // Clear any output channels that don't contain input data.
    for (auto i = totalNumInputChannels; i < totalNumOutputChannels; ++i) {
        buffer.clear(i, 0, buffer.getNumSamples());
    }

    update();

    const float *in1 = buffer.getReadPointer(0);
    const float *in2 = buffer.getReadPointer(1);
    float *out1 = buffer.getWritePointer(0);
    float *out2 = buffer.getWritePointer(1);

    int p = _pos;

    // The main structure of this effect is four allpass filters in series.
    // Each of these is made up of a delay line with a different delay length.
    // The _size variable sets the actual delay time, with 1024 samples being
    // the maximum. (Note: 379 * the maximum size 2.69 is just less than 1024.)
    // The d1 - d4 variables are the write indices into the four delay lines.
    // We do `& 1023` to wrap around the index when it goes past the end.
    int d1 = (p + int(107 * _size)) & 1023;
    int d2 = (p + int(142 * _size)) & 1023;
    int d3 = (p + int(277 * _size)) & 1023;
    int d4 = (p + int(379 * _size)) & 1023;

    const float feedback = _feedback;
    const float damp = _damp;
    const float dry = _dry;
    const float wet = _wet;
    float f = _filter;

    for (int i = 0; i < buffer.getNumSamples(); ++i) {
        float a = in1[i];
        float b = in2[i];

        // Combine the left and right stereo inputs into a single mono value.
        // Also multiply by the wetness amount. We can do this here already
        // because everything that follows are linear operations. Note that
        // the maximum value of wet is 0.8, not 1.0.
        float x = wet * (a + b);

        // HF damping. This is a simple low-pass filter: f = a*x + (1 - a)*f.
        f += damp * (x - f);
        float r = f;

        /*
          Next there are four sections of allpass filters, as explained here:
          https://ccrma.stanford.edu/~jos/pasp/Schroeder_Allpass_Sections.html

          Each of these allpass stages does the following:

                          ^-----------------------+
                          |                       |
                          |                       v
              r ---> + --------> delay ---------> + ---> new r
                     ^                      |
                     |                      |
                     <---- *(-feedback) ----v

          The feedback gain is always 0.8. In a typical Schroeder allpass,
          feedforward gain is equal to the feedback gain (and positive) but
          here it is always 1.0!
         */

        // First allpass stage.
        float t = _buf1[p];
        r -= feedback * t;
        _buf1[d1] = r;
        r += t;

        // Second allpass stage.
        t = _buf2[p];
        r -= feedback * t;
        _buf2[d2] = r;
        r += t;

        // Third allpass stage.
        t = _buf3[p];
        r -= feedback * t;
        _buf3[d3] = r;
        r += t;

        // The left channel output is a mix of the dry input with the allpass
        // filtered signal. We want the wet output to be a diffuse version of
        // the input, but the very first thing coming out of the delay lines
        // is `f`, the low-pass filtered input. We don't want a copy of the dry
        // signal in the wet signal, which is why `f` gets subtracted here.
        a = dry * a + r - f;

        // Fourth allpass stage.
        t = _buf4[p];
        r -= feedback * t;
        _buf4[d4] = r;
        r += t;

        // The right channel output. This has one more delay stage than the
        // left channel, making the reverb tail a bit longer on the right.
        // You can clearly hear this on headphones; it's a little weird.
        // The reverb sounds mono until the left channel fades out and the
        // right keeps playing.
        b = dry * b + r - f;

        // Advance the read and write indices for the delay lines, wrap if needed.
        ++p  &= 1023;
        ++d1 &= 1023;
        ++d2 &= 1023;
        ++d3 &= 1023;
        ++d4 &= 1023;

        out1[i] = a;
        out2[i] = b;
    }

    _pos = p;
    _filter = f;

    // N.B. The original code had denormal handling on `f` here that also resets
    // the contents of the delay lines. That could interrupt the reverb tail if
    // the input signal stops playing but the reverb was still ringing. It would
    // have made more sense to put that denormal handling on the outputs of the
    // all-pass filter sections. But we use mm_dsp::ScopedNoDenormals instead. :-)
}
mm_dsp::AudioProcessorValueTreeState::ParameterLayout MDAAmbienceAudioProcessor::createParameterLayout()
{
    mm_dsp::AudioProcessorValueTreeState::ParameterLayout layout;

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Size", 1),
        "Size",
        mm_dsp::NormalisableRange<float>(0.0f, 10.0f, 0.01f),
        7.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("HF Damp", 1),
        "HF Damp",
        mm_dsp::NormalisableRange<float>(0.0f, 100.0f, 0.01f),
        70.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Mix", 1),
        "Mix",
        mm_dsp::NormalisableRange<float>(0.0f, 100.0f, 0.01f),
        90.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Output", 1),
        "Output",
        mm_dsp::NormalisableRange<float>(-20.0f, 20.0f, 0.01f),
        0.0f));

    return layout;
}

class MDABandistoAudioProcessor:public MdaBase { public:
MDABandistoAudioProcessor(){apvts.params=std::move(createParameterLayout().params);}
void prepareToPlay(double,int) override;void processBlock(mm_dsp::AudioBuffer<float>&,mm_dsp::MidiBuffer&) override;void reset();void releaseResources();
private:

    mm_dsp::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();

    void update();
    void resetState();

    float sampleRate;
    float driv1, trim1;   // drive and gain for low band
    float driv2, trim2;   // ... mid band
    float driv3, trim3;   // ... high band
    float fi1, fo1;       // filter coefficients
    float fi2, fo2;
    float fb1, fb2, fb3;  // filter delays
    float sideLevel;      // output level for the stereo data
    int valve;            // 1 if unipolar mode, 0 if bipolar

    
};

void MDABandistoAudioProcessor::prepareToPlay(double newSampleRate, int samplesPerBlock)
{
    sampleRate = float(newSampleRate);
    resetState();
}
void MDABandistoAudioProcessor::releaseResources()
{
}
void MDABandistoAudioProcessor::reset()
{
    resetState();
}
void MDABandistoAudioProcessor::resetState()
{
    fb1 = 0.0f;
    fb2 = 0.0f;
    fb3 = 0.0f;
}
void MDABandistoAudioProcessor::update()
{
    float param4 = apvts.getRawParameterValue("L Dist")->load();
    float param5 = apvts.getRawParameterValue("M Dist")->load();
    float param6 = apvts.getRawParameterValue("H Dist")->load();

    // The drive parameter follows a rather dramatic curve from 0.1 to 100000.
    // On the UI this is shown as 0 - 60 dB, which I don't think corresponds
    // to what actually happens.
    driv1 = std::pow(10.0f, 6.0f * param4 * param4 - 1.0f);
    driv2 = std::pow(10.0f, 6.0f * param5 * param5 - 1.0f);
    driv3 = std::pow(10.0f, 6.0f * param6 * param6 - 1.0f);

    // Calculate gain based on bipolar (transistor) or unipolar (value) mode.
    // In unipolar mode, the gain is 0.5 because the mid-side signal is 6 dB
    // too loud, and so this compensates for that. In bipolar mode, the higher
    // the drive is for a band, the larger the gain for that band is too, from
    // 0.3 all the way up to 3000! This is because at high drive, the output
    // from the waveshaper is tiny and so it needs a big boost.
    valve = int(apvts.getRawParameterValue("Mode")->load());
    if (valve) {  // valve (tube)
        trim1 = 0.5f;
        trim2 = 0.5f;
        trim3 = 0.5f;
    } else {  // transistor
        trim1 = 0.3f * std::pow(10.0f, 4.0 * std::pow(param4, 3.0f));
        trim2 = 0.3f * std::pow(10.0f, 4.0 * std::pow(param5, 3.0f));
        trim3 = 0.3f * std::pow(10.0f, 4.0 * std::pow(param6, 3.0f));
    }

    float param7 = apvts.getRawParameterValue("L Out")->load();
    float param8 = apvts.getRawParameterValue("M Out")->load();
    float param9 = apvts.getRawParameterValue("H Out")->load();

    // The output level goes from -20 dB to 20 dB. Convert to linear gain
    // and combine it with the gain that is based on the drive.
    trim1 *= std::pow(10.0f, 2.0f * param7 - 1.0f);
    trim2 *= std::pow(10.0f, 2.0f * param8 - 1.0f);
    trim3 *= std::pow(10.0f, 2.0f * param9 - 1.0f);

    // Determine which band will be output by setting the levels of the
    // other bands to zero. In "Output" mode, all bands are included.
    int param1 = int(apvts.getRawParameterValue("Listen")->load());
    switch (param1) {
        case 0: trim2 = 0.0f; trim3 = 0.0f; sideLevel = 0.0f; break;
        case 1: trim1 = 0.0f; trim3 = 0.0f; sideLevel = 0.0f; break;
        case 2: trim1 = 0.0f; trim2 = 0.0f; sideLevel = 0.0f; break;
        default: sideLevel = 0.5f; break;
    }

    // Calculate the coefficients for the crossover filters. The actual
    // frequencies in Hz depend on the sample rate.
    float param2 = apvts.getRawParameterValue("L <> M")->load();
    float param3 = apvts.getRawParameterValue("M <> H")->load();
    fi1 = std::pow(10.0f, param2 - 1.70f);
    fi2 = std::pow(10.0f, param3 - 1.05f);
    fo1 = 1.0f - fi1;
    fo2 = 1.0f - fi2;
}
void MDABandistoAudioProcessor::processBlock(mm_dsp::AudioBuffer<float> &buffer, mm_dsp::MidiBuffer &midiMessages)
{
    mm_dsp::ScopedNoDenormals noDenormals;
    auto totalNumInputChannels = getTotalNumInputChannels();
    auto totalNumOutputChannels = getTotalNumOutputChannels();

    // Clear any output channels that don't contain input data.
    for (auto i = totalNumInputChannels; i < totalNumOutputChannels; ++i) {
        buffer.clear(i, 0, buffer.getNumSamples());
    }

    update();

    const float *in1 = buffer.getReadPointer(0);
    const float *in2 = buffer.getReadPointer(1);
    float *out1 = buffer.getWritePointer(0);
    float *out2 = buffer.getWritePointer(1);

    for (int i = 0; i < buffer.getNumSamples(); ++i) {
        float a = in1[i];
        float b = in2[i];

        // Keep stereo component for later. This side signal won't be distorted.
        float s = (a - b) * sideLevel;

        // Create mono signal (mids). Only these mids will be distorted.
        // Also add a small DC offset. The original comment said "dope filter",
        // so I figured this made the filter extra dope but they probably meant
        // doping as in adding small "impurities" that give the filter something
        // to do?! Not sure it actually matters.
        a += b + 0.00002f;

        // Crossover filters. Three basic 6 dB/oct one-pole low-pass filters in
        // series. Their frequency responses look a little funky, but `l + m + h`
        // does reconstruct the signal, since `fb3 + (fb2 - fb3) + (a - fb2) = a`.
        fb2 = fi2 * a   + fo2 * fb2;
        fb1 = fi1 * fb2 + fo1 * fb1;
        fb3 = fi1 * fb1 + fo1 * fb3;

        float l = fb3;       // low band
        float m = fb2 - l;   // mid band
        float h = a - fb2;   // high band

        // Distort. First rectify the signal so it's always positive, then use
        // the magnitude to calculate a gain for each band. Because the formula
        // is 1 / (1 + drive * amplitude), the higher the amplitude, the lower
        // the gain, so this compresses the louder parts of the signal. And the
        // higher the drive, the more extreme this compression is, eventually
        // turning the distortion into hard clipping.
        float g1 = (l > 0.0f) ? l : -l;
        g1 = 1.0f / (1.0f + driv1 * g1);

        float g2 = (m > 0.0f) ? m : -m;
        g2 = 1.0f / (1.0f + driv2 * g2);

        float g3 = (h > 0.0f) ? h : -h;
        g3 = 1.0f / (1.0f + driv3 * g3);

        /*
            To plot the output of this waveshaper in something like Desmos,
            use the following:

            // the drive parameter
            k: a number between 0 and 1

            // the drive value (0.1 - 100000)
            d = 10^(6k^2 - 1)

            // the gain compensation value (trim)
            t = 0.3 * 10^(4 * k^3)

            // the waveshaper formula
            x * t / (1 + d * x)  { 0 <= x <= 1 }

            This plots the transfer curve. You'll see that for low values of
            `k`, the line is straight. But for higher values of `k` the curve
            will bend and eventually drops in height.
        */

        // In unipolar mode, only distort samples with a negative polarity.
        if (valve) {
            if (l > 0.0f) { g1 = 1.0f; }
            if (m > 0.0f) { g2 = 1.0f; }
            if (h > 0.0f) { g3 = 1.0f; }
        }

        // Apply the distortion to each band and recombine the bands.
        a = l*g1*trim1 + m*g2*trim2 + h*g3*trim3;

        // Add the side signal back in to restore the stereo nature.
        out1[i] = a + s;
        out2[i] = a - s;
    }
}
mm_dsp::AudioProcessorValueTreeState::ParameterLayout MDABandistoAudioProcessor::createParameterLayout()
{
    mm_dsp::AudioProcessorValueTreeState::ParameterLayout layout;

    layout.add(std::make_unique<mm_dsp::AudioParameterChoice>(
        mm_dsp::ParameterID("Listen", 1),
        "Listen",
        mm_dsp::StringArray { "Low", "Mid", "High", "Output" },
        3));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("L <> M", 1),
        "L <> M",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.4f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("M <> H", 1),
        "M <> H",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.5f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("L Dist", 1),
        "L Dist",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.5f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("M Dist", 1),
        "M Dist",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.5f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("H Dist", 1),
        "H Dist",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.5f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("L Out", 1),
        "L Out",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.5f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("M Out", 1),
        "M Out",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.5f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("H Out", 1),
        "H Out",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.5f));

    layout.add(std::make_unique<mm_dsp::AudioParameterChoice>(
        mm_dsp::ParameterID("Mode", 1),
        "Mode",
        mm_dsp::StringArray { "Bipolar", "Unipolar" },
        3));

    return layout;
}

class MDABeatBoxAudioProcessor:public MdaBase { public:
MDABeatBoxAudioProcessor(){apvts.params=std::move(createParameterLayout().params);}
void prepareToPlay(double,int) override;void processBlock(mm_dsp::AudioBuffer<float>&,mm_dsp::MidiBuffer&) override;void reset();void releaseResources();
private:

    mm_dsp::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();

    void update();
    void resetState();
    void synth();

    float sampleRate;

    float hthr;           // hi-hat threshold
    float hfil;           // high-pass filter state
    float hlev;           // hat mix
    int hdel;             // minimum delay in samples between successive hi-hats

    float kthr;           // kick threshold
    float ksf1, ksf2;     // filter coeffs
    float ksb1, ksb2;     // filter state
    float klev;           // kick mix
    int kdel;             // minimum delay in samples between successive kicks

    float kww;            // Kik Trig parameter
    int ksfx;             // if 0, normal operation; if > 0, key listen mode

    float sthr;           // snare threshold
    float sf1, sf2, sf3;  // filter coeffs
    float sb1, sb2;       // filter state
    float slev;           // snare mix
    int sdel;             // minimum delay in samples between successive snares

    float ww;             // Snr Trig parameter
    int sfx;              // if 0, normal operation; if > 0, key listen mode

    float dyne;           // current envelope level
    float dyna;           // envelope attack
    float dynr;           // envelope release

    float dynm;           // Dynamics parameter
    float mix;            // Thru Mix parameter

    std::unique_ptr<float[]> hbuf;   // buffer containing hi-hat sound
    std::unique_ptr<float[]> kbuf;   // kick sound
    std::unique_ptr<float[]> sbufL;  // snare is stereo
    std::unique_ptr<float[]> sbufR;

    int hbuflen;          // hi-hat buffer length
    int kbuflen;          // kick buffer length
    int sbuflen;          // snare buffer length

    int hbufpos;          // current position in hi-hat buffer
    int kbufpos;          // in kick buffer
    int sbufpos;          // in snare buffer

    
};

void MDABeatBoxAudioProcessor::prepareToPlay(double newSampleRate, int samplesPerBlock)
{
    sampleRate = float(newSampleRate);

    hbuflen = 20000;
    kbuflen = 20000;
    sbuflen = 60000;

    // See the note in synth() on higher sampling rates.
    if (sampleRate > 49000.0f) {
        hbuflen *= 2;
        kbuflen *= 2;
        sbuflen *= 2;
    }

    hbuf.reset(new float[hbuflen]);
    kbuf.reset(new float[kbuflen]);
    sbufL.reset(new float[sbuflen]);
    sbufR.reset(new float[sbuflen]);

    synth();

    // These variables store after how many samples the kick and snare
    // are allowed to repeat. For the hi-hat this is a parameter.
    kdel = int(0.10f * sampleRate);
    sdel = int(0.12f * sampleRate);

    // Fixed attack and release times for envelope follower.
    dyna = std::pow(10.0f, -1000.0f / sampleRate);
    dynr = std::pow(10.0f, -6.0f / sampleRate);
    dyne = 0.0f;

    resetState();
}
void MDABeatBoxAudioProcessor::releaseResources()
{
}
void MDABeatBoxAudioProcessor::reset()
{
    resetState();
}
void MDABeatBoxAudioProcessor::synth()
{
    float o = 0.0f;
    float p = 0.2f;

    // Generate hi-hat. This is a burst of noise with an exponentially decaying
    // envelope, and some basic filtering applied. This filter cuts around 5k
    // and boosts around 10k to make it brighter.
    // Note that randomInt() is bad and should be avoided in modern C++ code.
    // The original plug-in never seeded the random number generator, so the
    // hi-hat sounds different every time you run the plug-in. For even more
    // varied results, a new sound could be synthesized on-the-fly every time
    // the hi-hat is triggered.
    {
        std::memset(hbuf.get(), 0, hbuflen * sizeof(float));
        float e = 0.00012f;
        float de = std::pow(10.0f, -36.0f/sampleRate);
        float o1 = 0.0f;
        float o2 = 0.0f;
        for (int t = 0; t < 5000; ++t) {
            o = float((randomInt() % 2000) - 1000);
            hbuf[t] = e * (2.0f*o1 - o2 - o);
            e *= de;
            o2 = o1;
            o1 = o;
        }
    }

    // Generate kick sample. This is a sine wave that decays exponentially
    // in amplitude as well as in frequency.
    {
        std::memset(kbuf.get(), 0, kbuflen * sizeof(float));
        float e = 0.5f;
        float de = std::pow(10.0f, -3.8f/sampleRate);
        float dp = 1588.0f / sampleRate;
        for (int t = 0; t < 14000; ++t) {
            kbuf[t] = e * std::sin(p);
            e *= de;
            p = std::fmod(p + dp * e, 6.2831853f);
        }
    }

    // Generate snare. This is a sine wave with an exponentially decaying
    // envelope and a small amount of added (filtered) noise. The snare
    // buffer is stereo, although that feature was only used for recording
    // your own sounds, which I did not include in the JUCE version.
    {
        std::memset(sbufL.get(), 0, sbuflen * sizeof(float));
        std::memset(sbufR.get(), 0, sbuflen * sizeof(float));
        float e = 0.38f;
        float de = std::pow(10.0f, -15.0f/sampleRate);
        for (int t = 0; t < 7000; ++t) {
            o = 0.3f * o + float((randomInt() % 2000) - 1000);
            sbufL[t] = e * (std::sin(p) + 0.0004f * o);
            sbufR[t] = sbufL[t];
            e *= de;
            p = std::fmod(p + 0.025f, 6.2831853f);
        }
    }

    // Note that the synthesized sounds are not independent of the sample
    // rate! The higher the sample rate, the higher the pitch. Additionally,
    // the durations are wrong for higher sample rates. Also not sure why
    // the snare code uses the `p` and `o` variables from the kick.
}
void MDABeatBoxAudioProcessor::resetState()
{
    hbufpos = hbuflen - 1;
    kbufpos = kbuflen - 1;
    sbufpos = sbuflen - 1;

    hfil = 0.0f;

    kww = 0.0f;
    ksfx = 0;
    ksb1 = 0.0f;
    ksb2 = 0.0f;

    ww = 0.0f;
    sfx = 0;
    sb1 = 0.0f;
    sb2 = 0.0f;
}
void MDABeatBoxAudioProcessor::update()
{
    // Convert from decibels (-40 dB ... 0 dB) to a linear value.
    float param1 = apvts.getRawParameterValue("Hat Thr")->load();
    hthr = std::pow(10.0f, 2.0f * param1 - 2.0f);

    // After how many samples the hi-hat is allowed to repeat.
    float param2 = apvts.getRawParameterValue("Hat Rate")->load();
    hdel = int((0.04f + 0.2f * param2) * sampleRate);

    // Convert from decibels (-80 dB ... +12 dB) to a linear value.
    float param3 = apvts.getRawParameterValue("Hat Mix")->load();
    hlev = 0.0001f + param3 * param3 * 4.0f;

    // Convert from decibels (-40 dB ... 0 dB) to a linear value.
    float param4 = apvts.getRawParameterValue("Kik Thr")->load();
    kthr = 220.0f * std::pow(10.0f, 2.0f * param4 - 2.0f);

    // Keep track of the old values so we can see if they changed.
    float kwwx = kww;
    float wwx = ww;

    // The frequency for the kick filter.
    float param5 = apvts.getRawParameterValue("Kik Trig")->load();
    kww = std::pow(10.0f, -3.0f + 2.2f * param5);
    ksf1 = std::cos(3.1415927f * kww);  // p
    ksf2 = std::sin(3.1415927f * kww);  // q

    // Convert from decibels (-80 dB ... +12 dB) to a linear value.
    float param6 = apvts.getRawParameterValue("Kik Mix")->load();
    klev = 0.0001f + param6 * param6 * 4.0f;

    // Convert from decibels (-40 dB ... 0 dB) to a linear value.
    float param7 = apvts.getRawParameterValue("Snr Thr")->load();
    sthr = 40.0f * std::pow(10.0f, 2.0f * param7 - 2.0f);

    // The frequency for the snare filter.
    float param8 = apvts.getRawParameterValue("Snr Trig")->load();
    ww = std::pow(10.0f, -3.0f + 2.2f * param8);
    sf1 = std::cos(3.1415927f * ww);  // p
    sf2 = std::sin(3.1415927f * ww);  // q
    sf3 = 0.991f;  // r

    // The plug-in switches to "key listen" mode while the Kik Trig
    // or Snr Trig parameter is being adjusted. This sets the counter
    // to two seconds.
    if (kwwx != kww) {
        ksfx = int(2.0f * sampleRate);
    }
    if (wwx != ww) {
        sfx = int(2.0f * sampleRate);
    }

    // Convert from decibels (-80 dB ... +12 dB) to a linear value.
    float param9 = apvts.getRawParameterValue("Snr Mix")->load();
    slev = 0.0001f + param9 * param9 * 4.0f;

    // This parameter is a percentage.
    dynm = apvts.getRawParameterValue("Dynamics")->load();

    // This parameter is displayed as -inf dB ... 0 dB but is already
    // 0 - 1, so we don't need to convert anything.
    mix = apvts.getRawParameterValue("Thru Mix")->load();
}
void MDABeatBoxAudioProcessor::processBlock(mm_dsp::AudioBuffer<float> &buffer, mm_dsp::MidiBuffer &midiMessages)
{
    mm_dsp::ScopedNoDenormals noDenormals;
    auto totalNumInputChannels = getTotalNumInputChannels();
    auto totalNumOutputChannels = getTotalNumOutputChannels();

    // Clear any output channels that don't contain input data.
    for (auto i = totalNumInputChannels; i < totalNumOutputChannels; ++i) {
        buffer.clear(i, 0, buffer.getNumSamples());
    }

    update();

    const float *in1 = buffer.getReadPointer(0);
    const float *in2 = buffer.getReadPointer(1);
    float *out1 = buffer.getWritePointer(0);
    float *out2 = buffer.getWritePointer(1);

    int hbufmax = hbuflen - 1;
    int kbufmax = kbuflen - 1;
    int sbufmax = sbuflen - 1;

    float mix3 = 0.0f;

    // Key listen (snare). This turns off everything except the snare filter
    // output. This continues until two seconds worth of samples have elapsed.
    if (sfx > 0) {
        mix3 = 0.08f;
        slev = 0.0f;
        klev = 0.0f;
        hlev = 0.0f;
        mix = 0.0f;
        sfx -= buffer.getNumSamples();
    }

    // Key listen (kick). This also uses the snare filter but swaps the coeffs
    // to those from the kick filter.
    if (ksfx > 0) {
        mix3 = 0.03f;
        slev = 0.0f;
        klev = 0.0f;
        hlev = 0.0f;
        mix = 0.0f;
        ksfx -= buffer.getNumSamples();
        sf1 = ksf1;
        sf2 = ksf2;
    }

    for (int i = 0; i < buffer.getNumSamples(); ++i) {
        float a = in1[i];
        float b = in2[i];

        // Convert the stereo signal to mono. (This is 6 dB too loud.)
        float e = a + b;

        // Envelope follower. If the new sample is lower than the envelope,
        // apply release, otherwise apply attack (using a one-pole filter).
        // Note that in a regular envelope follower the `e` value would be
        // absolute but here it can be negative as well. Not sure if that
        // was an error in the original code or intentional.
        dyne = (e < dyne) ? dyne * dynr : e - dyna * (e - dyne);

        // Basic first order high-pass filter with a cutoff around 10 kHz.
        // This computes the differences between two successive samples.
        // This difference is close to zero at low frequencies but large
        // at high frequencies.
        hfil = e - hfil;

        // Start playing the hi-hat sample when the filter output exceeds
        // the threshold and we've waited long enough since the last hi-hat.
        if ((hbufpos > hdel) && (hfil > hthr)) {
            hbufpos = 0;
        } else if (hbufpos < hbufmax) {  // play until end and hold there
            hbufpos++;
        }
        float o = hlev * hbuf[hbufpos];

        // Low filter. This is a low-pass that gradually turns into a band
        // pass. It has a massive gain for some reason... Unlike the hi-hat
        // filter, which is fixed, the kick and snare filters can be changed.
        float kfil = e + ksb1*ksf1 - ksb2*ksf2;
        ksb2 = sf3 * (ksb1*ksf2 + ksb2*ksf1);
        ksb1 = sf3 * kfil;

        // Start playing the kick sample when the filter output exceeds
        // the threshold and we've waited long enough since the last kick.
        // Note that a short `kdel` time can cause the existing kick sample
        // to be cut off suddenly when the new kick starts, which may glitch
        // a little. It would be better to quickly fade out the old kick.
        if ((kbufpos > kdel) && (kfil > kthr)) {
            kbufpos = 0;
        } else if (kbufpos < kbufmax) {
            kbufpos++;
        }
        o += klev * kbuf[kbufpos];

        // Mid filter. Similar to the kick filter; same coeffs but slightly
        // different formula.
        float sfil = hfil + 0.3f*e + sb1*sf1 - sb2*sf2;
        sb2 = sf3 * (sb1*sf2 + sb2*sf1);
        sb1 = sf3 * sfil;

        // Play the snare sample.
        if ((sbufpos > sdel) && (sfil > sthr)) {
            sbufpos = 0;
        } else if (sbufpos < sbufmax) {
            sbufpos++;
        }
        float c = o + slev*sbufL[sbufpos];
        float d = o + slev*sbufR[sbufpos];

        // Dynamics. This applies the envelope of the original sound
        // to the synthesized samples. It does make things a lot louder!
        float mix4 = 1.0f + dynm * (dyne + dyne - 1.0f);

        // The final output mixes the original input with the synthesized
        // signal. In "key listen" mode, we mix in the snare / kick filter
        // output, and everything else is turned off.
        out1[i] = mix*a + mix3*sfil + mix4*c;
        out2[i] = mix*b + mix3*sfil + mix4*d;

        hfil = e;
    }
}
mm_dsp::AudioProcessorValueTreeState::ParameterLayout MDABeatBoxAudioProcessor::createParameterLayout()
{
    mm_dsp::AudioProcessorValueTreeState::ParameterLayout layout;

    // -40 dB ... 0 dB
    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Hat Thr", 1),
        "Hat Thr",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f, 0.01f),
        0.3f
        ));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Hat Rate", 1),
        "Hat Rate",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f, 0.01f),
        0.45f
        ));

    // -80 dB ... +12 dB
    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Hat Mix", 1),
        "Hat Mix",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f, 0.01f),
        0.5f
        ));

    // -40 dB ... 0 dB
    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Kik Thr", 1),
        "Kik Thr",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f, 0.01f),
        0.46f
        ));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Kik Trig", 1),
        "Kik Trig",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f, 0.01f),
        0.15f
        ));

    // -80 dB ... +12 dB
    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Kik Mix", 1),
        "Kik Mix",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f, 0.01f),
        0.5f
        ));

    // -40 dB ... 0 dB
    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Snr Thr", 1),
        "Snr Thr",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f, 0.01f),
        0.5f
        ));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Snr Trig", 1),
        "Snr Trig",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f, 0.01f),
        0.7f
        ));

    // -80 dB ... +12 dB
    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Snr Mix", 1),
        "Snr Mix",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f, 0.01f),
        0.5f
        ));

    // 0% ... 100%
    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Dynamics", 1),
        "Dynamics",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f, 0.01f),
        0.0f
        ));

    // -inf dB ... 0 dB
    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Thru Mix", 1),
        "Thru Mix",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f, 0.01f),
        0.0f
        ));

    return layout;
}

class MDADegradeAudioProcessor:public MdaBase { public:
MDADegradeAudioProcessor(){apvts.params=std::move(createParameterLayout().params);}
void prepareToPlay(double,int) override;void processBlock(mm_dsp::AudioBuffer<float>&,mm_dsp::MidiBuffer&) override;void reset();void releaseResources();
private:

    mm_dsp::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();

    void update();
    float filterFreq(float hz);
    void resetState();

    // To reduce the sampling rate, we only read from the input buffer every
    // sampleInterval samples.
    int _sampleInterval, _sampleIndex;

    // This is 1.0 if sample-and-hold mode is active, 0.0 if not.
    float _mode;

    // Used to perform quantizing.
    float _g1, _g2;

    // Non-linearity values for negative and positive samples.
    float _linNeg, _linPos;

    // Level for headroom clipping.
    float _clip;

    // Output gain.
    float _g3;

    // Filter coefficients.
    float _fi, _fo;

    // Sum of the previous samples in sample-and-hold mode.
    float _accum;

    // The most recently computed sample value, before filtering.
    float _currentSample;

    // Delay units for the 8 filter stages.
    float _buf1, _buf2, _buf3, _buf4, _buf6, _buf7, _buf8, _buf9;

    
};

void MDADegradeAudioProcessor::prepareToPlay(double sampleRate, int samplesPerBlock)
{
    resetState();
}
void MDADegradeAudioProcessor::releaseResources()
{
}
void MDADegradeAudioProcessor::reset()
{
    resetState();
}
void MDADegradeAudioProcessor::resetState()
{
    _accum = 0.0f;
    _currentSample = 0.0f;
    _buf1 = _buf2 = _buf3 = _buf4 = _buf6 = _buf7 = _buf8 = _buf9 = 0.0f;
    _sampleIndex = 1;
}
float MDADegradeAudioProcessor::filterFreq(float hz)
{
    /*
      The filter is a one-pole filter: y(n) = b*x(n) + a*y(n - 1).

      This function calculates the value of the coefficient a, given a cutoff
      frequency. And then we set b = (1 - a) to get unity gain.

      I'm not sure where the formula used here comes from. It does not give a
      response that has -3 dB at the cutoff frequency.

      The formula that does give a -3 dB cutoff is:
            k = 1.0 - cos(2.0 * pi * hz / sampleRate)
            a = 1 + k - sqrt(k * k + 2.0 * k)

      Another way to calculate the coefficient is the approximation:
            a = exp(-2 * pi * hz / sampleRate)
     */
    float r = 0.999f;
    float j = r * r - 1.0f;
    float k = 2.0f - 2.0f * r * r * std::cos(0.647f * hz / float(getSampleRate()));
    return (std::sqrt(k * k - 4.0f * j * j) - k) / (2.0f * j);
}
void MDADegradeAudioProcessor::update()
{
    // The sample interval is a number between 1 and 10. If 1, we read from the
    // input buffer at the project's sample rate. If the interval is 2, we skip
    // every other sample, cutting the effective sample rate in half; and so on.
    _sampleInterval = apvts.getRawParameterValue("Sample Rate")->load();

    // In sample-and-hold mode, we do read every single sample but add them up
    // over the sampleInterval and take the mean once we have enough samples.
    _mode = apvts.getRawParameterValue("Sample Mode")->load();

    // Headroom: convert from decibels to a linear value.
    float headroom = apvts.getRawParameterValue("Headroom")->load();
    _clip = mm_dsp::Decibels::decibelsToGain(headroom);

    // Filtering coefficients. The filter does: y(n) = fi * x(n) + fo * y(n - 1).
    // fo is the coefficient for the feedback loop, fi controls the overall gain.
    // We choose fi so that it keeps the amplitude of the unfiltered frequencies
    // at 0 dB (unity gain). Since fi = 1 - fo, that makes this an "exponentially
    // weighted moving average" or EWMA filter.
    _fo = filterFreq(apvts.getRawParameterValue("PostFilter")->load());
    _fi = 1.0f - _fo;

    // The filter stage actually consists of several identical filters in series.
    // As an optimization, make the gain coefficient (1 - fo)^4, which will save
    // doing a few multiplications.
    _fi = _fi * _fi;
    _fi = _fi * _fi;

    // The formula used for quantization is:
    //   x = int(x * 2^bits) / (2^bits) = int(x * g1) / g1 = int(x * g1) * g2
    //
    // Note that g2 = 1.0f / (2.0f * g1) instead of just 1.0f / g1. That extra
    // 1/2 compensates for the signal being twice as loud because we will convert
    // from stereo to mono by adding up both channels.
    //
    // The original code had a bug here, where it went from 2 - 14 bits instead
    // of the 4 - 16 bits that was intended.
    int numBits = apvts.getRawParameterValue("Quantize")->load();
    _g1 = float(1 << numBits);
    _g2 = 1.0f / (2.0f * _g1);

    // In sample-and-hold mode, the samples get accumulated over the interval,
    // which means the accumulator can grow very large. To take the mean value,
    // we divide by the number of samples added. We can do this in g1 and save
    // an extra instruction in the processing loop.
    if (_mode == 1.0f) {
        _g1 /= float(_sampleInterval);
    }

    // Note: the original plug-in made g1 negative which serves no purpose except
    // to invert the signal's phase. Not sure if that was intended or not; enable
    // it by uncommenting the line below.
    // _g1 = -_g1;

    // The output level is in dB, so convert to linear gain.
    float outputLevel = apvts.getRawParameterValue("Output")->load();
    _g3 = mm_dsp::Decibels::decibelsToGain(outputLevel);

    // Non-linearity: 0 = x^1 ... 1 = x^0.707. The plug-in uses an exponential
    // curve between these two points but a linear interpolation using jmap()
    // between 1 and 1/sqrt(2) would give virtually the same result.
    float nonlinearity = apvts.getRawParameterValue("Non-Linearity")->load();
    _linNeg = std::pow(10.0f, -0.15f * nonlinearity);

    // If the mode is odd, the non-linearity is not applied to positive samples.
    if (apvts.getRawParameterValue("Non-Lin Mode")->load() == 0.0f) {
        _linPos = 1.0f;
    } else {
        _linPos = _linNeg;
    }
}
void MDADegradeAudioProcessor::processBlock(mm_dsp::AudioBuffer<float> &buffer, mm_dsp::MidiBuffer &midiMessages)
{
    mm_dsp::ScopedNoDenormals noDenormals;
    auto totalNumInputChannels = getTotalNumInputChannels();
    auto totalNumOutputChannels = getTotalNumOutputChannels();

    // Clear any output channels that don't contain input data.
    for (auto i = totalNumInputChannels; i < totalNumOutputChannels; ++i) {
        buffer.clear(i, 0, buffer.getNumSamples());
    }

    update();

    const float *in1 = buffer.getReadPointer(0);
    const float *in2 = buffer.getReadPointer(1);
    float *out1 = buffer.getWritePointer(0);
    float *out2 = buffer.getWritePointer(1);

    // Make local copies for moar speeed!
    const float linNeg = _linNeg;
    const float linPos = _linPos;
    const float clip = _clip;
    const float mode = _mode;
    const float fi = _fi, fo = _fo;
    const float g1 = _g1, g2 = _g2, g3 = _g3;
    const int sampleInterval = _sampleInterval;

    int sampleIndex = _sampleIndex;
    float accum = _accum;
    float x = _currentSample;
    float b1 = _buf1, b2 = _buf2, b3 = _buf3, b4 = _buf4,
    b6 = _buf6, b7 = _buf7, b8 = _buf8, b9 = _buf9;

    for (int i = 0; i < buffer.getNumSamples(); ++i) {
        /*
          Order of the FX:

          1. Convert stereo to mono
          2. Skip samples depending on the sampling interval
          3. Quantize
          4. Apply non-linearity x^lin
          5. Headroom clipping
          6. Apply output gain
          7. Filter
         */

        // Combine the stereo channels into a single mono value.
        // In regular sample mode (mode = 0.0), accum holds only the most
        // recent sample value. If sample-and-hold is active (mode = 1.0), we
        // add the new value to accum so that it holds the sum of the previous
        // samples in this sampling interval. Multiplying by mode (0 or 1) is
        // simpler and possibly faster than using an if-statement.
        accum = in1[i] + in2[i] + mode * accum;

        // Time to take a new sample?
        // The original code used == to check if the sample interval is over,
        // but sampleInterval can change when the user adjusts the parameters.
        // Now the current index may end up being larger than the interval, so
        // we use >= to do the check.
        if (sampleIndex >= sampleInterval) {

            // Quantize. In sample-and-hold mode, this also takes the mean over
            // the sampling interval (in g1). It also divides by 2 (in g2) to
            // compensate for the stereo -> mono conversion which made the signal
            // twice as loud.
            x = float(g2 * int(accum * g1));

            // Apply the non-linearity, followed by headroom clipping.
            if (x > 0.0f) {
                x = std::pow(x, linPos);
                if (x > clip) x = clip;
            } else {
                x = -std::pow(-x, linNeg);
                if (x < -clip) x = -clip;
            }

            // Reset the accumulator.
            accum = 0.0f;
            sampleIndex = 0;
        }
        sampleIndex += 1;

        // Apply output gain (g3) and then filter.
        // The filter is eight identical first-order feedback filters in a row.
        // Each filter does: y(n) = fi * x(n) + fo * y(n - 1) where fi = 1 - fo.
        // In the implementation below only the first filter (b1) and the fifth
        // filter (b6) actually apply the fi coefficient, but the coefficient
        // they're using is (1 - fo)^4. This saves some multiplications but is
        // otherwise equivalent. You could also do (1 - fo)^8 and only apply it
        // to b1, not b6, but that can get numerically unstable when fo is large.
        b1 = fi * (x * g3) + fo * b1;
        b2 =       b1      + fo * b2;
        b3 =       b2      + fo * b3;
        b4 =       b3      + fo * b4;
        b6 = fi *  b4      + fo * b6;
        b7 =       b6      + fo * b7;
        b8 =       b7      + fo * b8;
        b9 =       b8      + fo * b9;

        // If you're wondering what happened to b5, that's the name the original
        // code used for x, but I found this confusing. b9 is the final output of
        // the filter stage. The other b1-b8 are the intermediate output values
        // for the individual filters and also their delay units (z^-1).

        out1[i] = b9;
        out2[i] = b9;
    }

    // Reset the state if we have numeric underflow in the output.
    if (std::abs(b1) < 1.0e-10f) {
        _buf1 = 0.0f; _buf2 = 0.0f; _buf3 = 0.0f; _buf4 = 0.0f;
        _buf6 = 0.0f; _buf7 = 0.0f; _buf8 = 0.0f; _buf9 = 0.0f;
        _accum = 0.0f;
        _currentSample = 0.0f;
    } else {
        // Copy the local variables back into the member variables, so we can
        // resume from where we left off in the next call to processBlock.
        _buf1 = b1; _buf2 = b2; _buf3 = b3; _buf4 = b4;
        _buf6 = b6; _buf7 = b7; _buf8 = b8; _buf9 = b9;

        // We also keep track of x and accum in between calls to processBlock,
        // in case the sampleInterval is greater than 1.
        _accum = accum;
        _currentSample = x;
    }
    _sampleIndex = sampleIndex;
}
mm_dsp::AudioProcessorValueTreeState::ParameterLayout MDADegradeAudioProcessor::createParameterLayout()
{
    mm_dsp::AudioProcessorValueTreeState::ParameterLayout layout;

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Headroom", 1),
        "Headroom",
        mm_dsp::NormalisableRange<float>(-30.0f, 0.0f, 0.01f),
        -6.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterInt>(
        mm_dsp::ParameterID("Quantize", 1),
        "Quantize",
        4, 16, 10));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Sample Rate", 1),
        "Sample Rate",
        mm_dsp::NormalisableRange<float>(1.0f, 10.0f, 1.0f),
        3.0f));

    // Note: In the original plug-in, the sample rate parameter was shown as
    // getSampleRate() / sampleInterval. For an interval of 1, it would show
    // 44100; for an interval of 2, it showed 22050; and so on, depending on
    // the project's actual sample rate. We simply display the sample interval
    // but a custom UI could display the effective sample rate.

    layout.add(std::make_unique<mm_dsp::AudioParameterChoice>(
        mm_dsp::ParameterID("Sample Mode", 1),
        "Sample Mode",
        mm_dsp::StringArray({ "Sample", "Hold" }),
        0));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("PostFilter", 1),
        "PostFilter",
        mm_dsp::NormalisableRange<float>(200.0f, 20000.0f, 1.0f, 0.5f),
        10000.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Non-Linearity", 1),
        "Non-Linearity",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f, 0.01f),
        0.58f));

    layout.add(std::make_unique<mm_dsp::AudioParameterChoice>(
        mm_dsp::ParameterID("Non-Lin Mode", 1),
        "Non-Lin Mode",
        mm_dsp::StringArray({ "Odd", "Even" }),
        0));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Output", 1),
        "Output",
        mm_dsp::NormalisableRange<float>(-20.0f, 20.0f, 0.01f),
        0.0f));

    return layout;
}

class MDADelayAudioProcessor:public MdaBase { public:
MDADelayAudioProcessor(){apvts.params=std::move(createParameterLayout().params);}
void prepareToPlay(double,int) override;void processBlock(mm_dsp::AudioBuffer<float>&,mm_dsp::MidiBuffer&) override;void reset();void releaseResources();
private:

    mm_dsp::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();

    void update();
    void resetState();

    // Maps the position of the right delay slider to a percentage of the left
    // channel delay time. Moving the slider to the left gives you a variable
    // ratio between 0% and 200%. Moving the slider to the right chooses from a
    // set of fixed ratios. At the center position, the ratio is 200%.
    static float rightDelayRatio(float param);

    // Maximum delay time in milliseconds. Feel free to make this smaller or
    // larger. The original plug-in used a fixed number of samples, but that
    // would make the maximum delay time different if the sample rate changed.
    static constexpr float _delayMaxMsec = 500.0f;

    // Maximum length of the delay buffer in samples.
    int _delayMax;

    // This buffer stores the delayed samples. There is only one delay buffer,
    // which means any echos from the delayed signal are actually mono.
    std::vector<float> _delayBuffer;

    // Delay time in samples for the left & right channels.
    int _ldel, _rdel;

    // Write position in the delay buffer. This is where we will write the next
    // new sample value.
    int _pos;

    // Wet & dry mix.
    float _wet, _dry;

    // Amount of echo feedback.
    float _feedback;

    // Low & high mix for the crossover filter.
    float _lmix, _hmix;

    // Low-pass filter coefficient.
    float _filt;

    // Delay unit for the low-pass filter.
    float _filt0;

    
};

void MDADelayAudioProcessor::prepareToPlay(double sampleRate, int samplesPerBlock)
{
    // Calculate how many samples we need for the delay buffer. This depends
    // on the sample rate and the maximum allowed delay time: the larger the
    // sample rate, the larger the buffer must be.
    _delayMax = int(std::ceil(float(sampleRate) * _delayMaxMsec / 1000.0f));
    _delayBuffer.resize(_delayMax);

    resetState();
}
void MDADelayAudioProcessor::releaseResources()
{
}
void MDADelayAudioProcessor::reset()
{
    resetState();
}
void MDADelayAudioProcessor::resetState()
{
    _pos = 0;
    _filt0 = 0.0f;

    // Clear out the delay buffer.
    memset(_delayBuffer.data(), 0, _delayMax * sizeof(float));
}
void MDADelayAudioProcessor::update()
{
    const float samplesPerMsec = float(getSampleRate()) / 1000.0f;

    // In the original plug-in, the parameter for the left channel delay length
    // went from 0 to 1. To compute the number of samples of delay, it used the
    // formula: ldel = int(delayMax * ldelParam * ldelParam).
    // The reason the parameter got squared, is that this makes it easier to
    // pick smaller delays. For example, at a sample rate of 44100, the maximum
    // delay length is 22050 samples. With the parameter set to 0.5, the delay
    // is not 11025 (= half) but 5512 samples (= half squared). In JUCE, we can
    // simply have the parameter be in milliseconds and give the slider a skew.
    float ldelParam = apvts.getRawParameterValue("L Delay")->load();
    _ldel = int(ldelParam * samplesPerMsec);

    // Make the minimum delay time 4 samples, not 0. A delay time of 0 would be
    // equal to the maximum delay because of wrap-around, so that's not very
    // useful. Although I'm not sure why the minimum delay is 4 samples, not 1.
    // Notice that really short delays introduce a filtering effect.
    if (_ldel < 4) _ldel = 4;

    // This shouldn't happen, but a bit of defensive programming never hurts.
    if (_ldel > _delayMax) _ldel = _delayMax;

    // The right channel delay is a percentage of the left channel delay length.
    float rdelParam = apvts.getRawParameterValue("R Delay")->load();
    _rdel = int(ldelParam * samplesPerMsec * rightDelayRatio(rdelParam));

    // Make sure the delay time does not become too large or too small.
    if (_rdel > _delayMax) _rdel = _delayMax;
    if (_rdel < 4) _rdel = 4;

    // The "tone" control goes from -100 to +100. Moving to the left means
    // low-pass filtering and to the right means high-pass filtering. When the
    // tone control is centered, there is no filtering.
    float toneParam = apvts.getRawParameterValue("Fb Tone")->load();
    _filt = toneParam / 200.0f + 0.5f;

    // Set the crossover frequency & high/low mix. Here, lmix determines how much
    // the low-pass filtered sample is combined with the unfiltered sample, whose
    // proportion is given by hmix.
    if (_filt > 0.5f) {                 // high-pass:
        _filt = 0.5f * _filt - 0.25f;   // filt now goes 0 to 0.25
        _lmix = -2.0f * _filt;          // lmix goes from 0 to -0.5
        _hmix = 1.0f;
    } else {                            // low-pass:
        _hmix = 2.0f * _filt;           // hmix goes from 0 to 1
        _lmix = 1.0f - _hmix;           // lmix goes from 1 to 0
    }

    // At this point, _filt is a value between 0 and 0.5 (low-pass) or 0.25
    // (high-pass). Turn this value into a cutoff frequency. On the left of
    // the slider, the frequency goes from 158 Hz - 28 kHz. On the right, it
    // goes from 158 Hz - 2113 Hz.
    float hz = std::pow(10.0f, 2.2f + 4.5f * _filt);

    // The filter itself is a one-pole filter: y(n) = (1 - f)*x(n) + f*y(n - 1).
    // Calculate the coefficient f using the formula exp(-2pi * hz / sampleRate).
    _filt = std::exp(-6.2831853f * hz / float(getSampleRate()));

    // Feedback: value between 0 and 0.49. If this is 0, the delay repeats only
    // once. For higher values, the delay will keeping echoing.
    float feedbackParam = apvts.getRawParameterValue("Feedback")->load() / 100.0f;
    _feedback = 0.495f * feedbackParam;

    // Output gain is in decibels, so convert to a linear value.
    float gain = apvts.getRawParameterValue("Output")->load();
    gain = mm_dsp::Decibels::decibelsToGain(gain);

    // For the wet/dry mix, don't do a linear interpolation but use a curve:
    // dry = (1 - x^2) and wet = (1 - (1 - x)^2). Each curve is approx -3 dB at
    // mix = 0.5, but added together they increase the gain by +3 dB at 50% mix
    // (i.e. it's not an equal power curve).
    float mix = apvts.getRawParameterValue("FX Mix")->load() / 100.0f;
    _wet = gain * (1.0f - (1.0f - mix) * (1.0f - mix)) * 0.5f;
    _dry = gain * (1.0f - mix * mix);

    // Note: the original plug-in had an additional factor 2.0 in the formula
    // for dry, which seems wrong. That would boost the signal by 6 dB if the
    // mix is all dry (i.e. mix = 0), so I removed it.
    // However, during processing we add the left and right signals to create a
    // mono signal, which boosts the level by 6 dB if the mix is all wet (i.e.
    // when mix = 1).
    // To compensate for this, I'm lowering the wet level by -6 dB; that's what
    // the * 0.5 is for. This is probably why dry originally had that extra
    // factor 2. Now the volume of all dry vs. all wet is the same again but at
    // 0 dB instead of +6 dB.
}
void MDADelayAudioProcessor::processBlock(mm_dsp::AudioBuffer<float> &buffer, mm_dsp::MidiBuffer &midiMessages)
{
    mm_dsp::ScopedNoDenormals noDenormals;
    auto totalNumInputChannels = getTotalNumInputChannels();
    auto totalNumOutputChannels = getTotalNumOutputChannels();

    // Clear any output channels that don't contain input data.
    for (auto i = totalNumInputChannels; i < totalNumOutputChannels; ++i) {
        buffer.clear(i, 0, buffer.getNumSamples());
    }

    update();

    const float *in1 = buffer.getReadPointer(0);
    const float *in2 = buffer.getReadPointer(1);
    float *out1 = buffer.getWritePointer(0);
    float *out2 = buffer.getWritePointer(1);

    const float wet = _wet;
    const float dry = _dry;
    const float fb = _feedback;
    const float f = _filt;
    const float lmix = _lmix;
    const float hmix = _hmix;
    const int s = _delayMax;

    float f0 = _filt0;

    // This keeps track of where we will write new values in the delay buffer.
    int p = _pos;

    // Get the read positions for the left and right channels. These need to
    // wrap around when they reach the end of the buffer, which is why we use
    // the modulo operator.
    int l = (p + _ldel) % s;
    int r = (p + _rdel) % s;

    // Note: This logic cannot tell apart a delay of 0 from a delay of _delayMax
    // samples. Due to the modulo operation these point to the same index in the
    // delay buffer. So a delay of 0 samples is not possible.
    // The original plug-in didn't so this, and I have not implemented it either,
    // but we could allow the user to set the delay time to 0, and then simply
    // bypass all of the delay logic in the loop.

    for (int i = 0; i < buffer.getNumSamples(); ++i) {
        float a = in1[i];
        float b = in2[i];

        // Read from the delay buffer.
        float dl = _delayBuffer[l];
        float dr = _delayBuffer[r];

        // Combine the left and right input samples into a mono signal.
        // Also add the delayed values but attenuated by the feedback factor.
        // The larger the feedback, the longer the sound will keep echoing.
        float tmp = wet * (a + b) + fb * (dl + dr);

        // Apply the low-pass filter. As seen in the other MDA plug-ins, this
        // is a simple exponentially weighted moving average filter (EWMA).
        f0 = f * (f0 - tmp) + tmp;

        // Do the crossover mix and write the new value back into the delay line.
        //
        // How this mix works: lmix determines how important the low-pass
        // filtered value is, while hmix says how important the unfiltered
        // sample value is.
        //
        // When the tone control is on the left, hmix is (1 - lmix). The further
        // to the left, the larger lmix and the smaller hmix, meaning that the
        // low-pass filtered sample value is more now important. The closer the
        // control is to the center, the larger hmix and the less important the
        // low-pass filtered value becomes.
        //
        // When the tone control is on the right, hmix is 1 but lmix is negative,
        // which means the low-pass filtered value is subtracted from the original,
        // turning this into a high-pass filter. The more to the right the control
        // is, the more this happens.
        //
        // The high-pass mode is more subtle than the low-pass mode since the
        // maximum cutoff frequency is lower. It's mostly useful for getting rid
        // of a muddy low end.
        //
        // Note that the filtering only applies to the delayed signal, not to the
        // original (dry) signal.
        _delayBuffer[p] = lmix * f0 + hmix * tmp;

        // Update the locations where we will write into and read from the delay
        // buffer. Since l = p + ldel and r = p + rdel, it means older / delayed
        // values are to the right in the buffer (higher indices) and newer values
        // are to the left (lower indices). Therefore, we must decrement and wrap
        // around when we have reached the end (well, beginning) of the buffer.
        p--; if (p < 0) p = s - 1;
        l--; if (l < 0) l = s - 1;
        r--; if (r < 0) r = s - 1;

        // The output for this sample is the original sample mixed with the values
        // we read from the delay buffer. Note that the output is still stereo but
        // the delayed signal that gets mixed into the stream is always mono.
        out1[i] = dry * a + dl;
        out2[i] = dry * b + dr;
    }

    _pos = p;

    // Trap denormals
    if (std::abs(f0) < 1.0e-10f) _filt0 = 0.0f; else _filt0 = f0;
}
float MDADelayAudioProcessor::rightDelayRatio(float param) {
    switch (int(param * 17.9f)) {
        case 17: return 0.5000f;       // fixed left/right ratios
        case 16: return 0.6667f;
        case 15: return 0.7500f;
        case 14: return 0.8333f;
        case 13: return 1.0000f;
        case 12: return 1.2000f;
        case 11: return 1.3333f;
        case 10: return 1.5000f;
        case  9: return 2.0000f;
        default: return 4.0f * param;  // variable ratio (param < 0.5)
    }
}
mm_dsp::AudioProcessorValueTreeState::ParameterLayout MDADelayAudioProcessor::createParameterLayout()
{
    mm_dsp::AudioProcessorValueTreeState::ParameterLayout layout;

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("L Delay", 1),
        "L Delay",
        mm_dsp::NormalisableRange<float>(0.1f, _delayMaxMsec, 0.01f, 0.4f),
        250.0f));

    // The right channel delay time is expressed as a percentage of the left
    // channel delay time, allowing you to make the delay appear earlier or later
    // on the right than on the left. However, the parameter itself is a little
    // tricky. The parameter value goes between 0 and 1. Moving the slider to the
    // left (parameter between 0 and 0.5) makes the right channel delay between
    // 0% and 200% of the left channel delay. Moving the slider to the right
    // (parameter between 0.5 and 1) will choose from a set of fixed left:right
    // ratios. To make this clearer to the user, we don't display the parameter
    // value but the actual percentage that has been chosen.

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("R Delay", 1),
        "R Delay",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.27f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Feedback", 1),
        "Feedback",
        mm_dsp::NormalisableRange<float>(0.0f, 99.0f, 0.01f),
        70.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Fb Tone", 1),
        "Fb Tone",
        mm_dsp::NormalisableRange<float>(-100.0f, 100.0f, 0.01f),
        0.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("FX Mix", 1),
        "FX Mix",
        mm_dsp::NormalisableRange<float>(0.0f, 100.0f, 0.01f),
        33.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Output", 1),
        "Output",
        mm_dsp::NormalisableRange<float>(-30.0f, 6.0f, 0.01f),
        0.0f));

    return layout;
}

class MDADetuneAudioProcessor:public MdaBase { public:
MDADetuneAudioProcessor(){apvts.params=std::move(createParameterLayout().params);}
void prepareToPlay(double,int) override;void processBlock(mm_dsp::AudioBuffer<float>&,mm_dsp::MidiBuffer&) override;void reset();void releaseResources();
private:

    mm_dsp::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();

    void update();
    void resetState();

    static constexpr int BUFMAX = 4096;

    float buf[BUFMAX];  // circular buffer for delay line (mono)
    float win[BUFMAX];  // crossfade window

    float sampleRate;
    int buflen;         // delay length

    int pos0;           // write pointer in the circular buffer
    float pos1, dpos1;  // read pointer and step size for left channel
    float pos2, dpos2;  // and for right channel

    float wet, dry;     // output levels

    
};

void MDADetuneAudioProcessor::prepareToPlay(double newSampleRate, int samplesPerBlock)
{
    sampleRate = float(newSampleRate);
    resetState();
}
void MDADetuneAudioProcessor::releaseResources()
{
}
void MDADetuneAudioProcessor::reset()
{
    resetState();
}
void MDADetuneAudioProcessor::resetState()
{
    std::memset(buf, 0, sizeof(buf));
    std::memset(win, 0, sizeof(win));
    pos0 = 0;
    pos1 = pos2 = 0.0f;
}
void MDADetuneAudioProcessor::update()
{
    // Number of semitones expressed as a value between 0 and 300 cents.
    // This is skewed using a x^3 curve, putting ~38 cents at the middle
    // of the slider and 300 cents at the rightmost position.
    float param0 = apvts.getRawParameterValue("Detune")->load();
    float semi = 3.0f * param0 * param0 * param0;

    // 1.0594631^semi is the same as 2^(semi/12) and gives the step size
    // used to pitch the sound down by this number of semitones.
    dpos2 = std::pow(1.0594631f, semi);

    // This is the same as 2^(-semi/12) and is the delta used to pitch up
    // the sound by the given number of semitones.
    dpos1 = 1.0f / dpos2;

    // Output gain is -20 to +20 dB. Convert decibels to linear.
    float param2 = apvts.getRawParameterValue("Output")->load();
    float gain = mm_dsp::Decibels::decibelsToGain(param2);

    // Dry/wet curve of (1 - x^2) for dry and (2x - x^2) for wet, with the
    // output gain amount already multiplied into it.
    float param1 = apvts.getRawParameterValue("Mix")->load();
    dry = gain - gain * param1 * param1;
    wet = (gain + gain - gain * param1) * param1;

    // The latency parameter determines the length of the delay line.
    // Since this parameter is a value between 0.0f and 1.0f, the expression
    // (8 + int(4.9f * param)) produces a discrete set of values: 8, 9, 10, 11,
    // and 12. The bit shift converts this into buffer lengths of respectively
    // 256, 512, 1024, 2048, and 4096 samples. Note that `buflen` should always
    // be a power of two.
    float param3 = apvts.getRawParameterValue("Latency")->load();
    int tmp = 1 << (8 + int(4.9f * param3));

    // Recalculate the crossfade window (hanning half-overlap-and-add).
    if (tmp != buflen) {
        buflen = tmp;
        if (buflen > BUFMAX) { buflen = BUFMAX; }

        double phase = 0.0;
        double step = 6.28318530718 / buflen;
        for (int i = 0; i < buflen; i++) {
            win[i] = float(0.5 - 0.5 * std::cos(phase));
            phase += step;
        }
    }
}
void MDADetuneAudioProcessor::processBlock(mm_dsp::AudioBuffer<float> &buffer, mm_dsp::MidiBuffer &midiMessages)
{
    mm_dsp::ScopedNoDenormals noDenormals;
    auto totalNumInputChannels = getTotalNumInputChannels();
    auto totalNumOutputChannels = getTotalNumOutputChannels();

    // Clear any output channels that don't contain input data.
    for (auto i = totalNumInputChannels; i < totalNumOutputChannels; ++i) {
        buffer.clear(i, 0, buffer.getNumSamples());
    }

    update();

    const float *in1 = buffer.getReadPointer(0);
    const float *in2 = buffer.getReadPointer(1);
    float *out1 = buffer.getWritePointer(0);
    float *out2 = buffer.getWritePointer(1);

    /*
        How this works:

        If you have a delay line where you keep steadily increasing the delay
        length on every timestep, this will pitch down the sound because it
        slows down the reading of the waveform. Conversely, if you decrease
        the delay length by the same amount on every timestep, the sound gets
        pitched up (it skips parts of the waveform).

        There is a practical problem with this approach: It's not possible to
        have a delay length smaller than zero and so we can't keep decreasing
        it forever. Likewise, we can't keep increasing the delay length either
        as that requires an infinitely long delay line.

        A compromise is to use a short delay length and simply reset the read
        pointer when it catches up with the write head. The Latency parameter
        determines the length of the delay, with a minimum of 256 samples and
        a maximum of 4096 samples. (A shorter delay has less latency but sounds
        worse.)

        However, resetting the read pointer will produce a nasty discontinuity.
        The solution is to read from two places at once and crossfade between
        them. The crossfading is done using a Hann window, which tapers off at
        the edges, so that the discontinuity is suppressed.
     */

    // For wrapping around the write pointer in the delay line.
    // This only works correctly if `buflen` is a power of two!
    const int mask = buflen - 1;

    // For wrapping around the read pointers, which are floats.
    const float wrapAround = float(buflen);

    // We'll read the second sample half the delay length ahead.
    const int halfLength = buflen >> 1;

    for (int i = 0; i < buffer.getNumSamples(); ++i) {
        // Read the input samples.
        float a = in1[i];
        float b = in2[i];

        // Put the dry signal into the output variables already.
        float c = dry * a;
        float d = dry * b;

        // Update the write position. For some reason this plug-in counts
        // backwards, but that shouldn't matter. The wrap-around is handled
        // by bitwise-and with the mask.
        --pos0 &= mask;

        // Write the input as a mono signal into the delay line. This already
        // applies the wet gain, so we don't have to do this later.
        buf[pos0] = wet * (a + b);

        // Update the read position for the left channel, wrapping around
        // if necessary. Note that this is a float because `dpos1` is the
        // speed at which to step through the delay line, which will be a
        // fractional number of samples. (In fact, dpos1 will be less than
        // 1.0 so that this read index moves slower than the write index,
        // which results in the sound being pitched down.)
        pos1 -= dpos1;
        if (pos1 < 0.0f) { pos1 += wrapAround; }

        // Read the delay line using linear interpolation. This involves
        // taking a weighted average between the sample ahead and behind.
        int p1i = int(pos1);            // integer position
        float p1f = pos1 - float(p1i);  // fraction
        float u = buf[p1i];             // read first sample
        ++p1i &= mask;                  // move to next sample, maybe wrap
        u += p1f * (buf[p1i] - u);      // read next sample and blend

        // Also read the sample half the delay length away (i.e. offset by
        // 180 degrees). This also uses linear interpolation, same fraction.
        int p2i = (p1i + halfLength) & mask;
        float v = buf[p2i];
        ++p2i &= mask;
        v += p1f * (buf[p2i] - v);

        // Crossfade between the two interpolated samples, and add to the
        // left channel output. The `u` sample is multiplied by the window,
        // and the `v` sample by (1 - window), so this is another linear
        // interpolation. The window index is chosen so that the closer the
        // read pointer gets to the write pointer, the less the `u` sample
        // and the more `v` sample will contribute, and vice versa.
        int xi = (p1i - pos0) & mask;
        c += v + win[xi] * (u - v);

        // Apply the same logic to the right channel. This will pitch up the
        // sound because `dpos2` is larger than 1.0, so that this read index
        // will move faster than the write index and shorten the waveform.
        pos2 -= dpos2;
        if (pos2 < 0.0f) { pos2 += wrapAround; }

        // First sample.
        p1i = int(pos2);
        p1f = pos2 - float(p1i);
        u = buf[p1i];
        ++p1i &= mask;
        u += p1f * (buf[p1i] - u);

        // Second sample offset by half delay length.
        p2i = (p1i + halfLength) & mask;
        v = buf[p2i];
        ++p2i &= mask;
        v += p1f * (buf[p2i] - v);

        // Crossfade.
        p2i = (p1i - pos0) & mask;
        d += v + win[p2i] * (u - v);

        // Write the output samples.
        out1[i] = c;
        out2[i] = d;
    }
}
mm_dsp::AudioProcessorValueTreeState::ParameterLayout MDADetuneAudioProcessor::createParameterLayout()
{
    mm_dsp::AudioProcessorValueTreeState::ParameterLayout layout;

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Detune", 1),
        "Detune",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.2f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Mix", 1),
        "Mix",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.9f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Output", 1),
        "Output",
        mm_dsp::NormalisableRange<float>(-20.0f, 20.0f, 0.1f),
        0.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Latency", 1),
        "Latency",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.5f));

    return layout;
}

class MDADitherAudioProcessor:public MdaBase { public:
MDADitherAudioProcessor(){apvts.params=std::move(createParameterLayout().params);}
void prepareToPlay(double,int) override;void processBlock(mm_dsp::AudioBuffer<float>&,mm_dsp::MidiBuffer&) override;void reset();void releaseResources();
private:

    mm_dsp::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();

    void update();
    void resetState();

    // Dither level.
    float dith;

    // Previous random value (used in HP-TRI and N-SHAPE modes).
    int rnd1, rnd3;

    // Noise shaping buffers.
    float shap, sh1, sh2, sh3, sh4;

    // DC offset for fine-tuning.
    float offs;

    // Output word length in bits (8 - 24).
    float bits;

    // Word length. This is 2^(bits - 1) so 16 bits has wlen = 32768.
    float wlen;

    // Inverse word length, 1/wlen.
    float iwlen;

    // Input signal gain level for zoom mode.
    float gain;

    // Dither mode. 0 = TRI, 1 = HP-TRI or N-SHAPE.
    int mode = 0;

    
};

void MDADitherAudioProcessor::prepareToPlay(double newSampleRate, int samplesPerBlock)
{
    resetState();
}
void MDADitherAudioProcessor::releaseResources()
{
}
void MDADitherAudioProcessor::reset()
{
    resetState();
}
void MDADitherAudioProcessor::resetState()
{
    sh1 = sh2 = sh3 = sh4 = 0.0f;
    rnd1 = rnd3 = 0;
}
void MDADitherAudioProcessor::update()
{
    // This is a value between 8 and 24, rounded to a whole number.
    float param0 = apvts.getRawParameterValue("Word Len")->load();
    bits = 8.0f + 2.0f * std::floor(8.9f * param0);

    // If zoom mode is enabled, the number of bits is set to 6 and the input
    // audio signal is gained down so that the dither noise is clearly audible
    // (around -36 dB).
    gain = 1.0f;
    float param4 = apvts.getRawParameterValue("Zoom...")->load();
    if (param4 > 0.1f) {
        wlen = 32.0f;
        gain = 1.0f - param4;
        gain *= gain;
    } else {
        wlen = std::pow(2.0f, bits - 1.0f);  // word length in quanta
    }

    /*
        If the signal has N bits, the noise floor is at -6N dB. For example,
        if bits = 8, the noise floor is at 10^(-6.02 * 8/20) = 0.00391, which
        is the same as 1/256.

        However, `wlen` is a factor 2 smaller, so if bits = 8, wlen is not
        256 but 128. Presumably this is done because TPDF dither is supposed
        to add 6 dB to the noise floor.

        At 8 bits, with a dither amplitude of 2 LSB or less, the dither noise
        level is at -42 dB. This is correct: the noise floor is at -48 dB, plus
        6 dB for the added bit.

        However, a dither amplitude of 1 LSB or less does not generate noise
        but silence. I think this is due to the truncation it performs, but I
        don't really feel like figuring out why.
    */

    // Using WaveLab 2.01 (unity gain) as a reference:
    // 16-bit output is (int)floor(floating_point_value*32768.0f)

    // The dither amplitude is a value between 0 and 4 LSB. Exactly how large
    // the LSB is depends on the "bits" parameter: more bits means the smallest
    // bit represents a smaller amplitude.
    // The reason for dividing by an additional factor 32767 is that the random
    // numbers used to create the dither noise will be between 0 and 32767.
    float param2 = apvts.getRawParameterValue("Dith Amp")->load();
    dith = 2.0f * param2 / (wlen * 32767.0f);

    // DC offset is a value between -2 and +2 LSB. This formula adds 0.5 to
    // round the dither instead of truncating.
    float param3 = apvts.getRawParameterValue("DC Trim")->load();
    offs = (4.0f * param3 - 1.5f) / wlen;

    // Dither mode
    mode = 1;
    shap = 0.0f;
    float param1 = apvts.getRawParameterValue("Dither")->load();
    switch (int(param1)) {
        case 0: dith = 0.0f; break;  // off
        case 1: mode = 0;    break;  // tri
        case 3: shap = 0.5f; break;  // noise shaping
        default: break;              // tri, hp-tri
    }

    iwlen = 1.0f / wlen;
}
void MDADitherAudioProcessor::processBlock(mm_dsp::AudioBuffer<float> &buffer, mm_dsp::MidiBuffer &midiMessages)
{
    mm_dsp::ScopedNoDenormals noDenormals;
    auto totalNumInputChannels = getTotalNumInputChannels();
    auto totalNumOutputChannels = getTotalNumOutputChannels();

    // Clear any output channels that don't contain input data.
    for (auto i = totalNumInputChannels; i < totalNumOutputChannels; ++i) {
        buffer.clear(i, 0, buffer.getNumSamples());
    }

    update();

    const float *in1 = buffer.getReadPointer(0);
    const float *in2 = buffer.getReadPointer(1);
    float *out1 = buffer.getWritePointer(0);
    float *out2 = buffer.getWritePointer(1);

    float s1 = sh1, s2 = sh2, s3 = sh3, s4 = sh4;  // shaping level, buffers
    int r1 = rnd1, r3 = rnd3;                      // random numbers for dither

    float aa, bb;

    for (int i = 0; i < buffer.getNumSamples(); ++i) {
        float a = in1[i];
        float b = in2[i];

        // Calculate the first random number. In HP-TRI and noise shaping
        // modes, this is the previous random number. In TRI mode, this is
        // a new random number.

        // HP-TRI dither (also used when noise shaping)
        int r2 = r1;
        int r4 = r3;

        // TRI dither
        if (mode == 0) {
            r4 = randomInt() & 0x7FFF;
            r2 = (r4 & 0x7F) << 8;
        }

        // Calculate the second random number.
        r1 = randomInt() & 0x7FFF;
        r3 = (r1 & 0x7F) << 8;

        /*
          Processing goes like this:

          1. Apply input gain. Normally this is 1.0, except in zoom mode where
             we turn down the input signal.
          2. Add a noise shaping filter.
          3. Add the DC-offset. This is a tiny value.
          4. Subtract the two random numbers (this is what creates the high
             pass in HP-TRI mode) and multiply by the dither level. The random
             values are 0 - 32767 but `dith` compensates for this, so that the
             added noise is also a tiny value.
          5. Apply the bitcrushing formula. This is done so that the signal
             won't have any amplitudes smaller than the "bits" parameter.
          6. Update the noise shaping filter state using the error between
             the original signal and the dithered signal.
        */

        a  = gain * a + shap * (s1 + s1 - s2);  // target level + error feedback
        aa = a + offs + dith * float(r1 - r2);  //              + offset + dither

        if (aa < 0.0f) { aa -= iwlen; }         // because we truncate towards zero!
        aa = iwlen * float(int(wlen * aa));     // truncate

        s2 = s1;
        s1 = a - aa;                            // error feedback: 2nd order noise shaping

        // Do the same for the right channel...
        b  = gain * b + shap * (s3 + s3 - s4);
        bb = b + offs + dith * float(r3 - r4);
        if (bb < 0.0f) { bb -= iwlen; }
        bb = iwlen * float(int(wlen * bb));
        s4 = s3;
        s3 = b - bb;

        out1[i] = aa;
        out2[i] = bb;
    }

    sh1 = s1; sh2 = s2; sh3 = s3; sh4 = s4;  // doesn't actually matter if these are
    rnd1 = r1; rnd3 = r3;                    // saved or not as effect is so small!
}
mm_dsp::AudioProcessorValueTreeState::ParameterLayout MDADitherAudioProcessor::createParameterLayout()
{
    mm_dsp::AudioProcessorValueTreeState::ParameterLayout layout;

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Word Len", 1),
        "Word Len",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.5f));

    layout.add(std::make_unique<mm_dsp::AudioParameterChoice>(
        mm_dsp::ParameterID("Dither", 1),
        "Dither",
        mm_dsp::StringArray { "OFF", "TRI", "HP-TRI", "N-SHAPE" },
        2));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Dith Amp", 1),
        "Dith Amp",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.5f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("DC Trim", 1),
        "DC Trim",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.5f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Zoom...", 1),
        "Zoom...",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.0f));

    return layout;
}

class MDADynamicsAudioProcessor:public MdaBase { public:
MDADynamicsAudioProcessor(){apvts.params=std::move(createParameterLayout().params);}
void prepareToPlay(double,int) override;void processBlock(mm_dsp::AudioBuffer<float>&,mm_dsp::MidiBuffer&) override;void reset();void releaseResources();
private:

    mm_dsp::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();

    void update();
    void resetState();

    float threshold;         // threshold for compressor
    float ratio;             // ratio for compressor
    float trim;              // makeup gain
    float attack;            // envelope attack coefficient
    float release;           // envelope release coefficient
    float limiterThreshold;  // limiter threshold
    float gateThreshold;     // gate threshold
    float gateAttack;        // gate attack coefficient
    float gateRelease;       // gate release coefficient
    float dry;               // dry/wet mix amount
    bool compressOnly;       // if false, also apply limiter & gate

    float env;               // current envelope level
    float limiterEnv;        // envelope used by the limiter
    float gateEnv;           // envelope used by the gate

    
};

void MDADynamicsAudioProcessor::prepareToPlay(double sampleRate, int samplesPerBlock)
{
    resetState();
}
void MDADynamicsAudioProcessor::releaseResources()
{
}
void MDADynamicsAudioProcessor::reset()
{
    resetState();
}
void MDADynamicsAudioProcessor::resetState()
{
    env = 0.0f;
    limiterEnv = 0.0f;
    gateEnv = 0.0f;
}
void MDADynamicsAudioProcessor::update()
{
    compressOnly = true;

    // Convert decibels to linear value
    float param1 = apvts.getRawParameterValue("Thresh")->load();
    threshold = std::pow(10.0f, param1 * 0.05f);

    // The different values that `ratio` can take on are:
    //    -0.3 - 0.0  = ratio of 0.5:1 to 0.99:1 (upwards expand)
    //     0.0 - 0.95 = ratio 1:1 to 20:1 (regular compression)
    //    0.95 - 1.04 = "Limit" (essentially infinite ratio)
    //    1.04 - 17.0 = ratio -1:1 to -17:1 (compress upside down?)
    float param2 = apvts.getRawParameterValue("Ratio")->load();
    ratio = 2.5f * param2 - 0.5f;
    if (ratio > 1.0f) {
        ratio = 1.0f + 16.0f*(ratio - 1.0f)*(ratio - 1.0f);
        compressOnly = false;
    }
    if (ratio < 0.0f) {
        ratio = 0.6f * ratio;
        compressOnly = false;
    }
    // When threshold is below -20 dB and we're upwards expanding,
    // make the ratio more extreme. (Not sure why this is done.)
    if (ratio < 0.0f && threshold < 0.1f) {
        ratio *= threshold * 15.0f;
    }

    // Convert decibels to linear value
    float param3 = apvts.getRawParameterValue("Output")->load();
    trim = std::pow(10.0f, param3 * 0.05f);

    // Filter coefficient for envelope attack
    float param4 = apvts.getRawParameterValue("Attack")->load();
    attack = std::pow(10.0f, -0.002f - 2.0f * param4);

    // Filter coefficient for envelope release
    float param5 = apvts.getRawParameterValue("Release")->load();
    release = 1.0f - std::pow(10.0f, -2.0f - 3.0f * param5);

    // Limiter threshold to linear gain
    float param6 = apvts.getRawParameterValue("Limiter")->load();
    if (param6 > 0.98f) {
        limiterThreshold = 1000.0f;  // limiter off
    } else {
        limiterThreshold = 0.99f * std::pow(10.0f, std::floor(30.0*param6 - 20.0)/20.f);
        compressOnly = false;
    }

    // Gate threshold to linear gain
    float param7 = apvts.getRawParameterValue("Gate Thr")->load();
    if (param7 < 0.02f) {
        gateThreshold = 0.0f;   // expander
    } else {
        gateThreshold = std::pow(10.0f, 3.0f * param7 - 3.0f);
        compressOnly = false;
    }

    // Filter coefficient for gate attack
    float param8 = apvts.getRawParameterValue("Gate Att")->load();
    gateAttack = std::pow(10.0f, -0.002f - 3.0f * param8);

    // Filter coefficient for envelope release
    float param9 = apvts.getRawParameterValue("Gate Rel")->load();
    gateRelease = 1.0f - std::pow(10.0f, -2.0f - 3.3f * param9);

    // Dry/wet mix
    float param10 = apvts.getRawParameterValue("Mix")->load() * 0.01f;
    dry = 1.0f - param10;
    trim *= param10;
}
void MDADynamicsAudioProcessor::processBlock(mm_dsp::AudioBuffer<float> &buffer, mm_dsp::MidiBuffer &midiMessages)
{
    mm_dsp::ScopedNoDenormals noDenormals;
    auto totalNumInputChannels = getTotalNumInputChannels();
    auto totalNumOutputChannels = getTotalNumOutputChannels();

    // Clear any output channels that don't contain input data.
    for (auto i = totalNumInputChannels; i < totalNumOutputChannels; ++i) {
        buffer.clear(i, 0, buffer.getNumSamples());
    }

    update();

    const float *in1 = buffer.getReadPointer(0);
    const float *in2 = buffer.getReadPointer(1);
    float *out1 = buffer.getWritePointer(0);
    float *out2 = buffer.getWritePointer(1);

    if (compressOnly) {
        for (int s = 0; s < buffer.getNumSamples(); ++s) {
            float a = in1[s];
            float b = in2[s];

            // Rectify the level so it's always a positive value.
            float i = (a < 0.0f) ? -a : a;
            float j = (b < 0.0f) ? -b : b;

            // Get peak level. There is only one envelope follower that works
            // on both channels, using whichever channel is loudest.
            i = (j > i) ? j : i;

            // Simple envelope follower.
            env = (i > env) ? env + attack * (i - env) : env * release;

            // Calculate the gain. If the envelope level is over the threshold,
            // the ratio kicks in to reduce the gain. `trim` is makeup gain.
            float g = (env > threshold) ? trim / (1.0f + ratio * (env/threshold - 1.0f)) : trim;

            // Apply the same gain to both channels and mix in the dry signal.
            out1[s] = a * (g + dry);
            out2[s] = b * (g + dry);
        }
    } else {
        for (int s = 0; s < buffer.getNumSamples(); ++s) {
            float a = in1[s];
            float b = in2[s];

            // Calculate the compressor's gain (same code as above).
            float i = (a < 0.0f) ? -a : a;
            float j = (b < 0.0f) ? -b : b;
            i = (j > i) ? j : i;
            env = (i > env) ? env + attack * (i - env) : env * release;
            float g = (env > threshold) ? trim / (1.0f + ratio * (env/threshold - 1.0f)) : trim;

            // Limiter envelope, this has no attack and uses the same
            // release time as the compressor.
            limiterEnv = (i > env) ? i : limiterEnv * release;

            // Limit the gain. This applies the limiter envelope to the
            // gain from the compressor.
            if (g < 0.0f) {
                g = 0.0f;
            }
            if (g * limiterEnv > limiterThreshold) {
                g = limiterThreshold / limiterEnv;
            }

            // Gate. When the current envelope level exceeds the threshold,
            // the gate envelope increases towards 1.0 using the attack rate.
            // When the current envelope level falls below the threshold,
            // the gate envelope decays towards 0.0 using the release rate.
            gateEnv = (env > gateThreshold) ? gateEnv + gateAttack * (1.0f - gateEnv) : gateEnv * gateRelease;

            // Apply the gated gain from the compressor and limiter.
            out1[s] = a * (g * gateEnv + dry);
            out2[s] = b * (g * gateEnv + dry);
        }
    }
}
mm_dsp::AudioProcessorValueTreeState::ParameterLayout MDADynamicsAudioProcessor::createParameterLayout()
{
    mm_dsp::AudioProcessorValueTreeState::ParameterLayout layout;

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Thresh", 1),
        "Thresh",
        mm_dsp::NormalisableRange<float>(-40.0f, 0.0f, 0.01f),
        -16.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Ratio", 1),
        "Ratio",
        mm_dsp::NormalisableRange<float>(),
        0.4f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Output", 1),
        "Output",
        mm_dsp::NormalisableRange<float>(0.0f, 40.0f, 0.01f),
        4.0f));

    // Not sure if the displayed attack and release times actually make sense!
    // With JUCE it's much simpler to define the actual range in milliseconds
    // and convert that into the filter coefficient, instead of the other way
    // around like it was done here. (Also, microseconds?!)

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Attack", 1),
        "Attack",
        mm_dsp::NormalisableRange<float>(),
        0.18f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Release", 1),
        "Release",
        mm_dsp::NormalisableRange<float>(),
        0.55f));

    // -20 to +10 dB
    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Limiter", 1),
        "Limiter",
        mm_dsp::NormalisableRange<float>(),
        1.0f));

    // -60 to 0 dB
    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Gate Thr", 1),
        "Gate Thr",
        mm_dsp::NormalisableRange<float>(),
        0.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Gate Att", 1),
        "Gate Att",
        mm_dsp::NormalisableRange<float>(),
        0.10f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Gate Rel", 1),
        "Gate Rel",
        mm_dsp::NormalisableRange<float>(),
        0.50f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Mix", 1),
        "Mix",
        mm_dsp::NormalisableRange<float>(0.0f, 100.0f, 0.01f),
        100.0f));

    return layout;
}

class MDAEnvelopeAudioProcessor:public MdaBase { public:
MDAEnvelopeAudioProcessor(){apvts.params=std::move(createParameterLayout().params);}
void prepareToPlay(double,int) override;void processBlock(mm_dsp::AudioBuffer<float>&,mm_dsp::MidiBuffer&) override;void reset();void releaseResources();
private:

    mm_dsp::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();

    void update();
    void resetState();

    int mode;           // 0, 1, 2
    float attack;       // attack coefficient
    float release;      // release coefficient
    float gain;         // output gain

    float env;          // current envelope level
    float releaseRate;  // release delta

    
};

void MDAEnvelopeAudioProcessor::prepareToPlay(double sampleRate, int samplesPerBlock)
{
    resetState();
}
void MDAEnvelopeAudioProcessor::releaseResources()
{
}
void MDAEnvelopeAudioProcessor::reset()
{
    resetState();
}
void MDAEnvelopeAudioProcessor::resetState()
{
    env = 0.0f;
    releaseRate = 0.0f;
}
void MDAEnvelopeAudioProcessor::update()
{
    mode = apvts.getRawParameterValue("Output")->load();

    // Convert attack time to filter coefficient.
    float fParam2 = apvts.getRawParameterValue("Attack")->load();
    attack = std::pow(10.0f, -0.002f - 4.0f * fParam2);

    // Convert release time to filter coefficient.
    float fParam3 = apvts.getRawParameterValue("Release")->load();
    release = 1.0f - std::pow(10.0f, -2.0f - 3.0f * fParam3);

    // Convert decibels to linear gain.
    float fParam4 = apvts.getRawParameterValue("Gain")->load();
    gain = std::pow(10.f, 0.05f * fParam4);

    // Constant gain across modes.
    if (mode == 0) {
        gain *= 2.0f;
    } else {
        gain *= 0.5f;
    }
}
void MDAEnvelopeAudioProcessor::processBlock(mm_dsp::AudioBuffer<float> &buffer, mm_dsp::MidiBuffer &midiMessages)
{
    mm_dsp::ScopedNoDenormals noDenormals;
    auto totalNumInputChannels = getTotalNumInputChannels();
    auto totalNumOutputChannels = getTotalNumOutputChannels();

    // Clear any output channels that don't contain input data.
    for (auto i = totalNumInputChannels; i < totalNumOutputChannels; ++i) {
        buffer.clear(i, 0, buffer.getNumSamples());
    }

    update();

    const float *in1 = buffer.getReadPointer(0);
    const float *in2 = buffer.getReadPointer(1);
    float *out1 = buffer.getWritePointer(0);
    float *out2 = buffer.getWritePointer(1);

    if (releaseRate > 1.0f - release) {  // can be unstable if release changed
        releaseRate = 1.0f - release;
    }

    bool flat = (mode == 2);  // flatten output audio?

    if (mode > 0) {  // envelope follower mode
        for (int i = 0; i < buffer.getNumSamples(); ++i) {
            // Combine both channels into a single signal (make mono).
            float b = in1[i] + in2[i];

            // Basic envelope follower: If the input signal exceeds the current
            // envelope value, then increase the envelope. The attack determines
            // how fast this happens, using a one-pole filter. Otherwise, if the
            // signal is below the current envelope value, decrease the envelope
            // level using the release coefficient.

            float x = (b < 0.0f) ? -b : b;     // rectify
            if (x > env) {
                env += attack * (x - env);     // attack
                releaseRate = 1.0f - release;  // reset rate
            } else {
                env *= release + releaseRate;  // release
                releaseRate *= 0.9999f;        // increase release rate
            }

            // Audio output is the combined signal.
            x = gain * b;

            // Flatten audio signal by dividing by envelope value.
            if (flat) {
                if (env < 0.01f) {  // don't divide by zero
                    x *= 100.0f;
                } else {
                    x /= env;
                }
            }

            out1[i] = x;           // audio on left channel
            out2[i] = 0.5f * env;  // envelope on right channel
        }
    } else {  // VCA mode
        for (int i = 0; i < buffer.getNumSamples(); ++i) {
            float a = in1[i];
            float b = in2[i];

            // Same envelope follower as above, but only on right channel.
            float x = (b < 0.0f) ? -b : b;
            if (x > env) {
                env += attack * (x - env);
                releaseRate = 1.0f - release;
            } else {
                env *= release + releaseRate;
                releaseRate *= 0.9999f;
            }

            // Apply the envelope to the audio from the left channel.
            x = gain * a * env;

            // Output the same audio signal on both channels.
            out1[i] = x;
            out2[i] = x;
        }
    }
}
mm_dsp::AudioProcessorValueTreeState::ParameterLayout MDAEnvelopeAudioProcessor::createParameterLayout()
{
    mm_dsp::AudioProcessorValueTreeState::ParameterLayout layout;

    layout.add(std::make_unique<mm_dsp::AudioParameterChoice>(
        mm_dsp::ParameterID("Output", 1),
        "Output",
        mm_dsp::StringArray { "L x |R|", "IN/ENV", "FLAT/ENV" },
        0));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Attack", 1),
        "Attack",
        mm_dsp::NormalisableRange<float>(),
        0.25f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Release", 1),
        "Release",
        mm_dsp::NormalisableRange<float>(),
        0.4f));

    // Comment from original code:
    // attack & release times not accurate - included to make user happy!

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Gain", 1),
        "Gain",
        mm_dsp::NormalisableRange<float>(-20.0f, 20.0f, 0.01f),
        0.0f));

    return layout;
}

class MDAImageAudioProcessor:public MdaBase { public:
MDAImageAudioProcessor(){apvts.params=std::move(createParameterLayout().params);}
void prepareToPlay(double,int) override;void processBlock(mm_dsp::AudioBuffer<float>&,mm_dsp::MidiBuffer&) override;void reset();void releaseResources();
private:

    mm_dsp::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();

    void update();
    void resetState();

    float l2l, l2r, r2l, r2r;

    
};

void MDAImageAudioProcessor::prepareToPlay(double sampleRate, int samplesPerBlock)
{
    resetState();
}
void MDAImageAudioProcessor::releaseResources()
{
}
void MDAImageAudioProcessor::reset()
{
    resetState();
}
void MDAImageAudioProcessor::resetState()
{
    l2l = 1.0f;
    r2r = 1.0f;
    l2r = 0.0f;
    r2l = 0.0f;
}
void MDAImageAudioProcessor::update()
{
    float w = apvts.getRawParameterValue("S Width")->load() * 0.01f;       // -2 .. 2
    float k = apvts.getRawParameterValue("S Pan")->load() * 0.01f + 1.0f;  //  0 .. 2
    float c = apvts.getRawParameterValue("M Level")->load() * 0.01f;       // -2 .. 2
    float b = apvts.getRawParameterValue("M Pan")->load() * 0.01f + 1.0f;  //  0 .. 2

    // Convert decibels to linear gain.
    float g = std::pow(10.0, 0.05f * apvts.getRawParameterValue("Output")->load());

    /*
        Calculate gain coefficients, where:

        l2l = gain for left input going into left output
        r2l = gain for right input going into left output
        r2r = gain for right input going into right output
        l2r = gain for left input going into right output

        ┌────────────┐       l2l                         ┌────────────┐
        │            ├──────────────────────────────────▶│            │
        │    L In    ├─────────────────────┐             │   L Out    │
        │            │       l2r        ┌──┼────────────▶│            │
        └────────────┘                  │  │             └────────────┘
                                        │  │
        ┌────────────┐                  │  │             ┌────────────┐
        │            │       r2l        │  └────────────▶│            │
        │    R In    ├──────────────────┘                │   R Out    │
        │            ├──────────────────────────────────▶│            │
        └────────────┘       r2r                         └────────────┘
    */

    int mode = int(apvts.getRawParameterValue("Mode")->load());
    switch (mode) {
        case 0:  // SM->LR
            r2l =  g * c * (2.0f - b);
            l2l =  g * w * (2.0f - k);
            r2r =  g * c * b;
            l2r = -g * w * k;
            break;

        case 1:  // MS->LR
            l2l =  g * c * (2.0f - b);
            r2l =  g * w * (2.0f - k);
            l2r =  g * c * b;
            r2r = -g * w * k;
            break;

        case 2:  // LR->LR
            g *= 0.5f;
            l2l = g * (c * (2.0f - b) + w * (2.0f - k));
            r2l = g * (c * (2.0f - b) - w * (2.0f - k));
            l2r = g * (c * b - w * k);
            r2r = g * (c * b + w * k);
            break;

        case 3:  // LR->MS
            g *= 0.5f;
            l2l =  g * (2.0f - b) * (2.0f - k);
            r2l =  g * (2.0f - b) * k;
            l2r = -g * b * (2.0f - k);
            r2r =  g * b * k;
            break;
    }
}
void MDAImageAudioProcessor::processBlock(mm_dsp::AudioBuffer<float> &buffer, mm_dsp::MidiBuffer &midiMessages)
{
    mm_dsp::ScopedNoDenormals noDenormals;
    auto totalNumInputChannels = getTotalNumInputChannels();
    auto totalNumOutputChannels = getTotalNumOutputChannels();

    // Clear any output channels that don't contain input data.
    for (auto i = totalNumInputChannels; i < totalNumOutputChannels; ++i) {
        buffer.clear(i, 0, buffer.getNumSamples());
    }

    update();

    const float *in1 = buffer.getReadPointer(0);
    const float *in2 = buffer.getReadPointer(1);
    float *out1 = buffer.getWritePointer(0);
    float *out2 = buffer.getWritePointer(1);

    for (int i = 0; i < buffer.getNumSamples(); ++i) {
        float a = in1[i];
        float b = in2[i];

        float c = l2l * a + r2l * b;
        float d = r2r * b + l2r * a;

        out1[i] = c;
        out2[i] = d;
    }
}
mm_dsp::AudioProcessorValueTreeState::ParameterLayout MDAImageAudioProcessor::createParameterLayout()
{
    mm_dsp::AudioProcessorValueTreeState::ParameterLayout layout;

    layout.add(std::make_unique<mm_dsp::AudioParameterChoice>(
        mm_dsp::ParameterID("Mode", 1),
        "Mode",
        mm_dsp::StringArray { "SM->LR", "MS->LR", "LR->LR", "LR->MS" },
        2));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("S Width", 1),
        "S Width",
        mm_dsp::NormalisableRange<float>(-200.0f, 200.0f, 0.01f),
        100.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("S Pan", 1),
        "S Pan",
        mm_dsp::NormalisableRange<float>(-100.0f, 100.0f, 0.01f),
        0.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("M Level", 1),
        "M Level",
        mm_dsp::NormalisableRange<float>(-200.0f, 200.0f, 0.01f),
        100.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("M Pan", 1),
        "M Pan",
        mm_dsp::NormalisableRange<float>(-100.0f, 100.0f, 0.01f),
        0.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Output", 1),
        "Output",
        mm_dsp::NormalisableRange<float>(-20.0f, 20.0f, 0.01f),
        0.0f));

    return layout;
}

class MDALimiterAudioProcessor:public MdaBase { public:
MDALimiterAudioProcessor(){apvts.params=std::move(createParameterLayout().params);}
void prepareToPlay(double,int) override;void processBlock(mm_dsp::AudioBuffer<float>&,mm_dsp::MidiBuffer&) override;void reset();void releaseResources();
private:

    mm_dsp::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();

    void update();
    void resetState();

    // The maximum amplitude you want the sound to have. Louder sounds will be
    // reduced to approximately this level. With a very slow attack it may take
    // a long time before the sound drops below the threshold level, if ever.
    // In soft knee mode, the threshold only indirectly represents the maximum
    // amplitude, but it still serves the same purpose.
    float _threshold;

    // The attack time determines how quickly the limiter responds when the sound
    // level approaches (in soft knee mode) or exceeds the threshold (hard knee).
    float _attack;

    // The release time determines how quickly the gain level resets itself after
    // the limiter no longer needs to be active.
    float _release;

    // Use this for boosting the output level when setting a low threshold.
    float _trim;

    // Choose between hard knee and soft knee modes.
    bool _softKnee;

    // The current gain level. When the audio signal exceeds or approaches the
    // threshold, the gain level is lowered to reduce the amplitude. The limiter
    // essentially calculates a gain signal over time that is used to prevent the
    // audio from becoming too loud. This is the gain signal's most recent value.
    float _gain;

    
};

void MDALimiterAudioProcessor::prepareToPlay(double sampleRate, int samplesPerBlock)
{
    resetState();
}
void MDALimiterAudioProcessor::releaseResources()
{
}
void MDALimiterAudioProcessor::reset()
{
    resetState();
}
void MDALimiterAudioProcessor::resetState()
{
    // Always start at maximum volume.
    _gain = 1.0f;
}
void MDALimiterAudioProcessor::update()
{
    _softKnee = apvts.getRawParameterValue("Knee")->load() == 1.0f;

    // The threshold parameter is in decibels. The formulas below assume this is
    // a value between 0 and 1, so ask APVTS for the normalized parameter value.
    float fParam1 = apvts.getParameter("Thresh")->getValue();

    // What the threshold means depends on the knee mode: in hard knee mode it's
    // literally the maximum amplitude the sound is allowed to have, but I'm not
    // sure exactly how to interpret it in soft knee mode. In both modes, moving
    // the slider to the left gives you more limiting than sliding to the right.
    // But it's not entirely clear to me how the numbers on the slider relate to
    // the actual amount of limiting that happens in soft knee mode -- the amount
    // of reduction is less than the dB value you see on the slider!
    if (_softKnee) {
        // Soft knee sets the threshold between 10 (+20 dB) and 0.1 (-20 dB),
        // but this isn't the actual level that the signal will be limited to.
        // Experiments have shown this results in a reduction of -20 dB when
        // the slider is to the left, about -6 dB when it's in the center, and
        // only a tiny bit when the slider is all the way to the right. I'm half
        // convinced the formula below contains a typo and that the 1.0 should
        // be a 2.0, which would give this a range of +40 dB to 0 dB, which makes
        // more sense.
        _threshold = std::pow(10.0f, 1.0f - (2.0f * fParam1));
    } else {
        // Hard knee sets the threshold between 0.01 (-40 dB) and 1.0 (0 dB).
        // If you're wondering how this calculation works, it's the usual formula
        // for converting decibels to linear gain, 10^(dB/20), except the decibels
        // are already represented as a number between 0 and 1, where 0 = -40 dB.
        _threshold = std::pow(10.0f, (2.0f * fParam1) - 2.0f);
    }

    // The output level is in decibels; convert to a linear gain.
    float fParam2 = apvts.getRawParameterValue("Output")->load();
    _trim = mm_dsp::Decibels::decibelsToGain(fParam2);

    // The attack parameter goes between 0 and 1. Convert it into a value from
    // 1 to 0.01, where 1 means the attack is fast and 0.01 means it's slow.
    float fParam3 = apvts.getRawParameterValue("Attack")->load();
    _attack = std::pow(10.0f, -2.0f * fParam3);

    // The release parameter goes between 0 and 1. Convert this into the range
    // 0.01 to 0.00001. Like with the attack, a smaller number means slower.
    float fParam4 = apvts.getRawParameterValue("Release")->load();
    _release = std::pow(10.0f, -2.0f - (3.0f * fParam4));
}
void MDALimiterAudioProcessor::processBlock(mm_dsp::AudioBuffer<float> &buffer, mm_dsp::MidiBuffer &midiMessages)
{
    mm_dsp::ScopedNoDenormals noDenormals;
    auto totalNumInputChannels = getTotalNumInputChannels();
    auto totalNumOutputChannels = getTotalNumOutputChannels();

    // Clear any output channels that don't contain input data.
    for (auto i = totalNumInputChannels; i < totalNumOutputChannels; ++i) {
        buffer.clear(i, 0, buffer.getNumSamples());
    }

    update();

    const float *in1 = buffer.getReadPointer(0);
    const float *in2 = buffer.getReadPointer(1);
    float *out1 = buffer.getWritePointer(0);
    float *out2 = buffer.getWritePointer(1);

    const float threshold = _threshold;
    const float attack = _attack;
    const float release = _release;
    const float trim = _trim;

    float g = _gain;

    /*
      How this works:

      The limiter algorithm computes a gain signal by comparing the audio input
      to the threshold level. And then it multiplies the audio by this gain to
      prevent the sound from becoming too loud.

      The gain level smoothly goes down whenever the threshold is exceeded. Once
      the audio is no longer exceeding the threshold, the gain signal can go back
      up again, so that softer passages in the audio will play at their original
      loudness as much as possible.

      You can think of this as a tug-of-war: on any given sample, we're either
      reducing the gain because the sound is too loud, or we're increasing the
      gain to bring the volume back to its original level.

      In practice, the gain signal is constantly going up and down. The attack
      and release settings determine how smooth this motion is.

      If the attack is too slow, the gain reduction can't keep up with the sound
      and it will keep exceeding the threshold. If the attack is too fast, the
      sound level will change too abruptly (although for a limiter it's usually
      OK to have a really fast attack).

      The release should generally be slower than the attack. But if the release
      is too slow, the audio may end up sounding too soft because it will never
      recover back to its full level. On the other hand, if the release is too
      fast, the gain signal may end up rapidly bouncing around the threshold.

      This plug-in provides two modes: soft knee and hard knee. Hard knee waits
      until the threshold has been crossed and will then limit the audio signal.
      Soft knee does not wait until the threshold has been reached, but gradually
      starts to attentuate the audio as it approaches the threshold. Generally,
      soft knee gives a more gentle effect, while hard knee tends to be harsher.
     */

    if (_softKnee) {
        for (int i = 0; i < buffer.getNumSamples(); ++i) {
            float inL = in1[i];
            float inR = in2[i];

            // In soft knee mode we don't wait until the audio exceeds the threshold.
            // Instead, calculate what the current audio amplitude should be to stay
            // below the threshold. If the audio is silent, the level variable is 1.
            // The louder the audio becomes, the smaller level will be and the more
            // the gain will be reduced. Also, the larger the threshold, the quicker
            // this variable will drop towards 0 (although it will never reach 0).
            float level = 1.0f / (1.0f + threshold * std::abs(inL + inR));

            // If we wanted to immediately squash the audio signal to fit within the
            // desired levels, we could use the value of the level variable as the
            // gain for the current sample. In fact, that is what happens when attack
            // and release are both 1.0. However, this also distorts the signal.
            // By using smaller attack and release values, the gain signal becomes a
            // smoothed version of the instantaneous audio level.
            if (g > level) {
                g = g - attack * (g - level);
            } else {
                g = g + release * (level - g);
            }

            out1[i] = inL * trim * g;
            out2[i] = inR * trim * g;
        }

    // Hard knee mode
    } else {
        for (int i = 0; i < buffer.getNumSamples(); ++i) {
            float inL = in1[i];
            float inR = in2[i];

            // To find out whether the sound exceeds the threshold, we need to find
            // out how loud it is. First, convert the signal from stereo to mono by
            // adding up the two channels. To compensate for the increased amplitude
            // from this, multiply by 0.5 (i.e. we average the two channels).
            // Then take the absolute value so that negative sample values don't
            // cancel out positive sample values. Also multiply by the current gain
            // level, because we'll be using this variable to determine if that gain
            // is still appropriate.
            // This is a very simple level detector: it only looks at the current
            // sample. If this sample is way below the threshold but the overall
            // sound is still too loud, we'll actually start incrementing the gain
            // again. One way to improve that is to use an envelope detector, which
            // looks at the general trend of the signal instead of single samples.
            float level = 0.5f * g * std::abs(inL + inR);

            if (level > threshold) {
                // If the signal level goes over the threshold, the current gain
                // value is too high and we must reduce it. The attack determines
                // how quickly the gain responds -- we don't immediately drop the
                // gain all the way but only reduce it a little bit. We keep doing
                // this for the next samples too, until the sound level no longer
                // exceeds the threshold.
                g = g - (attack * (level - threshold));
            } else {
                // If the signal is below the threshold, the gain setting might
                // now be too low. Slowly bring the gain level back to 1.0.
                // This follows a logarithmic curve. The smaller the release,
                // the longer this takes. This continues until the gain is 1 or
                // until the sound level crosses the threshold again.
                g = g + (release * (1.0f - g));
            }

            out1[i] = inL * trim * g;
            out2[i] = inR * trim * g;
        }
    }

    _gain = g;
}
mm_dsp::AudioProcessorValueTreeState::ParameterLayout MDALimiterAudioProcessor::createParameterLayout()
{
    mm_dsp::AudioProcessorValueTreeState::ParameterLayout layout;

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Thresh", 1),
        "Thresh",
        mm_dsp::NormalisableRange<float>(-40.0f, 0.0f, 0.01f),
        -16.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Output", 1),
        "Output",
        mm_dsp::NormalisableRange<float>(-20.0f, 20.0f, 0.01f),
        4.0f));

    // In the original plug-in, the attack and release times are kind of weird.
    // For example, the time in milliseconds was computed as follows:
    //   -301030.1 / (getSampleRate() * log10(1.0 - att)))
    // where att is a value between 1 (fastest attack) and 0.01 (slowest).
    // I don't understand where that formula comes from. It might be some kind
    // of approximation based on the actual behavior of the limiting algorithm?
    // In the JUCE version, we just keep the attack and release sliders between
    // 0 and 1 where 0 means fastest and 1 means slowest.

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Attack", 1),
        "Attack",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.15f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Release", 1),
        "Release",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.5f));

    layout.add(std::make_unique<mm_dsp::AudioParameterChoice>(
        mm_dsp::ParameterID("Knee", 1),
        "Knee",
        mm_dsp::StringArray({ "Hard", "Soft" }),
        0));

    return layout;
}

class MDALoudnessAudioProcessor:public MdaBase { public:
MDALoudnessAudioProcessor(){apvts.params=std::move(createParameterLayout().params);}
void prepareToPlay(double,int) override;void processBlock(mm_dsp::AudioBuffer<float>&,mm_dsp::MidiBuffer&) override;void reset();void releaseResources();
private:

    mm_dsp::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();

    void update();
    void resetState();

    float z0, z1, z2, z3;  // filter delays (0+1 = left channel, 2+3 = right)
    float a0, a1, a2;      // filter coefficients
    float gain;            // output gain
    int mode;              // 0 = cut, 1 = boost

    
};

void MDALoudnessAudioProcessor::prepareToPlay(double sampleRate, int samplesPerBlock)
{
    resetState();
}
void MDALoudnessAudioProcessor::releaseResources()
{
}
void MDALoudnessAudioProcessor::reset()
{
    resetState();
}
void MDALoudnessAudioProcessor::resetState()
{
    z0 = z1 = z2 = z3 = 0.0f;
}
void MDALoudnessAudioProcessor::update()
{
    float igain = apvts.getRawParameterValue("Loudness")->load();
    float ogain = apvts.getRawParameterValue("Output")->load();

    // Convert the decibel value to an index in the `loudness` table.
    float f = 0.1f * igain + 6.0f;
    int i = int(f);  // coefficient index
    f -= float(i);   // fractional part

    // Do a linear interpolation between the entries in the table.
    a0 = loudness[i][0] + f * (loudness[i + 1][0] - loudness[i][0]);
    a1 = loudness[i][1] + f * (loudness[i + 1][1] - loudness[i][1]);
    a2 = loudness[i][2] + f * (loudness[i + 1][2] - loudness[i][2]);

    // The first entry in the table is a frequency; convert it to a
    // low-pass filter coefficient.
    a0 = 1.0f - std::exp(-6.283153f * a0 / getSampleRate());

    // Cutting or boosting?
    mode = (igain > 0.0f);

    // Calculate the output gain, convert from dB to linear value.
    float tmp = ogain;
    if (apvts.getRawParameterValue("Link")->load()) {  // linked gain
        tmp -= igain;
        if (tmp > 0.0f) {  // limit max gain
            tmp = 0.0f;
        }
    }
    gain = std::pow(10.0f, 0.05f * tmp);
}
void MDALoudnessAudioProcessor::processBlock(mm_dsp::AudioBuffer<float> &buffer, mm_dsp::MidiBuffer &midiMessages)
{
    mm_dsp::ScopedNoDenormals noDenormals;
    auto totalNumInputChannels = getTotalNumInputChannels();
    auto totalNumOutputChannels = getTotalNumOutputChannels();

    // Clear any output channels that don't contain input data.
    for (auto i = totalNumInputChannels; i < totalNumOutputChannels; ++i) {
        buffer.clear(i, 0, buffer.getNumSamples());
    }

    update();

    const float *in1 = buffer.getReadPointer(0);
    const float *in2 = buffer.getReadPointer(1);
    float *out1 = buffer.getWritePointer(0);
    float *out2 = buffer.getWritePointer(1);

    if (mode == 0) {  // cut
        for (int i = 0; i < buffer.getNumSamples(); ++i) {
            float a = in1[i];
            float b = in2[i];

            // Filter for the left channel.
            z0 += a0 * (a - z0 + 0.3f * z1);
            a -= z0;
            z1 += a0 * (a - z1);
            a -= z1;
            a -= z0 * a1;

            // Filter for the right channel.
            z2 += a0 * (b - z2 + 0.3f * z1);
            b -= z2;
            z3 += a0 * (b - z3);
            b -= z3;
            b -= z2 * a1;

            out1[i] = a * gain;
            out2[i] = b * gain;
        }
    } else {  // boost
        for (int i = 0; i < buffer.getNumSamples(); ++i) {
            float a = in1[i];
            float b = in2[i];

            // This is two one-pole low-pass filters in a row.
            z0 += a0 * (a - z0);
            z1 += a0 * (z0 - z1);

            // Add the low-pass filtered values to the input to boost it.
            a += a1 * (z1 - a2 * z0);

            // Same filtering but for the right channel.
            z2 += a0 * (b - z2);
            z3 += a0 * (z2 - z3);
            b += a1 * (z3 - a2 * z2);

            out1[i] = a * gain;
            out2[i] = b * gain;
        }
    }
}
mm_dsp::AudioProcessorValueTreeState::ParameterLayout MDALoudnessAudioProcessor::createParameterLayout()
{
    mm_dsp::AudioProcessorValueTreeState::ParameterLayout layout;

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Loudness", 1),
        "Loudness",
        mm_dsp::NormalisableRange<float>(-60.0f, 60.0f, 0.01f, 0.5f, true),
        9.6f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Output", 1),
        "Output",
        mm_dsp::NormalisableRange<float>(-60.0f, 60.0f, 0.01f, 0.5f, true),
        0.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterBool>(
        mm_dsp::ParameterID("Link", 1),
        "Link",
        false));

    return layout;
}

class MDAOverdriveAudioProcessor:public MdaBase { public:
MDAOverdriveAudioProcessor(){apvts.params=std::move(createParameterLayout().params);}
void prepareToPlay(double,int) override;void processBlock(mm_dsp::AudioBuffer<float>&,mm_dsp::MidiBuffer&) override;void reset();void releaseResources();
private:

    mm_dsp::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();

    void update();
    void resetState();

    // Amount of overdrive, a value between 0 and 1. This controls the mix
    // between the original signal and the overdriven one.
    float _drive;

    // Filter coefficient, a value between 1 and 0.025.
    float _filt;

    // Output gain, a value between 0.1 (for -20 dB) and 10 (for +20 dB).
    float _gain;

    // Delay units for the left and right channel filters.
    float _filtL, _filtR;

    
};

void MDAOverdriveAudioProcessor::prepareToPlay(double sampleRate, int samplesPerBlock)
{
    resetState();
}
void MDAOverdriveAudioProcessor::releaseResources()
{
}
void MDAOverdriveAudioProcessor::reset()
{
    resetState();
}
void MDAOverdriveAudioProcessor::resetState()
{
    // Set the filter delay units back to zero.
    _filtL = _filtR = 0.0f;
}
void MDAOverdriveAudioProcessor::update()
{
    // The amount of drive is a percentage; divide by 100 to make it 0 - 1.
    _drive = apvts.getRawParameterValue("Drive")->load() / 100.0f;

    // The muffle parameter controls a low-pass filter. Muffle is a percentage;
    // convert it to a value between 1.0 and 0.025 that drops off exponentially.
    // 1.0 means the filter is disabled; the smaller the value, the lower the
    // cutoff frequency will be. At 100%, the cutoff is around 200 Hz somewhere.
    // Since it's a first-order filter, it has a gentle roll-off of 6 dB/octave.
    float muffle = apvts.getRawParameterValue("Muffle")->load();
    _filt = std::pow(10.0f, -1.6f * muffle / 100.0f);

    // The output level is between -20 dB and +20 dB. Convert to linear gain.
    float output = apvts.getRawParameterValue("Output")->load();
    _gain = mm_dsp::Decibels::decibelsToGain(output);
}
void MDAOverdriveAudioProcessor::processBlock(mm_dsp::AudioBuffer<float> &buffer, mm_dsp::MidiBuffer &midiMessages)
{
    mm_dsp::ScopedNoDenormals noDenormals;
    auto totalNumInputChannels = getTotalNumInputChannels();
    auto totalNumOutputChannels = getTotalNumOutputChannels();

    // Clear any output channels that don't contain input data.
    for (auto i = totalNumInputChannels; i < totalNumOutputChannels; ++i) {
        buffer.clear(i, 0, buffer.getNumSamples());
    }

    update();

    const float *in1 = buffer.getReadPointer(0);
    const float *in2 = buffer.getReadPointer(1);
    float *out1 = buffer.getWritePointer(0);
    float *out2 = buffer.getWritePointer(1);

    const float drive = _drive;
    const float gain = _gain;
    const float f = _filt;

    float fa = _filtL, fb = _filtR;

    for (int i = 0; i < buffer.getNumSamples(); ++i) {
        float a = in1[i];
        float b = in2[i];

        // Overdrive: this applies a sqrt to slightly distort and boost the sound.
        float aa = (a > 0.0f) ? std::sqrt(a) : -std::sqrt(-a);
        float bb = (b > 0.0f) ? std::sqrt(b) : -std::sqrt(-b);

        // Filter: this is a simple exponentially weighted moving average filter.
        // The difference equation is: y(n) = f * x(n) + (1 - f) * y(n - 1).
        //
        // When f = 1, the filter is disabled as that makes (1 - f) equal to 0.
        // As you move the "muffle" slider to the right, f becomes exponentially
        // smaller and more smoothing is applied, making the filter cutoff lower.
        //
        // The input to the filter x(n) = drive * (aa - a) + a. This is simply
        // the linear interpolation between the original sample value and the
        // overdriven version, aa or sqrt(a). Rewriting the math shows that this
        // is the same as x(n) = drive * aa + (1 - drive) * a. The larger drive
        // is, the more we add the overdriven signal into the mix.
        //
        // fa is y(n - 1) for the left channel and fb is for the right channel.
        fa = fa + f * (drive * (aa - a) + a - fa);
        fb = fb + f * (drive * (bb - b) + b - fb);

        // Apply output gain and write to output buffer.
        out1[i] = fa * gain;
        out2[i] = fb * gain;
    }

    // Catch denormals
    if (std::abs(fa) > 1.0e-10f) _filtL = fa; else _filtL = 0.0f;
    if (std::abs(fb) > 1.0e-10f) _filtR = fb; else _filtR = 0.0f;
}
mm_dsp::AudioProcessorValueTreeState::ParameterLayout MDAOverdriveAudioProcessor::createParameterLayout()
{
    mm_dsp::AudioProcessorValueTreeState::ParameterLayout layout;

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Drive", 1),
        "Drive",
        mm_dsp::NormalisableRange<float>(0.0f, 100.0f, 0.01f),
        0.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Muffle", 1),
        "Muffle",
        mm_dsp::NormalisableRange<float>(0.0f, 100.0f, 0.01f),
        0.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Output", 1),
        "Output",
        mm_dsp::NormalisableRange<float>(-20.0f, 20.0f, 0.01f),
        0.0f));

    return layout;
}

class MDARezFilterAudioProcessor:public MdaBase { public:
MDARezFilterAudioProcessor(){apvts.params=std::move(createParameterLayout().params);}
void prepareToPlay(double,int) override;void processBlock(mm_dsp::AudioBuffer<float>&,mm_dsp::MidiBuffer&) override;void reset();void releaseResources();
private:

    mm_dsp::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();

    void update();
    void resetState();

    float cutoff;      // filter cutoff
    float q;           // filter Q
    float gain;        // output gain
    float filterMax;   // maximum cutoff

    float envDepth;    // envelope modulation amount
    float attack;      // envelope attack coefficient
    float release;     // envelope release coefficient
    float env;         // current envelope level

    float lfo;         // most recent LFO value
    float lfoPhase;    // current LFO phase
    float lfoInc;      // LFO rate
    float lfoDepth;    // LFO modulation amount
    bool sampleHold;   // 0 = sine, 1 = sample & hold

    float buf0, buf1;  // filter delay units

    float threshold;     // envelope trigger threshold
    float triggerEnv;    // secondary envelope used when triggered
    bool triggered;      // whether envelope exceeded threshold
    bool triggerAttack;  // triggerEnv currently in attack mode

    
};

void MDARezFilterAudioProcessor::prepareToPlay(double sampleRate, int samplesPerBlock)
{
    resetState();
}
void MDARezFilterAudioProcessor::releaseResources()
{
}
void MDARezFilterAudioProcessor::reset()
{
    resetState();
}
void MDARezFilterAudioProcessor::resetState()
{
    env = 0.0f;
    lfo = 0.0f;
    lfoPhase = 0.0f;
    buf0 = 0.0f;
    buf1 = 0.0f;
    triggerEnv = 0.0f;
    triggered = false;
    triggerAttack = false;
}
void MDARezFilterAudioProcessor::update()
{
    // Map [0%, 100%] to [-0.15, 1.35]
    float param0 = apvts.getRawParameterValue("Freq")->load() * 0.01f;
    cutoff = 1.5f * param0 * param0 - 0.15f;

    // Map [0%, 100%] to [0.0, 0.99]
    float param1 = apvts.getRawParameterValue("Res")->load() * 0.01f;
    q = 0.99f * std::pow(param1, 0.3f);

    // Convert decibels to linear value
    float param2 = apvts.getRawParameterValue("Output")->load();
    gain = 0.5f * std::pow(10.0f, param2 * 0.05f);

    // Map [-100, 100] to [-0.5, 0.5]
    float param3 = (apvts.getRawParameterValue("Env->VCF")->load() + 100.0f) / 200.0f;
    envDepth = 2.0f * (0.5f - param3) * (0.5f - param3);
    envDepth = (param3 > 0.5f) ? envDepth : -envDepth;

    // Filter coefficient for envelope attack
    float param4 = apvts.getRawParameterValue("Attack")->load();
    attack = std::pow(10.0f, -0.01f - 4.0f * param4);

    // Filter coefficient for envelope release
    float param5 = apvts.getRawParameterValue("Release")->load();
    release = 1.0f - std::pow(10.0f, -2.0f - 4.0f * param5);

    // Map [-100, 100] to [-0.5, 0.5]
    float param6 = (apvts.getRawParameterValue("LFO->VCF")->load() + 100.0f) / 200.0f;
    lfoDepth = 2.0f * (param6 - 0.5f) * (param6 - 0.5f);

    // Map [0, 1] to [0.01 Hz, 100 Hz] and calculate phase increment
    float param7 = apvts.getRawParameterValue("LFO Rate")->load();
    lfoInc = 6.2832f * std::pow(10.0f, 4.0f * param7 - 2.0f) / getSampleRate();

    // Need to enable sample&hold mode?
    sampleHold = false;
    if (param6 < 0.5f) {
        sampleHold = true;
        lfoInc *= 0.15915f;  // divide by 2π so lfoPhase counts from 0 to 1
        lfoDepth *= 0.001f;  // random numbers are between ±1000
    }

    // Map [0, 1] to [0, 3]
    float param8 = apvts.getRawParameterValue("Trigger")->load();
    if (param8 < 0.1f) {
        threshold = 0.0f;
    } else {
        threshold = 3.0f * param8 * param8;
    }

    // For this filter, the allowed maximum frequency is higher when Q is
    // larger, but make sure it doesn't exceed the value set by the user.
    float param9 = apvts.getRawParameterValue("Max Freq")->load() * 0.01f;
    // Monitor Mix: keep the legacy state-variable filter in its stable domain.
    filterMax = std::min(1.0f, 0.99f + 0.3f * param1);
    if (filterMax > 1.3f * param9) {
        filterMax = 1.3f * param9;
    }
}
void MDARezFilterAudioProcessor::processBlock(mm_dsp::AudioBuffer<float> &buffer, mm_dsp::MidiBuffer &midiMessages)
{
    mm_dsp::ScopedNoDenormals noDenormals;
    auto totalNumInputChannels = getTotalNumInputChannels();
    auto totalNumOutputChannels = getTotalNumOutputChannels();

    // Clear any output channels that don't contain input data.
    for (auto i = totalNumInputChannels; i < totalNumOutputChannels; ++i) {
        buffer.clear(i, 0, buffer.getNumSamples());
    }

    update();

    const float *in1 = buffer.getReadPointer(0);
    const float *in2 = buffer.getReadPointer(1);
    float *out1 = buffer.getWritePointer(0);
    float *out2 = buffer.getWritePointer(1);

    float b0 = buf0, b1 = buf1;

    if (threshold == 0.0f) {
        for (int i = 0; i < buffer.getNumSamples(); ++i) {
            // Process as mono
            float a = in1[i] + in2[i];

            // Simple envelope follower
            float level = (a > 0.0f) ? a : -a;
            env = (level > env) ? env + attack * (level - env) : env * release;

            // LFO
            if (!sampleHold) {
                lfo = lfoDepth * std::sin(lfoPhase);
            } else if (lfoPhase > 1.0f) {
                lfo = lfoDepth * (randomInt() % 2000 - 1000);
                lfoPhase = 0.0f;
            }
            lfoPhase += lfoInc;

            // Calculate modulated frequency
            float f = cutoff + envDepth * env + lfo;
            if (f < 0.0f) {
                f = 0.0f;
            } else if (f > filterMax) {
                f = filterMax;
            }

            // Apply the filter
            // I didn't study this filter in detail but it's likely some kind
            // of variation of the Chamberlin SVF, which can be unstable for
            // cutoff frequencies close to Nyquist. Probably shouldn't use it
            // in modern audio code!
            float tmp = q + q * (1.0f + f * (1.0f + 1.1f * f));
            b0 += f * (gain * a - b0 + tmp * (b0 - b1));
            b1 += f * (b0 - b1);

            /*
            // Alternative implementation also found in the original code:
            float o = 1.0f - f;
            b0 = o * b0 + f * (gain*a + q*(1.0f + (1.0f/o)) * (b0 - b1));
            b1 = o * b1 + f * b0;
            b2 = o * b2 + f * b1;
            */

            out1[i] = b1;
            out2[i] = b1;
        }
    } else {
        for (int i = 0; i < buffer.getNumSamples(); ++i) {
            // Process as mono.
            float a = in1[i] + in2[i];

            // This envelope follower has instant attack.
            float level = (a > 0.0f) ? a : -a;
            env = (level > env) ? level : env * release;

            // Envelope level exceeds threshold?
            if (env > threshold) {
                if (!triggered) {
                    triggerAttack = true;
                    if (sampleHold) {  // trigger new S&H
                        lfoPhase = 2.0f;
                    }
                }
                triggered = true;
            } else {
                triggered = false;
            }

            // NOTE: Not sure what the point is of the second envelope, as
            // it's not actually used anywhere. When the main envelope hits
            // the threshold, nothing actually happens to the audio signal.
            // (It's possible I missed something when converting the code,
            // or that this part of the plug-in wasn't finished.)

            if (triggerAttack) {
                triggerEnv += attack * (1.0f - triggerEnv);
                if (triggerEnv > 0.999f) {
                    triggerAttack = false;
                }
            } else {
                triggerEnv *= release;
            }

            // The code below is the same as before.

            if (!sampleHold) {
                lfo = lfoDepth * std::sin(lfoPhase);
            } else if (lfoPhase > 1.0f) {
                lfo = lfoDepth * (randomInt() % 2000 - 1000);
                lfoPhase = 0.0f;
            }
            lfoPhase += lfoInc;

            float f = cutoff + envDepth * env + lfo;
            if (f < 0.0f) {
                f = 0.0f;
            } else if (f > filterMax) {
                f = filterMax;
            }

            float tmp = q + q * (1.0f + f * (1.0f + 1.1f * f));
            b0 += f * (gain * a - b0 + tmp * (b0 - b1));
            b1 += f * (b0 - b1);

            out1[i] = b1;
            out2[i] = b1;
        }
    }

    if (std::abs(b0) < 1.0e-10f) {
        buf0 = 0.0f;
        buf1 = 0.0f;
    } else {
        buf0 = b0;
        buf1 = b1;
    }

    lfoPhase = std::fmod(lfoPhase, 6.2831853f);
}
mm_dsp::AudioProcessorValueTreeState::ParameterLayout MDARezFilterAudioProcessor::createParameterLayout()
{
    mm_dsp::AudioProcessorValueTreeState::ParameterLayout layout;

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Freq", 1),
        "Freq",
        mm_dsp::NormalisableRange<float>(0.0f, 100.0f, 0.01f),
        33.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Res", 1),
        "Res",
        mm_dsp::NormalisableRange<float>(0.0f, 100.0f, 0.01f),
        70.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Output", 1),
        "Output",
        mm_dsp::NormalisableRange<float>(-20.0f, 20.0f, 0.01f),
        0.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Env->VCF", 1),
        "Env->VCF",
        mm_dsp::NormalisableRange<float>(-100.0f, 100.0f, 0.01f),
        70.0f));

    // Not sure if the displayed attack and release times actually make sense!
    // With JUCE it's much simpler to define the actual range in milliseconds
    // and convert that into the filter coefficient, instead of the other way
    // around like it was done here.

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Attack", 1),
        "Attack",
        mm_dsp::NormalisableRange<float>(),
        0.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Release", 1),
        "Release",
        mm_dsp::NormalisableRange<float>(),
        0.5f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("LFO->VCF", 1),
        "LFO->VCF",
        mm_dsp::NormalisableRange<float>(-100.0f, 100.0f, 0.01f),
        70.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("LFO Rate", 1),
        "LFO Rate",
        mm_dsp::NormalisableRange<float>(),
        0.4f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Trigger", 1),
        "Trigger",
        mm_dsp::NormalisableRange<float>(),
        0.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Max Freq", 1),
        "Max Freq",
        mm_dsp::NormalisableRange<float>(0.0f, 100.0f, 0.01f),
        75.0f));

    return layout;
}

class MDARingModAudioProcessor:public MdaBase { public:
MDARingModAudioProcessor(){apvts.params=std::move(createParameterLayout().params);}
void prepareToPlay(double,int) override;void processBlock(mm_dsp::AudioBuffer<float>&,mm_dsp::MidiBuffer&) override;void reset();void releaseResources();
private:

    mm_dsp::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();

    void update();
    void resetState();

    // Output level. This was not in the original plug-in, but with a lot of
    // feedback it's useful to dial back the total volume to prevent clipping.
    float _level;

    // Phase increment for the sine wave.
    float _phaseInc;

    // Amount of feedback to add (value between 0 and 1).
    float _feedbackAmount;

    // Current phase for the sine wave.
    float _phase;

    // Previous output values for the left and right channels; used for feedback.
    float _prevL, _prevR;

    
};

void MDARingModAudioProcessor::prepareToPlay(double sampleRate, int samplesPerBlock)
{
    resetState();
}
void MDARingModAudioProcessor::releaseResources()
{
}
void MDARingModAudioProcessor::reset()
{
    resetState();
}
void MDARingModAudioProcessor::resetState()
{
    _phase = 0.0f;
    _prevL = 0.0f;
    _prevR = 0.0f;
}
void MDARingModAudioProcessor::update()
{
    float freq = apvts.getRawParameterValue("Frequency")->load();
    float fine = apvts.getRawParameterValue("Fine-tune")->load();

    const auto twoPi = mm_dsp::MathConstants<float>::twoPi;

    // The phase increment is 2*pi*freq/sampleRate. The frequency is between 0 Hz
    // and 16000 Hz, in steps of 100 Hz. The fine-tune amount is from 0 - 100 Hz.
    _phaseInc = twoPi * (fine + freq) / float(getSampleRate());

    // Feedback is a percentage from 0 to 95%.
    _feedbackAmount = apvts.getRawParameterValue("Feedback")->load() / 100.0f;

    // Convert from decibels to a linear gain value.
    float level = apvts.getRawParameterValue("Level")->load();
    _level = mm_dsp::Decibels::decibelsToGain(level);
}
void MDARingModAudioProcessor::processBlock(mm_dsp::AudioBuffer<float> &buffer, mm_dsp::MidiBuffer &midiMessages)
{
    mm_dsp::ScopedNoDenormals noDenormals;
    auto totalNumInputChannels = getTotalNumInputChannels();
    auto totalNumOutputChannels = getTotalNumOutputChannels();

    // Clear any output channels that don't contain input data.
    for (auto i = totalNumInputChannels; i < totalNumOutputChannels; ++i) {
        buffer.clear(i, 0, buffer.getNumSamples());
    }

    update();

    const float *in1 = buffer.getReadPointer(0);
    const float *in2 = buffer.getReadPointer(1);
    float *out1 = buffer.getWritePointer(0);
    float *out2 = buffer.getWritePointer(1);

    const float level = _level;
    const float phaseInc = _phaseInc;
    const float feedback = _feedbackAmount;

    float phase = _phase;
    float prevL = _prevL;
    float prevR = _prevR;

    const auto twoPi = mm_dsp::MathConstants<float>::twoPi;

    for (int i = 0; i < buffer.getNumSamples(); ++i) {
        // The value of the sine is the instantaneous gain.
        const float g = std::sin(phase);

        // Increment the phase and wrap at 2 pi.
        phase = std::fmod(phase + phaseInc, twoPi);

        // Add the previous output value to the new input sample, multiplied by
        // the feedback factor. Then multiply by sine wave for ring modulation.
        prevL = (feedback * prevL + in1[i]) * g;
        prevR = (feedback * prevR + in2[i]) * g;

        // Before putting the value into the output buffer, multiply it by the
        // output level in order to attenuate it, if necessary.
        out1[i] = prevL * level;
        out2[i] = prevR * level;
    }

    _phase = phase;
    _prevL = prevL;
    _prevR = prevR;
}
mm_dsp::AudioProcessorValueTreeState::ParameterLayout MDARingModAudioProcessor::createParameterLayout()
{
    mm_dsp::AudioProcessorValueTreeState::ParameterLayout layout;

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Frequency", 1),
        "Frequency",
        mm_dsp::NormalisableRange<float>(0.0f, 16000.0f, 100.0f, 0.5f),
        1000.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Fine-tune", 1),
        "Fine-tune",
        mm_dsp::NormalisableRange<float>(0.0f, 100.0f, 0.01f),
        0.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Feedback", 1),
        "Feedback",
        mm_dsp::NormalisableRange<float>(0.0f, 95.0f, 0.01f),
        0.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Level", 1),
        "Level",
        mm_dsp::NormalisableRange<float>(-30.0f, 0.0f, 0.01f),
        -6.0f));

    return layout;
}

class MDAShepardAudioProcessor:public MdaBase { public:
MDAShepardAudioProcessor(){apvts.params=std::move(createParameterLayout().params);}
void prepareToPlay(double,int) override;void processBlock(mm_dsp::AudioBuffer<float>&,mm_dsp::MidiBuffer&) override;void reset();void releaseResources();
private:

    mm_dsp::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();

    void update();
    void resetState();

    // The currently selected mode.
    int _mode;

    // Output gain level.
    float _level;

    // The speed of rising or falling.
    float _delta;

    // Two wavetables containing sine waves.
    static const int bufferSize = 512;
    float _buf1[bufferSize];
    float _buf2[bufferSize];

    // Read position in the wavetables.
    float _pos;

    // Increment of read position. This is what determines the current pitch.
    float _rate;

    
};

void MDAShepardAudioProcessor::prepareToPlay(double sampleRate, int samplesPerBlock)
{
    resetState();
}
void MDAShepardAudioProcessor::releaseResources()
{
}
void MDAShepardAudioProcessor::reset()
{
    resetState();
}
void MDAShepardAudioProcessor::resetState()
{
    _pos = 0.0f;
    _rate = 1.0f;
}
void MDAShepardAudioProcessor::update()
{
    _mode = int(apvts.getRawParameterValue("Mode")->load());

    // The rate is a percentage from -100% to +100%. Convert this into a curve
    // that is 1.0 at 0% and goes gently downward to the left (to make a falling
    // sound) and upward to the right (for a rising sound).
    float fParam1 = int(apvts.getRawParameterValue("Rate")->load());
    fParam1 = (fParam1 + 100) / 200.0f;
    _delta = 1.0f + 10.0f * std::pow(fParam1 - 0.5f, 3.0f) / float(getSampleRate());

    // Convert the output level from decibels to a linear gain.
    float fParam2 = int(apvts.getRawParameterValue("Output")->load());
    _level = 0.4842f * mm_dsp::Decibels::decibelsToGain(fParam2);
}
void MDAShepardAudioProcessor::processBlock(mm_dsp::AudioBuffer<float> &buffer, mm_dsp::MidiBuffer &midiMessages)
{
    mm_dsp::ScopedNoDenormals noDenormals;
    auto totalNumInputChannels = getTotalNumInputChannels();
    auto totalNumOutputChannels = getTotalNumOutputChannels();

    // Clear any output channels that don't contain input data.
    for (auto i = totalNumInputChannels; i < totalNumOutputChannels; ++i) {
        buffer.clear(i, 0, buffer.getNumSamples());
    }

    update();

    const float *in1 = buffer.getReadPointer(0);
    const float *in2 = buffer.getReadPointer(1);
    float *out1 = buffer.getWritePointer(0);
    float *out2 = buffer.getWritePointer(1);

    const float *buf1 = _buf1;
    const float *buf2 = _buf2;
    const float max = float(bufferSize - 1);
    const int mode = _mode;
    const float delta = _delta;
    const float level = _level;

    float rate = _rate;
    float pos = _pos;

    for (int i = 0; i < buffer.getNumSamples(); ++i) {
        // The illusion is a sound that appears to be rising (or falling) in pitch
        // forever. We make the pitch go up by stepping through the wavetable at an
        // increasing rate. The rate goes up from 1.0 to 2.0 and then wraps back to
        // 1.0. That wrap-around by itself would give a jarring effect. To hide
        // this, we don't just play a plain sine wave (buf2) but also 1, 2, 4, 8
        // octaves higher (buf1). As the pitch goes up, the higher notes fade out
        // while lower notes fade in. In the case of a falling tone, we do it the
        // other way around (the pitch goes down because the wavetable read pos is
        // incremented using a rate that becomes gradually smaller).

        rate *= delta;
        if (rate > 2.0f) {  // rising tone
            rate *= 0.5f;
            pos *= 0.5f;
        } else if (rate < 1.0f) {  // falling tone
            rate *= 2.0f;
            pos *= 2.0f;
            if (pos >= max) pos -= max;
        }

        // Increment the read position in the wavetables and wrap around when we
        // get to the end.
        pos += rate;
        if (pos > max) pos -= max;

        // Calculate position interpolation:
        int i1 = int(pos);           // current sample
        int i2 = i1 + 1;             // next sample
        float di = float(i2) - pos;  // fractional part between the two

        // di and (1 - di) are used for interpolating between two successive
        // positions in the wavetable.
        // buf1 contains the sine waves in the higher octaves, with each higher
        // octave having half the amplitude as the previous octave.
        // buf2 contains a plain sine wave. We multiply this by (rate - 2) in
        // order to fade it in or out as the pitch changes.
        float b =           di  * (buf1[i1] + (rate - 2.0f) * buf2[i1])
                  + (1.0f - di) * (buf1[i2] + (rate - 2.0f) * buf2[i2]);

        // Dividing by the rate means we fade out the sound as the pitch goes
        // up, or fade it in when the pitch goes down.
        b *= level / rate;

        // Do we need to combine the tone with incoming audio?
        if (mode > 0) {
            float a = in1[i] + in2[i];  // stereo to mono
            if (mode == 1) {
                b *= a;                 // ring modulation with input
            } else if (mode == 2) {
                b += 0.5f * a;          // mix with input (at -6 dB)
            }
        }

        out1[i] = b;
        out2[i] = b;
    }

    _pos = pos;
    _rate = rate;
}
mm_dsp::AudioProcessorValueTreeState::ParameterLayout MDAShepardAudioProcessor::createParameterLayout()
{
    mm_dsp::AudioProcessorValueTreeState::ParameterLayout layout;

    layout.add(std::make_unique<mm_dsp::AudioParameterChoice>(
        mm_dsp::ParameterID("Mode", 1),
        "Mode",
        mm_dsp::StringArray({ "TONES", "RING MOD", "TONES+IN" }),
        0));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Rate", 1),
        "Rate",
        mm_dsp::NormalisableRange<float>(-100.0f, 100.0f, 0.01f),
        40.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Output", 1),
        "Output",
        mm_dsp::NormalisableRange<float>(-20.0f, 20.0f, 0.01f),
        0.0f));

    return layout;
}

class MDASplitterAudioProcessor:public MdaBase { public:
MDASplitterAudioProcessor(){apvts.params=std::move(createParameterLayout().params);}
void prepareToPlay(double,int) override;void processBlock(mm_dsp::AudioBuffer<float>&,mm_dsp::MidiBuffer&) override;void reset();void releaseResources();
private:

    mm_dsp::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();

    void update();
    void resetState();

    float freq;                // filter coefficient
    float a0, a1, b0, b1;      // filter states (a = left, b = right channel)

    float level;               // gate threshold
    float env;                 // current envelope level
    float att, rel;            // attack and release constants

    float ff, ll, pp;          // routing: freq, level, polarity
    float i2l, i2r, o2l, o2r;  // routing: gain for left/right dry&wet

    
};

void MDASplitterAudioProcessor::prepareToPlay(double newSampleRate, int samplesPerBlock)
{
    resetState();
}
void MDASplitterAudioProcessor::releaseResources()
{
}
void MDASplitterAudioProcessor::reset()
{
    resetState();
}
void MDASplitterAudioProcessor::resetState()
{
    env = a0 = a1 = b0 = b1 = 0.0f;
}
void MDASplitterAudioProcessor::update()
{
    // The frequency goes from 100 Hz to 10 kHz.
    // The `freq` variable is the coefficient for a filter.
    float param1 = apvts.getRawParameterValue("Freq")->load();
    float fdisp = std::pow(10.0f, 2.0f + 2.0f * param1);
    freq = 5.5f * fdisp / getSampleRate();
    if (freq > 1.0f) { freq = 1.0f; }

    // Frequency switching. This determines whether the filter is low-pass
    // (BELOW), high-pass (ABOVE), or disabled (ALL).
    float param2 = int(apvts.getRawParameterValue("Freq SW")->load());
    ff = -1.0f;                          // above
    if (param2 == 0) { ff = 0.0f; }      // below
    if (param2 == 1) { freq = 0.001f; }  // all

    // The level threshold goes from -40 dB to 0 dB. Convert to linear gain.
    // The + 0.3 adds 6 dB, so if the parameter is set to 0 dB, the `level`
    // variable is actually 1.9953. This is done because computing the signal
    // amplitude adds up the left and right channels, which can boost the
    // combined amplitude by 2x or 6 dB.
    float param3 = apvts.getRawParameterValue("Level")->load();
    float ldisp = 40.0f * param3 - 40.0f;
    level = std::pow(10.0f, 0.05f * ldisp + 0.3f);

    // Level switching. This determines whether the gate lets through low
    // levels only (BELOW), high levels only (ABOVE), or everything (ALL).
    float param4 = int(apvts.getRawParameterValue("Level SW")->load());
    ll = 0.0f;                          // above
    if (param4 == 0) { ll = -1.0f; }    // below
    if (param4 == 1) { level = 0.0f; }  // all

    // Phase (polarity) correction. This is used with the envelope follower
    // and depends on the frequency switching and level switching modes.
    // This is just a helper variable that avoids branching.
    pp = -1.0f;
    if (ff == ll) { pp = 1.0f; }
    if (ff == 0.0f && ll == -1.0f) { ll *= -1.0f; }

    // The envelope goes from 10 ms to 1000 ms. This sets an approximate
    // attack and release time.
    float param5 = int(apvts.getRawParameterValue("Envelope")->load());
    att = 0.05f - 0.05f * param5;
    rel = 1.0f - std::exp(-6.0f - 4.0f * param5);
    if (att > 0.02f) { att = 0.02f; }
    if (rel < 0.9995f) { rel = 0.9995f; }

    // The output level goes from -20 dB to 20 dB. Convert to linear gain.
    float param6 = apvts.getRawParameterValue("Output")->load();
    i2l = i2r = o2l = o2r = std::pow(10.0f, 2.0f * param6 - 1.0f);

    // Output routing. This changes the gain levels for the dry (`i2l`, `i2r`)
    // and wet (`o2l`, `o2r`) signals for the left and right channels.
    int mode = int(apvts.getRawParameterValue("Mode")->load());
    switch (mode) {
        case  0: i2l  =  0.0f;  i2r  =  0.0f; break;  // NORMAL
        case  1: o2l *= -1.0f;  o2r *= -1.0f; break;  // INVERSE
        case  2: i2l  =  0.0f;  o2r *= -1.0f; break;  // NORM INV
        default: o2l *= -1.0f;  i2r  =  0.0f; break;  // INV NORM
    }
}
void MDASplitterAudioProcessor::processBlock(mm_dsp::AudioBuffer<float> &buffer, mm_dsp::MidiBuffer &midiMessages)
{
    mm_dsp::ScopedNoDenormals noDenormals;
    auto totalNumInputChannels = getTotalNumInputChannels();
    auto totalNumOutputChannels = getTotalNumOutputChannels();

    // Clear any output channels that don't contain input data.
    for (auto i = totalNumInputChannels; i < totalNumOutputChannels; ++i) {
        buffer.clear(i, 0, buffer.getNumSamples());
    }

    update();

    const float *in1 = buffer.getReadPointer(0);
    const float *in2 = buffer.getReadPointer(1);
    float *out1 = buffer.getWritePointer(0);
    float *out2 = buffer.getWritePointer(1);

    for (int i = 0; i < buffer.getNumSamples(); ++i) {
        float a = in1[i];
        float b = in2[i];

        // Frequency split. This applies a basic low-pass filter. Analyzing
        // it in PluginDoctor shows this has 12 dB/octave slope and a bit of
        // a resonant bump. Note that that filter coefficient `freq` only
        // roughly corresponds to the Hz from the parameter.
        a0 += freq * (a - a0 - a1);
        a1 += freq * a0;

        // If the setting is BELOW, `ff` is 0.0f. If the setting is ABOVE,
        // `ff` is -1.0f, which subtracts the original signal from the filtered
        // signal to create a high-pass filter instead. The phase response for
        // the high pass filter is a little strange because subtracting the
        // original input flips its polarity.
        // If the setting is ALL, the filter coefficient is fixed to a small
        // value giving the high-pass filter's a cutoff of 10 Hz, effectively
        // disabling the filter.
        float aa = a1 + ff * a;

        // Also frequency split the other channel.
        b0 += freq * (b - b0 - b1);
        b1 += freq * b0;
        float bb = b1 + ff * b;

        // Combine the two channels into a single amplitude value
        // and make it positive.
        float ee = aa + bb;
        if (ee < 0.0f) { ee = -ee; }

        // Level split. This is an envelope follower that is calculated on the
        // filtered sound. If the new amplitude exceeds the previous envelope
        // level, use the attack. Also always apply decay.
        if (ee > level) {
            env += att * (pp - env);
        }
        env *= rel;

        // The `pp` variable is the target for the envelope follower when in
        // attack mode. It's either -1.0 or 1.0, according to this table:
        //
        //    Freq mode | Level mode |  ff  |  ll  |  pp
        //    --------- | ---------- | ---- | ---- | ----
        //    ABOVE     | ABOVE      | -1.0 |  0.0 | -1.0
        //              | BELOW      | -1.0 | -1.0 |  1.0
        //              | ALL        | -1.0 |  0.0 | -1.0
        //    BELOW     | ABOVE      |  0.0 |  0.0 |  1.0
        //              | BELOW      |  0.0 |  1.0 | -1.0
        //              | ALL        |  0.0 |  0.0 |  1.0
        //    ALL       | ABOVE      | -1.0 |  0.0 | -1.0
        //              | BELOW      | -1.0 | -1.0 |  1.0
        //              | ALL        | -1.0 |  0.0 | -1.0
        //
        // The `ll` variable determines whether we select low levels (below the
        // `level` threshold) or high levels (above the `level` threshold).
        //
        // The envelope follower will create a positive envelope (between 0.0
        // and 1.0) for some combinations of the freq mode and level mode, and
        // a negative envelope (between -1.0 and 0.0) for other combinations.
        // In level mode ALL, `level` is 0.0 so that the amplitude always exceeds
        // it, making the envelope a flat line.
        //
        // The reason for using these `ff`, `ll`, `pp` helper variables is so
        // that we can support the BELOW & ABOVE modes without having to branch.

        // Calculate the final sample values. This first applies the envelope
        // to the filtered (wet) signal and then mixes it with the dry signal.
        // The dry/wet mix is determined by the mode:
        //  - NORMAL: wet only
        //  - INVERSE: wet is subtracted from dry
        //  - NORM INV: left channel wet only, right wet subtracted from dry
        //  - INV NORM: left wet subtracted from dry, right channel wet only
        //
        // Note that using freq ABOVE in NORMAL mode is the same as freq BELOW
        // in INVERSE mode, and vice versa (except maybe a polarity flip). Same
        // for the level mode.

        a = i2l * a + o2l * aa * (env + ll);
        b = i2r * b + o2r * bb * (env + ll);

        out1[i] = a;
        out2[i] = b;
    }
}
mm_dsp::AudioProcessorValueTreeState::ParameterLayout MDASplitterAudioProcessor::createParameterLayout()
{
    mm_dsp::AudioProcessorValueTreeState::ParameterLayout layout;

    layout.add(std::make_unique<mm_dsp::AudioParameterChoice>(
        mm_dsp::ParameterID("Mode", 1),
        "Mode",
        mm_dsp::StringArray { "NORMAL", "INVERSE", "NORM/INV", "INV/NORM" },
        0));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Freq", 1),
        "Freq",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.5f));

    layout.add(std::make_unique<mm_dsp::AudioParameterChoice>(
        mm_dsp::ParameterID("Freq SW", 1),
        "Freq SW",
        mm_dsp::StringArray { "BELOW", "ALL", "ABOVE" },
        0));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Level", 1),
        "Level",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.5f));

    layout.add(std::make_unique<mm_dsp::AudioParameterChoice>(
        mm_dsp::ParameterID("Level SW", 1),
        "Level SW",
        mm_dsp::StringArray { "BELOW", "ALL", "ABOVE" },
        1));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Envelope", 1),
        "Envelope",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.5f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Output", 1),
        "Output",
        mm_dsp::NormalisableRange<float>(0.0f, 1.0f),
        0.5f));

    return layout;
}

class MDAStereoAudioProcessor:public MdaBase { public:
MDAStereoAudioProcessor(){apvts.params=std::move(createParameterLayout().params);}
void prepareToPlay(double,int) override;void processBlock(mm_dsp::AudioBuffer<float>&,mm_dsp::MidiBuffer&) override;void reset();void releaseResources();
private:

    mm_dsp::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();

    void update();
    void resetState();

    // Used to calculate the release time in milliseconds in the UI.
    float _sampleRate;

    // This buffer stores the delayed samples (mono).
    std::vector<float> _delayBuffer;

    // Maximum length of the delay buffer in samples.
    int _delayMax;

    // Where we will write the next new sample value in the delay buffer.
    int _writePos;

    // Delay time in samples.
    float _delayTime;

    // Amount of delay modulation (0.0 = off).
    float _mod;

    // Current phase and phase increment for the sine wave that is used to
    // modulate the read position in the delay buffer.
    float _phase, _phaseInc;

    // Filter coefficients for l=left, r=right, i=input, d=delayed sample.
    float _fli, _fld, _fri, _frd;

    // Output level.
    float _gain;

    
};

void MDAStereoAudioProcessor::prepareToPlay(double sampleRate, int samplesPerBlock)
{
    // Store this in a variable so we can use it to format the parameters.
    _sampleRate = sampleRate;

    // Allocate the delay buffer. Because the maximum values of the Delay and
    // Mod parameters are 2100 samples, the maximum delay length is 4200 samples.
    // The delay buffer should be at least that size.
    // As explained in createParameterLayout(), expressing Delay and Mod in terms
    // of samples isn't the best choice, and this should really be independent of
    // the current sample rate.
    _delayMax = 4800;
    _delayBuffer.resize(_delayMax);

    resetState();
}
void MDAStereoAudioProcessor::releaseResources()
{
}
void MDAStereoAudioProcessor::reset()
{
    resetState();
}
void MDAStereoAudioProcessor::resetState()
{
    _phase = 0.0f;
    _writePos = 0;

    // Clear out the delay buffer.
    memset(_delayBuffer.data(), 0, _delayMax * sizeof(float));
}
void MDAStereoAudioProcessor::update()
{
    // The Rate parameter is displayed as going from 100 to 0.1 sec. That makes
    // it a period. Here, we convert the 0 - 1 value into 0.01 - 10 Hz, which is
    // a frequency, or 1 divided by the period that's being displayed.
    // The formula to go from a frequency to radians is 2*pi*freq / sampleRate,
    // but here the factor 2 is missing. That's probably because the waveform for
    // modulation isn't a regular sine wave but the absolute value of the sine,
    // which has twice the frequency. So we compensate for that factor 2 here.
    float fParam5 = apvts.getRawParameterValue("Rate")->load();
    _phaseInc = 3.141f * std::pow(10.0f, -2.0f + 3.0f * fParam5) / _sampleRate;

    // Delay time is between 20 and 2100 samples.
    float fParam2 = apvts.getRawParameterValue("Delay")->load();
    _delayTime = 20.0f + 2080.0f * std::pow(fParam2, 2.0f);

    // Modulation amount goes from 0 to 2100. In the audio callback, this is
    // added to the number of samples from the delay time. So the total delay
    // buffer size must be 2100 + 2100 = 4200. It's actually slightly larger.
    float fParam4 = apvts.getRawParameterValue("Mod")->load();
    _mod = 2100.0f * std::pow(fParam4, 2.0f);

    // The Width parameter is split into two halves that both go from 0 to 100.
    // The left half is for Haas panning; the right half is for comb filtering.
    float fParam1 = apvts.getRawParameterValue("Width")->load();
    if (fParam1 < 0.5f) {
        /*
          Haas panning: put the same sound on both sides but apply a short delay
          to only one of the sides (the right channel). This makes the sound appear
          to come from the left, so make the left channel quieter to compensate.

          If the Width parameter = 0 ("100 Haas" in the UI), the right channel is
          a purely delayed version of the input. As width goes from 0.0 to 0.5,
          the right channel acts more like a comb filter (see below).

          In Haas mode, only the signal in the right channel is modulated, since
          that is the only channel that uses the delay buffer.
         */
        _fli = 0.25f + 1.5f * fParam1;  // 0.25 - 1.0
        _fld = 0.0f;
        _fri = 2.0f * fParam1;          // 0.0 - 1.0
        _frd = 1.0f - _fri;             // 1.0 - 0.0
    } else {
        /*
          Comb filter mode:

          Summing the input with a delayed version of itself creates a so-called
          comb filter.

          This plugin uses a feed-forward comb filter with the difference equation:
          y(n) = fli * x(n) - fld * x(n - delay)  for the left channel, and
          y(n) = fri * x(n) - frd * x(n - delay)  for the right channel.

          Note that this formula is actually subtracting, not summing. However,
          that is only true for the left channel; for the right channel `frd`
          will be negative. This is what causes a 90-degree phase shift between
          the left and right channels.

          How this works: the amount of delay determines which frequencies get
          boosted and which get attenuated. Any frequencies that are a multiple
          of `sampleRate / delay` Hz will be doubled, while other frequencies get
          more suppressed the further away they are from one of these multiples.

          Because the left channel subtracts the delayed input rather than adds,
          its filtering frequencies are shifted by half the comb width. So when
          a frequency is boosted on the right channel, it is attenuated on the
          left channel, and vice versa. This is what pulls the sound apart into
          separate channels.

          The smaller delay is, the fewer combs there are in the spectrum and the
          coarser / softer this separation is. With a longer delay, there are more
          combs and the separation effect is more extreme. You can also start to
          hear the echo now.

          Modulating the delay length will change which frequencies get panned left
          vs. right, and it makes the sound appear to move between left and right.

          The Width parameter determines the mix between the current input sample
          and the delayed sample. If width = 0.5 then fli and fri are 1.0, and fld
          and frd are 0.0, so no actual filtering takes place. For larger values of
          width, fli and fri go down, while fld / frd go up. It's necessary to mix
          the two signals in this way otherwise their sum would have an amplitude
          that is twice as large as the input sound.
         */
        _fli = 1.5f - fParam1;  // 1.0 - 0.5
        _fld = 1.0f - _fli;     // 0.0 - 0.5
        _fri = _fli;
        _frd = -(_fld);
    }

    // If the Balance parameter is to the right, then attentuate the left filter
    // coefficients, and vice versa. This is a simple linear mix. It's especially
    // useful in Haas mode because that makes one side louder than the other.
    float fParam3 = apvts.getRawParameterValue("Balance")->load();
    if (fParam3 > 0.5f) {
        _fli *= (1.0f - fParam3) * 2.0f;
        _fld *= (1.0f - fParam3) * 2.0f;
    } else {
        _fri *= 2.0f * fParam3;
        _frd *= 2.0f * fParam3;
    }

    // I'm not entirely sure where this comes from, but it makes the mixing curve
    // between the current sample and the delayed sample less linear.
    _fri *= 0.5f + std::abs(fParam1 - 0.5f);
    _frd *= 0.5f + std::abs(fParam1 - 0.5f);
    _fli *= 0.5f + std::abs(fParam1 - 0.5f);
    _fld *= 0.5f + std::abs(fParam1 - 0.5f);

    // The output level is in decibels, so convert to a linear gain.
    float level = apvts.getRawParameterValue("Output")->load();
    _gain = mm_dsp::Decibels::decibelsToGain(level);
}
void MDAStereoAudioProcessor::processBlock(mm_dsp::AudioBuffer<float> &buffer, mm_dsp::MidiBuffer &midiMessages)
{
    mm_dsp::ScopedNoDenormals noDenormals;
    auto totalNumInputChannels = getTotalNumInputChannels();
    auto totalNumOutputChannels = getTotalNumOutputChannels();

    // Clear any output channels that don't contain input data.
    for (auto i = totalNumInputChannels; i < totalNumOutputChannels; ++i) {
        buffer.clear(i, 0, buffer.getNumSamples());
    }

    update();

    const float *in1 = buffer.getReadPointer(0);
    const float *in2 = buffer.getReadPointer(1);
    float *out1 = buffer.getWritePointer(0);
    float *out2 = buffer.getWritePointer(1);

    const int delayMax = _delayMax;
    const float delayTime = _delayTime;
    const float mod = _mod;
    const float phaseInc = _phaseInc;
    const float fli = _fli;
    const float fld = _fld;
    const float fri = _fri;
    const float frd = _frd;

    float phase = _phase;
    int writePos = _writePos;

    for (int i = 0; i < buffer.getNumSamples(); ++i) {
        // Sum the stereo channels into a mono signal. The goal of this plug-in
        // is to take a mono sound and widen it, but in its current form it only
        // accepts input from a stereo bus. Since the input is mono(-ish), it's
        // fair to assume both channels are mostly the same anyway.
        float a = in1[i] + in2[i];

        // Write the current input into the delay buffer. Note: because the delay
        // buffer is slightly larger than the maximum delay time this works fine,
        // but it would be better to first read from the buffer, then write into
        // it. Just in case the readPos wraps around to be the same as writePos.
        _delayBuffer[writePos] = a;

        // Figure out where to read from the delay buffer. We will read at least
        // delayTime samples after the write position. If modulation is enabled,
        // add an additional amount to the delay time (unipolar modulation) that
        // varies in the shape of a sine wave (but only the positive half).
        float readOffset = delayTime;
        if (mod > 0.0f) {
            readOffset += std::abs(mod * std::sin(phase));
            phase += phaseInc;
        }

        // Read a delayed sample from the delay buffer.
        int readPos = (writePos + int(readOffset)) % delayMax;
        float b = _delayBuffer[readPos];

        // Apply the filter. Each channel has its own filter coefficients, which
        // is what creates the stereo effect.
        float c = (a * fli) - (b * fld);   // left channel
        float d = (a * fri) - (b * frd);   // right channel

        // Update the write position for the next sample. Wrap around when we
        // reach the edge of the buffer.
        writePos -= 1;
        if (writePos < 0) writePos += delayMax;

        // The original plug-in didn't have this but I found that it's possible
        // to make the sound become too loud, so I added an output level parameter
        // to compensate the gain.
        out1[i] = c * _gain;
        out2[i] = d * _gain;
    }

    _writePos = writePos;

    // Wrap-around the phase so it stays within 0 - 2pi. It's possible the phase
    // already goes over 2pi in the loop, but that's OK, the sin() function can
    // handle this. We just don't want it to grow too big over time.
    _phase = std::fmod(phase, 6.2831853f);
}
mm_dsp::AudioProcessorValueTreeState::ParameterLayout MDAStereoAudioProcessor::createParameterLayout()
{
    mm_dsp::AudioProcessorValueTreeState::ParameterLayout layout;

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Width", 1),
        "Width",
        mm_dsp::NormalisableRange<float>(),
        0.78f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Balance", 1),
        "Balance",
        mm_dsp::NormalisableRange<float>(),
        0.5f));

    // The Delay and Mod parameters are displayed as milliseconds but the plug-in
    // actually treats them as number of samples (between 20 and 2100 samples for
    // Delay and 0 - 2100 samples for Mod). That's not ideal because it means the
    // max delay time becomes shorter when the project's sample rate is higher.
    // I think the assumption was that the sample rate would be at most 44100 or
    // 48000 Hz. It would better to let the user choose the delay and modulation
    // depth in milliseconds instead.

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Delay", 1),
        "Delay",
        mm_dsp::NormalisableRange<float>(),
        0.43f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Mod", 1),
        "Mod",
        mm_dsp::NormalisableRange<float>(),
        0.0f));

    // The Rate parameter is displayed as going from 100 to 0.1 sec in a
    // logaritmic fashion. This is the period of the sine wave that's used for
    // modulation, so 1 / frequency. Rather than doing pow(10, 2 - 3 * value),
    // with value going from 0 to 1, we could use a NormalisableRange from 100
    // to 0.1 with a skew factor. Or just display in Hz instead.

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Rate", 1),
        "Rate",
        mm_dsp::NormalisableRange<float>(),
        0.5f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Output", 1),
        "Output",
        mm_dsp::NormalisableRange<float>(-48.0, 0.0f, 0.1f),
        -6.0f));

    return layout;
}

class MDASubSynthAudioProcessor:public MdaBase { public:
MDASubSynthAudioProcessor(){apvts.params=std::move(createParameterLayout().params);}
void prepareToPlay(double,int) override;void processBlock(mm_dsp::AudioBuffer<float>&,mm_dsp::MidiBuffer&) override;void reset();void releaseResources();
private:

    mm_dsp::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();

    void update();
    void resetState();

    // Used to calculate the release time in milliseconds in the UI.
    float _sampleRate;

    // The kind of sub-bass sound to add.
    int _type;

    // Amount of synthesized low-frequency signal to be added.
    float _wet;

    // Reduces the level of the original signal.
    float _dry;

    // Threshold level. The lower this is, the more intense the effect.
    float _threshold;

    // Used to find the octave below the input frequency.
    float _sign, _phase;

    // Oscillator phase and phase increment, for "Key Osc" mode.
    float _oscPhase, _phaseInc;

    // Current envelope level for the oscillator.
    float _env;

    // Decay amount for "Key Osc" mode.
    float _decay;

    // Low-pass filter coefficients.
    float _filti, _filto;

    // Filter delays. We use the same filter four times.
    float _filt1, _filt2, _filt3, _filt4;

    
};

void MDASubSynthAudioProcessor::prepareToPlay(double sampleRate, int samplesPerBlock)
{
    // Store this in a variable so we can use it to format the parameters.
    _sampleRate = sampleRate;

    resetState();
}
void MDASubSynthAudioProcessor::releaseResources()
{
}
void MDASubSynthAudioProcessor::reset()
{
    resetState();
}
void MDASubSynthAudioProcessor::resetState()
{
    _oscPhase = 0.0f;
    _env = 0.0f;
    _filt1 = _filt2 = _filt3 = _filt4 = 0.0f;
}
void MDASubSynthAudioProcessor::update()
{
    // Reset these to their starting values.
    _sign = 1.0f;
    _phase = 1.0f;

    // What is the current mode? The first three modes -- Distort, Divide, and
    // Invert -- manipulate the input signal to enhance the bass frequencies.
    // The fourth mode, Key Osc, outputs a decaying sine wave.
    _type = apvts.getRawParameterValue("Type")->load();

    // The filter is a basic one-pole filter. The difference equation is:
    //   y(n + 1) = a * x(n) + (1 - a) * y(n)
    // The coefficient a goes from 0.001 to 0.1. This roughly corresponds to
    // the -3 dB frequency being at 10 - 320 Hz, but I'm not 100% convinced
    // these are correct numbers.
    // In "Key Osc" mode, the cut-off is set to a fixed frequency.
    float fParam3 = apvts.getRawParameterValue("Tune")->load();
    _filti = (_type == 3) ? 0.018f : std::pow(10.0f, -3.0f + (2.0f * fParam3));
    _filto = 1.0f - _filti;

    // In "Key Osc" mode, the Tune parameter sets the oscillator frequency.
    _phaseInc = 0.456159f * std::pow(10.0f, -2.5f + (1.5f * fParam3));

    // The wet/dry levels are percentages (not decibel levels).
    _wet = apvts.getRawParameterValue("Level")->load() * 0.01f;
    _dry = apvts.getRawParameterValue("Dry Mix")->load() * 0.01f;

    // The threshold parameter is in decibels, so convert this to linear gain.
    float fParam5 = apvts.getRawParameterValue("Thresh")->load();
    _threshold = mm_dsp::Decibels::decibelsToGain(fParam5);

    // In "Key Osc" mode, the plug-in creates a sine wave. This sine will decay
    // when the level of the input sound is below the threshold parameter.
    // The envelope is computed by env *= decay, making it an exponential decay.
    // Here, we convert the parameter into a value between 0.99 and 0.99999.
    // The smaller this value, the quicker the sound decays.
    float fParam6 = apvts.getRawParameterValue("Release")->load();
    _decay = 1.0f - std::pow(10.0f, -2.0f - (3.0f * fParam6));
}
void MDASubSynthAudioProcessor::processBlock(mm_dsp::AudioBuffer<float> &buffer, mm_dsp::MidiBuffer &midiMessages)
{
    mm_dsp::ScopedNoDenormals noDenormals;
    auto totalNumInputChannels = getTotalNumInputChannels();
    auto totalNumOutputChannels = getTotalNumOutputChannels();

    // Clear any output channels that don't contain input data.
    for (auto i = totalNumInputChannels; i < totalNumOutputChannels; ++i) {
        buffer.clear(i, 0, buffer.getNumSamples());
    }

    update();

    const float *in1 = buffer.getReadPointer(0);
    const float *in2 = buffer.getReadPointer(1);
    float *out1 = buffer.getWritePointer(0);
    float *out2 = buffer.getWritePointer(1);

    const int type = _type;
    const float phaseInc = _phaseInc;
    const float decay = _decay;
    const float threshold = _threshold;
    const float wet = _wet;
    const float dry = _dry;
    const float fi = _filti;
    const float fo = _filto;

    float sign = _sign;
    float phase = _phase;
    float osc = _oscPhase;
    float env = _env;
    float f1 = _filt1;
    float f2 = _filt2;
    float f3 = _filt3;
    float f4 = _filt4;

    for (int i = 0; i < buffer.getNumSamples(); ++i) {
        float a = in1[i];
        float b = in2[i];

        // Combine the two channels into one (stereo to mono) and low-pass filter
        // it twice. Filtering twice gives us a roll-off of 12 dB/octave.
        f1 = (fo * f1) + (fi * (a + b));
        f2 = (fo * f2) + (fi * f1);

        float sub = 0.0f;

        // Only in Distort, Divide, and Invert modes:
        if (type != 3) {
            // Turn the low-pass filtered input into a very harsh "binary" signal
            // that is either -1, 0, or +1, with nothing in between. The threshold
            // level, together with the amplitude of the input audio, determines how
            // extreme this effect is. Where the sound is louder than the threshold,
            // you get a series of "pulses" going between -1 and +1; where the sound
            // is softer, you get silence. This is like applying a limiter and also
            // sampling with a resolution of 2 bits. To reduce the intensity of this
            // plug-in's effect, move the threshold closer to 0 dB, so that there
            // are fewer of these "pulses".
            if (f2 > threshold) {
                sub = 1.0f;
            } else if (f2 < -threshold) {
                sub = -1.0f;
            } else {
                sub = 0.0f;
            }

            // In Distort mode, the above clipping to produce harmonics is all
            // that we do, followed by another series of low-pass filters below.
            // However, the Divide and Invert modes do additional processing.

            // Octave divider
            if (sub * sign < 0.0f) {
                sign = -sign;
                if (sign < 0.0f) phase = -phase;
            }

            // In Divide mode, multiply the distorted signal by the value of phase.
            // This causes the distorted signal to become an octave lower for extra
            // nastiness.
            if (type == 1) {
                sub = phase * sub;
            }

            /*
              How the above works:

              The value of sign is initially 1. The distorted signal "sub" is
              always -1, 0, or +1. When sign = 1, nothing happens for sub = 0
              or +1, because sub * sign is >= 0 in those cases. But when sub
              becomes -1, sub * sign is now < 0 and we enter the if-statement.

              There we flip the value of sign, making it -1. Phase is initially 1.
              Since sign becomes negative, phase becomes -1.

              The output signal is then the distorted signal, which was -1, times
              the value of phase, also -1. So the output is +1.

              While sub remains at -1 (or becomes 0), nothing changes: sub * sign
              is now 0 * -1 or -1 * -1, which both are >= 0.

              At some point, sub becomes +1 again. Now sub * sign is < 0 and we
              enter the if-statement: sign becomes +1, phase remains -1.

              The output signal is sub (+1) times phase (-1), which is -1.

              What happened so far is that the output signal is the inverse of
              the input signal. But that's not all, keep reading...

              While sub remains at +1 (or becomes 0), nothing changes.

              Eventually, sub becomes -1. Now sub * sign is < 0. We flip sign
              to -1, which causes phase to be flipped to +1. The output is now
              -1 * +1 = -1.

              Note that this two cycles of -1 in a row. Even though the signal
              went from - to + to -, the output stayed at -1.

              When sub becomes +1 again, we flip sign to +1, phase stays at +1.
              The output is +1.

              When sub becomes -1 again, sign flips to -1 and phase becomes -1.
              The output is -1 * -1 = +1.

              So now we've done two cycles where the output is +1. And so on...

              What happens is that the output stays high or low for twice as
              long as the input does. This makes its period twice as large and
              the frequency two times (i.e. one octave) lower.
             */

            // In Invert mode, we also apply the phase value to make the sound an
            // octave lower, but this time we use the filtered, undistorted input.
            // The factor 2 is to add a +6 dB boost.
            if (type == 2) {
                sub = phase * f2 * 2.0f;
            }
        }

        // Key Osc mode
        if (type == 3) {
            // As long as the low-pass filtered input sound is over the threshold,
            // output a sustained sine wave. But when the input level drops below
            // the threshold, decay the sine wave. This creates a "boom" sound when
            // new notes are played or drums are hit. (There is no attack on the
            // envelope, so this may have DC offset glitches, but these get smoothed
            // out by the low-pass filter that follows.)
            if (f2 > threshold) {
                env = 1.0f;
            } else {
                env = env * decay;  // exponential decay
            }

            // The sound itself is just a basic sine wave oscillator.
            sub = env * std::sin(osc);
            osc = std::fmod(osc + phaseInc, 6.283185f);
        }

        // Low-pass filter the sub-bass sound two more times using the same filter.
        // This is needed to get rid of discontinuities introduced above.
        f3 = (fo * f3) + (fi * sub);
        f4 = (fo * f4) + (fi * f3);

        // Mix the sub-bass signal with the original into the buffer.
        out1[i] = (a * dry) + (f4 * wet);
        out2[i] = (b * dry) + (f4 * wet);
    }

    // Fix numerical underflow.
    if (std::abs(f1) < 1.0e-10f) _filt1 = 0.0f; else _filt1 = f1;
    if (std::abs(f2) < 1.0e-10f) _filt2 = 0.0f; else _filt2 = f2;
    if (std::abs(f3) < 1.0e-10f) _filt3 = 0.0f; else _filt3 = f3;
    if (std::abs(f4) < 1.0e-10f) _filt4 = 0.0f; else _filt4 = f4;

    _sign = sign;
    _phase = phase;
    _oscPhase = osc;
    _env = env;
}
mm_dsp::AudioProcessorValueTreeState::ParameterLayout MDASubSynthAudioProcessor::createParameterLayout()
{
    mm_dsp::AudioProcessorValueTreeState::ParameterLayout layout;

    layout.add(std::make_unique<mm_dsp::AudioParameterChoice>(
        mm_dsp::ParameterID("Type", 1),
        "Type",
        mm_dsp::StringArray({ "Distort", "Divide", "Invert", "Key Osc." }),
        0));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Level", 1),
        "Level",
        mm_dsp::NormalisableRange<float>(0.0f, 100.0f, 0.01f),
        30.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Tune", 1),
        "Tune",
        mm_dsp::NormalisableRange<float>(),
        0.6f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Dry Mix", 1),
        "Dry Mix",
        mm_dsp::NormalisableRange<float>(0.0f, 100.0f, 0.01f),
        100.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Thresh", 1),
        "Thresh",
        mm_dsp::NormalisableRange<float>(-60.0f, 0.0f, 0.01f),
        -24.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Release", 1),
        "Release",
        mm_dsp::NormalisableRange<float>(),
        0.65f));

    return layout;
}

class MDATestToneAudioProcessor:public MdaBase { public:
MDATestToneAudioProcessor(){apvts.params=std::move(createParameterLayout().params);}
void prepareToPlay(double,int) override;void processBlock(mm_dsp::AudioBuffer<float>&,mm_dsp::MidiBuffer&) override;void reset();void releaseResources();
private:

    mm_dsp::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();

    std::atomic<bool> _parametersChanged { false };

    void update();
    void resetState();

    mm_dsp::String midi2string(float n);
    mm_dsp::String iso2string(float b);

    // The meaning of the F1 and F2 parameters changes based on the current mode.
    // What is displayed for the output level depends on the calibration setting.
    // These functions return the parameter value that is displayed to the user.
    mm_dsp::String stringFromValueF1(float value);
    mm_dsp::String stringFromValueF2(float value);
    mm_dsp::String stringFromValueOutputLevel(float value);
    mm_dsp::String stringFromValueCalibration(float value);

    // The currently selected mode.
    int _mode;

    // Gain for the left and right channels.
    float _left, _right;

    // Incoming audio is mixed into the generated sound at this gain level.
    float _thru;

    // Sweep time in seconds (1 - 32 sec).
    float _durationInSeconds;

    // Sweep time in number of samples.
    int _durationInSamples;

    // Remaining sweep time in number of samples.
    int _sweepRemaining;

    // Start and end frequencies for sweep.
    float _sweepStart, _sweepEnd;

    // Current sweep frequency.
    float _sweepFreq;

    // Step value for the frequency sweeps.
    float _sweepInc;

    // 2 pi / sample rate. Used for converting frequencies to radians.
    float _fscale;

    // Current phase and phase increment for sine wave oscillator.
    float _phase, _phaseInc;

    // Filter delay units for the pink noise filter.
    float _z0, _z1, _z2, _z3, _z4, _z5;

    
};

void MDATestToneAudioProcessor::prepareToPlay(double sampleRate, int samplesPerBlock)
{
    resetState();
    _parametersChanged.store(true);
}
void MDATestToneAudioProcessor::releaseResources()
{
}
void MDATestToneAudioProcessor::reset()
{
    resetState();
}
void MDATestToneAudioProcessor::resetState()
{
    // Reset the filter delays and the oscillator phase.
    _z0 = _z1 = _z2 = _z3 = _z4 = _z5 = _phase = 0.0f;
}
void MDATestToneAudioProcessor::update()
{
    _mode = int(apvts.getRawParameterValue("Mode")->load());

    // Level is -60 db to 0 dB.
    float fParam1 = apvts.getRawParameterValue("Level")->load();
    _left = mm_dsp::Decibels::decibelsToGain(fParam1);

    // For white noise and pink noise, the random generator outputs numbers
    // between ±16384, so scale these back to the range 0 - 1 (more or less).
    if (_mode == 2) _left *= 0.0000610f;  // white noise
    if (_mode == 3) _left *= 0.0000243f;  // pink noise

    // Using left channel, right channel, or both?
    float fParam2 = apvts.getRawParameterValue("Channel")->load();
    if (fParam2 < 0.3f) _right = 0.0f; else _right = _left;
    if (fParam2 > 0.6f) _left = 0.0f;

    // The sweep time parameter is in milliseconds. Convert to number of samples.
    float fParam6 = apvts.getRawParameterValue("Sweep")->load();
    _durationInSeconds = fParam6 / 1000.0f;
    _durationInSamples = int(_durationInSeconds * getSampleRate());
    _sweepRemaining = _durationInSamples;

    // Calibration: This code looks complicated but it just creates a slider that
    // goes from -20 dB to 0 dB; there's also a section between -1 dB and 0 dB
    // that goes in smaller steps.
    float fParam7 = apvts.getRawParameterValue("0dB =")->load();
    if (fParam7 > 0.8f) {  // -1 dB to 0 dB
        float cal;
        if (fParam7 > 0.96f) cal = 0.0f;
        else if (fParam7 > 0.92f) cal = -0.01000001f;
        else if (fParam7 > 0.88f) cal = -0.02000001f;
        else if (fParam7 > 0.84f) cal = -0.1f;
        else cal = -0.2f;

        // Convert decibels to a linear gain. Use this to trim the output level.
        cal = std::pow(10.0f, 0.05f * cal);
        _left *= cal;
        _right *= cal;
    } else {
        // Here, the slider is between -21 dB to -1 dB. We only use this to change
        // how the output level is displayed. There's nothing to calculate here.
    }

    // The meaning of F1 and F2 depends on the mode the user has chosen.
    float fParam3 = apvts.getRawParameterValue("F1")->load();
    float fParam4 = apvts.getRawParameterValue("F2")->load();

    // F2 is sometimes used to "fine tune" the value of F1. In that case, the F2
    // slider goes from -0.5 to +0.5. However, this is not entirely linear: there
    // is an area around the center (between 40% and 60%) that's always 0. Maybe
    // that was done to make it easier to set this parameter to zero.
    float df = 0.0f;
    if (fParam4 > 0.6) df = 1.25f*fParam4 - 0.75f;  // 0 to 0.5
    if (fParam4 < 0.4) df = 1.25f*fParam4 - 0.50f;  // -0.5 to 0

    // To get radians, multiply the frequency in Hz by 2 pi / sampleRate.
    _fscale = twopi / getSampleRate();

    switch (_mode) {
        // MIDI note:
        // F1 chooses the MIDI note number
        // F2 is used to tune the note up or down by ±50 cents
        case 0: {
            float f = std::floor(128.0f * fParam3);

            // The usual formula for converting a MIDI note number into a frequency
            // is 440 * 2^((note - 69) / 12). Then to find the phase increment, we
            // multiply by 2 pi and divide by the sample rate. That works out to be
            // exactly the same as the formula used here:
            _phaseInc = 51.37006f * std::pow(1.0594631f, f + df) / getSampleRate();
            break;
        }

        // Sine wave:
        // F1 chooses the frequency on a logaritmic scale
        // F2 fine-tunes that frequency by 2 semitones up or down
        case 5: {
            // Convert F1 from 0 - 1 into the range 13 - 43 (integers only), and then
            // turn it in to a frequency between 20 and 20000 Hz. Why these strange
            // numbers? This is from the ISO 266 standard for preferred frequencies
            // for acoustical measurements. The formula is 1000 * 10^(n / 10) where n
            // is an integer. For n = -17, the freq is 20 Hz; for n = 13, the freq is
            // 20 kHz. Here, that formula was rewritten so that param = 0 gives 20 Hz
            // and param = 1 gives 20 kHz. The new formula is 1000 * 10^((f - 30)/10)
            // where f is now between 13 and 43. The factor 1000 cancels out against
            // the -30/10 in the exponent, which leaves 10^(f / 10).
            float f = 13.0f + std::floor(30.0f * fParam3);
            f = std::pow(10.0f, 0.1f * (f + df));
            _phaseInc = _fscale * f;
            break;
        }

        // Log sweep & step:
        // F1 is the first frequency
        // F2 is the second frequency
        case 6:
        case 7:
            // Convert F1 and F2 from 0 - 1 into the range 13 - 43 (integers only),
            // but don't turn these numbers into real frequencies yet. In the audio
            // callback, we'll turn this into a frequency using pow(10, 0.1 * value).
            // That makes these variables log-frequencies, i.e. they are the base-10
            // logarithms of the actual frequencies, where 13 corresponds to 20 Hz
            // and 43 to 20 kHz. We do this to keep these values in log-space so that
            // interpolating from start to finish is done in logarithmic steps.
            _sweepStart = 13.0f + std::floor(30.0f * fParam3);
            _sweepEnd = 13.0f + std::floor(30.0f * fParam4);

            // Only sweep up. Swap the frequencies if start is lower than end.
            // (It shouldn't be very hard to change the plug-in so it allows sweeps
            // from high to low frequencies as well.)
            if (_sweepStart > _sweepEnd) {
                float tmp = _sweepEnd;
                _sweepEnd = _sweepStart;
                _sweepStart = tmp;
            }

            // In step mode, make sure to include the end frequency as the final step.
            if (_mode == 7) _sweepEnd += 1.0f;

            // How much do we need to increment the log-frequency on every sample?
            _sweepInc = (_sweepEnd - _sweepStart) / (_durationInSeconds * getSampleRate());
            _sweepFreq = _sweepStart;

            // Put two seconds of silence between sweeps.
            _sweepRemaining = _durationInSamples = 2 * int(getSampleRate());
            break;

        // Linear sweep:
        // F1 is the first frequency
        // F2 is the second frequency
        case 8:
            // Turn F1 and F2 directly into a frequency between 0 Hz and 20000 Hz.
            _sweepStart = 200.0f * std::floor(100.0f * fParam3);
            _sweepEnd = 200.0f * std::floor(100.0f * fParam4);

            // Only sweep up. Swap the frequencies if start is lower than end.
            if (_sweepStart > _sweepEnd) {
                float tmp = _sweepEnd;
                _sweepEnd = _sweepStart;
                _sweepStart = tmp;
            }

            // Convert the start / end frequencies to an angle in radians.
            _sweepStart = _fscale * _sweepStart;
            _sweepEnd = _fscale * _sweepEnd;
            _sweepFreq = _sweepStart;

            // We increment the angle by this step size on every sample. This will
            // linearly move the frequency of the tone from the start angle to the
            // end angle over the total duration of the sweep.
            _sweepInc = (_sweepEnd - _sweepStart) / (_durationInSeconds * getSampleRate());

            // Put two seconds of silence between sweeps.
            _sweepRemaining = _durationInSamples = 2 * int(getSampleRate());
            break;
    }

    // Audio thru determines the loudness of input audio in the mix. This is a
    // decibel value between -40 dB and 0 dB, so convert it to linear gain.
    float fParam5 = apvts.getRawParameterValue("Thru")->load();
    if (fParam5 == 0.0f) {
        _thru = 0.0f;  // no audio thru
    } else {
        _thru = mm_dsp::Decibels::decibelsToGain(fParam5);
    }
}
void MDATestToneAudioProcessor::processBlock(mm_dsp::AudioBuffer<float> &buffer, mm_dsp::MidiBuffer &midiMessages)
{
    mm_dsp::ScopedNoDenormals noDenormals;
    auto totalNumInputChannels = getTotalNumInputChannels();
    auto totalNumOutputChannels = getTotalNumOutputChannels();

    // Clear any output channels that don't contain input data.
    for (auto i = totalNumInputChannels; i < totalNumOutputChannels; ++i) {
        buffer.clear(i, 0, buffer.getNumSamples());
    }

    // Only recalculate when a parameter has changed.
    // The original plug-in used two counters for this, updateRx and updateTx.
    // Whenever a parameter changed, updateTx was incremented. At the start of
    // the audio processing code, it would call update() if updateRx != updateTx.
    // Inside update(), updateRx would be set equal to updateTx again. (The way
    // we do it is a bit more thread-safe.)
    bool expected = true;
    if (_parametersChanged.compare_exchange_strong(expected, false)) {
        update();
    }

    const float *in1 = buffer.getReadPointer(0);
    const float *in2 = buffer.getReadPointer(1);
    float *out1 = buffer.getWritePointer(0);
    float *out2 = buffer.getWritePointer(1);

    const int mode = _mode;
    const float thru = _thru;
    const float left = _left;
    const float right = _right;
    const float fscale = _fscale;
    const float sweepStart = _sweepStart;
    const float sweepEnd = _sweepEnd;
    const float step = _sweepInc;
    const int sweepDuration = _durationInSamples;

    int samplesRemaining = _sweepRemaining;
    float freq = _sweepFreq;
    float phase = _phase;
    float phaseInc = _phaseInc;
    float z0 = _z0, z1 = _z1, z2 = _z2, z3 = _z3, z4 = _z4, z5 = _z5;

    for (int i = 0; i < buffer.getNumSamples(); ++i) {
        float a = in1[i];
        float b = in2[i];

        float x = 0.0f;

        switch (mode) {
            // Impulse
            case 1:
                // This simply counts down. When the timer is zero, output a short
                // pulse and reset the timer. Otherwise, always output silence.
                if (samplesRemaining > 0) {
                    samplesRemaining--;
                    x = 0.0f;
                } else {
                    samplesRemaining = sweepDuration;
                    x = 1.0f;
                }
                break;

            // White noise and pink noise
            case 2:
            case 3:
                // The original plug-in assumed RAND_MAX is 32767, but that's
                // wrong now. It would be better to use mm_dsp::Random for this.
                x = float(randomInt()) / RAND_MAX * 32767 - 16384;

                // Filter the white noise to get pink noise.
                if (mode == 3) {
                    z0 = 0.997f * z0 + 0.029591f * x;
                    z1 = 0.985f * z1 + 0.032534f * x;
                    z2 = 0.950f * z2 + 0.048056f * x;
                    z3 = 0.850f * z3 + 0.090579f * x;
                    z4 = 0.620f * z4 + 0.108990f * x;
                    z5 = 0.250f * z5 + 0.255784f * x;
                    x = z0 + z1 + z2 + z3 + z4 + z5;
                }
                break;

            // Mute
            case 4:
                x = 0.0f;
                break;

            // Tones, for MIDI note / sine wave mode
            case 0:
            case 5:
                // Just a simple sine wave oscillator. Increment the phase using
                // the phase increment and wrap around when it reaches 2 pi.
                phase = std::fmod(phase + phaseInc, twopi);
                x = std::sin(phase);
                break;

            // Log sweep & step
            case 6:
            case 7:
                // Before the sweep starts, count down and output zeros.
                // This puts two seconds of silence between sweeps.
                if (samplesRemaining > 0) {
                    samplesRemaining--;
                    phase = 0.0f;
                    x = 0.0f;
                } else {
                    // Output sine value for the current frequency.
                    x = std::sin(phase);

                    // Increment the frequency in log-space.
                    freq += step;

                    // Turn the log-frequency into an actual frequency in radians.
                    if (mode == 7) {
                        // In step mode, round off the frequency value. This steps
                        // upwards by roughly 4 semitones (a major third interval).
                        phaseInc = fscale * std::pow(10.0f, 0.1f * float(int(freq)));
                    } else {
                        phaseInc = fscale * std::pow(10.0f, 0.1f * freq);
                    }

                    // Use the frequency in radians to increment the oscillator phase.
                    phase = std::fmod(phase + phaseInc, twopi);

                    // If we've reached the end frequency, restart the sweep.
                    if (freq > sweepEnd) {
                        samplesRemaining = sweepDuration;
                        freq = sweepStart;
                    }
                }
                break;

            // Linear sweep
            case 8:
                // Before the sweep starts, count down and output zeros.
                // This puts two seconds of silence between sweeps.
                if (samplesRemaining > 0) {
                    samplesRemaining--;
                    phase = 0.0f;
                    x = 0.0f;
                } else {
                    // Output sine value for the current frequency.
                    x = std::sin(phase);

                    // Increment the phase linearly and wrap around at 2 pi.
                    freq += step;
                    phase = std::fmod(phase + freq, twopi);

                    // If we've reached the end frequency, restart the sweep.
                    if (freq > sweepEnd) {
                        samplesRemaining = sweepDuration;
                        freq = sweepStart;
                    }
                }
                break;
        }

        // Mix the incoming audio signal with the sound we produced.
        out1[i] = thru*a + left*x;
        out2[i] = thru*b + right*x;
    }

    _phase = phase;
    _sweepFreq = freq;
    _sweepRemaining = samplesRemaining;
    _z0 = z0; _z1 = z1; _z2 = z2; _z3 = z3; _z4 = z4; _z5 = z5;
}
mm_dsp::AudioProcessorValueTreeState::ParameterLayout MDATestToneAudioProcessor::createParameterLayout()
{
    mm_dsp::AudioProcessorValueTreeState::ParameterLayout layout;

    // The UI for this plug-in takes a little getting used to. Not all parameters
    // are relevant to all modes, and some parameters change meaning based on mode.

    layout.add(std::make_unique<mm_dsp::AudioParameterChoice>(
        mm_dsp::ParameterID("Mode", 1),
        "Mode",
        mm_dsp::StringArray({ "MIDI #", "IMPULSE", "WHITE", "PINK", "---",
                            "SINE", "LOG SWP.", "LOG STEP", "LIN SWP." }),
        4));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Level", 1),
        "Level",
        mm_dsp::NormalisableRange<float>(-60.0f, 0.0f, 0.01f),
        -16.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Channel", 1),
        "Channel",
        mm_dsp::NormalisableRange<float>(),
        0.5f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("F1", 1),
        "F1",
        mm_dsp::NormalisableRange<float>(),
        0.57f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("F2", 1),
        "F2",
        mm_dsp::NormalisableRange<float>(),
        0.50f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Sweep", 1),
        "Sweep",
        mm_dsp::NormalisableRange<float>(1000.0f, 32000.0f, 500.0f),
        10000.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("Thru", 1),
        "Thru",
        mm_dsp::NormalisableRange<float>(-40.0f, 0.0f, 0.01f),
        0.0f));

    layout.add(std::make_unique<mm_dsp::AudioParameterFloat>(
        mm_dsp::ParameterID("0dB =", 1),
        "0dB =",
        mm_dsp::NormalisableRange<float>(),
        1.0f));

    return layout;
}

inline std::unique_ptr<MdaBase> createMda(int index){switch(index){
case 0:return std::make_unique<MDAAmbienceAudioProcessor>();
case 1:return std::make_unique<MDABandistoAudioProcessor>();
case 2:return std::make_unique<MDABeatBoxAudioProcessor>();
case 3:return std::make_unique<MDADegradeAudioProcessor>();
case 4:return std::make_unique<MDADelayAudioProcessor>();
case 5:return std::make_unique<MDADetuneAudioProcessor>();
case 6:return std::make_unique<MDADitherAudioProcessor>();
case 7:return std::make_unique<MDADynamicsAudioProcessor>();
case 8:return std::make_unique<MDAEnvelopeAudioProcessor>();
case 9:return std::make_unique<MDAImageAudioProcessor>();
case 10:return std::make_unique<MDALimiterAudioProcessor>();
case 11:return std::make_unique<MDALoudnessAudioProcessor>();
case 12:return std::make_unique<MDAOverdriveAudioProcessor>();
case 13:return std::make_unique<MDARezFilterAudioProcessor>();
case 14:return std::make_unique<MDARingModAudioProcessor>();
case 15:return std::make_unique<MDAShepardAudioProcessor>();
case 16:return std::make_unique<MDASplitterAudioProcessor>();
case 17:return std::make_unique<MDAStereoAudioProcessor>();
case 18:return std::make_unique<MDASubSynthAudioProcessor>();
case 19:return std::make_unique<MDATestToneAudioProcessor>();
default:return nullptr;}}

static const char* mdaNames[]={"MDA Ambience","MDA Bandisto","MDA BeatBox","MDA Degrade","MDA Delay","MDA Detune","MDA Dither","MDA Dynamics","MDA Envelope","MDA Image","MDA Limiter","MDA Loudness","MDA Overdrive","MDA RezFilter","MDA RingMod","MDA Shepard","MDA Splitter","MDA Stereo","MDA SubSynth","MDA TestTone"};
