mod bridge;
mod config;
mod fx;
mod mixer;
mod native;

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
    controls: Arc<Controls>,
    audio: native::Audio,
    bridge: Arc<bridge::Bridge>,
    demo: Option<Mixer>,
    live: bool,
    latencies: [usize; CHANNELS + BUSES],
    phase: usize,
    input: Vec<f32>,
    output: Vec<f32>,
    next: Instant,
}
impl App {
    fn new() -> Result<Self, String> {
        let config = Config::default();
        let controls = Controls::new(&config);
        Ok(Self {
            config,
            controls,
            audio: native::Audio::new()?,
            bridge: bridge::Bridge::start()?,
            demo: None,
            live: false,
            latencies: [0; CHANNELS + BUSES],
            phase: 0,
            input: vec![0.0; 256 * CHANNELS],
            output: vec![0.0; 256 * 64],
            next: Instant::now(),
        })
    }
    fn stop(&mut self) {
        self.audio.stop();
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
        json!({"mode":if self.live {"ASIO"} else if self.demo.is_some(){"DEMO"}else{"DETENIDO"},
            "meters":meters,"emergency":self.controls.emergency.load(Relaxed),"failed":self.controls.failed.load(Relaxed),
            "xruns":self.controls.xruns.load(Relaxed),"clipped_blocks":self.controls.clipped.load(Relaxed),
            "bridge_drops":self.bridge.dropped.load(Relaxed),"latencies":self.latencies,"actual_buffer":if self.live {self.audio.frames}else{256}})
    }
    fn start(&mut self, mode: &str) -> Result<Value, String> {
        if mode != "demo" && mode != "asio" {
            return Err("Unknown audio mode".into());
        }
        self.stop();
        self.controls.failed.store(false, Relaxed);
        self.controls.emergency.store(false, Relaxed);
        self.controls.xruns.store(0, Relaxed);
        self.controls.clipped.store(0, Relaxed);
        let mixer = Mixer::new(&self.config, self.controls.clone())?;
        self.latencies = mixer.latencies;
        if mode == "demo" {
            self.demo = Some(mixer);
            self.next = Instant::now();
        } else {
            if self.config.buses.iter().all(|b| b.output.is_none()) && !self.config.obs_bridge {
                return Err(
                    "Assign at least one physical output pair or enable the OBS bridge".into(),
                );
            }
            self.audio.start(&self.config, mixer, self.bridge.clone())?;
            self.live = true;
        }
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
            for f in 0..256 {
                for ch in 0..CHANNELS {
                    let t = (self.phase + f) as f32 / self.config.sample_rate as f32;
                    self.input[f * CHANNELS + ch] =
                        (t * std::f32::consts::TAU * (220.0 + ch as f32 * 55.0)).sin() * 0.03;
                }
            }
            m.process(&self.input, &mut self.output, 256, 64);
            self.bridge.push(&m.buses[0], 256, self.config.sample_rate);
            self.phase += 256;
            self.next += Duration::from_secs_f64(256.0 / self.config.sample_rate as f64);
        }
        if self.next + Duration::from_millis(100) < Instant::now() {
            self.next = Instant::now();
        }
    }
    fn api(&mut self, path: &str, body: &str) -> Result<Value, String> {
        match path {
            "/api/config" => {
                let c: Config = serde_json::from_str(body).map_err(|e| e.to_string())?;
                c.validate()?;
                if (self.live || self.demo.is_some()) && !self.config.same_graph(&c) {
                    return Err("Stop audio before changing device, routing, sample rate, buffer or effects".into());
                }
                self.controls.update(&c);
                self.bridge.enabled.store(c.obs_bridge, Relaxed);
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
            "/api/devices" => {
                if self.live {
                    return Err("Stop ASIO before scanning devices".into());
                }
                Ok(json!(self.audio.devices()?))
            }
            "/api/inspect" => {
                if self.live || self.demo.is_some() {
                    return Err("Stop audio before inspecting a VST3 bundle".into());
                }
                let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
                let path = v["path"].as_str().ok_or("Missing VST3 path")?;
                if path.len() > 4096 {
                    return Err("Path too long".into());
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
        "Monitor Mix 0.1 — prototipo\n{url}\nAudio detenido. Abre la URL en tu navegador. Cierra esta ventana o pulsa Ctrl+C para salir."
    );
    if !args.iter().any(|a| a == "--no-browser") {
        #[cfg(windows)]
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .spawn();
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
                "/api/config" => reply(
                    req,
                    200,
                    "application/json",
                    serde_json::to_string(&app.config).map_err(|e| e.to_string())?,
                ),
                "/api/status" => reply(req, 200, "application/json", app.status().to_string()),
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
            .take(262145)
            .read_to_string(&mut body)
            .is_err()
            || body.len() > 262144
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
fn main() {
    if let Err(e) = run() {
        eprintln!("Monitor Mix: {e}");
        std::process::exit(1);
    }
}
