#include "audio_obs/mixer.hpp"

#include <algorithm>
#include <cmath>
#include <numbers>
#include <stdexcept>

namespace audio_obs {
namespace {
void validate_gain(float value)
{
    if (!std::isfinite(value) || value < 0.0f || value > 4.0f)
        throw std::invalid_argument("gain must be finite and between 0 and 4");
}

bool fits(StereoBuffer buffer, std::size_t frames) noexcept
{
    return buffer.left.size() >= frames && buffer.right.size() >= frames;
}

void clear(StereoBuffer buffer, std::size_t frames) noexcept
{
    std::fill_n(buffer.left.begin(), frames, 0.0f);
    std::fill_n(buffer.right.begin(), frames, 0.0f);
}
} // namespace

Mixer::Mixer(std::size_t channels, std::size_t auxiliaries)
    : channels_(channels), auxiliaries_(auxiliaries)
{
    for (std::size_t index = 0; index < channels; ++index) {
        Channel channel;
        channel.sends.resize(auxiliaries);
        set_channel(index, channel);
    }
}

void Mixer::set_channel(std::size_t index, const Channel& settings)
{
    auto& state = channels_.at(index);
    validate_gain(settings.gain);
    if (!std::isfinite(settings.pan) || std::abs(settings.pan) > 1.0f)
        throw std::invalid_argument("pan must be between -1 and 1");
    if (settings.sends.size() != auxiliaries_.size())
        throw std::invalid_argument("one send per auxiliary is required");
    for (const auto& send : settings.sends) {
        validate_gain(send.gain);
        if (send.tap != Tap::pre_fader && send.tap != Tap::post_fader)
            throw std::invalid_argument("invalid send tap");
    }
    state.channel = settings;
    const float angle = (settings.pan + 1.0f) * std::numbers::pi_v<float> / 4.0f;
    state.left = std::cos(angle);
    state.right = std::sin(angle);
}

void Mixer::set_master(Bus settings)
{
    validate_gain(settings.gain);
    master_ = settings;
}

void Mixer::set_auxiliary(std::size_t index, Bus settings)
{
    validate_gain(settings.gain);
    auxiliaries_.at(index) = settings;
}

bool Mixer::process(std::span<const float* const> inputs,
                    std::size_t frames, StereoBuffer master,
                    std::span<const StereoBuffer> auxiliaries) const noexcept
{
    if (inputs.size() != channels_.size() || auxiliaries.size() != auxiliaries_.size()
        || !fits(master, frames))
        return false;
    for (auto output : auxiliaries)
        if (!fits(output, frames))
            return false;

    clear(master, frames);
    for (auto output : auxiliaries)
        clear(output, frames);

    for (std::size_t channel_index = 0; channel_index < channels_.size(); ++channel_index) {
        const auto& state = channels_[channel_index];
        const auto& channel = state.channel;
        const auto* input = inputs[channel_index];
        if (channel.mute || input == nullptr)
            continue;
        if (!master_.mute) {
            const float gain = channel.gain * master_.gain;
            for (std::size_t frame = 0; frame < frames; ++frame) {
                master.left[frame] += input[frame] * gain * state.left;
                master.right[frame] += input[frame] * gain * state.right;
            }
        }
        for (std::size_t bus_index = 0; bus_index < auxiliaries.size(); ++bus_index) {
            const auto& bus = auxiliaries_[bus_index];
            const auto& send = channel.sends[bus_index];
            if (bus.mute || send.gain == 0.0f)
                continue;
            const float gain = send.gain * bus.gain *
                (send.tap == Tap::post_fader ? channel.gain : 1.0f);
            const auto output = auxiliaries[bus_index];
            for (std::size_t frame = 0; frame < frames; ++frame) {
                output.left[frame] += input[frame] * gain * state.left;
                output.right[frame] += input[frame] * gain * state.right;
            }
        }
    }
    return true;
}
} // namespace audio_obs
