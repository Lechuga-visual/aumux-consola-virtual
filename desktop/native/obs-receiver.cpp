// SPDX-License-Identifier: GPL-2.0-or-later
// Receives Monitor Mix's stereo master on loopback UDP 17840.
#ifdef _WIN32
#include <winsock2.h>
#include <ws2tcpip.h>
using Socket = SOCKET;
static constexpr Socket invalid_socket = INVALID_SOCKET;
static void close_socket(Socket s) { closesocket(s); }
#else
#include <arpa/inet.h>
#include <sys/socket.h>
#include <unistd.h>
using Socket = int;
static constexpr Socket invalid_socket = -1;
static void close_socket(Socket s) { close(s); }
#endif
#include <obs-module.h>
#include <util/platform.h>
#include <array>
#include <atomic>
#include <cmath>
#include <cstring>
#include <memory>
#include <thread>

OBS_DECLARE_MODULE()

namespace {
uint32_t u32(const unsigned char* p) {
    return uint32_t(p[0]) | uint32_t(p[1]) << 8 | uint32_t(p[2]) << 16 | uint32_t(p[3]) << 24;
}
struct Receiver {
    obs_source_t* source;
    Socket socket = invalid_socket;
    std::atomic<bool> running{true};
    std::thread thread;
    explicit Receiver(obs_source_t* s) : source(s) {}
    ~Receiver() {
        running.store(false);
        if (thread.joinable()) thread.join();
        if (socket != invalid_socket) close_socket(socket);
    }
    void receive() {
        std::array<unsigned char, 2065> bytes{};
        std::array<float, 512> audio{};
        uint64_t next = 0, last_arrival = 0;
        uint32_t last_rate = 0;
        while (running.load()) {
            fd_set readers; FD_ZERO(&readers); FD_SET(socket, &readers);
            timeval timeout{0, 50000};
            if (select(int(socket + 1), &readers, nullptr, nullptr, &timeout) <= 0) continue;
            const auto size = recv(socket, reinterpret_cast<char*>(bytes.data()), int(bytes.size()), 0);
            if (size < 16 || std::memcmp(bytes.data(), "MMX1", 4) != 0) continue;
            const auto rate = u32(bytes.data() + 8), frames = u32(bytes.data() + 12);
            if ((rate != 44100 && rate != 48000 && rate != 88200 && rate != 96000)
                || !frames || frames > 256 || size != int(16 + frames * 8)) continue;
            for (unsigned i = 0; i < frames * 2; ++i) {
                const auto bits = u32(bytes.data() + 16 + i * 4);
                float value; std::memcpy(&value, &bits, 4);
                audio[i] = std::isfinite(value) ? std::max(-1.0f, std::min(1.0f, value)) : 0.0f;
            }
            const auto now = os_gettime_ns();
            if (!next || last_rate != rate || now - last_arrival > 100000000ULL
                || next + 100000000ULL < now || next > now + 100000000ULL) next = now;
            last_arrival = now; last_rate = rate;
            obs_source_audio packet{};
            packet.data[0] = reinterpret_cast<const uint8_t*>(audio.data());
            packet.frames = frames; packet.speakers = SPEAKERS_STEREO;
            packet.format = AUDIO_FORMAT_FLOAT; packet.samples_per_sec = rate; packet.timestamp = next;
            obs_source_output_audio(source, &packet);
            next += uint64_t(frames) * 1000000000ULL / rate;
        }
    }
};
const char* name(void*) { return "Monitor Mix — master local"; }
void* create(obs_data_t*, obs_source_t* source) {
    try {
        auto r = std::make_unique<Receiver>(source);
        r->socket = ::socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
        if (r->socket == invalid_socket) return nullptr;
#ifdef _WIN32
        BOOL exclusive = TRUE;
        setsockopt(r->socket, SOL_SOCKET, SO_EXCLUSIVEADDRUSE, reinterpret_cast<const char*>(&exclusive), sizeof(exclusive));
#endif
        sockaddr_in addr{}; addr.sin_family = AF_INET; addr.sin_port = htons(17840);
        addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        if (bind(r->socket, reinterpret_cast<sockaddr*>(&addr), sizeof(addr)) != 0) {
            blog(LOG_ERROR, "[Monitor Mix] UDP 17840 is occupied. Reuse the existing source instead of creating another.");
            return nullptr;
        }
        r->thread = std::thread([p = r.get()] { p->receive(); });
        return r.release();
    } catch (...) { blog(LOG_ERROR, "[Monitor Mix] Cannot create receiver"); return nullptr; }
}
void destroy(void* data) { delete static_cast<Receiver*>(data); }
}
MODULE_EXPORT bool obs_module_load() {
#ifdef _WIN32
    WSADATA wsa{};
    if (WSAStartup(MAKEWORD(2,2), &wsa) != 0) return false;
#endif
    obs_source_info info{};
    info.id = "monitor_mix_receiver"; info.type = OBS_SOURCE_TYPE_INPUT;
    info.output_flags = OBS_SOURCE_AUDIO; info.get_name = name; info.create = create; info.destroy = destroy;
    obs_register_source(&info);
    return true;
}
MODULE_EXPORT void obs_module_unload() {
#ifdef _WIN32
    WSACleanup();
#endif
}
MODULE_EXPORT const char* obs_module_description() { return "Stereo master from the local Monitor Mix prototype"; }
