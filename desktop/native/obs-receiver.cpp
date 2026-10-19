// SPDX-License-Identifier: GPL-2.0-or-later
// AuMux Audio: local MMX1 compatibility and named AUM1 network streams.
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
#include "audio-wire.h"
#include "obs-receiver-ui.h"
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <map>
#include <memory>
#include <mutex>
#include <obs-module.h>
#include <thread>
#include <util/platform.h>
#include <vector>

OBS_DECLARE_MODULE()
namespace {
struct Receiver;
struct Hub {
  Socket socket = invalid_socket;
  std::atomic<bool> running{true};
  std::mutex mutex;
  std::vector<Receiver *> receivers;
  std::thread thread;
  ~Hub() {
    running = false;
    if (thread.joinable())
      thread.join();
    if (socket != invalid_socket)
      close_socket(socket);
  }
  void receive();
};
std::mutex hubs_mutex;
std::map<std::string, std::weak_ptr<Hub>> hubs;
std::shared_ptr<Hub> get_hub(const AumuxSettings &settings,
                             std::string &error) {
  const auto key = settings.bind + ":" + std::to_string(settings.port);
  std::lock_guard lock(hubs_mutex);
  if (auto h = hubs[key].lock())
    return h;
  auto h = std::make_shared<Hub>();
  h->socket = ::socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
  if (h->socket == invalid_socket) {
    error = "No se pudo crear el socket UDP";
    return {};
  }
#ifdef _WIN32
  BOOL exclusive = TRUE;
  setsockopt(h->socket, SOL_SOCKET, SO_EXCLUSIVEADDRUSE,
             reinterpret_cast<const char *>(&exclusive), sizeof(exclusive));
#endif
  sockaddr_in address{};
  address.sin_family = AF_INET;
  address.sin_port = htons(uint16_t(settings.port));
  if (inet_pton(AF_INET, settings.bind.c_str(), &address.sin_addr) != 1 ||
      bind(h->socket, reinterpret_cast<sockaddr *>(&address),
           sizeof(address))) {
    error =
        "No se pudo abrir " + key + ": puerto ocupado o interfaz no disponible";
    return {};
  }
  // Kernel buffering is bounded; the playout queue below always prioritizes
  // recent audio.
  int bytes = 256 * 1024;
  setsockopt(h->socket, SOL_SOCKET, SO_RCVBUF,
             reinterpret_cast<const char *>(&bytes), sizeof(bytes));
  h->thread = std::thread([p = h.get()] { p->receive(); });
  hubs[key] = h;
  for (auto i = hubs.begin(); i != hubs.end();)
    if (i->second.expired())
      i = hubs.erase(i);
    else
      ++i;
  return h;
}
struct Receiver {
  obs_source_t *source;
  AumuxSettings settings;
  std::shared_ptr<Hub> hub;
  mutable std::mutex mutex;
  std::condition_variable wake;
  std::map<uint64_t, aumux::Frame> pending;
  AumuxStats stats;
  bool running = true, started = false, played = false;
  uint64_t last_arrival = 0, epoch = 0, expected = 0, due = 0,
           legacy_sample = 0, last_ack = 0;
  std::string sender_key;
  uint32_t rate = 48000;
  sockaddr_in sender{};
  std::thread thread;
  Receiver(obs_source_t *source_, AumuxSettings settings_)
      : source(source_), settings(std::move(settings_)) {}
  ~Receiver() {
    if (hub) {
      std::lock_guard lock(hub->mutex);
      auto &v = hub->receivers;
      v.erase(std::remove(v.begin(), v.end(), this), v.end());
    }
    {
      std::lock_guard lock(mutex);
      running = false;
      wake.notify_all();
    }
    if (thread.joinable())
      thread.join();
  }
  void invalid() {
    std::lock_guard lock(mutex);
    ++stats.invalid;
  }
  void enqueue(aumux::Frame f, const sockaddr_in &address) {
    if (f.stream != settings.stream)
      return;
    char ip[64]{};
    inet_ntop(AF_INET, &address.sin_addr, ip, sizeof(ip));
    if (!settings.sender.empty() && settings.sender != ip)
      return;
    const auto now = os_gettime_ns();
    const auto key =
        std::string(ip) + ":" +
        std::to_string(f.legacy ? ntohs(address.sin_port) : f.publisher);
    std::lock_guard lock(mutex);
    if (started && key != sender_key && now - last_arrival < 500000000ULL) {
      ++stats.collisions;
      return;
    }
    if (!started || key != sender_key || f.epoch != epoch || rate != f.rate ||
        now - last_arrival > 500000000ULL) {
      pending.clear();
      started = true;
      played = false;
      epoch = f.epoch;
      sender_key = key;
      rate = f.rate;
      legacy_sample = 0;
      expected = f.legacy ? 0 : f.sample;
      due = now + uint64_t(settings.buffer_ms) * 1000000ULL;
      ++stats.resyncs;
    }
    if (f.legacy) {
      f.sample = legacy_sample;
      legacy_sample += f.frames;
    }
    if (played && f.sample + f.frames <= expected) {
      ++stats.late;
      return;
    }
    if (pending.count(f.sample)) {
      ++stats.duplicates;
      return;
    }
    if (pending.size() >= 64 ||
        (played && f.sample > expected + uint64_t(rate) / 5)) {
      pending.clear();
      expected = f.sample;
      due = now + uint64_t(settings.buffer_ms) * 1000000ULL;
      played = false;
      ++stats.resyncs;
    }
    if (!played)
      expected = std::min(expected, f.sample);
    ++stats.received;
    last_arrival = now;
    sender = address;
    stats.sender = ip;
    pending.emplace(f.sample, std::move(f));
    wake.notify_all();
  }
  void play() {
    std::unique_lock lock(mutex);
    while (running) {
      const auto now = os_gettime_ns();
      if (!started || now - last_arrival > 500000000ULL) {
        if (started) {
          pending.clear();
          started = false;
          stats.peak[0] = stats.peak[1] = stats.rms[0] = stats.rms[1] = 0;
        }
        wake.wait_for(lock, std::chrono::milliseconds(20));
        continue;
      }
      if (now < due) {
        wake.wait_for(lock, std::chrono::nanoseconds(due - now));
        continue;
      }
      // Recover from suspension without replaying a backlog at high speed.
      if (now > due + 50000000ULL && !pending.empty()) {
        auto newest = std::prev(pending.end());
        auto f = std::move(newest->second);
        pending.clear();
        expected = f.sample;
        pending.emplace(f.sample, std::move(f));
        due = now;
        ++stats.resyncs;
      }
      std::array<float, 512> audio{};
      uint32_t frames = 128;
      bool legacy = false;
      uint32_t sequence = 0;
      while (!pending.empty() &&
             pending.begin()->first + pending.begin()->second.frames <=
                 expected) {
        pending.erase(pending.begin());
        ++stats.late;
      }
      if (!pending.empty() && pending.begin()->first <= expected) {
        auto f = std::move(pending.begin()->second);
        pending.erase(pending.begin());
        const auto trim = uint32_t(expected - f.sample);
        frames = f.frames - trim;
        std::copy_n(f.audio.begin() + trim * 2, frames * 2, audio.begin());
        legacy = f.legacy;
        sequence = f.sequence;
      } else if (settings.buffer_ms == 0) {
        if (pending.empty()) {
          wake.wait_for(lock, std::chrono::milliseconds(2));
          continue;
        }
        expected = pending.begin()->first;
        ++stats.resyncs;
        continue;
      } else {
        if (!pending.empty())
          frames = uint32_t(
              std::min<uint64_t>(128, pending.begin()->first - expected));
        stats.missing_frames += frames;
      }
      if (settings.mode == 1)
        for (size_t i = 0; i < frames; ++i)
          audio[2 * i] = audio[2 * i + 1] =
              (audio[2 * i] + audio[2 * i + 1]) * 0.5f;
      if (settings.mode == 2)
        for (size_t i = 0; i < frames; ++i)
          std::swap(audio[2 * i], audio[2 * i + 1]);
      for (unsigned c = 0; c < 2; ++c) {
        double energy = 0;
        float peak = 0;
        for (unsigned i = 0; i < frames; ++i) {
          const auto v = audio[2 * i + c];
          peak = std::max(peak, std::abs(v));
          energy += double(v) * v;
        }
        // Meter ballistics only: actual samples continue unmodified to OBS.
        const auto seconds = double(frames) / rate;
        stats.peak[c] =
            std::max(peak, stats.peak[c] * float(std::pow(0.1, seconds / 0.4)));
        const auto hold = std::exp(-seconds / 0.3);
        stats.rms[c] =
            float(std::sqrt(double(stats.rms[c]) * stats.rms[c] * hold +
                            energy / frames * (1.0 - hold)));
      }
      const auto timestamp = due;
      expected += frames;
      played = true;
      due += uint64_t(frames) * 1000000000ULL / rate;
      const auto remote = sender;
      const auto output_rate = rate;
      const bool ack = legacy && (!last_ack || now - last_ack >= 200000000ULL);
      if (ack)
        last_ack = now;
      lock.unlock();
      obs_source_audio packet{};
      packet.data[0] = reinterpret_cast<const uint8_t *>(audio.data());
      packet.frames = frames;
      packet.speakers = SPEAKERS_STEREO;
      packet.format = AUDIO_FORMAT_FLOAT;
      packet.samples_per_sec = output_rate;
      packet.timestamp = timestamp;
      obs_source_output_audio(source, &packet);
      if (ack && hub) {
        unsigned char bytes[8]{'M', 'M', 'X', 'A', 0, 0, 0, 0};
        for (unsigned i = 0; i < 4; ++i)
          bytes[4 + i] = (sequence >> (8 * i)) & 255;
        sendto(hub->socket, reinterpret_cast<const char *>(bytes), 8, 0,
               reinterpret_cast<const sockaddr *>(&remote), sizeof(remote));
      }
      lock.lock();
    }
  }
  AumuxStats snapshot() const {
    std::lock_guard lock(mutex);
    auto s = stats;
    s.connected = started && last_arrival &&
                  os_gettime_ns() - last_arrival < 500000000ULL;
    s.status = s.connected ? "Recibiendo audio" : "Esperando audio";
    uint64_t frames = 0;
    for (const auto &p : pending)
      frames += p.second.frames;
    s.queued_ms = double(frames) * 1000.0 / rate;
    return s;
  }
};
void Hub::receive() {
  std::array<unsigned char, 2065> bytes{};
  while (running) {
    fd_set fds;
    FD_ZERO(&fds);
    FD_SET(socket, &fds);
    timeval timeout{0, 10000};
    if (select(int(socket + 1), &fds, nullptr, nullptr, &timeout) <= 0)
      continue;
    sockaddr_in address{};
#ifdef _WIN32
    int length = sizeof(address);
#else
    socklen_t length = sizeof(address);
#endif
    auto n = recvfrom(socket, reinterpret_cast<char *>(bytes.data()),
                      int(bytes.size()), 0,
                      reinterpret_cast<sockaddr *>(&address), &length);
    aumux::Frame frame;
    const bool valid = n > 0 && aumux::decode(bytes.data(), size_t(n), frame);
    std::lock_guard lock(mutex);
    for (auto *r : receivers) {
      if (valid)
        r->enqueue(frame, address);
      else
        r->invalid();
    }
  }
}
struct Source {
  obs_source_t *source;
  std::mutex mutex;
  AumuxSettings settings;
  std::unique_ptr<Receiver> receiver;
  std::string error;
  explicit Source(obs_source_t *s) : source(s) {}
};
AumuxSettings read_settings(obs_data_t *data) {
  AumuxSettings s;
  s.bind = obs_data_get_string(data, "bind");
  s.stream = obs_data_get_string(data, "stream");
  s.sender = obs_data_get_string(data, "sender");
  s.port = int(obs_data_get_int(data, "port"));
  s.buffer_ms = int(obs_data_get_int(data, "buffer_ms"));
  s.delay_ms = int(obs_data_get_int(data, "delay_ms"));
  s.mode = int(obs_data_get_int(data, "mode"));
  return s;
}
std::string validate(const AumuxSettings &s) {
  in_addr ip{};
  if (inet_pton(AF_INET, s.bind.c_str(), &ip) != 1 ||
      (!s.sender.empty() && inet_pton(AF_INET, s.sender.c_str(), &ip) != 1))
    return "Interfaz y filtro de emisor requieren IPv4";
  if (s.port < 1024 || s.port > 65535 || s.buffer_ms < 0 || s.buffer_ms > 100 ||
      s.delay_ms < -2000 || s.delay_ms > 2000 || s.mode < 0 || s.mode > 2)
    return "Puerto, búfer, delay o canales fuera de rango";
  if (s.stream.empty() || s.stream.size() > 31 ||
      s.stream.find_first_not_of("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRS"
                                 "TUVWXYZ0123456789-_.") != std::string::npos)
    return "Identificador: 1–31 letras, números, guiones o puntos";
  return {};
}
void update(void *data, obs_data_t *config) {
  auto *s = static_cast<Source *>(data);
  const auto next = read_settings(config);
  std::lock_guard lock(s->mutex);
  if (auto error = validate(next); !error.empty()) {
    s->error = error;
    return;
  }
  const bool delay_changed = obs_data_has_user_value(config, "delay_ms") &&
                             next.delay_ms != s->settings.delay_ms;
  // Gain/synchronization changes don't restart a healthy network receiver.
  auto before = s->settings;
  before.delay_ms = next.delay_ms;
  if (s->receiver && before.bind == next.bind && before.port == next.port &&
      before.stream == next.stream && before.sender == next.sender &&
      before.buffer_ms == next.buffer_ms && before.mode == next.mode) {
    if (delay_changed)
      obs_source_set_sync_offset(s->source, int64_t(next.delay_ms) * 1000000LL);
    s->settings = next;
    s->error.clear();
    return;
  }
  auto candidate = std::make_unique<Receiver>(s->source, next);
  std::string error;
  candidate->hub = get_hub(next, error);
  if (!candidate->hub) {
    if (!s->receiver)
      s->settings = next;
    s->error = error;
    blog(LOG_ERROR, "[AuMux Audio] %s", error.c_str());
    return;
  }
  if (delay_changed)
    obs_source_set_sync_offset(s->source, int64_t(next.delay_ms) * 1000000LL);
  s->receiver.reset();
  s->error.clear();
  s->settings = next;
  {
    std::lock_guard hub_lock(candidate->hub->mutex);
    candidate->hub->receivers.push_back(candidate.get());
  }
  candidate->thread = std::thread([p = candidate.get()] { p->play(); });
  s->receiver = std::move(candidate);
}
void defaults(obs_data_t *data) {
  obs_data_set_default_string(data, "bind", "127.0.0.1");
  obs_data_set_default_string(data, "stream", "AUMUX-MAIN");
  obs_data_set_default_string(data, "sender", "");
  obs_data_set_default_int(data, "port", 17840);
  obs_data_set_default_int(data, "buffer_ms", 0);
  obs_data_set_default_int(data, "delay_ms", 0);
  obs_data_set_default_int(data, "mode", 0);
}
const char *name(void *) { return "AuMux Audio"; }
void *create(obs_data_t *config, obs_source_t *source) {
  try {
    auto s = std::make_unique<Source>(source);
    update(s.get(), config);
    return s.release();
  } catch (...) {
    blog(LOG_ERROR, "[AuMux Audio] Cannot create receiver");
    return nullptr;
  }
}
void destroy(void *data) { delete static_cast<Source *>(data); }
#ifdef AUMUX_WITH_UI
bool editor(obs_properties_t *, obs_property_t *, void *data) {
  auto *s = static_cast<Source *>(data);
  if (s)
    aumux_show_editor(s->source, s);
  return false;
}
#endif
obs_properties_t *properties(void *context) {
  auto *props = obs_properties_create();
  obs_properties_set_param(props, context, nullptr);
#ifdef AUMUX_WITH_UI
  obs_properties_add_button(props, "editor", "Configurar AuMux Audio…", editor);
#endif
  obs_properties_add_text(props, "guide", "AuMux Audio", OBS_TEXT_INFO);
  obs_properties_add_text(props, "bind",
                          "Escuchar en IPv4 (0.0.0.0: todas las interfaces)",
                          OBS_TEXT_DEFAULT);
  obs_properties_add_int(props, "port", "Puerto UDP", 1024, 65535, 1);
  obs_properties_add_text(props, "stream", "Identificador del flujo",
                          OBS_TEXT_DEFAULT);
  obs_properties_add_text(props, "sender", "Filtro IPv4 del emisor (opcional)",
                          OBS_TEXT_DEFAULT);
  obs_properties_add_int(props, "buffer_ms", "Búfer de recepción (ms)", 0, 100,
                         1);
  auto *mode = obs_properties_add_list(
      props, "mode", "Canales", OBS_COMBO_TYPE_LIST, OBS_COMBO_FORMAT_INT);
  obs_property_list_add_int(mode, "Estéreo", 0);
  obs_property_list_add_int(mode, "Mono", 1);
  obs_property_list_add_int(mode, "Intercambiar L/R", 2);
  return props;
}
} // namespace
AumuxSettings aumux_settings(void *data) {
  auto *s = static_cast<Source *>(data);
  std::lock_guard lock(s->mutex);
  auto config = s->settings;
  config.delay_ms = int(obs_source_get_sync_offset(s->source) / 1000000LL);
  return config;
}
AumuxStats aumux_stats(void *data) {
  auto *s = static_cast<Source *>(data);
  std::lock_guard lock(s->mutex);
  auto stats = s->receiver ? s->receiver->snapshot() : AumuxStats{};
  if (!s->error.empty())
    stats.status = s->error;
  return stats;
}
bool aumux_apply(void *data, const AumuxSettings &settings,
                 std::string &error) {
  if (!(error = validate(settings)).empty())
    return false;
  auto *s = static_cast<Source *>(data);
  auto *config = obs_data_create();
  obs_data_set_string(config, "bind", settings.bind.c_str());
  obs_data_set_string(config, "stream", settings.stream.c_str());
  obs_data_set_string(config, "sender", settings.sender.c_str());
  obs_data_set_int(config, "port", settings.port);
  obs_data_set_int(config, "buffer_ms", settings.buffer_ms);
  obs_data_set_int(config, "delay_ms", settings.delay_ms);
  obs_data_set_int(config, "mode", settings.mode);
  obs_source_update(s->source, config);
  obs_data_release(config);
  std::lock_guard lock(s->mutex);
  error = s->error;
  if (error.empty())
    obs_source_set_sync_offset(s->source,
                               int64_t(settings.delay_ms) * 1000000LL);
  return error.empty();
}
bool aumux_restart(void *data, std::string &error) {
  auto *s = static_cast<Source *>(data);
  const auto settings = aumux_settings(data);
  std::unique_ptr<Receiver> retired;
  {
    std::lock_guard lock(s->mutex);
    retired = std::move(s->receiver);
  }
  retired.reset();
  return aumux_apply(data, settings, error);
}
MODULE_EXPORT bool obs_module_load() {
#ifdef _WIN32
  WSADATA wsa{};
  if (WSAStartup(MAKEWORD(2, 2), &wsa) != 0)
    return false;
#endif
  obs_source_info info{};
  info.id = "monitor_mix_receiver";
  info.type = OBS_SOURCE_TYPE_INPUT;
  info.output_flags = OBS_SOURCE_AUDIO;
  info.get_name = name;
  info.create = create;
  info.destroy = destroy;
  info.get_defaults = defaults;
  info.get_properties = properties;
  info.update = update;
  obs_register_source(&info);
  blog(LOG_INFO, "[AuMux Audio] Receiver registered; local MMX1 and named AUM1 "
                 "network streams");
  return true;
}
MODULE_EXPORT void obs_module_unload() {
#ifdef AUMUX_WITH_UI
  aumux_close_editors();
#endif
#ifdef _WIN32
  WSACleanup();
#endif
}
MODULE_EXPORT const char *obs_module_description() {
  return "AuMux Audio: named stereo network audio, independent playout buffers "
         "and OBS synchronization";
}
