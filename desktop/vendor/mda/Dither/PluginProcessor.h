#pragma once

#include <JuceHeader.h>

class MDADitherAudioProcessor : public juce::AudioProcessor
{
public:
    MDADitherAudioProcessor();
    ~MDADitherAudioProcessor() override;

    void prepareToPlay(double sampleRate, int samplesPerBlock) override;
    void releaseResources() override;
    void reset() override;

    bool isBusesLayoutSupported(const BusesLayout &layouts) const override;

    void processBlock(juce::AudioBuffer<float> &, juce::MidiBuffer &) override;

    juce::AudioProcessorEditor *createEditor() override;
    bool hasEditor() const override { return true; }

    const juce::String getName() const override;

    bool acceptsMidi() const override { return false; }
    bool producesMidi() const override { return false; }
    bool isMidiEffect() const override { return false; }
    double getTailLengthSeconds() const override { return 0.0; }

    int getNumPrograms() override;
    int getCurrentProgram() override;
    void setCurrentProgram(int index) override;
    const juce::String getProgramName(int index) override;
    void changeProgramName(int index, const juce::String &newName) override;

    void getStateInformation(juce::MemoryBlock &destData) override;
    void setStateInformation(const void *data, int sizeInBytes) override;

    juce::AudioProcessorValueTreeState apvts { *this, nullptr, "Parameters", createParameterLayout() };

private:
    juce::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();

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

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR(MDADitherAudioProcessor)
};
