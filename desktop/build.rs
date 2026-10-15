fn main() {
    let windows = std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows";
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .warnings(false)
        .include("native/asio_abi")
        .include("vendor/rtaudio")
        .include("vendor/rtaudio/include")
        .file("native/audio.cpp")
        .file("native/effects.cpp")
        .file("vendor/rtaudio/RtAudio.cpp");
    if windows {
        if std::env::var("CARGO_CFG_TARGET_ENV").unwrap() == "msvc" {
            build.flag("/EHsc");
            build.flag("/utf-8");
        }
        if std::env::var("CARGO_CFG_TARGET_ENV").unwrap() == "gnu" {
            build.cpp_link_stdlib_static(true);
        }
        build.define("__WINDOWS_ASIO__", None);
        build.define("__WINDOWS_WASAPI__", None);
        build.file("native/asio_abi.cpp");
        build.file("native/tray.cpp");
    } else {
        build.define("__LINUX_ALSA__", None);
    }
    build.compile("monitor_audio");
    if windows {
        for lib in [
            "ole32",
            "shell32",
            "user32",
            "winmm",
            "advapi32",
            "uuid",
            "ksuser",
            "mfplat",
            "mfuuid",
            "wmcodecdspuuid",
            "psapi",
        ] {
            println!("cargo:rustc-link-lib={lib}");
        }
        windows_resources();
    } else {
        println!("cargo:rustc-link-lib=asound");
        println!("cargo:rustc-link-lib=pthread");
    }
    println!("cargo:rerun-if-changed=native");
    println!("cargo:rerun-if-changed=vendor");
}

fn windows_resources() {
    use std::{env, fs, path::PathBuf, process::Command};
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let icon = root.join("assets/aumux.ico");
    assert!(icon.is_file(), "AuMux icon missing: {}", icon.display());
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let mut parts = version.split('.');
    let source = fs::read_to_string(root.join("native/resources.rc"))
        .unwrap()
        .replace("__VERSION_MAJOR__", parts.next().unwrap_or("0"))
        .replace("__VERSION_MINOR__", parts.next().unwrap_or("0"))
        .replace("__VERSION_PATCH__", parts.next().unwrap_or("0"))
        .replace("__VERSION__", &version)
        .replace("__ICON_PATH__", &icon.to_string_lossy().replace('\\', "/"));
    let resource = output.join("aumux.rc");
    fs::write(&resource, source).unwrap();
    let target = env::var("TARGET").unwrap();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap();
    let compiled;
    let mut compiler;
    if target_env == "msvc" {
        compiled = output.join("aumux.res");
        compiler = if let Some(rc) = env::var_os("RC") {
            Command::new(rc)
        } else {
            cc::windows_registry::find(&target, "rc.exe").unwrap_or_else(|| Command::new("rc.exe"))
        };
        compiler
            .arg("/nologo")
            .arg(format!("/fo{}", compiled.display()))
            .arg(&resource);
    } else {
        compiled = output.join("aumux-resource.o");
        let target_key = format!("RC_{}", target.replace('-', "_"));
        let rc = env::var_os(&target_key)
            .or_else(|| env::var_os("WINDRES"))
            .or_else(|| env::var_os("RC"))
            .unwrap_or_else(|| "x86_64-w64-mingw32-windres".into());
        compiler = Command::new(rc);
        // Some MinGW installations only expose gcc/g++ with a -posix suffix.
        // Reuse cc's selected cross compiler rather than windres' gcc default.
        let preprocessor = cc::Build::new().cpp(true).get_compiler();
        compiler
            .arg("--preprocessor")
            .arg(preprocessor.path())
            .args([
                "--preprocessor-arg=-E",
                "--preprocessor-arg=-xc",
                "--preprocessor-arg=-DRC_INVOKED",
            ]);
        compiler
            .args(["-O", "coff", "-i"])
            .arg(&resource)
            .arg("-o")
            .arg(&compiled);
        println!("cargo:rerun-if-env-changed={target_key}");
    }
    let status = compiler.status().expect(
        "Could not start Windows resource compiler; install the Windows SDK or MinGW windres",
    );
    assert!(
        status.success(),
        "AuMux Windows resource compilation failed"
    );
    println!("cargo:rustc-link-arg={}", compiled.display());
    println!("cargo:rerun-if-changed=assets/aumux.ico");
    println!("cargo:rerun-if-env-changed=RC");
    println!("cargo:rerun-if-env-changed=WINDRES");
}
