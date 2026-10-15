mod bridge;
mod compressor;
mod config;
mod eq;
mod fx;
mod generator;
mod graph;
mod listen;
mod mixer;
mod native;
mod native_fx;
mod presets;
#[cfg(test)]
mod test_support;

use config::*;
use mixer::{Controls, Mixer};
use serde_json::{Value, json};
use std::{
    io::Read,
    sync::{Arc, atomic::Ordering::Relaxed},
    time::{Duration, Instant},
};
use tiny_http::{Header, Method, Request, Response, Server};

struct App {
    config: Config,
    listen: Arc<listen::Hub>,
    controls: Arc<Controls>,
    audio: native::Audio,
    system_audio: Option<native::Audio>,
    system_live: bool,
    bridge: Arc<bridge::Bridge>,
    demo: Option<Mixer>,
    live: bool,
    latencies: Vec<usize>,
    input: Vec<f32>,
    output: Vec<f32>,
    next: Instant,
}
impl App {
    fn new() -> Result<Self, String> {
        let config = Config::default();
        let controls = Controls::new(&config);
        let listen = listen::Hub::new()?;
        listen.configure(&config);
        Ok(Self {
            listen,
            config,
            controls,
            audio: native::Audio::new(false)?,
            system_audio: None,
            system_live: false,
            bridge: bridge::Bridge::start()?,
            demo: None,
            live: false,
            latencies: vec![0; DEFAULT_CHANNELS + DEFAULT_AUXES + 1],
            input: vec![0.0; 256 * DEFAULT_CHANNELS],
            output: vec![0.0; 256 * 64],
            next: Instant::now(),
        })
    }
    fn stop(&mut self) {
        self.listen.running(false);
        self.audio.stop();
        if let Some(audio) = &mut self.system_audio {
            audio.stop();
        }
        self.system_live = false;
        self.demo = None;
        self.live = false;
    }
    fn status(&self) -> Value {
        let meters: Vec<f32> = self
            .controls
            .meters
            .iter()
            .map(|m| f32::from_bits(m.swap(0, Relaxed)))
            .collect();
        let channel: Vec<_> = self
            .controls
            .channel_meters
            .iter()
            .map(|m| m.read())
            .collect();
        let bus: Vec<_> = self.controls.bus_meters.iter().map(|m| m.read()).collect();
        let active = self.live || self.demo.is_some();
        let channel_meters: Vec<_> = channel
            .iter()
            .map(|(p, _)| if active { *p } else { [0.0; 2] })
            .collect();
        let channel_rms: Vec<_> = channel
            .iter()
            .map(|(_, r)| if active { *r } else { [0.0; 2] })
            .collect();
        let meters_stereo: Vec<_> = bus
            .iter()
            .map(|(p, _)| if active { *p } else { [0.0; 2] })
            .collect();
        let rms_stereo: Vec<_> = bus
            .iter()
            .map(|(_, r)| if active { *r } else { [0.0; 2] })
            .collect();
        let actual_buffer = if self.system_live {
            self.system_audio.as_ref().map_or(256, |a| a.frames)
        } else if self.live {
            self.audio.frames
        } else {
            256
        };
        json!({"mode":if self.system_live {"PC"}else if self.live {"ASIO"} else if self.demo.is_some(){"DEMO"}else{"DETENIDO"},
            "meters":meters,"channel_meters":channel_meters,"channel_rms":channel_rms,
            "meters_stereo":meters_stereo,"rms_stereo":rms_stereo,
            "gain_reduction_db":self.controls.compressor.iter().map(|c| if active { f32::from_bits(c.reduction.load(Relaxed)) } else { 0.0 }).collect::<Vec<_>>(),
            "emergency":self.controls.emergency.load(Relaxed),"failed":self.controls.failed.load(Relaxed),
            "xruns":self.controls.xruns.load(Relaxed),"clipped_blocks":self.controls.clipped.load(Relaxed),
            "listen":self.listen.status(),
            "bridge_drops":self.bridge.dropped.load(Relaxed),"bridge":self.bridge.status(),"latencies":self.latencies,"actual_buffer":actual_buffer})
    }
    fn start(&mut self, mode: &str) -> Result<Value, String> {
        if mode != "demo" && mode != "asio" && mode != "system" {
            return Err("Unknown audio mode".into());
        }
        self.stop();
        self.controls = Controls::new(&self.config);
        self.controls.failed.store(false, Relaxed);
        self.controls.emergency.store(false, Relaxed);
        self.controls.xruns.store(0, Relaxed);
        self.controls.clipped.store(0, Relaxed);
        let mut mixer = Mixer::new(&self.config, self.controls.clone())?;
        mixer.listen = Some(self.listen.clone());
        self.latencies = mixer.latencies.clone();
        if mode == "demo" {
            self.demo = Some(mixer);
            self.next = Instant::now();
        } else if mode == "system" {
            if self.system_audio.is_none() {
                self.system_audio = Some(native::Audio::new(true)?);
            }
            self.system_audio
                .as_mut()
                .ok_or("System audio backend unavailable")?
                .start(&self.config, mixer, self.bridge.clone(), true)?;
            self.system_live = true;
            self.live = true;
        } else {
            if self.config.buses.iter().all(|b| b.output.is_none()) && !self.config.obs_bridge {
                return Err(
                    "Assign at least one physical output pair or enable the OBS bridge".into(),
                );
            }
            self.audio
                .start(&self.config, mixer, self.bridge.clone(), false)?;
            self.live = true;
        }
        self.listen.running(true);
        Ok(self.status())
    }
    fn demo_tick(&mut self) {
        let Some(m) = &mut self.demo else {
            return;
        };
        for _ in 0..8 {
            if self.next > Instant::now() {
                break;
            }
            self.input.fill(0.0); // Each strip's generator supplies its own signal in the mixer.
            m.process(&self.input, &mut self.output, 256, 64);
            self.bridge.push(&m.buses[0], 256, self.config.sample_rate);
            self.next += Duration::from_secs_f64(256.0 / self.config.sample_rate as f64);
        }
        if self.next + Duration::from_millis(100) < Instant::now() {
            self.next = Instant::now();
        }
    }
    fn api(&mut self, path: &str, body: &str) -> Result<Value, String> {
        match path {
            "/api/config" => {
                let mut c: Config = serde_json::from_str(body).map_err(|e| e.to_string())?;
                c.normalise();
                c.validate()?;
                let active = self.live || self.demo.is_some();
                if active && !self.config.same_device(&c) {
                    return Err("Detén el motor para cambiar dispositivo, entradas ASIO abiertas, frecuencia o buffer".into());
                }
                if self.live && !self.system_live && c.output_channels() > self.audio.outputs {
                    return Err("La ruta mono/estéreo excede las salidas ASIO abiertas".into());
                }
                if active && !self.config.same_graph(&c) {
                    let controls = Controls::new(&c);
                    let mut next =
                        Mixer::reconfigure(&c, controls.clone(), &self.config, &self.latencies)?;
                    next.listen = Some(self.listen.clone());
                    let latencies = next.latencies.clone();
                    if let Some(old) = &mut self.demo {
                        old.fade_removed(&next);
                        self.input.fill(0.0);
                        old.process(&self.input, &mut self.output, 256, MAX_IO);
                        self.bridge
                            .push(&old.buses[0], 256, self.config.sample_rate);
                        self.next +=
                            Duration::from_secs_f64(256.0 / self.config.sample_rate as f64);
                        next.inherit(old);
                        self.demo = Some(next); // retired graph destruction on this control thread
                    } else {
                        let audio = if self.system_live {
                            self.system_audio
                                .as_mut()
                                .ok_or("System audio unavailable")?
                        } else {
                            &mut self.audio
                        };
                        if let Err(error) = audio.replace_mixer(next) {
                            self.stop();
                            return Err(error);
                        }
                    }
                    self.controls = controls;
                    self.latencies = latencies;
                } else if self.config.channels.len() != c.channels.len()
                    || self.config.buses.len() != c.buses.len()
                {
                    self.controls = Controls::new(&c);
                    self.latencies = vec![0; c.channels.len() + c.buses.len()];
                } else {
                    // Atomic input/format/level/EQ updates are read at block boundaries.
                    self.controls.update(&c);
                }
                if !active {
                    self.input.resize(256 * c.inputs as usize, 0.0);
                }
                self.bridge.enabled.store(c.obs_bridge, Relaxed);
                self.listen.configure(&c);
                self.config = c;
                Ok(json!({"ok":true}))
            }
            "/api/start" => {
                let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
                self.start(v["mode"].as_str().unwrap_or(""))
            }
            "/api/stop" => {
                self.stop();
                Ok(self.status())
            }
            "/api/panic" => {
                self.controls.emergency.store(true, Relaxed);
                Ok(self.status())
            }
            "/api/resume" => {
                if self.controls.failed.load(Relaxed) {
                    return Err("El motor tiene un error de audio; detén y vuelve a iniciar para recuperarlo".into());
                }
                self.controls.resume_fade.store(true, Relaxed);
                self.controls
                    .emergency
                    .store(false, std::sync::atomic::Ordering::Release);
                Ok(self.status())
            }
            "/api/devices" => {
                if self.live {
                    return Err("Stop ASIO before scanning devices".into());
                }
                Ok(json!(self.audio.devices()?))
            }
            "/api/vst3" => {
                let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
                let extra = v["directory"].as_str();
                if extra.is_some_and(|p| p.len() > 4096) {
                    return Err("Path too long".into());
                }
                Ok(fx::installed(extra))
            }
            "/api/listen" => {
                let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
                if v["enabled"].as_bool() == Some(true) {
                    let p = v["port"].as_u64().unwrap_or(17841);
                    if p > 65535 {
                        return Err("Puerto inválido".into());
                    }
                    self.listen.start(p as u16)
                } else {
                    self.listen.stop();
                    Ok(self.listen.status())
                }
            }
            "/api/presets" => presets::command(body),
            "/api/inspect" => {
                if self.live || self.demo.is_some() {
                    return Err("Stop audio before inspecting a VST3 bundle".into());
                }
                let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
                let path = v["path"].as_str().ok_or("Missing VST3 path")?;
                if path.len() > 4096 {
                    return Err("Path too long".into());
                }
                if let Some(id) = path.strip_prefix("native:") {
                    let p = native_fx::instantiate(id)?;
                    let params:Vec<Value>=p.info().params.iter().map(|v|json!({"id":v.id,"name":v.name,"min":v.min,"max":v.max,"value":v.default})).collect();
                    return Ok(json!([{"id":p.info().id,"name":p.info().name,"params":params}]));
                }
                let list = fx::load(std::path::Path::new(path))?;
                let mut effects = Vec::new();
                for d in list.iter().filter(|d| !d.is_instrument) {
                    let p = soundcraft_vst3_host::instantiate(&d.id).map_err(|e| e.to_string())?;
                    let params: Vec<Value> = p
                        .info()
                        .params
                        .iter()
                        .map(|pi| {
                            json!({"id":pi.id,"name":pi.name,
                        "min":pi.min,"max":pi.max,"value":p.param(pi.id).unwrap_or(pi.default)})
                        })
                        .collect();
                    effects.push(json!({"id":d.id,"name":d.name,"params":params}));
                }
                Ok(json!(effects))
            }
            _ => Err("Unknown API operation".into()),
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.listen.stop();
    }
}
fn reply(req: Request, status: u16, content_type: &str, body: String) {
    let response = Response::from_string(body)
        .with_status_code(status)
        .with_header(Header::from_bytes("Content-Type", content_type).unwrap())
        .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
        .with_header(Header::from_bytes("X-Content-Type-Options", "nosniff").unwrap());
    let _ = req.respond(response);
}

fn self_test() -> Result<(), String> {
    let mut c = Config::default();
    for ch in &mut c.channels {
        ch.mute = true;
    }
    c.channels[0].mute = false;
    c.channels[0].gain_db = -6.0206;
    c.channels[0].sends_db[0] = 0.0;
    for b in &mut c.buses {
        b.gain_db = 0.0;
    }
    c.buses[0].output = Some(0);
    c.buses[1].output = Some(2);
    let ctl = Controls::new(&c);
    let mut m = Mixer::new(&c, ctl.clone())?;
    let mut input = vec![0.0; 8 * 256];
    for f in 0..256 {
        input[f * 8] = 0.5;
    }
    let mut output = vec![0.0; 4 * 256];
    m.process(&input, &mut output, 256, 4);
    m.process(&input, &mut output, 256, 4);
    if (output[0] - 0.25).abs() > 0.0001 || (output[2] - 0.5).abs() > 0.0001 {
        return Err("Independent mix check failed".into());
    }
    ctl.emergency.store(true, Relaxed);
    m.process(&input, &mut output, 256, 4);
    if output.iter().any(|v| *v != 0.0) {
        return Err("Emergency mute failed".into());
    }
    println!(
        "PASS: SoundCraft DSP linked, master and pre-fader auxiliary routed independently, emergency silence verified. No hardware was opened."
    );
    Ok(())
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--self-test") {
        return self_test();
    }
    let server = Server::http("127.0.0.1:0").map_err(|e| e.to_string())?;
    let url = format!("http://{}", server.server_addr());
    let mut random = [0u8; 32];
    getrandom::fill(&mut random).map_err(|e| e.to_string())?;
    let token: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let page = include_str!("../web/index.html").replace("__TOKEN__", &token);
    let mut app = App::new()?;
    println!(
        "Monitor Mix {} — prototipo\n{url}\nAudio detenido. Cierra esta ventana o pulsa Ctrl+C para salir.",
        env!("CARGO_PKG_VERSION")
    );
    if !args.iter().any(|a| a == "--no-browser") {
        #[cfg(windows)]
        open_app_window(&url);
        #[cfg(not(windows))]
        let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
    }
    loop {
        app.demo_tick();
        let Some(mut req) = server
            .recv_timeout(Duration::from_millis(5))
            .map_err(|e| e.to_string())?
        else {
            continue;
        };
        let path = req.url().split('?').next().unwrap_or("").to_string();
        if *req.method() == Method::Get {
            match path.as_str() {
                "/" => reply(req, 200, "text/html; charset=utf-8", page.clone()),
                "/console.css" => reply(
                    req,
                    200,
                    "text/css; charset=utf-8",
                    include_str!("../web/console.css").into(),
                ),
                "/console.js" => reply(
                    req,
                    200,
                    "text/javascript; charset=utf-8",
                    include_str!("../web/console.js").into(),
                ),
                "/workspace-ui.js" => reply(req,200,"text/javascript; charset=utf-8",include_str!("../web/workspace-ui.js").into()),
                "/effects-ui.js" => reply(req,200,"text/javascript; charset=utf-8",include_str!("../web/effects-ui.js").into()),
                "/api/config" => reply(
                    req,
                    200,
                    "application/json",
                    serde_json::to_string(&app.config).map_err(|e| e.to_string())?,
                ),
                "/api/status" => reply(req, 200, "application/json", app.status().to_string()),
                "/api/presets" => match presets::load() {Ok(p)=>reply(req,200,"application/json",serde_json::json!(p).to_string()),Err(e)=>reply(req,400,"application/json",serde_json::json!({"error":e}).to_string())},
                "/api/native" => reply(req,200,"application/json",serde_json::to_string(native_fx::registry()).map_err(|e|e.to_string())?),
                "/api/limits" => reply(req, 200, "application/json", json!({"channels":MAX_CHANNELS,"auxes":MAX_AUXES,"inputs":MAX_IO,"outputs":MAX_IO}).to_string()),
                _ => reply(req, 404, "text/plain", "Not found".into()),
            }
            continue;
        }
        let authorised = *req.method() == Method::Post
            && req
                .headers()
                .iter()
                .any(|h| h.field.equiv("X-Monitor-Token") && h.value.as_str() == token)
            && req
                .headers()
                .iter()
                .filter(|h| h.field.equiv("Origin"))
                .all(|h| h.value.as_str() == url);
        if !authorised {
            reply(req, 403, "text/plain", "Forbidden".into());
            continue;
        }
        let mut body = String::new();
        if req
            .as_reader()
            .take(2097153)
            .read_to_string(&mut body)
            .is_err()
            || body.len() > 2097152
        {
            reply(req, 413, "text/plain", "Invalid or oversized body".into());
            continue;
        }
        match app.api(&path, &body) {
            Ok(value) => reply(req, 200, "application/json", value.to_string()),
            Err(error) => reply(
                req,
                400,
                "application/json",
                json!({"error":error}).to_string(),
            ),
        }
    }
}
#[cfg(windows)]
fn open_app_window(url: &str) {
    // App mode provides a separate window without browser tabs/address bar.
    // A dedicated profile never uses the user's regular browser session.
    for (variable, relative) in [
        ("ProgramFiles(x86)", "Microsoft/Edge/Application/msedge.exe"),
        ("ProgramFiles", "Microsoft/Edge/Application/msedge.exe"),
        ("LOCALAPPDATA", "Microsoft/Edge/Application/msedge.exe"),
        ("ProgramFiles", "Google/Chrome/Application/chrome.exe"),
        ("LOCALAPPDATA", "Google/Chrome/Application/chrome.exe"),
    ] {
        let Some(base) = std::env::var_os(variable) else {
            continue;
        };
        let browser = std::path::PathBuf::from(base).join(relative);
        if !browser.is_file() {
            continue;
        }
        let profile = std::env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("MonitorMix/browser-profile");
        if std::process::Command::new(browser)
            .arg(format!("--app={url}"))
            .arg(format!("--user-data-dir={}", profile.display()))
            .args(["--window-size=1640,980", "--no-first-run"])
            .spawn()
            .is_ok()
        {
            return;
        }
    }
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
}
fn main() {
    if let Err(e) = run() {
        eprintln!("Monitor Mix: {e}");
        std::process::exit(1);
    }
}
