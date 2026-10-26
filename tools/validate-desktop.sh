#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
export CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup
export PATH="/workspace/.cargo/bin:/workspace/.audio-obs-sysroot/usr/bin:$PATH"
export LD_LIBRARY_PATH=/workspace/.audio-obs-sysroot/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
export PYTHONPATH="/workspace/.aumux-test-deps${PYTHONPATH:+:$PYTHONPATH}"
export CXXFLAGS=-I/workspace/.audio-obs-sysroot/usr/include
export RUSTFLAGS='-L native=/workspace/.audio-obs-sysroot/usr/lib/x86_64-linux-gnu'
mkdir -p /workspace/scratch/obs-build
g++ -std=c++17 -O1 -g -fsanitize=address,undefined -fno-omit-frame-pointer desktop/native/effects.cpp desktop/native/effects-test.cpp -o /workspace/scratch/native-effects-test
/workspace/scratch/native-effects-test
cargo +1.95.0 metadata --manifest-path desktop/Cargo.toml --locked --format-version 1 > /workspace/scratch/monitor-cargo-metadata.json
fixture_manifest=$(python3 - <<'PY'
import json
from pathlib import Path
m=json.load(open('/workspace/scratch/monitor-cargo-metadata.json'))
p=next(p for p in m['packages'] if p['name']=='soundcraft-vst3-host')
print(Path(p['manifest_path']).parent/'tests/fixture/Cargo.toml')
PY
)
cargo +1.95.0 build --manifest-path "$fixture_manifest" --locked --target-dir /workspace/scratch/vst3-fixture-target -j 2
cp /workspace/scratch/vst3-fixture-target/debug/libvst3_fixture.so /workspace/scratch/Fixture.vst3
export MONITOR_VST3_FIXTURE=/workspace/scratch/Fixture.vst3
cargo +1.95.0 test --manifest-path desktop/Cargo.toml --locked -j 2 -- --include-ignored
cargo +1.95.0 build --manifest-path desktop/Cargo.toml --locked -j 2
desktop/target/debug/aumux --self-test
g++ -std=c++17 -O1 -I/workspace/.audio-obs-sysroot/usr/include/obs -I/workspace/.audio-obs-sysroot/usr/include \
  desktop/native/obs-receiver.cpp desktop/native/obs-receiver-test.cpp \
  -L/workspace/.audio-obs-sysroot/usr/lib/x86_64-linux-gnu \
  -Wl,-rpath,/workspace/.audio-obs-sysroot/usr/lib/x86_64-linux-gnu -lobs -pthread \
  -o /workspace/scratch/obs-build/receiver-test
g++ -std=c++17 -O1 -I/workspace/.audio-obs-sysroot/usr/include/obs -I/workspace/.audio-obs-sysroot/usr/include \
  desktop/native/obs-receiver.cpp desktop/native/obs-network-test.cpp \
  -L/workspace/.audio-obs-sysroot/usr/lib/x86_64-linux-gnu \
  -Wl,-rpath,/workspace/.audio-obs-sysroot/usr/lib/x86_64-linux-gnu -lobs -pthread \
  -o /workspace/scratch/obs-build/network-test
cmake -S desktop/native/obs-plugin -B /workspace/scratch/aumux-obs-linux \
  -DCMAKE_PREFIX_PATH=/workspace/.audio-obs-sysroot/usr \
  -DOBS_INCLUDE_DIR=/workspace/.audio-obs-sysroot/usr/include/obs \
  -DOBS_LIBRARY=/workspace/.audio-obs-sysroot/usr/lib/x86_64-linux-gnu/libobs.so \
  -DAUMUX_TEST_EDITOR=ON
cmake --build /workspace/scratch/aumux-obs-linux -j 2
# A private virtual display for libobs startup, not a user-facing preview.
export DISPLAY=:93
LD_LIBRARY_PATH=/workspace/.audio-obs-sysroot/usr/lib/x86_64-linux-gnu \
  /workspace/.audio-obs-sysroot/usr/bin/Xvfb "$DISPLAY" -screen 0 1280x900x24 -nolisten tcp > /workspace/scratch/monitor-xvfb.log 2>&1 &
display_pid=$!
trap 'kill "$display_pid" 2>/dev/null || true' EXIT
sleep 1
kill -0 "$display_pid"
/workspace/scratch/obs-build/receiver-test > /workspace/scratch/monitor-obs-test.log 2>&1
tail -n 1 /workspace/scratch/monitor-obs-test.log
MONITOR_OBS_TEST=/workspace/scratch/obs-build/receiver-test python3 desktop/tests/bridge-smoke.py
/workspace/scratch/obs-build/network-test > /workspace/scratch/aumux-obs-network-test.log 2>&1
tail -n 1 /workspace/scratch/aumux-obs-network-test.log
QT_QPA_PLATFORM=offscreen /workspace/scratch/aumux-obs-linux/aumux-editor-test docs/aumux-0.12-obs-editor-linux.png > /workspace/scratch/aumux-editor-test.log 2>&1
rg 'PASS:' /workspace/scratch/aumux-editor-test.log
AUMUX_OBS_NETWORK_TEST=/workspace/scratch/obs-build/network-test python3 desktop/tests/audio-network-smoke.py
python3 desktop/tests/listen-smoke.py
python3 desktop/tests/live-fx-smoke.py
python3 desktop/tests/remote-smoke.py
python3 desktop/tests/websocket-smoke.py
python3 desktop/tests/access-smoke.py
python3 desktop/tests/network-smoke.py
python3 desktop/tests/local-http-smoke.py
if command -v chromium >/dev/null; then
  npm_config_cache=/workspace/scratch/npm-cache npm install --prefix /workspace/scratch/monitor-ui playwright-core@1.56.1 --no-audit --no-fund
  NODE_PATH=/workspace/scratch/monitor-ui/node_modules node desktop/tests/studio-ui.cjs
  NODE_PATH=/workspace/scratch/monitor-ui/node_modules node desktop/tests/plugin-browser-ui.cjs
  NODE_PATH=/workspace/scratch/monitor-ui/node_modules node desktop/tests/windows-ui.cjs
  NODE_PATH=/workspace/scratch/monitor-ui/node_modules node desktop/tests/diagnostics-ui.cjs
  NODE_PATH=/workspace/scratch/monitor-ui/node_modules node desktop/tests/audio-network-ui.cjs
  NODE_PATH=/workspace/scratch/monitor-ui/node_modules node desktop/tests/ui-smoke.cjs
  NODE_PATH=/workspace/scratch/monitor-ui/node_modules node desktop/tests/workspace-smoke.cjs
  NODE_PATH=/workspace/scratch/monitor-ui/node_modules node desktop/tests/remote-ui-smoke.cjs
  NODE_PATH=/workspace/scratch/monitor-ui/node_modules node desktop/tests/console-network-ui.cjs
  NODE_PATH=/workspace/scratch/monitor-ui/node_modules node desktop/tests/websocket-ui.cjs
  NODE_PATH=/workspace/scratch/monitor-ui/node_modules node desktop/tests/websocket-network-ui.cjs
  NODE_PATH=/workspace/scratch/monitor-ui/node_modules node desktop/tests/touch-access-ui.cjs
else
  echo 'Browser validation not run: Chromium unavailable'; exit 1
fi
