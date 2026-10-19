// SPDX-License-Identifier: GPL-2.0-or-later
// Actual libobs: named stream isolation, shared UDP port, reordering, sync
// offset and teardown.
#include "audio-wire.h"
#include "obs-receiver-ui.h"
#include <arpa/inet.h>
#include <atomic>
#include <chrono>
#include <iostream>
#include <obs.h>
#include <stdexcept>
#include <sys/socket.h>
#include <thread>
#include <unistd.h>
extern "C" bool obs_module_load();
extern "C" void obs_module_unload();
static void check(bool value, const char *message) {
  if (!value)
    throw std::runtime_error(message);
}
struct Capture {
  std::atomic<unsigned> blocks{0}, bad{0};
  float level;
  explicit Capture(float v) : level(v) {}
};
static void capture(void *data, obs_source_t *, const audio_data *audio, bool) {
  auto *c = static_cast<Capture *>(data);
  auto *p = reinterpret_cast<const float *>(audio->data[0]);
  bool valid = false;
  for (unsigned i = 0; i < audio->frames; ++i)
    if (std::abs(p[i]) > 0.01) {
      valid = true;
      if (std::abs(std::abs(p[i]) - c->level) > 0.02)
        c->bad++;
    }
  if (valid)
    c->blocks++;
}
static void put32(unsigned char *p, uint32_t v) {
  for (unsigned i = 0; i < 4; ++i)
    p[i] = (v >> (i * 8)) & 255;
}
static void put64(unsigned char *p, uint64_t v) {
  put32(p, uint32_t(v));
  put32(p + 4, uint32_t(v >> 32));
}
static std::array<unsigned char, 1104> packet(const char *name,
                                              uint32_t sequence, float level) {
  std::array<unsigned char, 1104> p{};
  std::memcpy(p.data(), "AUM1", 4);
  p[4] = 1;
  p[5] = 2;
  p[6] = 80;
  put64(p.data() + 8, 42);
  put32(p.data() + 16, sequence);
  put32(p.data() + 20, 48000);
  p[24] = 128;
  put64(p.data() + 32, uint64_t(sequence) * 128);
  std::memcpy(p.data() + 40, name, std::strlen(name));
  for (unsigned i = 0; i < 256; ++i)
    std::memcpy(p.data() + 80 + i * 4, &level, 4);
  return p;
}
int main(int argc, char **argv) {
  try {
    check(obs_startup("en-US", nullptr, nullptr), "obs startup");
    obs_audio_info info{};
    info.samples_per_sec = 48000;
    info.speakers = SPEAKERS_STEREO;
    check(obs_reset_audio(&info) && obs_module_load(), "obs audio/module");
    if (argc == 3) {
      auto *settings = obs_data_create();
      obs_data_set_string(settings, "bind", "127.0.0.1");
      obs_data_set_int(settings, "port", std::stoi(argv[1]));
      obs_data_set_string(settings, "stream", argv[2]);
      obs_data_set_int(settings, "buffer_ms", 8);
      auto *source = obs_source_create_private("monitor_mix_receiver",
                                               "network capture", settings);
      obs_data_release(settings);
      check(source, "network source");
      Capture c(0.2f);
      obs_source_add_audio_capture_callback(source, capture, &c);
      std::cout << "READY: named libobs receiver" << std::endl;
      for (int i = 0; i < 500 && c.blocks < 4; ++i)
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
      obs_source_remove_audio_capture_callback(source, capture, &c);
      obs_source_release(source);
      obs_module_unload();
      obs_shutdown();
      check(c.blocks >= 4, "app network audio did not arrive");
      std::cout
          << "PASS: app bus -> AUM1 network packets -> actual libobs capture"
          << std::endl;
      return 0;
    }
    int probe = socket(AF_INET, SOCK_DGRAM, 0);
    sockaddr_in address{};
    address.sin_family = AF_INET;
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    check(bind(probe, reinterpret_cast<sockaddr *>(&address),
               sizeof(address)) == 0,
          "bind ephemeral");
    socklen_t length = sizeof(address);
    getsockname(probe, reinterpret_cast<sockaddr *>(&address), &length);
    const auto port = ntohs(address.sin_port);
    close(probe);
    auto create = [port](const char *name, int buffer) {
      auto *settings = obs_data_create();
      obs_data_set_string(settings, "bind", "127.0.0.1");
      obs_data_set_int(settings, "port", port);
      obs_data_set_string(settings, "stream", name);
      obs_data_set_int(settings, "buffer_ms", buffer);
      auto *s =
          obs_source_create_private("monitor_mix_receiver", name, settings);
      obs_data_release(settings);
      check(s, "source create");
      return s;
    };
    auto *a = create("VOZ", 24);
    auto *b = create("BANDA", 24);
    auto *duplicate = create("VOZ", 24);
    auto context = [](obs_source_t *s) {
      auto *p = obs_source_properties(s);
      auto *data = obs_properties_get_param(p);
      obs_properties_destroy(p);
      return data;
    };
    auto *ca = context(a);
    auto *cb = context(b);
    auto *cc = context(duplicate);
    Capture ac(0.2f), bc(0.4f), dc(0.2f);
    obs_source_add_audio_capture_callback(a, capture, &ac);
    obs_source_add_audio_capture_callback(b, capture, &bc);
    obs_source_add_audio_capture_callback(duplicate, capture, &dc);
    std::string error;
    auto config = aumux_settings(ca);
    config.delay_ms = 120;
    check(aumux_apply(ca, config, error), "set delay");
    check(obs_source_get_sync_offset(a) == 120000000LL,
          "shared native OBS offset");
    check(obs_source_get_sync_offset(b) == 0, "independent source delay");
    obs_source_set_sync_offset(a, 37000000LL);
    check(aumux_settings(ca).delay_ms == 37,
          "editor reads native offset changes");
    const int socket_ = socket(AF_INET, SOCK_DGRAM, 0);
    auto send = [&](const char *id, uint32_t seq, float level) {
      auto p = packet(id, seq, level);
      check(sendto(socket_, p.data(), p.size(), 0,
                   reinterpret_cast<sockaddr *>(&address),
                   sizeof(address)) == int(p.size()),
            "send");
    };
    sendto(socket_, "bad", 3, 0, reinterpret_cast<sockaddr *>(&address),
           sizeof(address));
    send("OTHER", 0, 0.9f);
    std::this_thread::sleep_for(std::chrono::milliseconds(20));
    check(ac.blocks == 0 && bc.blocks == 0, "unknown stream isolation");
    send("VOZ", 0, 0.2f);
    send("VOZ", 2, 0.2f);
    send("VOZ", 1, 0.2f);
    send("VOZ", 1, 0.2f);
    send("BANDA", 0, 0.4f);
    for (unsigned i = 3; i < 70; ++i) {
      send("VOZ", i, 0.2f);
      send("BANDA", i, 0.4f);
      std::this_thread::sleep_for(std::chrono::microseconds(2667));
    }
    std::this_thread::sleep_for(std::chrono::milliseconds(80));
    check(ac.blocks > 4 && bc.blocks > 4 && dc.blocks > 4,
          "all shared-port sources receive audio");
    check(!ac.bad && !bc.bad && !dc.bad, "streams are never mixed");
    auto as = aumux_stats(ca), bs = aumux_stats(cb);
    check(as.duplicates > 0, "duplicate packet counted");
    check(as.invalid > 0, "malformed packet counted");
    check(bs.missing_frames >= 256, "missing samples concealed at playout");
    check(as.received >= 69, "reordered packets accepted");
    // Destroy one subscriber; the remaining source must retain its shared
    // socket.
    obs_source_remove_audio_capture_callback(a, capture, &ac);
    obs_source_release(a);
    const auto before = bc.blocks.load();
    for (unsigned i = 110; i < 150; ++i) {
      send("BANDA", i, 0.4f);
      std::this_thread::sleep_for(std::chrono::microseconds(2667));
    }
    std::this_thread::sleep_for(std::chrono::milliseconds(60));
    check(bc.blocks > before, "other subscriber survives teardown");
    // A new epoch after a sender restart must resynchronize without replaying
    // old audio.
    auto restart = packet("BANDA", 0, 0.4f);
    put64(restart.data() + 8, 999);
    sendto(socket_, restart.data(), restart.size(), 0,
           reinterpret_cast<sockaddr *>(&address), sizeof(address));
    std::this_thread::sleep_for(std::chrono::milliseconds(10));
    check(aumux_stats(cb).resyncs > 1, "sender epoch restart");
    config = aumux_settings(cc);
    config.stream = "../bad";
    check(!aumux_apply(cc, config, error), "reject malformed settings");
    close(socket_);
    obs_source_remove_audio_capture_callback(b, capture, &bc);
    obs_source_remove_audio_capture_callback(duplicate, capture, &dc);
    obs_source_release(b);
    obs_source_release(duplicate);
    obs_module_unload();
    obs_shutdown();
    std::cout << "PASS: actual libobs named streams, shared socket fanout, "
                 "ordering/duplicates/loss concealment, native sync offset, "
                 "sender restart and subscriber teardown\n";
    return 0;
  } catch (const std::exception &e) {
    std::cerr << "FAIL: " << e.what() << "\n";
    return 1;
  }
}
