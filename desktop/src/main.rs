#![cfg_attr(windows, windows_subsystem = "windows")]
mod access;
mod audio_network;
mod bridge;
mod compressor;
mod config;
mod diagnostics;
mod eq;
mod fx;
mod generator;
mod graph;
mod latency;
mod listen;
mod mixer;
mod native;
mod native_fx;
mod network;
mod presets;
mod remote;
mod telemetry;
#[cfg(test)]
mod test_support;
mod tray;
mod web_http;

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
    diagnostics: Arc<diagnostics::Hub>,
    benchmark: Option<diagnostics::bench::Job>,
    bench_results: Vec<Value>,
    probe_restore: bool,
    inventory: diagnostics::inventory::Inventory,
    device_name: Option<String>,
    listen: Arc<listen::Hub>,
    remote: Arc<remote::Remote>,
    controls: Arc<Controls>,
    audio: native::Audio,
    system_audio: Option<native::Audio>,
    system_live: bool,
    bridge: Arc<bridge::Bridge>,
    audio_network: Arc<audio_network::Hub>,
    demo: Option<Mixer>,
    live: bool,
    latencies: Vec<usize>,
    input: Vec<f32>,
    output: Vec<f32>,
    next: Instant,
    system: telemetry::Telemetry,
}
impl App {
    fn new() -> Result<Self, String> {
        let mut config = Config::default();
        config.normalise();
        let controls = Controls::new(&config);
        let listen = listen::Hub::new()?;
        listen.configure(&config);
        Ok(Self {
            listen,
            diagnostics: diagnostics::Hub::new(),
            benchmark: None,
            bench_results: Vec::new(),
            probe_restore: false,
            inventory: diagnostics::inventory::Inventory::new(),
            device_name: None,
            remote: remote::Remote::new(),
            config,
            controls,
            audio: native::Audio::new(false)?,
            system_audio: None,
            system_live: false,
            bridge: bridge::Bridge::start()?,
            audio_network: audio_network::Hub::new(),
            demo: None,
            live: false,
            latencies: vec![0; DEFAULT_CHANNELS + DEFAULT_AUXES + 1],
            input: vec![0.0; 256 * DEFAULT_CHANNELS],
            output: vec![0.0; 256 * 64],
            next: Instant::now(),
            system: telemetry::Telemetry::new(),
        })
    }
    fn stop(&mut self) {
        self.diagnostics.cancel();
        if self.probe_restore {
            self.controls.update(&self.config);
            self.probe_restore = false;
        }
        self.listen.running(false);
        self.audio.stop();
        if let Some(audio) = &mut self.system_audio {
            audio.stop();
        }
        self.system_live = false;
        self.demo = None;
        self.live = false;
    }
    fn restore_probe(&mut self) {
        if self.probe_restore && !self.diagnostics.busy() {
            self.controls.update(&self.config);
            self.probe_restore = false;
        }
    }
    fn diagnostics_report(&self) -> Value {
        let mut config = json!(self.config);
        for kind in ["channels", "buses"] {
            if let Some(nodes) = config[kind].as_array_mut() {
                for node in nodes {
                    if let Some(fx) = node.get_mut("fx").filter(|f| !f.is_null()) {
                        fx["path"] = json!("Ruta local omitida");
                    }
                    if let Some(inserts) = node.get_mut("inserts").and_then(Value::as_array_mut) {
                        for fx in inserts {
                            fx["path"] = json!("Ruta local omitida");
                        }
                    }
                }
            }
        }
        let mut status = self.status();
        if let Some(remote) = status["remote"].as_object_mut() {
            remote.remove("sessions");
        }
        if let Some(listen) = status["listen"].as_object_mut() {
            listen.remove("url");
        }
        let mut diagnostics = self.diagnostics_status();
        fn omit_paths(v: &mut Value) {
            match v {
                Value::Object(o) => {
                    for (k, v) in o {
                        if k == "path" {
                            *v = json!("Ruta local omitida");
                        } else {
                            omit_paths(v);
                        }
                    }
                }
                Value::Array(a) => {
                    for v in a {
                        omit_paths(v);
                    }
                }
                _ => {}
            }
        }
        omit_paths(&mut diagnostics);
        json!({"application":{"name":"AuMux","version":env!("CARGO_PKG_VERSION"),"architecture":std::env::consts::ARCH,"report_format":1},"generated_unix_seconds":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0,|d|d.as_secs()),"inventory":self.inventory.snapshot(),"interface":{"name":self.device_name,"selected_id":self.config.device,"hardware_revision":Value::Null,"asio_driver_version":Value::Null,"note":"El inventario enumera controladores de audio de Windows; no identifica automáticamente qué versión ASIO usa cada dispositivo. Revisión de hardware/firmware y versión ASIO: no informadas, completar manualmente."},"status":status,"configuration":config,"diagnostics":diagnostics,"measurement_notes":["Loopback: ida/vuelta física medida por correlación a 48 kHz. No divide automáticamente el resultado en entrada y salida.","Buffers y plugins: datos calculados o declarados, separados de mediciones físicas.","Picos CPU/RAM/E/S: muestras del proceso del motor desde su inicio, cada ~1 s; no incluyen el navegador ni picos entre muestras.","E/S de proceso no equivale al porcentaje de actividad del disco ni a su máximo instantáneo."]})
    }
    fn diagnostics_status(&self) -> Value {
        let mut v = self.diagnostics.status();
        v["benchmark"] = self.benchmark.as_ref().map_or(Value::Null, |j| j.status());
        v["bench_results"] = json!(self.bench_results);
        v
    }
    fn finish_bench(&mut self, job: diagnostics::bench::Job) {
        self.stop();
        self.config = job.original;
        self.controls = Controls::new(&self.config);
        self.latencies = vec![0; self.config.channels.len() + self.config.buses.len()];
        self.remote.invalidate();
    }
    fn cancel_bench(&mut self) {
        if let Some(job) = self.benchmark.take() {
            if job.began.is_some() {
                self.bench_results.push(json!({"requested_buffer":job.buffers[job.index],"mode":job.mode,"cancelled":true,"performance":self.controls.performance.snapshot(),"system":self.system.cached()}));
            }
            self.finish_bench(job);
        }
    }
    fn benchmark_tick(&mut self) {
        let Some(mut job) = self.benchmark.take() else {
            return;
        };
        if job.began.is_some() && (job.due() || self.controls.failed.load(Relaxed)) {
            let performance = self.controls.performance.snapshot();
            let actual = if self.system_live {
                self.system_audio.as_ref().map_or(0, |a| a.frames)
            } else if self.live {
                self.audio.frames
            } else {
                self.config.buffer
            };
            self.bench_results.push(json!({"mode":job.mode,"hardware":job.mode!="demo","requested_buffer":job.buffers[job.index],"actual_buffer":actual,"seconds":job.began.unwrap().elapsed().as_secs_f64(),"xruns":self.controls.xruns.load(Relaxed),"clipped_blocks":self.controls.clipped.load(Relaxed),"failed":self.controls.failed.load(Relaxed),"performance":performance,"system":self.system.cached(),"sample_rate":self.config.sample_rate,"device":self.config.device}));
            self.stop();
            job.index += 1;
            job.began = None;
        }
        while job.began.is_none() && job.index < job.buffers.len() {
            self.config.buffer = job.buffers[job.index];
            match self.start(&job.mode) {
                Ok(_) => {
                    job.began = Some(Instant::now());
                }
                Err(error) => {
                    self.bench_results.push(json!({"mode":job.mode,"requested_buffer":job.buffers[job.index],"error":error}));
                    job.index += 1;
                }
            }
        }
        if job.index >= job.buffers.len() {
            self.finish_bench(job);
        } else {
            self.benchmark = Some(job);
        }
        if self.bench_results.len() > 100 {
            self.bench_results.drain(..self.bench_results.len() - 100);
        }
    }
    fn status(&self) -> Value {
        self.status_peaks(true)
    }
    fn panel_status(&self) -> Value {
        self.status_peaks(false)
    }
    fn status_peaks(&self, consume: bool) -> Value {
        let meters: Vec<f32> = self
            .controls
            .meters
            .iter()
            .map(|m| {
                f32::from_bits(if consume {
                    m.swap(0, Relaxed)
                } else {
                    m.load(Relaxed)
                })
            })
            .collect();
        let channel: Vec<_> = self
            .controls
            .channel_meters
            .iter()
            .map(|m| if consume { m.read() } else { m.snapshot() })
            .collect();
        let bus: Vec<_> = self
            .controls
            .bus_meters
            .iter()
            .map(|m| if consume { m.read() } else { m.snapshot() })
            .collect();
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
            self.config.buffer
        };
        json!({"mode":if self.system_live {"PC"}else if self.live {"ASIO"} else if self.demo.is_some(){"DEMO"}else{"DETENIDO"},
            "meters":meters,"channel_meters":channel_meters,"channel_rms":channel_rms,
            "meters_stereo":meters_stereo,"rms_stereo":rms_stereo,
            "gain_reduction_db":self.controls.compressor.iter().map(|c| if active { f32::from_bits(c.reduction.load(Relaxed)) } else { 0.0 }).collect::<Vec<_>>(),
            "emergency":self.controls.emergency.load(Relaxed),"failed":self.controls.failed.load(Relaxed),
            "xruns":self.controls.xruns.load(Relaxed),"clipped_blocks":self.controls.clipped.load(Relaxed),
            "audio_network":self.audio_network.status(),"listen":self.listen.status(),
            "remote":self.remote.status(),
            "latency":self.latency(),
            "system":self.system.cached(),"performance":self.controls.performance.snapshot(),
            "bridge_drops":self.bridge.dropped.load(Relaxed),"bridge":self.bridge.status(),"latencies":self.latencies,"actual_buffer":actual_buffer})
    }
    fn start(&mut self, mode: &str) -> Result<Value, String> {
        if mode != "demo" && mode != "asio" && mode != "system" {
            return Err("Unknown audio mode".into());
        }
        self.stop();
        if mode == "asio" {
            self.device_name = self
                .audio
                .devices()?
                .iter()
                .find(|d| Some(d.id) == self.config.device)
                .map(|d| d.name.clone());
        } else if mode == "system" {
            self.device_name = Some("Salida predeterminada de Windows (WASAPI)".into());
        }
        self.controls = Controls::new(&self.config);
        self.controls.failed.store(false, Relaxed);
        self.controls.emergency.store(false, Relaxed);
        self.controls.xruns.store(0, Relaxed);
        self.controls.clipped.store(0, Relaxed);
        let mut mixer = Mixer::new(&self.config, self.controls.clone())?;
        mixer.diagnostics = Some(self.diagnostics.tap());
        mixer.listen = Some(self.listen.clone());
        mixer.audio_network = Some(self.audio_network.clone());
        self.latencies = mixer.latencies.clone();
        if mode == "demo" {
            self.input
                .resize(MAX_FRAMES * self.config.inputs as usize, 0.0);
            self.output.resize(MAX_FRAMES * MAX_IO, 0.0);
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
            if self.config.buses.iter().all(|b| b.output.is_none())
                && !self.config.obs_bridge
                && !self.config.audio_network.iter().any(|r| r.enabled)
            {
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
        let frames = self.config.buffer as usize;
        for _ in 0..8 {
            if self.next > Instant::now() {
                break;
            }
            self.input.fill(0.0); // Each strip's generator supplies its own signal in the mixer.
            m.process(&self.input, &mut self.output, frames, MAX_IO);
            self.bridge
                .push(&m.buses[0], frames, self.config.sample_rate);
            self.next += Duration::from_secs_f64(frames as f64 / self.config.sample_rate as f64);
        }
        if self.next + Duration::from_millis(100) < Instant::now() {
            self.next = Instant::now();
        }
    }
    fn latency(&self) -> Value {
        latency::report(
            &self.config,
            &self.latencies,
            if self.system_live {
                self.system_audio.as_ref().map_or(256, |a| a.frames)
            } else if self.live {
                self.audio.frames
            } else {
                self.config.buffer
            },
            if self.system_live {
                self.system_audio.as_ref().and_then(|a| a.latency_frames())
            } else if self.live {
                self.audio.latency_frames()
            } else {
                None
            },
            self.live || self.demo.is_some(),
            self.bridge.queue_depth(),
        )
    }
    #[cfg(windows)]
    fn update_tray(&self, tray: &tray::Tray) {
        let remote = self.remote.status();
        let preferred = remote["host"].as_str().unwrap_or("");
        let ip = telemetry::local_ipv4(preferred, &network::addresses());
        tray.update(ip.as_deref(), &self.latency(), self.system.cached());
    }
    fn read_api(&mut self, path: &str) -> Result<Value, String> {
        match path {
            "/api/config" => Ok(json!(self.config)),
            "/api/panel-status" => Ok(self.panel_status()),
            "/api/status" => Ok(self.status()),
            "/api/mix-state" => Ok(
                json!({"revision":self.remote.status()["revision"],"channels":self.config.channels.iter().map(|ch|json!({"sends_db":ch.sends_db,"send_mute":ch.send_mute,"pre":ch.pre})).collect::<Vec<_>>() }),
            ),
            "/api/presets" => presets::load().map(|p| json!(p)),
            "/api/native" => Ok(json!(native_fx::registry())),
            "/api/limits" => Ok(
                json!({"channels":MAX_CHANNELS,"auxes":MAX_AUXES,"inputs":MAX_IO,"outputs":MAX_IO}),
            ),
            "/api/sync-state" => {
                Ok(json!({"revision":self.remote.status()["revision"],"config":self.config}))
            }
            "/api/diagnostics/report" => Ok(self.diagnostics_report()),
            "/api/diagnostics/live" => Ok(self.diagnostics.live_status()),
            "/api/diagnostics" => Ok(self.diagnostics_status()),
            "/api/latency" => Ok(self.latency()),
            "/api/addresses" => Ok(network::addresses()),
            _ => Err("API no disponible".into()),
        }
    }
    fn api(&mut self, path: &str, body: &str) -> Result<Value, String> {
        if self.diagnostics.probing()
            && !matches!(path, "/api/diagnostics/cancel" | "/api/stop" | "/api/panic")
        {
            return Err(
                "Medición física activa; espera o cancela antes de cambiar la consola".into(),
            );
        }
        if self.benchmark.is_some()
            && !matches!(
                path,
                "/api/diagnostics/bench/cancel" | "/api/stop" | "/api/panic"
            )
        {
            return Err("Prueba de buffers activa; cancélala antes de modificar la consola".into());
        }
        match path {
            "/api/diagnostics/scope-window" => {
                let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
                let frames = v["frames"]
                    .as_u64()
                    .filter(|n| (48..=4096).contains(n))
                    .ok_or("Ventana fuera de rango")?;
                self.diagnostics
                    .scope_window(frames as usize, v["rising"] == true);
                Ok(self.diagnostics.live_status())
            }
            "/api/diagnostics/monitor" => {
                if !self.live && self.demo.is_none() {
                    return Err("Inicia el motor antes de analizar una señal".into());
                }
                let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
                let a: diagnostics::Source =
                    serde_json::from_value(v["a"].clone()).map_err(|e| e.to_string())?;
                let b: diagnostics::Source =
                    serde_json::from_value(v["b"].clone()).map_err(|e| e.to_string())?;
                if !a.valid(&self.config) || !b.valid(&self.config) {
                    return Err("Fuente de análisis inexistente".into());
                }
                if self.system_live && (a.kind == 0 || b.kind == 0) {
                    return Err("AUDIO PC no abre entradas físicas; selecciona canal o bus".into());
                }
                self.diagnostics.monitor(a,b,json!({"owner":v["owner"].as_str().map(|s|s.chars().take(100).collect::<String>()),"a":a,"b":b,"rate":self.config.sample_rate,"buffer":self.config.buffer,"mode":if self.system_live{"PC"}else if self.live{"ASIO"}else{"DEMO"}}))
            }
            "/api/diagnostics/bench/start" => {
                if self.live || self.demo.is_some() || self.diagnostics.busy() {
                    return Err("Detén el motor y las capturas antes del barrido de buffers".into());
                }
                if self.remote.status()["enabled"] == true {
                    return Err(
                        "Detén el servicio de control remoto durante el barrido de buffers".into(),
                    );
                }
                let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
                let job = diagnostics::bench::Job::parse(&v, &self.config)?;
                self.benchmark = Some(job);
                self.benchmark_tick();
                Ok(self.diagnostics_status())
            }
            "/api/diagnostics/bench/cancel" => {
                self.cancel_bench();
                Ok(self.diagnostics_status())
            }
            "/api/diagnostics/cancel" => {
                let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
                if let Some(owner) = v["owner"].as_str() {
                    self.diagnostics.cancel_owner(owner);
                } else {
                    self.diagnostics.cancel();
                }
                self.restore_probe();
                Ok(self.diagnostics.status())
            }
            "/api/diagnostics/probe" => {
                if !self.live || self.system_live {
                    return Err(
                        "El loopback físico requiere el motor ASIO activo con entradas y salidas"
                            .into(),
                    );
                }
                if self.controls.emergency.load(Relaxed) || self.controls.failed.load(Relaxed) {
                    return Err("Recupera el motor antes de medir".into());
                }
                let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
                let input = v["input"].as_u64().ok_or("Entrada requerida")? as usize;
                let output = v["output"].as_u64().ok_or("Salida requerida")? as usize;
                if input >= self.config.inputs as usize || output >= self.audio.outputs {
                    return Err("Entrada o salida fuera del dispositivo abierto".into());
                }
                let label = v["label"].as_str().unwrap_or("Loopback físico");
                let owner = v["owner"]
                    .as_str()
                    .map(|s| s.chars().take(100).collect::<String>());
                let mut context = json!({"input":input+1,"output":output+1,"buffer":self.audio.frames,"device":self.config.device,"sample_rate":self.config.sample_rate,"mode":"ASIO","kind":"physical_round_trip"});
                context["owner"] = json!(owner);
                if let Some(route) = v.get("route") {
                    let channel = route["channel"].as_u64().ok_or("Canal requerido")? as usize;
                    let bus = route["bus"].as_u64().ok_or("Bus requerido")? as usize;
                    if channel >= self.config.channels.len() || bus >= self.config.buses.len() {
                        return Err("Ruta fuera de la escena".into());
                    }
                    if self.config.buses[bus].output != Some(output) {
                        return Err("La salida de prueba debe coincidir con la salida izquierda/mono asignada al bus".into());
                    }
                    let bypass = v["bypass"] == true;
                    context["kind"] = json!("physical_route");
                    context["channel_index"] = json!(channel);
                    context["bus_index"] = json!(bus);
                    context["channel"] = json!(self.config.channels[channel]);
                    context["bus"] = json!(self.config.buses[bus]);
                    context["bypass"] = json!(bypass);
                    let result = self
                        .diagnostics
                        .route_probe(input, output, channel, bus, label, context)?;
                    if bypass {
                        let mut c = self.config.clone();
                        c.channels[channel].eq.bypass = true;
                        c.channels[channel].compressor.bypass = true;
                        for fx in &mut c.channels[channel].inserts {
                            fx.bypass = true;
                        }
                        if let Some(fx) = &mut c.channels[channel].fx {
                            fx.bypass = true;
                        }
                        c.buses[bus].eq.bypass = true;
                        c.buses[bus].compressor.bypass = true;
                        for fx in &mut c.buses[bus].inserts {
                            fx.bypass = true;
                        }
                        if let Some(fx) = &mut c.buses[bus].fx {
                            fx.bypass = true;
                        }
                        self.controls.update(&c);
                        self.probe_restore = true;
                    }
                    Ok(result)
                } else {
                    self.diagnostics.probe(input, output, label, context)
                }
            }
            "/api/config" => {
                let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
                let wrapped = v.get("config").is_some();
                let mut c: Config = serde_json::from_value(if wrapped {
                    v["config"].clone()
                } else {
                    v.clone()
                })
                .map_err(|e| e.to_string())?;
                c.normalise();
                if wrapped && v["replace_scene"] != true {
                    let mut base: Config =
                        serde_json::from_value(v["base"].clone()).map_err(|e| e.to_string())?;
                    base.normalise();
                    if base.channels.len() != self.config.channels.len()
                        || base.buses.len() != self.config.buses.len()
                    {
                        return Err("La estructura cambió desde otra consola; espera la sincronización y repite el ajuste".into());
                    }
                    let structure = c.channels.len() != base.channels.len()
                        || c.buses.len() != base.buses.len();
                    if structure && json!(base) != json!(self.config) {
                        return Err("La mezcla cambió; espera la sincronización antes de modificar la estructura".into());
                    }
                    c = serde_json::from_value(remote::merge_values(
                        &json!(base),
                        &json!(c),
                        &json!(self.config),
                    ))
                    .map_err(|e| e.to_string())?;
                }
                let topology_changed = c.channels.len() != self.config.channels.len()
                    || c.buses.len() != self.config.buses.len();
                c.validate()?;
                let active = self.live || self.demo.is_some();
                if active && !self.config.same_device(&c) {
                    return Err("Detén el motor para cambiar dispositivo, entradas ASIO abiertas, frecuencia o buffer".into());
                }
                if self.live && !self.system_live && c.output_channels() > self.audio.outputs {
                    return Err("La ruta mono/estéreo excede las salidas ASIO abiertas".into());
                }
                if self.diagnostics.busy() && !self.config.same_graph(&c) {
                    return Err(
                        "Detén la captura antes de cambiar la estructura o las cadenas".into(),
                    );
                }
                if active && !self.config.same_graph(&c) {
                    let controls = Controls::new(&c);
                    let mut next =
                        Mixer::reconfigure(&c, controls.clone(), &self.config, &self.latencies)?;
                    next.diagnostics = Some(self.diagnostics.tap());
                    next.listen = Some(self.listen.clone());
                    next.audio_network = Some(self.audio_network.clone());
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
                self.audio_network.configure(&c.audio_network);
                if topology_changed || !wrapped || v["replace_scene"] == true {
                    self.remote.invalidate();
                } else {
                    self.remote.changed();
                }
                if c.device != self.config.device {
                    self.device_name = None;
                }
                self.config = c;
                self.remote.refresh(
                    &self.config,
                    &self.controls,
                    self.live || self.demo.is_some(),
                );
                Ok(
                    json!({"ok":true,"config":self.config,"remote_revision":self.remote.status()["revision"]}),
                )
            }
            "/api/start" => {
                let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
                self.start(v["mode"].as_str().unwrap_or(""))
            }
            "/api/stop" => {
                self.cancel_bench();
                self.stop();
                Ok(self.status())
            }
            "/api/panic" => {
                self.cancel_bench();
                self.diagnostics.cancel();
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
                let devices = self.audio.devices()?;
                self.device_name = devices
                    .iter()
                    .find(|d| Some(d.id) == self.config.device)
                    .map(|d| d.name.clone());
                Ok(json!(devices))
            }
            "/api/vst3" => {
                let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
                let extra = v["directory"].as_str();
                if extra.is_some_and(|p| p.len() > 4096) {
                    return Err("Path too long".into());
                }
                Ok(fx::installed(extra))
            }
            "/api/remote" => {
                let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
                let result = self.remote.admin(&v, &self.config)?;
                self.remote.refresh(
                    &self.config,
                    &self.controls,
                    self.live || self.demo.is_some(),
                );
                Ok(result)
            }
            "/api/network-host" => {
                let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
                let host = listen::share_host(v["host"].as_str().ok_or("Dirección requerida")?)?;
                self.listen.set_host(&host)?;
                self.remote.set_host(&host)?;
                Ok(json!({"listen":self.listen.status(),"remote":self.remote.status()}))
            }
            "/api/listen-access" => {
                let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
                self.listen.gate.configure(
                    v["mode"].as_str().ok_or("Modo requerido")?,
                    v["password"].as_str(),
                )?;
                Ok(self.listen.status())
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
                    effects.push(json!({"id":d.id,"name":d.name,"vendor":d.vendor,"sub_categories":d.sub_categories,"params":params}));
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
        self.remote.stop();
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
    #[cfg(windows)]
    let tray = if args.iter().any(|a| a == "--no-browser") {
        None
    } else {
        let Some(tray) = tray::Tray::claim()? else {
            return Ok(());
        };
        Some(tray)
    };
    let server = Server::http("127.0.0.1:0").map_err(|e| e.to_string())?;
    let url = format!("http://{}", server.server_addr());
    let mut random = [0u8; 32];
    getrandom::fill(&mut random).map_err(|e| e.to_string())?;
    let token: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let page = include_str!("../web/index.html").replace("__TOKEN__", &token);
    let mut app = App::new()?;
    println!(
        "AuMux {}\n{url}\nAudio detenido. Usa Cerrar servicio en el icono de notificación para salir.",
        env!("CARGO_PKG_VERSION")
    );
    if !args.iter().any(|a| a == "--no-browser") {
        #[cfg(windows)]
        open_app_window(&url);
        #[cfg(not(windows))]
        let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
    }
    #[cfg(windows)]
    if let Some(tray) = &tray {
        tray.start()?;
        app.update_tray(tray);
    }
    let mut next_remote = Instant::now();
    loop {
        if app.system.refresh_if_due() {
            #[cfg(windows)]
            if let Some(tray) = &tray {
                app.update_tray(tray);
            }
        }
        #[cfg(windows)]
        if let Some(tray) = &tray {
            let actions = tray.poll();
            if actions & 2 != 0 {
                app.stop();
                app.listen.stop();
                app.remote.stop();
                return Ok(());
            }
            if actions & 1 != 0 {
                open_app_window(&url);
            }
        }
        for _ in 0..4 {
            let Some(rpc) = app.remote.take_rpc() else {
                break;
            };
            let result = if Instant::now() > rpc.deadline
                || !app.remote.console_gate.authorized(&rpc.cookie)
                || app.remote.status()["enabled"] != true
            {
                Err("El acceso caducó o fue revocado".into())
            } else {
                match rpc.body {
                    Some(body) => app.api(&rpc.path, &body),
                    None => app.read_api(&rpc.path),
                }
            };
            let _ = rpc.reply.try_send(result);
        }
        app.benchmark_tick();
        if app.controls.failed.load(Relaxed) {
            app.diagnostics.cancel();
        }
        app.diagnostics.tick();
        app.restore_probe();
        app.demo_tick();
        if !app.diagnostics.probing() {
            app.remote.drain(
                &mut app.config,
                &app.controls,
                app.live || app.demo.is_some(),
            );
        }
        if Instant::now() >= next_remote {
            let latency = app.latency();
            app.remote.set_latency(latency.clone());
            app.listen.set_latency(latency);
            app.remote
                .refresh(&app.config, &app.controls, app.live || app.demo.is_some());
            next_remote = Instant::now() + Duration::from_millis(80);
        }
        let Some(mut req) = server
            .recv_timeout(Duration::from_millis(5))
            .map_err(|e| e.to_string())?
        else {
            continue;
        };
        let path = req.url().split('?').next().unwrap_or("").to_string();
        if *req.method() == Method::Get {
            match path.as_str() {
                "/brand-logo.png" => {
                    let response = Response::from_data(include_bytes!("../assets/aumux-logo.png").as_slice())
                        .with_header(Header::from_bytes("Content-Type", "image/png").unwrap())
                        .with_header(Header::from_bytes("X-Content-Type-Options", "nosniff").unwrap());
                    let _ = req.respond(response);
                }
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
                "/tools-ui.js" => reply(req,200,"text/javascript; charset=utf-8",include_str!("../web/tools-ui.js").into()),
                "/api/sync-state" => reply(req,200,"application/json",json!({"revision":app.remote.status()["revision"],"config":app.config}).to_string()),
                "/api/diagnostics/report" => reply(req,200,"application/json",app.diagnostics_report().to_string()),
                "/api/diagnostics/live" => reply(req,200,"application/json",app.diagnostics.live_status().to_string()),
                "/api/diagnostics" => reply(req,200,"application/json",app.diagnostics_status().to_string()),
                "/panels-ui.js" => reply(req,200,"text/javascript; charset=utf-8",include_str!("../web/panels-ui.js").into()),
                "/diagnostics-ui.js" => reply(req,200,"text/javascript; charset=utf-8",include_str!("../web/diagnostics-ui.js").into()),
                "/api/latency" => reply(req,200,"application/json",app.latency().to_string()),
                "/api/addresses" => reply(req,200,"application/json",network::addresses().to_string()),
                "/effects-ui.js" => reply(req,200,"text/javascript; charset=utf-8",include_str!("../web/effects-ui.js").into()),
                "/plugin-browser.js" => reply(req,200,"text/javascript; charset=utf-8",include_str!("../web/plugin-browser.js").into()),
                "/api/mix-state" => reply(req,200,"application/json",json!({"revision":app.remote.status()["revision"],"channels":app.config.channels.iter().map(|ch|json!({"sends_db":ch.sends_db,"send_mute":ch.send_mute,"pre":ch.pre})).collect::<Vec<_>>()}).to_string()),
                "/api/config" => reply(
                    req,
                    200,
                    "application/json",
                    serde_json::to_string(&app.config).map_err(|e| e.to_string())?,
                ),
                "/api/panel-status" => reply(req,200,"application/json",app.panel_status().to_string()),
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
    tray::open_url(url);
}
fn main() {
    if let Err(e) = run() {
        eprintln!("AuMux: {e}");
        #[cfg(windows)]
        if !std::env::args().any(|arg| arg == "--no-browser" || arg == "--self-test") {
            tray::error(&e);
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod panel_status_tests {
    use super::*;
    #[test]
    fn panel_observers_do_not_consume_main_console_peak_accumulators() {
        let app = App::new().unwrap();
        app.controls.meters[0].store(0.75f32.to_bits(), Relaxed);
        assert_eq!(app.panel_status()["meters"][0], 0.75);
        assert_eq!(app.panel_status()["meters"][0], 0.75);
        assert_eq!(app.status()["meters"][0], 0.75);
        assert_eq!(app.controls.meters[0].load(Relaxed), 0);
    }
}
