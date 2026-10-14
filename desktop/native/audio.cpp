#include "RtAudio.h"
#include <algorithm>
#include <cstdio>
#include <cstring>
#include <memory>
#include <string>
#include <vector>

using Callback = int (*)(float*, const float*, unsigned, unsigned, void*);
struct Handle {
#ifdef _WIN32
    RtAudio audio;
    explicit Handle(bool system) : audio(system ? RtAudio::WINDOWS_WASAPI : RtAudio::WINDOWS_ASIO) {}
#else
    RtAudio audio{RtAudio::LINUX_ALSA};
    explicit Handle(bool) {}
#endif
    Callback callback{};
    void* user{};
    std::vector<unsigned> ids;
    std::vector<RtAudio::DeviceInfo> devices;
    std::string error;
};
struct Device {
    unsigned id, inputs, outputs;
    char name[512];
};

static int process(void* out, void* in, unsigned frames, double,
                   RtAudioStreamStatus status, void* opaque) {
    auto* h = static_cast<Handle*>(opaque);
    return h->callback(static_cast<float*>(out), static_cast<const float*>(in), frames, status, h->user);
}

extern "C" {
void* mm_create(bool system) noexcept {
    try { return new Handle(system); } catch (...) { return nullptr; }
}
void mm_close(void* opaque) noexcept {
    auto* h = static_cast<Handle*>(opaque);
    if (!h) return;
    try {
        if (h->audio.isStreamRunning()) h->audio.stopStream();
        if (h->audio.isStreamOpen()) h->audio.closeStream();
    } catch (...) {}
}
void mm_destroy(void* opaque) noexcept {
    mm_close(opaque);
    delete static_cast<Handle*>(opaque);
}
const char* mm_error(void* opaque) noexcept {
    return static_cast<Handle*>(opaque)->error.c_str();
}
int mm_enumerate(void* opaque) noexcept {
    auto* h = static_cast<Handle*>(opaque);
    try {
        h->ids = h->audio.getDeviceIds();
        h->devices.clear();
        for (auto id : h->ids) h->devices.push_back(h->audio.getDeviceInfo(id));
        return static_cast<int>(h->devices.size());
    } catch (const std::exception& e) { h->error = e.what(); return -1; }
    catch (...) { h->error = "Cannot enumerate audio devices"; return -1; }
}
bool mm_device(void* opaque, unsigned index, Device* result) noexcept {
    auto* h = static_cast<Handle*>(opaque);
    if (index >= h->devices.size()) return false;
    const auto& d = h->devices[index];
    *result = {};
    result->id = h->ids[index]; result->inputs = d.inputChannels; result->outputs = d.outputChannels;
    std::snprintf(result->name, sizeof(result->name), "%s", d.name.c_str());
    return true;
}
int mm_open(void* opaque, unsigned device, unsigned inputs, unsigned outputs,
            unsigned rate, unsigned* frames, Callback callback, void* user) noexcept {
    auto* h = static_cast<Handle*>(opaque);
    try {
        auto info = h->audio.getDeviceInfo(device);
        if (!inputs || !outputs || inputs > info.inputChannels || outputs > info.outputChannels) {
            h->error = "Requested channel count exceeds this device's available inputs/outputs"; return -1;
        }
        if (std::find(info.sampleRates.begin(), info.sampleRates.end(), rate) == info.sampleRates.end()) {
            h->error = "Sample rate is not reported as supported by this driver"; return -1;
        }
        RtAudio::StreamParameters in, out;
        in.deviceId = out.deviceId = device;
        in.nChannels = inputs; out.nChannels = outputs;
        h->callback = callback; h->user = user;
        const auto result = h->audio.openStream(&out, &in, RTAUDIO_FLOAT32, rate, frames, process, h);
        if (result != RTAUDIO_NO_ERROR) { h->error = h->audio.getErrorText(); return -1; }
        return 0;
    } catch (const std::exception& e) { h->error = e.what(); return -1; }
    catch (...) { h->error = "Cannot open audio stream"; return -1; }
}
int mm_start(void* opaque) noexcept {
    auto* h = static_cast<Handle*>(opaque);
    try {
        if (h->audio.startStream() == RTAUDIO_NO_ERROR) return 0;
        h->error = h->audio.getErrorText();
    } catch (const std::exception& e) { h->error = e.what(); }
    catch (...) { h->error = "Cannot start audio stream"; }
    return -1;
}
int mm_open_system(void* opaque, unsigned rate, unsigned* frames, Callback callback, void* user) noexcept {
    auto* h = static_cast<Handle*>(opaque);
    try {
        if (!h->audio.getDeviceCount()) { h->error = "No system audio output is available"; return -1; }
        const auto id = h->audio.getDefaultOutputDevice();
        const auto info = h->audio.getDeviceInfo(id);
        if (info.outputChannels < 2) { h->error = "The default output must support stereo"; return -1; }
        RtAudio::StreamParameters out;
        out.deviceId = id; out.nChannels = 2;
        h->callback = callback; h->user = user;
        if (h->audio.openStream(&out, nullptr, RTAUDIO_FLOAT32, rate, frames, process, h) != RTAUDIO_NO_ERROR) {
            h->error = h->audio.getErrorText(); return -1;
        }
        return 0;
    } catch (const std::exception& e) { h->error=e.what();return -1; }
    catch (...) { h->error="Cannot open the default Windows output";return -1; }
}
}
