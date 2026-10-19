// SPDX-License-Identifier: GPL-2.0-or-later
#pragma once
#include <cstdint>
#include <string>
struct obs_source;
using obs_source_t = obs_source;
struct AumuxSettings {
  std::string bind = "127.0.0.1", stream = "AUMUX-MAIN", sender;
  int port = 17840, buffer_ms = 0, delay_ms = 0, mode = 0;
};
struct AumuxStats {
  std::string status = "Esperando audio", sender;
  uint64_t received = 0, invalid = 0, late = 0, duplicates = 0,
           missing_frames = 0, collisions = 0, resyncs = 0;
  double queued_ms = 0;
  float peak[2]{}, rms[2]{};
  bool connected = false;
};
AumuxSettings aumux_settings(void *context);
AumuxStats aumux_stats(void *context);
bool aumux_apply(void *context, const AumuxSettings &, std::string &error);
bool aumux_restart(void *context, std::string &error);
void aumux_show_editor(obs_source_t *, void *context);
void aumux_close_editors();
