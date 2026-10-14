#pragma once

#include <cstddef>
#include <span>
#include <vector>

namespace audio_obs {

enum class Tap { pre_fader, post_fader };

struct Send {
    float gain = 0.0f;
    Tap tap = Tap::post_fader;
};

struct Channel {
    float gain = 1.0f;
    float pan = 0.0f; // -1 left, 0 center, +1 right; equal-power law
    bool mute = false; // Mutes every bus, including pre-fader sends.
    std::vector<Send> sends;
};

struct Bus {
    float gain = 1.0f;
    bool mute = false;
};

struct StereoBuffer {
    std::span<float> left;
    std::span<float> right;
};

// Portable routing core. Mono input channels feed stereo master/aux buses.
// Configure only between process() calls on the owning audio thread. A future
// OBS adapter must deliver UI changes through a bounded command queue.
class Mixer {
public:
    Mixer(std::size_t channels, std::size_t auxiliaries);
    void set_channel(std::size_t index, const Channel& settings);
    void set_master(Bus settings);
    void set_auxiliary(std::size_t index, Bus settings);

    // Allocation/lock-free. Buffers must not alias each other or the inputs.
    // A null input pointer is silence. All non-null pointers must address
    // at least frames samples. Returns false without writes for bad sizes.
    [[nodiscard]] bool process(std::span<const float* const> inputs,
                               std::size_t frames,
                               StereoBuffer master,
                               std::span<const StereoBuffer> auxiliaries) const noexcept;

private:
    struct State {
        Channel channel;
        float left = 0.0f;
        float right = 0.0f;
    };
    std::vector<State> channels_;
    Bus master_;
    std::vector<Bus> auxiliaries_;
};

} // namespace audio_obs
