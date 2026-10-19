// SPDX-License-Identifier: GPL-2.0-or-later
#include "obs-receiver-ui.h"
#include <QApplication>
#include <QComboBox>
#include <QDialog>
#include <QLineEdit>
#include <QPushButton>
#include <QSpinBox>
#include <QTimer>
#include <arpa/inet.h>
#include <atomic>
#include <chrono>
#include <cstring>
#include <iostream>
#include <obs.h>
#include <stdexcept>
#include <sys/socket.h>
#include <thread>
#include <unistd.h>
extern "C" bool obs_module_load();
extern "C" void obs_module_unload();
static void check(bool x, const char *m) {
  if (!x)
    throw std::runtime_error(m);
}
int main(int argc, char **argv) {
  QApplication app(argc, argv);
  app.setQuitOnLastWindowClosed(false);
  if (!obs_startup("en-US", nullptr, nullptr))
    return 1;
  obs_audio_info audio{};
  audio.samples_per_sec = 48000;
  audio.speakers = SPEAKERS_STEREO;
  if (!obs_reset_audio(&audio) || !obs_module_load())
    return 2;
  auto *source = obs_source_create_private("monitor_mix_receiver",
                                           "AuMux Audio UI", nullptr);
  auto *props = obs_source_properties(source);
  auto *context = obs_properties_get_param(props);
  auto *button = obs_properties_get(props, "editor");
  obs_property_button_clicked(button, source);
  obs_property_button_clicked(button, source);
  obs_properties_destroy(props);
  QDialog *dialog = nullptr;
  unsigned count = 0;
  for (auto *window : QApplication::topLevelWidgets())
    if (auto *d = qobject_cast<QDialog *>(window)) {
      dialog = d;
      ++count;
    }
  if (!dialog || count != 1)
    return 3;
  int result = 0;
  int socket_ = socket(AF_INET, SOCK_DGRAM, 0);
  sockaddr_in address{};
  address.sin_family = AF_INET;
  address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
  bind(socket_, reinterpret_cast<sockaddr *>(&address), sizeof(address));
  socklen_t len = sizeof(address);
  getsockname(socket_, reinterpret_cast<sockaddr *>(&address), &len);
  auto port = ntohs(address.sin_port);
  close(socket_);
  socket_ = socket(AF_INET, SOCK_DGRAM, 0);
  auto *bind = dialog->findChild<QLineEdit *>("bind");
  auto *flow = dialog->findChild<QLineEdit *>("stream");
  auto *portField = dialog->findChild<QSpinBox *>("port");
  auto *buffer = dialog->findChild<QSpinBox *>("buffer");
  auto *delay = dialog->findChild<QSpinBox *>("delay");
  auto findButton = [dialog](const char *text) {
    for (auto *b : dialog->findChildren<QPushButton *>())
      if (b->text() == text)
        return b;
    return static_cast<QPushButton *>(nullptr);
  };
  std::atomic<bool> go{false}, stop{false};
  QTimer::singleShot(100, [&] {
    try {
      findButton("LAN / Tailscale")->click();
      check(bind->text() == "0.0.0.0" && portField->value() == 17843 &&
                buffer->value() == 8,
            "LAN preset");
      bind->setText("127.0.0.1");
      portField->setValue(port);
      flow->setText("AUMUX-MAIN");
      delay->setValue(120);
      findButton("Aplicar")->click();
      check(obs_source_get_sync_offset(source) == 120000000LL, "delay apply");
      obs_source_set_sync_offset(source, 45000000LL);
      go = true;
    } catch (const std::exception &e) {
      std::cerr << e.what() << "\n";
      result = 4;
      app.quit();
    }
  });
  std::thread sender([&] {
    while (!go && !stop)
      std::this_thread::sleep_for(std::chrono::milliseconds(1));
    uint32_t sequence = 0;
    auto due = std::chrono::steady_clock::now();
    while (!stop) {
      unsigned char p[1104]{};
      std::memcpy(p, "AUM1", 4);
      p[4] = 1;
      p[5] = 2;
      p[6] = 80;
      p[8] = 42;
      auto put = [&](int at, uint32_t v) {
        for (int i = 0; i < 4; ++i)
          p[at + i] = (v >> (8 * i)) & 255;
      };
      put(16, sequence);
      put(20, 48000);
      p[24] = 128;
      put(32, sequence++ * 128);
      std::memcpy(p + 40, "AUMUX-MAIN", 10);
      for (int i = 0; i < 256; ++i) {
        float v = (i & 1) ? 0.1f : 0.2f;
        std::memcpy(p + 80 + 4 * i, &v, 4);
      }
      sendto(socket_, p, sizeof(p), 0, reinterpret_cast<sockaddr *>(&address),
             sizeof(address));
      due += std::chrono::nanoseconds(2666667);
      std::this_thread::sleep_until(due);
    }
  });
  QTimer::singleShot(350, [&] {
    findButton("Reiniciar receptor")->click();
    if (obs_source_get_sync_offset(source) != 45000000LL) {
      result = 6;
      app.quit();
    }
  });
  QTimer::singleShot(600, [&] {
    try {
      check(delay->value() == 45, "live OBS offset reflected in editor");
      check(aumux_stats(context).connected, "live receiver connected");
      check(aumux_stats(context).peak[0] > 0, "real audio meters");
      check(dialog->grab().save(
                argc > 1 ? argv[1] : "/workspace/scratch/aumux-obs-editor.png"),
            "editor screenshot");
      std::cout
          << "PASS: real Qt/libobs editor, singleton per source, LAN preset, "
             "apply/restart, native offset synchronization and real signal "
             "meters\n";
    } catch (const std::exception &e) {
      std::cerr << e.what() << "\n";
      result = 5;
    }
    app.quit();
  });
  app.exec();
  stop = true;
  sender.join();
  close(socket_);
  obs_source_release(source);
  obs_module_unload();
  obs_shutdown();
  return result;
}
