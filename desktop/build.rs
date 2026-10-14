fn main() {
    let windows = std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows";
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .warnings(false)
        .include("vendor/rtaudio")
        .include("vendor/rtaudio/include")
        .file("native/audio.cpp")
        .file("vendor/rtaudio/RtAudio.cpp");
    if windows {
        if std::env::var("CARGO_CFG_TARGET_ENV").unwrap() == "msvc" {
            build.flag("/EHsc");
        }
        if std::env::var("CARGO_CFG_TARGET_ENV").unwrap() == "gnu" {
            build.cpp_link_stdlib_static(true);
        }
        build.define("__WINDOWS_ASIO__", None);
        build.define("__WINDOWS_WASAPI__", None);
        for name in ["asio", "asiodrivers", "asiolist", "iasiothiscallresolver"] {
            build.file(format!("vendor/rtaudio/include/{name}.cpp"));
        }
    } else {
        build.define("__LINUX_ALSA__", None);
    }
    build.compile("monitor_audio");
    if windows {
        for lib in [
            "ole32",
            "winmm",
            "advapi32",
            "uuid",
            "ksuser",
            "mfplat",
            "mfuuid",
            "wmcodecdspuuid",
        ] {
            println!("cargo:rustc-link-lib={lib}");
        }
    } else {
        println!("cargo:rustc-link-lib=asound");
        println!("cargo:rustc-link-lib=pthread");
    }
    println!("cargo:rerun-if-changed=native");
    println!("cargo:rerun-if-changed=vendor/rtaudio");
}
