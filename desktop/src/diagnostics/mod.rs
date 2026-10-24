//! One audio producer, bounded copies, analysis on a dedicated worker.
pub mod analysis;
pub mod bench;
pub mod inventory;
pub mod performance;
pub mod signal;
use crossbeam_queue::ArrayQueue;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{
        AtomicU64,
        Ordering::{Acquire, Relaxed, Release},
    },
};
use std::time::{Duration, Instant};
const CHUNK: usize = 128;
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Source {
    pub kind: u8,
    pub index: usize,
    pub side: usize,
}
impl Source {
    pub fn valid(self, c: &crate::config::Config) -> bool {
        self.side < 2
            && match self.kind {
                0 => self.index < c.inputs as usize,
                1 => self.index < c.channels.len(),
                2 => self.index < c.buses.len(),
                _ => false,
            }
    }
    fn sample(
        self,
        input: &[f32],
        stride: usize,
        channels: &[Vec<Vec<f32>>],
        buses: &[Vec<Vec<f32>>],
        f: usize,
    ) -> f32 {
        let v = match self.kind {
            0 => input.get(f * stride + self.index).copied(),
            1 => channels
                .get(self.index)
                .and_then(|c| c.get(self.side))
                .and_then(|v| v.get(f))
                .copied(),
            2 => buses
                .get(self.index)
                .and_then(|c| c.get(self.side))
                .and_then(|v| v.get(f))
                .copied(),
            _ => None,
        }
        .unwrap_or(0.0);
        if v.is_finite() { v } else { 0.0 }
    }
}
#[derive(Clone, Copy)]
struct Command {
    epoch: u64,
    probe: bool,
    input: usize,
    output: usize,
    route: Option<(usize, usize)>,
    a: Source,
    b: Source,
}
#[derive(Clone, Copy)]
struct Packet {
    epoch: u64,
    probe: bool,
    offset: usize,
    len: usize,
    a: [f32; CHUNK],
    b: [f32; CHUNK],
}
#[derive(Default)]
struct Snapshot {
    label: String,
    state: String,
    result: Option<signal::ProbeResult>,
    history: Vec<Value>,
    analysis: Value,
    context: Value,
    started: Option<Instant>,
    updated: Option<Instant>,
    probe: bool,
}
pub struct Hub {
    commands: ArrayQueue<Command>,
    packets: ArrayQueue<Packet>,
    epoch: AtomicU64,
    active: AtomicU64,
    progress: AtomicU64,
    drops: AtomicU64,
    scope_frames: AtomicU64,
    scope_rising: std::sync::atomic::AtomicBool,
    snapshot: Mutex<Snapshot>,
}
pub struct Tap {
    hub: Arc<Hub>,
    command: Option<Command>,
    cursor: usize,
    block_start: usize,
    resume: bool,
}
impl Hub {
    pub fn new() -> Arc<Self> {
        let hub = Arc::new(Self {
            commands: ArrayQueue::new(1),
            packets: ArrayQueue::new(256),
            epoch: AtomicU64::new(0),
            active: AtomicU64::new(0),
            progress: AtomicU64::new(0),
            drops: AtomicU64::new(0),
            scope_frames: AtomicU64::new(4096),
            scope_rising: std::sync::atomic::AtomicBool::new(false),
            snapshot: Mutex::new(Snapshot::default()),
        });
        let weak = Arc::downgrade(&hub);
        std::thread::Builder::new().name("aumux-analysis".into()).spawn(move || {
            let mut epoch=0;let mut capture=Vec::new();let mut gap=false;let mut a=Vec::new();let mut b=Vec::new();let mut expected=0;let mut analyzer=analysis::Analyzer::new();
            loop {
                let Some(hub)=weak.upgrade() else {break;};
                for _ in 0..256 {
                    let Some(p)=hub.packets.pop() else{break;};
                    if p.epoch!=hub.active.load(Acquire){continue;}
                    if p.epoch!=epoch {epoch=p.epoch;capture.clear();gap=false;a.clear();b.clear();expected=0;analyzer=analysis::Analyzer::new();}
                    if p.probe {
                        gap|=p.offset!=capture.len();capture.extend_from_slice(&p.a[..p.len]);
                        if capture.len()>=signal::CAPTURE {
                            let result=signal::detect_probe(&capture,gap||hub.drops.load(Relaxed)>0);
                            let mut s=hub.snapshot.lock().unwrap();
                            if hub.active.load(Acquire)==epoch {
                                let row=json!({"label":s.label,"context":s.context,"result":result});
                                if s.history.len()==100 {s.history.remove(0);}s.history.push(row);
                                s.state="complete".into();s.result=Some(result);hub.active.store(0,Release);
                            }
                        }
                    }else {
                        if p.offset!=expected {a.clear();b.clear();analyzer=analysis::Analyzer::new();}
                        expected=p.offset+p.len;a.extend_from_slice(&p.a[..p.len]);b.extend_from_slice(&p.b[..p.len]);
                        while a.len()>=analysis::WINDOW {
                            let mut result=analyzer.analyze_window(&a[..analysis::WINDOW],&b[..analysis::WINDOW],hub.scope_frames.load(Relaxed) as usize,hub.scope_rising.load(Relaxed));
                            result["through_frame"]=json!(expected-a.len()+analysis::WINDOW);result["dropped_chunks"]=json!(hub.drops.load(Relaxed));
                            let mut s=hub.snapshot.lock().unwrap();if hub.active.load(Acquire)==epoch{s.analysis=result;s.updated=Some(Instant::now());s.state="monitoring".into();}
                            drop(s);a.drain(..analysis::HOP);b.drain(..analysis::HOP);
                        }
                    }
                }
                drop(hub);std::thread::sleep(Duration::from_millis(4));
            }
        }).expect("analysis worker");
        hub
    }
    pub fn tap(self: &Arc<Self>) -> Tap {
        Tap {
            hub: self.clone(),
            command: None,
            cursor: 0,
            block_start: 0,
            resume: false,
        }
    }
    fn begin(&self, mut c: Command, label: &str, context: Value) -> Result<Value, String> {
        if self.active.load(Acquire) != 0 {
            return Err("Ya hay una captura activa; detenla primero".into());
        }
        while self.commands.pop().is_some() {} // discard a command cancelled before any callback.
        c.epoch = self.epoch.fetch_add(1, Relaxed) + 1;
        {
            let mut s = self.snapshot.lock().unwrap();
            s.label = label.chars().take(120).collect();
            s.context = context;
            s.state = "capturing".into();
            s.started = Some(Instant::now());
            s.probe = c.probe;
            s.result = None;
            s.updated = None;
            s.analysis = Value::Null;
        }
        self.progress.store(0, Relaxed);
        self.drops.store(0, Relaxed);
        self.active.store(c.epoch, Release);
        if self.commands.push(c).is_err() {
            self.active.store(0, Release);
            return Err("Captura pendiente".into());
        }
        Ok(self.status())
    }
    pub fn probe(
        &self,
        input: usize,
        output: usize,
        label: &str,
        context: Value,
    ) -> Result<Value, String> {
        let source = Source {
            kind: 0,
            index: input,
            side: 0,
        };
        self.begin(
            Command {
                epoch: 0,
                probe: true,
                input,
                output,
                route: None,
                a: source,
                b: source,
            },
            label,
            context,
        )
    }
    pub fn scope_window(&self, frames: usize, rising: bool) {
        self.scope_frames
            .store(frames.clamp(48, 4096) as u64, Relaxed);
        self.scope_rising.store(rising, Relaxed);
    }
    pub fn monitor(&self, a: Source, b: Source, context: Value) -> Result<Value, String> {
        self.begin(
            Command {
                epoch: 0,
                probe: false,
                input: 0,
                output: 0,
                route: None,
                a,
                b,
            },
            "Análisis de señal",
            context,
        )
    }
    pub fn route_probe(
        &self,
        input: usize,
        output: usize,
        channel: usize,
        bus: usize,
        label: &str,
        context: Value,
    ) -> Result<Value, String> {
        let source = Source {
            kind: 0,
            index: input,
            side: 0,
        };
        self.begin(
            Command {
                epoch: 0,
                probe: true,
                input,
                output,
                route: Some((channel, bus)),
                a: source,
                b: source,
            },
            label,
            context,
        )
    }
    pub fn probing(&self) -> bool {
        self.busy() && self.snapshot.lock().unwrap().probe
    }
    pub fn cancel(&self) {
        self.active.store(0, Release);
        let mut s = self.snapshot.lock().unwrap();
        if s.state == "capturing" || s.state == "monitoring" {
            s.state = "cancelled".into();
        }
    }
    pub fn tick(&self) {
        let mut s = self.snapshot.lock().unwrap();
        if self.busy()
            && s.probe
            && s.started
                .is_some_and(|t| t.elapsed() > Duration::from_secs(3))
        {
            self.active.store(0, Release);
            s.state = "timeout".into();
            s.result = Some(signal::ProbeResult {
                samples: None,
                ms: None,
                correlation: 0.0,
                inverted: false,
                peak_dbfs: None,
                error: Some(
                    "El motor no completó la captura: timeout, interrupción o cola saturada".into(),
                ),
            });
        }
    }
    pub fn busy(&self) -> bool {
        self.active.load(Acquire) != 0
    }
    pub fn live_status(&self) -> Value {
        let s = self.snapshot.lock().unwrap();
        json!({"state":s.state,"label":s.label,"active":self.busy(),"captured_frames":self.progress.load(Relaxed),"dropped_chunks":self.drops.load(Relaxed),"result":s.result,"analysis":s.analysis,"analysis_age_ms":s.updated.map(|t|t.elapsed().as_millis() as u64),"context":s.context})
    }
    pub fn status(&self) -> Value {
        let mut value = self.live_status();
        value["history"] = json!(self.snapshot.lock().unwrap().history);
        value
    }
}
impl Tap {
    pub fn discontinuity(&self) {
        if self.hub.busy() {
            self.hub.drops.fetch_add(1, Relaxed);
        }
    }
    fn command(&mut self) -> Option<Command> {
        let was_probe = self.command.is_some_and(|c| c.probe);
        if let Some(c) = self.hub.commands.pop() {
            self.command = Some(c);
            self.cursor = 0;
        }
        self.command = self
            .command
            .filter(|c| c.epoch == self.hub.active.load(Acquire));
        if was_probe && !self.command.is_some_and(|c| c.probe) {
            self.resume = true;
        }
        self.command
    }
    pub fn take_resume(&mut self) -> bool {
        std::mem::take(&mut self.resume)
    }
    pub fn probe(
        &mut self,
        input: &[f32],
        stride: usize,
        output: &mut [f32],
        outputs: usize,
        frames: usize,
    ) -> bool {
        let Some(c) = self.command().filter(|c| c.probe) else {
            return false;
        };
        if c.input >= stride || c.output >= outputs {
            self.hub.active.store(0, Release);
            return false;
        }
        self.block_start = self.cursor;
        if c.route.is_none() {
            for f in 0..frames {
                output[f * outputs + c.output] = signal::probe_sample(self.cursor + f);
            }
        }
        self.capture(c, input, stride, &[], &[], frames);
        c.route.is_none()
    }
    pub fn monitor(
        &mut self,
        input: &[f32],
        stride: usize,
        channels: &[Vec<Vec<f32>>],
        buses: &[Vec<Vec<f32>>],
        frames: usize,
    ) {
        if let Some(c) = self.command().filter(|c| !c.probe) {
            self.capture(c, input, stride, channels, buses, frames);
        }
    }
    pub fn route(&self) -> Option<(usize, usize)> {
        self.command.and_then(|c| c.route)
    }
    pub fn excitation(&self, f: usize) -> f32 {
        signal::probe_sample(self.block_start + f)
    }
    fn capture(
        &mut self,
        c: Command,
        input: &[f32],
        stride: usize,
        channels: &[Vec<Vec<f32>>],
        buses: &[Vec<Vec<f32>>],
        frames: usize,
    ) {
        let available = if c.probe {
            signal::CAPTURE.saturating_sub(self.cursor).min(frames)
        } else {
            frames
        };
        for base in (0..available).step_by(CHUNK) {
            let len = (available - base).min(CHUNK);
            let mut p = Packet {
                epoch: c.epoch,
                probe: c.probe,
                offset: self.cursor + base,
                len,
                a: [0.0; CHUNK],
                b: [0.0; CHUNK],
            };
            for i in 0..len {
                p.a[i] = c.a.sample(input, stride, channels, buses, base + i);
                p.b[i] = c.b.sample(input, stride, channels, buses, base + i);
            }
            if self.hub.packets.push(p).is_err() {
                self.hub.drops.fetch_add(1, Relaxed);
            }
        }
        self.cursor += available;
        self.hub.progress.store(self.cursor as u64, Relaxed);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_crosses_arbitrary_block_boundaries_without_heap_or_other_output_leakage() {
        let hub = Hub::new();
        let mut tap = hub.tap();
        hub.probe(1, 2, "Cable", json!({})).unwrap();
        let mut output = vec![0.0; 127 * 4];
        let mut input = vec![0.0; 127 * 2];
        let delay = 513;
        let mut position = 0;
        while position < signal::CAPTURE {
            output.fill(0.0);
            for f in 0..127 {
                input[f * 2 + 1] = if position + f >= delay {
                    signal::probe_sample(position + f - delay)
                } else {
                    0.0
                };
            }
            crate::test_support::no_heap(|| assert!(tap.probe(&input, 2, &mut output, 4, 127)));
            for f in 0..127 {
                assert_eq!(output[f * 4 + 2], signal::probe_sample(position + f));
                assert_eq!(output[f * 4], 0.0);
                assert_eq!(output[f * 4 + 1], 0.0);
            }
            position += 127;
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while hub.busy() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(hub.status()["result"]["samples"], delay);
        assert_eq!(hub.status()["dropped_chunks"], 0);
        hub.cancel();
        output.fill(0.0);
        assert!(!tap.probe(&input, 2, &mut output, 4, 127));
    }
}
#[cfg(test)]
mod route_tests {
    use super::*;
    use crate::{
        config::*,
        mixer::{Controls, Mixer},
    };
    #[test]
    fn actual_route_preserves_dsp_isolates_other_generators_buses_and_network_audio() {
        let mut c = Config::default();
        c.inputs = 2;
        for ch in &mut c.channels {
            ch.input = None;
            ch.input_right = None;
            ch.generator.signal = Signal::Sine;
            ch.generator.level_db = -6.0;
            ch.sends_db.fill(0.0);
            ch.gain_db = 0.0;
        }
        for (i, b) in c.buses.iter_mut().enumerate() {
            b.gain_db = 0.0;
            b.output = Some(i * 2);
        }
        let controls = Controls::new(&c);
        let mut mixer = Mixer::new(&c, controls).unwrap();
        let hub = Hub::new();
        mixer.diagnostics = Some(hub.tap());
        hub.route_probe(1, 2, 0, 1, "Aux", json!({"kind":"physical_route"}))
            .unwrap();
        let (mut input, mut output) = (vec![0.0; 128], vec![0.0; 640]);
        let mut position = 0;
        while position < signal::CAPTURE {
            for f in 0..64 {
                input[f * 2 + 1] = if position + f >= 513 {
                    signal::probe_sample(position + f - 513)
                } else {
                    0.0
                };
            }
            crate::test_support::no_heap(|| assert!(mixer.process(&input, &mut output, 64, 10)));
            for f in 0..64 {
                for o in [0, 1, 4, 5, 6, 7, 8, 9] {
                    assert_eq!(output[f * 10 + o], 0.0);
                }
                assert!((output[f * 10 + 2] - signal::probe_sample(position + f)).abs() < 0.001);
            }
            assert!(
                mixer
                    .buses
                    .iter()
                    .flatten()
                    .all(|lane| lane[..64].iter().all(|&v| v == 0.0))
            );
            position += 64;
            if position % 2048 == 0 {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while hub.busy() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(hub.status()["result"]["samples"], 513);
        hub.cancel();
        mixer.process(&input, &mut output, 64, 10);
        assert!(output.iter().any(|&v| v != 0.0), "normal generators resume");
    }
}
#[cfg(test)]
mod monitor_tests {
    use super::*;
    #[test]
    fn bounded_monitor_capture_has_no_heap_and_reports_queue_overload() {
        let hub = Hub::new();
        let mut tap = hub.tap();
        let source = Source {
            kind: 0,
            index: 0,
            side: 0,
        };
        hub.monitor(source, source, json!({})).unwrap();
        let input = vec![0.1; 8192];
        let mut output = vec![0.0; 8192];
        for _ in 0..10 {
            crate::test_support::no_heap(|| {
                assert!(!tap.probe(&input, 1, &mut output, 1, 8192));
                tap.monitor(&input, 1, &[], &[], 8192);
            });
        }
        assert!(hub.progress.load(Relaxed) > 0);
        hub.cancel();
        crate::test_support::no_heap(|| tap.monitor(&input, 1, &[], &[], 8192));
    }
}
