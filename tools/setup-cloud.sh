#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
export CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup
export PATH="/workspace/.cargo/bin:/workspace/.audio-obs-sysroot/usr/bin:$PATH"
mkdir -p /workspace/scratch /workspace/.audio-obs-apt/{lists/partial,cache/archives/partial,etc/apt.conf.d,etc/preferences.d,etc/sources.list.d} /workspace/.audio-obs-sysroot
# Test-only Python dependencies live under the workspace; HTTPS keeps certificate verification enabled.
python3 -m pip install --upgrade --disable-pip-version-check --index-url https://pypi.org/simple \
  --cache-dir /workspace/scratch/pip-cache --target /workspace/.aumux-test-deps \
  --requirement desktop/tests/requirements.txt
export PYTHONPATH="/workspace/.aumux-test-deps${PYTHONPATH:+:$PYTHONPATH}"
if [[ ! -x /workspace/.cargo/bin/rustup ]]; then
  curl --fail --location --proto '=https' --tlsv1.2 https://sh.rustup.rs -o /workspace/scratch/rustup-init.sh
  sh /workspace/scratch/rustup-init.sh -y --no-modify-path --profile minimal --default-toolchain 1.95.0
fi
rustup toolchain install 1.95.0 --profile minimal
rustup target add --toolchain 1.95.0 x86_64-pc-windows-gnu
cat > /workspace/.audio-obs-apt/etc/sources.list <<'SOURCES'
deb [signed-by=/usr/share/keyrings/debian-archive-keyring.gpg] https://deb.debian.org/debian trixie main
SOURCES
cat > /workspace/.audio-obs-apt/apt.conf <<'APTCONF'
Dir::Etc "/workspace/.audio-obs-apt/etc";
Dir::Etc::sourcelist "sources.list";
Dir::State::lists "/workspace/.audio-obs-apt/lists";
Dir::Cache "/workspace/.audio-obs-apt/cache";
Debug::NoLocking "true";
APT::Get::List-Cleanup "false";
APTCONF
export APT_CONFIG=/workspace/.audio-obs-apt/apt.conf
/usr/bin/apt-get update
/usr/bin/apt-get --yes --download-only --no-install-recommends -o APT::Sandbox::User=agent install libasound2-dev libobs-dev libsimde-dev g++-mingw-w64-x86-64-posix xvfb
python3 - <<'PY'
from pathlib import Path
import subprocess
root=Path('/workspace/.audio-obs-sysroot')
for deb in Path('/workspace/.audio-obs-apt/cache/archives').glob('*.deb'):
    subprocess.run(['dpkg-deb','-x',str(deb),str(root)],check=True)
# libasound runtime is already installed on this Debian machine; point the
# local development symlink to it without changing the system filesystem.
p=root/'usr/lib/x86_64-linux-gnu/libasound.so'
if p.is_symlink(): p.unlink()
if not p.exists(): p.symlink_to('/usr/lib/x86_64-linux-gnu/libasound.so.2')
PY
export CXXFLAGS=-I/workspace/.audio-obs-sysroot/usr/include
export RUSTFLAGS='-L native=/workspace/.audio-obs-sysroot/usr/lib/x86_64-linux-gnu'
cargo +1.95.0 test --manifest-path desktop/Cargo.toml --locked -j 2
cargo +1.95.0 build --manifest-path desktop/Cargo.toml --locked -j 2
desktop/target/debug/aumux --self-test
unset RUSTFLAGS CXXFLAGS
bash tools/build-windows-cloud.sh
