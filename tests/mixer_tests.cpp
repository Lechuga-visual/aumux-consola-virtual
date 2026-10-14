#include "audio_obs/mixer.hpp"

#include <array>
#include <cmath>
#include <iostream>
#include <limits>
#include <stdexcept>

using namespace audio_obs;

void require(bool condition, const char* message)
{
    if (!condition)
        throw std::runtime_error(message);
}

void near(float actual, float expected)
{
    require(std::abs(actual - expected) < 0.00001f, "unexpected sample");
}

int main()
{
    try {
        Mixer mixer(2, 2);
        Channel voice;
        voice.pan = -1.0f;
        voice.gain = 0.25f;
        voice.sends = {{1.0f, Tap::pre_fader}, {0.5f, Tap::post_fader}};
        mixer.set_channel(0, voice);
        Channel music;
        music.pan = 1.0f;
        music.sends = {{0.5f, Tap::pre_fader}, {1.0f, Tap::post_fader}};
        mixer.set_channel(1, music);

        std::array<float, 3> input_voice{1.0f, -0.5f, 0.0f};
        std::array<float, 3> input_music{0.25f, 0.5f, -1.0f};
        std::array<const float*, 2> inputs{input_voice.data(), input_music.data()};
        std::array<float, 3> ml{}, mr{}, a0l{}, a0r{}, a1l{}, a1r{};
        StereoBuffer master{ml, mr};
        std::array<StereoBuffer, 2> aux{{{a0l, a0r}, {a1l, a1r}}};
        auto run = [&] { require(mixer.process(inputs, 3, master, aux), "processing rejected"); };

        run();
        for (std::size_t i = 0; i < 3; ++i) {
            near(ml[i], input_voice[i] * 0.25f);
            near(mr[i], input_music[i]);
            near(a0l[i], input_voice[i]);
            near(a0r[i], input_music[i] * 0.5f);
            near(a1l[i], input_voice[i] * 0.125f);
            near(a1r[i], input_music[i]);
        }
        std::cout << "PASS: two independent monitor mixes and stereo master\n";

        voice.gain = 0.0f;
        mixer.set_channel(0, voice);
        run();
        near(ml[0], 0.0f); near(a0l[0], 1.0f); near(a1l[0], 0.0f);
        std::cout << "PASS: pre-fader monitor survives master channel fader\n";

        voice.mute = true;
        mixer.set_channel(0, voice);
        run();
        near(a0l[0], 0.0f);
        std::cout << "PASS: channel mute silences pre-fader send\n";

        mixer.set_master({1.0f, true});
        run();
        near(mr[0], 0.0f); near(a1r[0], 0.25f);
        mixer.set_auxiliary(1, {1.0f, true});
        run();
        near(a1r[0], 0.0f); near(a0r[0], 0.125f);
        std::cout << "PASS: independent master and auxiliary mutes\n";

        mixer.set_auxiliary(0, {0.5f, false});
        run();
        near(a0r[0], 0.0625f);
        inputs[1] = nullptr;
        run();
        near(a0r[0], 0.0f);
        std::cout << "PASS: bus gain and disconnected input silence\n";

        ml.fill(42.0f);
        require(!mixer.process(inputs, 4, master, aux), "oversized block accepted");
        near(ml[0], 42.0f);
        require(!mixer.process(std::span(inputs).first(1), 3, master, aux), "wrong channel count accepted");
        require(!mixer.process(inputs, 3, master, std::span(aux).first(1)), "wrong bus count accepted");
        require(mixer.process(inputs, 0, master, aux), "empty block rejected");
        std::cout << "PASS: invalid blocks rejected without output writes\n";

        voice.gain = std::numeric_limits<float>::quiet_NaN();
        bool rejected = false;
        try { mixer.set_channel(0, voice); }
        catch (const std::invalid_argument&) { rejected = true; }
        require(rejected, "NaN gain accepted");
        std::cout << "PASS: invalid control value rejected\n";
    } catch (const std::exception& error) {
        std::cerr << "FAIL: " << error.what() << '\n';
        return 1;
    }
}
