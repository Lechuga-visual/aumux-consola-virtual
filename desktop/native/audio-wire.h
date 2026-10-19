// SPDX-License-Identifier: GPL-2.0-or-later
#pragma once
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <string>
namespace aumux {
constexpr size_t header_size = 80, network_frames = 128;
inline uint32_t u32(const unsigned char *p) {
  return uint32_t(p[0]) | uint32_t(p[1]) << 8 | uint32_t(p[2]) << 16 |
         uint32_t(p[3]) << 24;
}
inline uint64_t u64(const unsigned char *p) {
  return u32(p) | uint64_t(u32(p + 4)) << 32;
}
struct Frame {
  uint64_t epoch = 0, sample = 0, publisher = 0;
  uint32_t sequence = 0, rate = 48000, frames = 0;
  bool legacy = false;
  std::string stream;
  std::array<float, 512> audio{};
};
inline bool decode(const unsigned char *p, size_t size, Frame &f) {
  size_t header;
  if (size >= 16 && std::memcmp(p, "MMX1", 4) == 0) {
    f.legacy = true;
    header = 16;
    f.stream = "AUMUX-MAIN";
    f.sequence = u32(p + 4);
    f.rate = u32(p + 8);
    f.frames = u32(p + 12);
    if ((f.rate != 44100 && f.rate != 48000 && f.rate != 88200 &&
         f.rate != 96000) ||
        f.frames > 256)
      return false;
  } else if (size >= header_size && std::memcmp(p, "AUM1", 4) == 0) {
    header = header_size;
    if (p[4] != 1 || p[5] != 2 || p[6] != header_size || p[7] != 0)
      return false;
    f.epoch = u64(p + 8);
    f.sequence = u32(p + 16);
    f.rate = u32(p + 20);
    f.frames = uint32_t(p[24]) | uint32_t(p[25]) << 8;
    if (f.rate != 48000 || f.frames > network_frames || p[26] || p[27] ||
        p[28] || p[29] || p[30] || p[31])
      return false;
    f.sample = u64(p + 32);
    f.publisher = u64(p + 72);
    size_t n = 0;
    for (; n < 32 && p[40 + n]; ++n) {
      const auto c = p[40 + n];
      if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
            (c >= '0' && c <= '9') || c == '-' || c == '_' || c == '.'))
        return false;
    }
    if (!n || n == 32)
      return false;
    for (size_t i = n; i < 32; ++i)
      if (p[40 + i])
        return false;
    f.stream.assign(reinterpret_cast<const char *>(p + 40), n);
  } else
    return false;
  if (!f.frames || size != header + f.frames * 8 ||
      f.sample > UINT64_MAX - f.frames)
    return false;
  for (size_t i = 0; i < f.frames * 2; ++i) {
    const auto bits = u32(p + header + i * 4);
    float value;
    std::memcpy(&value, &bits, 4);
    f.audio[i] = std::isfinite(value) ? std::clamp(value, -1.0f, 1.0f) : 0.0f;
  }
  return true;
}
} // namespace aumux
