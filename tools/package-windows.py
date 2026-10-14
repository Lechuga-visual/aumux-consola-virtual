"""Build the portable ZIP from previously verified Windows artifacts."""
import hashlib
import json
from pathlib import Path
import shutil
import zipfile

root = Path(__file__).resolve().parents[1]
dist = root / "dist"
stage = dist / "MonitorMix-0.1.0-Windows-x64"
stage.mkdir(parents=True, exist_ok=True)
(stage / "obs-plugin").mkdir(exist_ok=True)
shutil.copy2(root / "desktop/target/x86_64-pc-windows-gnu/release/monitor-mix.exe", stage / "MonitorMix.exe")
shutil.copy2("/workspace/scratch/obs-build/monitor-mix-receiver.dll", stage / "obs-plugin/monitor-mix-receiver.dll")
for name in ["Instalar-receptor-OBS.cmd", "Instalar-receptor-OBS.ps1", "THIRD_PARTY.md"]:
    shutil.copy2(root / "desktop" / name, stage / name)
shutil.copy2(root / "docs/probar-en-windows.md", stage / "LEEME.md")
shutil.copy2(root / "LICENSE", stage / "LICENSE")
source = stage / "source"
shutil.copytree(root / "desktop", source / "desktop", dirs_exist_ok=True,
                ignore=shutil.ignore_patterns("target", "__pycache__"))
shutil.copytree(root / "tools", source / "tools", dirs_exist_ok=True,
                ignore=shutil.ignore_patterns("__pycache__"))
(source / "docs").mkdir(exist_ok=True)
shutil.copy2(root / "docs/probar-en-windows.md", source / "docs/probar-en-windows.md")
shutil.copy2(root / "LICENSE", source / "LICENSE")

metadata = json.loads(Path("/workspace/scratch/monitor-cargo-metadata.json").read_text())
licenses = stage / "licenses"
for package in metadata["packages"]:
    if package["name"] == "monitor-mix":
        continue
    folder = Path(package["manifest_path"]).parent
    if package["name"].startswith("soundcraft-"):
        folder = folder.parents[1]
    target = licenses / f"{package['name']}-{package['version']}"
    target.mkdir(parents=True, exist_ok=True)
    (target / "origin.txt").write_text(f"{package['name']} {package['version']}\n{package.get('source')}\nLicense: {package.get('license')}\n")
    for p in folder.iterdir():
        if p.is_file() and (p.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE"))):
            shutil.copy2(p, target / p.name)
    if package.get("license_file"):
        p = Path(package["manifest_path"]).parent / package["license_file"]
        if p.is_file():
            shutil.copy2(p, target / p.name)
(licenses / "OBS").mkdir(parents=True, exist_ok=True)
shutil.copy2("/workspace/.audio-obs-deps/obs-studio/COPYING", licenses / "OBS/GPL-2.0.txt")
runtime_license=Path('/workspace/.audio-obs-sysroot/usr/share/doc/gcc-mingw-w64-base/copyright')
if runtime_license.is_file():
    (licenses / 'GCC-MinGW').mkdir(exist_ok=True)
    shutil.copy2(runtime_license, licenses / 'GCC-MinGW/copyright')
for version in ['2','3']:
    p=Path('/usr/share/common-licenses') / ('GPL-'+version)
    if p.is_file():
        shutil.copy2(p, licenses / ('GPL-'+version+'.txt'))

checks = []
for p in sorted(stage.rglob("*")):
    if p.is_file() and p.name != "SHA256SUMS.txt":
        checks.append(f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.relative_to(stage).as_posix()}")
(stage / "SHA256SUMS.txt").write_text("\n".join(checks) + "\n")
archive = dist / (stage.name + ".zip")
with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED, compresslevel=7, strict_timestamps=False) as out:
    for p in sorted(stage.rglob("*")):
        if p.is_file():
            out.write(p, p.relative_to(stage))
with zipfile.ZipFile(archive) as check:
    assert check.testzip() is None
    assert {"MonitorMix.exe", "obs-plugin/monitor-mix-receiver.dll", "LEEME.md"} <= set(check.namelist())
digest = hashlib.sha256(archive.read_bytes()).hexdigest()
archive.with_suffix(".zip.sha256").write_text(f"{digest}  {archive.name}\n")
print(f"Created {archive} ({archive.stat().st_size:,} bytes); ZIP integrity verified")
