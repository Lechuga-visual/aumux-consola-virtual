#include "PluginProcessor.h"

MDADitherAudioProcessor::MDADitherAudioProcessor()
: AudioProcessor(BusesProperties()
                 .withInput ("Input",  juce::AudioChannelSet::stereo(), true)
                 .withOutput("Output", juce::AudioChannelSet::stereo(), true))
{
}

MDADitherAudioProcessor::~MDADitherAudioProcessor()
{
}

const juce::String MDADitherAudioProcessor::getName() const
{
    return JucePlugin_Name;
}

int MDADitherAudioProcessor::getNumPrograms()
{
    return 1;
}

int MDADitherAudioProcessor::getCurrentProgram()
{
    return 0;
}

void MDADitherAudioProcessor::setCurrentProgram(int index)
{
}

const juce::String MDADitherAudioProcessor::getProgramName(int index)
{
    return {};
}

void MDADitherAudioProcessor::changeProgramName(int index, const juce::String &newName)
{
}

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

bool MDADitherAudioProcessor::isBusesLayoutSupported(const BusesLayout &layouts) const
{
    return layouts.getMainOutputChannelSet() == juce::AudioChannelSet::stereo();
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

void MDADitherAudioProcessor::processBlock(juce::AudioBuffer<float> &buffer, juce::MidiBuffer &midiMessages)
{
    juce::ScopedNoDenormals noDenormals;
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
            r4 = rand() & 0x7FFF;
            r2 = (r4 & 0x7F) << 8;
        }

        // Calculate the second random number.
        r1 = rand() & 0x7FFF;
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

juce::AudioProcessorEditor *MDADitherAudioProcessor::createEditor()
{
    return new juce::GenericAudioProcessorEditor(*this);
}

void MDADitherAudioProcessor::getStateInformation(juce::MemoryBlock &destData)
{
    copyXmlToBinary(*apvts.copyState().createXml(), destData);
}

void MDADitherAudioProcessor::setStateInformation(const void *data, int sizeInBytes)
{
    std::unique_ptr<juce::XmlElement> xml(getXmlFromBinary(data, sizeInBytes));
    if (xml.get() != nullptr && xml->hasTagName(apvts.state.getType())) {
        apvts.replaceState(juce::ValueTree::fromXml(*xml));
    }
}

juce::AudioProcessorValueTreeState::ParameterLayout MDADitherAudioProcessor::createParameterLayout()
{
    juce::AudioProcessorValueTreeState::ParameterLayout layout;

    layout.add(std::make_unique<juce::AudioParameterFloat>(
        juce::ParameterID("Word Len", 1),
        "Word Len",
        juce::NormalisableRange<float>(0.0f, 1.0f),
        0.5f,
        juce::AudioParameterFloatAttributes()
            .withLabel("bits")
            .withStringFromValueFunction([this](float value, int)
            {
                float bits = 8.0f + 2.0f * (float)floor(8.9f * value);
                return juce::String(int(bits));
            })));

    layout.add(std::make_unique<juce::AudioParameterChoice>(
        juce::ParameterID("Dither", 1),
        "Dither",
        juce::StringArray { "OFF", "TRI", "HP-TRI", "N-SHAPE" },
        2));

    layout.add(std::make_unique<juce::AudioParameterFloat>(
        juce::ParameterID("Dith Amp", 1),
        "Dith Amp",
        juce::NormalisableRange<float>(0.0f, 1.0f),
        0.5f,
        juce::AudioParameterFloatAttributes()
            .withLabel("lsb")
            .withStringFromValueFunction([this](float value, int)
            {
                return juce::String(4.0f * value, 2);
            })));

    layout.add(std::make_unique<juce::AudioParameterFloat>(
        juce::ParameterID("DC Trim", 1),
        "DC Trim",
        juce::NormalisableRange<float>(0.0f, 1.0f),
        0.5f,
        juce::AudioParameterFloatAttributes()
            .withLabel("lsb")
            .withStringFromValueFunction([this](float value, int)
            {
                return juce::String(4.0f * value - 2.0f, 2);
            })));

    layout.add(std::make_unique<juce::AudioParameterFloat>(
        juce::ParameterID("Zoom...", 1),
        "Zoom...",
        juce::NormalisableRange<float>(0.0f, 1.0f),
        0.0f,
        juce::AudioParameterFloatAttributes()
            .withLabel("dB")
            .withStringFromValueFunction([this](float value, int)
            {
                if (value > 0.1f) {
                    float gain = (1.0f - value)*(1.0f - value);
                    if (gain < 0.0001f) {
                        return juce::String("-80");
                    } else {
                        return juce::String(20.0f * std::log10(gain));
                    }
                } else {
                    return juce::String("OFF");
                }
            })));

    return layout;
}

juce::AudioProcessor *JUCE_CALLTYPE createPluginFilter()
{
    return new MDADitherAudioProcessor();
}
