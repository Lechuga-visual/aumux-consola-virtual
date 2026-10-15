#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
export CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup
export PATH="/workspace/.cargo/bin:/workspace/.audio-obs-sysroot/usr/bin:$PATH"
export CXX_x86_64_pc_windows_gnu=x86_64-w64-mingw32-g++-posix
export AR_x86_64_pc_windows_gnu=x86_64-w64-mingw32-ar
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-g++-posix
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS='-C link-arg=-static'
cargo build --manifest-path desktop/Cargo.toml --release --locked --target x86_64-pc-windows-gnu -j 2
obs_checkout=/workspace/.audio-obs-deps/obs-studio
obs_revision=5533a277e4400203b50114c993f89878d50a27b9
if [[ ! -d "$obs_checkout" ]]; then
  git clone --depth 1 --branch 32.1.0 https://github.com/obsproject/obs-studio.git "$obs_checkout"
fi
[[ "$(git -C "$obs_checkout" rev-parse HEAD)" == "$obs_revision" ]] || { echo 'Unexpected OBS SDK revision'; exit 1; }
mkdir -p /workspace/scratch/obs-build
cat > /workspace/scratch/obs-build/obsconfig.h <<'CONFIG'
#pragma once
#define OBS_INSTALL_PREFIX ""
#define OBS_DATA_PATH "data"
#define OBS_PLUGIN_PATH "obs-plugins"
#define OBS_PLUGIN_DESTINATION "obs-plugins"
#define OBS_RELEASE_CANDIDATE 0
#define OBS_BETA 0
CONFIG
x86_64-w64-mingw32-dlltool -d desktop/native/obs.def -l /workspace/scratch/obs-build/libobs.a
x86_64-w64-mingw32-g++-posix -std=c++17 -shared -O2 -static -D_M_X64 \
  -I"$obs_checkout/libobs" -I/workspace/scratch/obs-build \
  desktop/native/obs-receiver.cpp /workspace/scratch/obs-build/libobs.a \
  -lws2_32 -o /workspace/scratch/obs-build/monitor-mix-receiver.dll
echo 'Windows x64 app and OBS 32.1 receiver built; this does not execute them on Windows.'
