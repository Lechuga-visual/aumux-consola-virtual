use crate::config::*;
use soundcraft_dsp::Plugin;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed},
};

pub struct Meter {
    peak: [AtomicU32; 2],
    last_peak: [AtomicU32; 2],
    rms: [AtomicU32; 2],
}
impl Meter {
    fn new() -> Self {
        Self {
            peak: std::array::from_fn(|_| AtomicU32::new(0)),
            last_peak: std::array::from_fn(|_| AtomicU32::new(0)),
            rms: std::array::from_fn(|_| AtomicU32::new(0)),
        }
    }
    pub fn read(&self) -> ([f32; 2], [f32; 2]) {
        (
            std::array::from_fn(|i| f32::from_bits(self.peak[i].swap(0, Relaxed))),
            std::array::from_fn(|i| f32::from_bits(self.rms[i].load(Relaxed))),
        )
    }
    pub fn snapshot(&self) -> ([f32; 2], [f32; 2]) {
        (
            std::array::from_fn(|i| f32::from_bits(self.last_peak[i].load(Relaxed))),
            std::array::from_fn(|i| f32::from_bits(self.rms[i].load(Relaxed))),
        )
    }
    fn record(&self, peak: [f32; 2], energy: [f64; 2], frames: usize, rate: f32) {
        for i in 0..2 {
            self.peak[i].fetch_max(peak[i].to_bits(), Relaxed);
            self.last_peak[i].store(peak[i].to_bits(), Relaxed);
            // Average POWER in the audio clock, independent of UI polling and block size.
            // 300 ms exponential integration, then convert to RMS amplitude for dBFS.
            let previous = f32::from_bits(self.rms[i].load(Relaxed));
            let alpha = 1.0 - (-(frames as f64) / (f64::from(rate) * 0.3)).exp();
            let power = f64::from(previous).powi(2) * (1.0 - alpha)
                + energy[i] / frames.max(1) as f64 * alpha;
            self.rms[i].store((power.sqrt() as f32).to_bits(), Relaxed);
        }
    }
    fn inherit(&self, old: &Self) {
        for i in 0..2 {
            self.peak[i].store(old.peak[i].load(Relaxed), Relaxed);
            self.last_peak[i].store(old.last_peak[i].load(Relaxed), Relaxed);
            self.rms[i].store(old.rms[i].load(Relaxed), Relaxed);
        }
    }
    fn silence(&self) {
        for p in &self.last_peak {
            p.store(0, Relaxed);
        }
        for x in &self.rms {
            x.store(0, Relaxed);
        }
    }
}

struct NativeValue {
    id: &'static str,
    default: f32,
    value: AtomicU32,
}
fn native_values(spec: Option<&Fx>) -> Vec<NativeValue> {
    let Some(spec) = spec.filter(|s| s.id.starts_with("native/")) else {
        return vec![];
    };
    let Some(info) = crate::native_fx::registry()
        .iter()
        .find(|p| p.id == spec.id)
    else {
        return vec![];
    };
    info.params
        .iter()
        .map(|p| NativeValue {
            id: p.id,
            default: p.default,
            value: AtomicU32::new(
                spec.params
                    .get(p.id)
                    .copied()
                    .unwrap_or(p.default)
                    .to_bits(),
            ),
        })
        .collect()
}
pub struct Controls {
    pub performance: crate::diagnostics::performance::Performance,
    eq: Vec<crate::eq::Control>,
    pub compressor: Vec<crate::compressor::Control>,
    native_params: Vec<[Vec<NativeValue>; INSERTS]>,
    input_route: Vec<AtomicU32>,
    bus_stereo: Vec<AtomicBool>,
    gain: Vec<AtomicU32>,
    pan: Vec<AtomicU32>,
    mute: Vec<AtomicBool>,
    sends: Vec<Vec<AtomicU32>>,
    pre: Vec<Vec<AtomicBool>>,
    bus_gain: Vec<AtomicU32>,
    bus_mute: Vec<AtomicBool>,
    pub emergency: AtomicBool,
    pub resume_fade: AtomicBool,
    pub failed: AtomicBool,
    pub xruns: AtomicU64,
    pub clipped: AtomicU64,
    pub meters: Vec<AtomicU32>,
    pub channel_meters: Vec<Meter>,
    pub bus_meters: Vec<Meter>,
    pub pc_monitor: AtomicU32,
    generator: Vec<AtomicU32>,
    generator_gain: Vec<AtomicU32>,
    frequency: Vec<AtomicU32>,
    bypass: Vec<[AtomicBool; INSERTS]>,
}
pub fn gain(db: f32) -> f32 {
    if db <= -90.0 {
        0.0
    } else {
        soundcraft_dsp::db_to_gain(db)
    }
}
impl Controls {
    pub fn new(c: &Config) -> Arc<Self> {
        let channels = c.channels.len();
        let buses = c.buses.len();
        let auxes = buses - 1;
        let floats = |n| (0..n).map(|_| AtomicU32::new(0)).collect::<Vec<_>>();
        let flags = |n| (0..n).map(|_| AtomicBool::new(false)).collect::<Vec<_>>();
        let s = Arc::new(Self {
            performance: Default::default(),
            compressor: c
                .channels
                .iter()
                .map(|c| c.compressor)
                .chain(c.buses.iter().map(|b| b.compressor))
                .map(crate::compressor::Control::new)
                .collect(),
            native_params: c
                .channels
                .iter()
                .map(|ch| std::array::from_fn(|slot| native_values(ch.effects().nth(slot))))
                .chain(
                    c.buses
                        .iter()
                        .map(|b| std::array::from_fn(|slot| native_values(b.effects().nth(slot)))),
                )
                .collect(),
            eq: c
                .channels
                .iter()
                .map(|ch| &ch.eq)
                .chain(c.buses.iter().map(|b| &b.eq))
                .map(crate::eq::Control::new)
                .collect(),
            input_route: floats(channels),
            bus_stereo: flags(buses),
            gain: floats(channels),
            pan: floats(channels),
            mute: flags(channels),
            sends: (0..channels).map(|_| floats(auxes)).collect(),
            pre: (0..channels).map(|_| flags(auxes)).collect(),
            bus_gain: floats(buses),
            bus_mute: flags(buses),
            emergency: AtomicBool::new(false),
            resume_fade: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            xruns: AtomicU64::new(0),
            clipped: AtomicU64::new(0),
            meters: floats(buses),
            channel_meters: (0..channels).map(|_| Meter::new()).collect(),
            bus_meters: (0..buses).map(|_| Meter::new()).collect(),
            pc_monitor: AtomicU32::new(0),
            generator: floats(channels),
            generator_gain: floats(channels),
            frequency: floats(channels),
            bypass: (0..channels + buses)
                .map(|_| std::array::from_fn(|_| AtomicBool::new(false)))
                .collect(),
        });
        s.update(c);
        s
    }
    fn update_native(&self, node: usize, slot: usize, spec: &Fx) {
        for p in &self.native_params[node][slot] {
            p.value.store(
                spec.params
                    .get(p.id)
                    .copied()
                    .unwrap_or(p.default)
                    .to_bits(),
                Relaxed,
            );
        }
    }
    fn apply_native(&self, node: usize, slot: usize, plugin: &mut dyn Plugin) {
        for p in &self.native_params[node][slot] {
            plugin.set_param(p.id, f32::from_bits(p.value.load(Relaxed)));
        }
    }
    pub fn set_send(&self, channel: usize, aux: usize, db: f32, pre: bool, muted: bool) {
        self.sends[channel][aux].store(gain(if muted { -90.0 } else { db }).to_bits(), Relaxed);
        self.pre[channel][aux].store(pre, Relaxed);
    }
    pub fn update(&self, c: &Config) {
        self.pc_monitor.store(c.pc_monitor as u32, Relaxed);
        for (i, ch) in c.channels.iter().enumerate() {
            self.eq[i].update(&ch.eq);
            self.compressor[i].update(ch.compressor);
            // One atomic word prevents torn input-L/input-R/format changes.
            self.input_route[i].store(
                ch.input.map_or(0, |v| v as u32 + 1)
                    | (ch.input_right.map_or(0, |v| v as u32 + 1) << 8)
                    | (u32::from(ch.format == Format::Stereo) << 16),
                Relaxed,
            );
            self.gain[i].store(gain(ch.gain_db).to_bits(), Relaxed);
            self.pan[i].store(ch.pan.to_bits(), Relaxed);
            self.mute[i].store(ch.mute, Relaxed);
            self.generator[i].store(ch.generator.signal as u32, Relaxed);
            self.generator_gain[i].store(gain(ch.generator.level_db).to_bits(), Relaxed);
            self.frequency[i].store(ch.generator.frequency.to_bits(), Relaxed);
            for (slot, fx) in ch.effects().take(INSERTS).enumerate() {
                self.bypass[i][slot].store(fx.bypass, Relaxed);
                self.update_native(i, slot, fx);
            }
            for a in 0..c.buses.len() - 1 {
                self.set_send(
                    i,
                    a,
                    ch.sends_db[a],
                    ch.pre[a],
                    ch.send_mute.get(a).copied().unwrap_or(false),
                );
            }
        }
        for (i, b) in c.buses.iter().enumerate() {
            self.eq[c.channels.len() + i].update(&b.eq);
            self.compressor[c.channels.len() + i].update(b.compressor);
            self.bus_stereo[i].store(b.format == Format::Stereo, Relaxed);
            self.bus_gain[i].store(gain(b.gain_db).to_bits(), Relaxed);
            self.bus_mute[i].store(b.mute, Relaxed);
            for (slot, fx) in b.effects().take(INSERTS).enumerate() {
                self.bypass[c.channels.len() + i][slot].store(fx.bypass, Relaxed);
                self.update_native(c.channels.len() + i, slot, fx);
            }
        }
    }
}

pub struct Mixer {
    pub diagnostics: Option<crate::diagnostics::Tap>,
    pub listen: Option<Arc<crate::listen::Hub>>,
    pub audio_network: Option<Arc<crate::audio_network::Hub>>,
    pub recording: Option<Arc<crate::recording::Tap>>,
    pub spectrum: Option<Arc<crate::spectrum::Hub>>,
    spectrum_pre: [Vec<f32>; 2],
    record_pre: Vec<Vec<Vec<f32>>>,
    record_post: Vec<Vec<Vec<f32>>>,
    pub controls: Arc<Controls>,
    channel: Vec<Vec<Vec<f32>>>,
    pub buses: Vec<Vec<Vec<f32>>>,
    channel_eq: Vec<crate::eq::Processor>,
    keys: Vec<Vec<Vec<f32>>>,
    channel_comp: Vec<crate::compressor::Processor>,
    bus_comp: Vec<crate::compressor::Processor>,
    bus_eq: Vec<crate::eq::Processor>,
    channel_fx: Vec<Vec<Option<Box<dyn Plugin>>>>,
    bus_fx: Vec<Vec<Option<Box<dyn Plugin>>>>,
    routes: Vec<Option<usize>>,
    previous: Vec<f32>,
    previous_sends: Vec<Vec<f32>>,
    previous_bus: Vec<f32>,
    pub latencies: Vec<usize>,
    pub fx_latencies: Vec<Vec<usize>>,
    channel_reuse: Vec<Vec<Option<usize>>>,
    bus_reuse: Vec<Vec<Option<usize>>>,
    chain_changed: Vec<bool>,
    chain_transition: Vec<i8>,
    fx_dry: [Vec<f32>; 2],
    generators: Vec<[crate::generator::Generator; 2]>,
    generator_modes: Vec<Signal>,
    generator_blend: Vec<f32>,
    generator_level: Vec<f32>,
    rate: f32,
    inputs: usize,
    previous_mono: Vec<f32>,
    previous_bus_mono: Vec<f32>,
    bus_mono: Vec<f32>,
}
fn begin_fx_transition(direction: i8, audio: &[Vec<f32>], dry: &mut [Vec<f32>; 2], frames: usize) {
    if direction != 0 {
        for side in 0..2 {
            dry[side][..frames].copy_from_slice(&audio[side][..frames]);
        }
    }
}
fn finish_fx_transition(
    direction: &mut i8,
    audio: &mut [Vec<f32>],
    dry: &[Vec<f32>; 2],
    frames: usize,
) {
    if *direction == 0 {
        return;
    }
    for side in 0..2 {
        for frame in 0..frames {
            let position = (frame + 1) as f32 / frames as f32;
            let wet = if *direction > 0 {
                position
            } else {
                1.0 - position
            };
            audio[side][frame] = dry[side][frame] + (audio[side][frame] - dry[side][frame]) * wet;
        }
    }
    *direction = 0;
}
fn prepare_fx(spec: &Fx, rate: u32) -> Result<Box<dyn Plugin>, String> {
    if spec.id.starts_with("native/") {
        let mut p = crate::native_fx::instantiate(&spec.id)?;
        p.prepare(rate as f32, MAX_FRAMES, 2);
        for (id, value) in &spec.params {
            if !p.set_param(id, *value) {
                return Err(format!("Parámetro nativo inválido: {id}"));
            }
        }
        return Ok(p);
    }
    let descriptors = crate::fx::load(std::path::Path::new(&spec.path))?;
    if !descriptors
        .iter()
        .any(|d| d.id == spec.id && !d.is_instrument)
    {
        return Err("The selected VST3 effect is not present in this bundle".into());
    }
    let mut p = soundcraft_vst3_host::instantiate(&spec.id).map_err(|e| e.to_string())?;
    p.prepare(rate as f32, MAX_FRAMES, 2);
    for (id, value) in &spec.params {
        if !p.set_param(id, *value) {
            return Err(format!("Unknown VST3 parameter: {id}"));
        }
    }
    Ok(p)
}
impl Mixer {
    pub fn attach_recording(&mut self, tap: Option<Arc<crate::recording::Tap>>) {
        let banks = |point| {
            (0..self.channel.len())
                .map(|i| {
                    (0..2)
                        .map(|_| {
                            if tap.as_ref().is_some_and(|t| t.wants(i, point)) {
                                vec![0.0; MAX_FRAMES]
                            } else {
                                Vec::new()
                            }
                        })
                        .collect()
                })
                .collect()
        };
        self.record_pre = banks(crate::recording::Point::Pre);
        self.record_post = banks(crate::recording::Point::Post);
        self.recording = tap;
    }
    /// Windows PC monitoring auditions one post-fader bus on the stereo device,
    /// independently of saved ASIO output assignments.
    pub fn copy_monitor(&self, output: &mut [f32], frames: usize, bus: usize) -> bool {
        output.fill(0.0);
        if bus >= self.buses.len() || frames > MAX_FRAMES || output.len() < frames * 2 {
            return false;
        }
        for f in 0..frames {
            for side in 0..2 {
                output[f * 2 + side] = self.buses[bus][side][f];
            }
        }
        true
    }
    /// Called on the control thread; all allocations and plugin creation happen here.
    pub fn new(c: &Config, controls: Arc<Controls>) -> Result<Self, String> {
        Self::prepare(c, controls, None)
    }
    pub fn reconfigure(
        c: &Config,
        controls: Arc<Controls>,
        old: &Config,
        latencies: &[Vec<usize>],
    ) -> Result<Self, String> {
        if !old.same_device(c) {
            return Err("Detén el motor para cambiar dispositivo, frecuencia o buffer".into());
        }
        Self::prepare(c, controls, Some((old, latencies)))
    }
    fn prepare(
        c: &Config,
        controls: Arc<Controls>,
        old: Option<(&Config, &[Vec<usize>])>,
    ) -> Result<Self, String> {
        c.validate()?;
        let channels = c.channels.len();
        let buses = c.buses.len();
        let mut channel_fx = Vec::with_capacity(channels);
        let mut bus_fx = Vec::with_capacity(buses);
        let mut channel_reuse = Vec::with_capacity(channels);
        let mut bus_reuse = Vec::with_capacity(buses);
        let mut fx_latencies = Vec::with_capacity(channels + buses);
        let mut chain_changed = Vec::with_capacity(channels + buses);
        for (is_bus, count) in [(false, channels), (true, buses)] {
            for i in 0..count {
                let specs: Vec<&Fx> = if is_bus {
                    c.buses[i].effects().collect()
                } else {
                    c.channels[i].effects().collect()
                };
                let previous: Vec<&Fx> = old
                    .map(|(prev, _)| {
                        if is_bus {
                            prev.buses
                                .get(i)
                                .map(|n| n.effects().collect())
                                .unwrap_or_default()
                        } else {
                            prev.channels
                                .get(i)
                                .map(|n| n.effects().collect())
                                .unwrap_or_default()
                        }
                    })
                    .unwrap_or_default();
                let previous_latencies = old
                    .and_then(|(prev, l)| l.get(if is_bus { prev.channels.len() + i } else { i }));
                let mut used = [false; INSERTS];
                let mut chain = Vec::with_capacity(specs.len());
                let mut reuse = Vec::with_capacity(specs.len());
                let mut latency = Vec::with_capacity(specs.len());
                for spec in &specs {
                    // Prefer matching parameters when two instances have the same plugin ID.
                    let matches = |slot: usize| !used[slot] && previous[slot].same_graph(spec);
                    let source = (0..previous.len())
                        .find(|&slot| matches(slot) && previous[slot].params == spec.params)
                        .or_else(|| (0..previous.len()).find(|&slot| matches(slot)));
                    if let Some(slot) = source {
                        let value = previous_latencies
                            .and_then(|l| l.get(slot))
                            .copied()
                            .ok_or("Falta la latencia del insert que se reutiliza")?;
                        used[slot] = true;
                        chain.push(None);
                        reuse.push(Some(slot));
                        latency.push(value);
                    } else {
                        let plugin = prepare_fx(spec, c.sample_rate)?;
                        latency.push(plugin.latency());
                        chain.push(Some(plugin));
                        reuse.push(None);
                    }
                }
                chain_changed.push(
                    old.is_some()
                        && (specs.len() != previous.len()
                            || reuse
                                .iter()
                                .enumerate()
                                .any(|(slot, source)| *source != Some(slot))),
                );
                fx_latencies.push(latency);
                if is_bus {
                    bus_fx.push(chain);
                    bus_reuse.push(reuse);
                } else {
                    channel_fx.push(chain);
                    channel_reuse.push(reuse);
                }
            }
        }
        let latencies = fx_latencies
            .iter()
            .map(|slots| slots.iter().sum())
            .collect();
        Ok(Self {
            diagnostics: None,
            listen: None,
            audio_network: None,
            recording: None,
            spectrum: None,
            spectrum_pre: std::array::from_fn(|_| vec![0.0; MAX_FRAMES]),
            record_pre: Vec::new(),
            record_post: Vec::new(),
            keys: (0..channels)
                .map(|_| vec![vec![0.0; MAX_FRAMES]; 2])
                .collect(),
            channel_comp: c
                .channels
                .iter()
                .map(|c| crate::compressor::Processor::new(c.compressor))
                .collect(),
            bus_comp: c
                .buses
                .iter()
                .map(|b| crate::compressor::Processor::new(b.compressor))
                .collect(),
            controls,
            channel: (0..channels)
                .map(|_| vec![vec![0.0; MAX_FRAMES]; 2])
                .collect(),
            buses: (0..buses).map(|_| vec![vec![0.0; MAX_FRAMES]; 2]).collect(),
            channel_eq: c
                .channels
                .iter()
                .map(|ch| crate::eq::Processor::new(ch.eq, c.sample_rate as f32))
                .collect(),
            bus_eq: c
                .buses
                .iter()
                .map(|b| crate::eq::Processor::new(b.eq, c.sample_rate as f32))
                .collect(),
            channel_fx,
            bus_fx,
            routes: c.buses.iter().map(|b| b.output).collect(),
            previous: vec![0.0; channels],
            previous_sends: vec![vec![0.0; buses - 1]; channels],
            previous_bus: vec![0.0; buses],
            latencies,
            fx_latencies,
            channel_reuse,
            bus_reuse,
            chain_changed,
            chain_transition: vec![0; channels + buses],
            fx_dry: std::array::from_fn(|_| vec![0.0; MAX_FRAMES]),
            generators: (0..channels)
                .map(|i| {
                    std::array::from_fn(|side| {
                        crate::generator::Generator::new(i + side * MAX_CHANNELS)
                    })
                })
                .collect(),
            generator_modes: vec![Signal::White; channels],
            generator_blend: vec![0.0; channels],
            generator_level: vec![0.0; channels],
            rate: c.sample_rate as f32,
            inputs: c.inputs as usize,
            previous_mono: c
                .channels
                .iter()
                .map(|ch| f32::from(ch.format == Format::Mono))
                .collect(),
            previous_bus_mono: c
                .buses
                .iter()
                .map(|b| f32::from(b.format == Format::Mono))
                .collect(),
            bus_mono: vec![0.0; buses],
        })
    }
    /// Request a one-block fade only for nodes that are being removed.
    pub fn fade_removed(&mut self, next: &Self) {
        for i in 0..next.channel.len().min(self.channel.len()) {
            if next.chain_changed[i] {
                self.chain_transition[i] = -1;
            }
        }
        for b in 0..next.buses.len().min(self.buses.len()) {
            if next.chain_changed[next.channel.len() + b] {
                self.chain_transition[self.channel.len() + b] = -1;
            }
        }
        for i in next.channel.len()..self.channel.len() {
            self.controls.mute[i].store(true, Relaxed);
        }
        for b in next.buses.len()..self.buses.len() {
            self.controls.bus_mute[b].store(true, Relaxed);
        }
    }
    /// Audio-thread handover: swaps ownership, never allocates, destroys or prepares plugins.
    pub fn inherit(&mut self, old: &mut Self) {
        self.controls.performance.inherit(&old.controls.performance);
        self.controls
            .emergency
            .store(old.controls.emergency.load(Relaxed), Relaxed);
        self.controls
            .resume_fade
            .store(old.controls.resume_fade.load(Relaxed), Relaxed);
        self.controls
            .xruns
            .store(old.controls.xruns.load(Relaxed), Relaxed);
        self.controls
            .clipped
            .store(old.controls.clipped.load(Relaxed), Relaxed);
        for i in 0..self.channel.len().min(old.channel.len()) {
            self.controls.channel_meters[i].inherit(&old.controls.channel_meters[i]);
            std::mem::swap(&mut self.channel_comp[i], &mut old.channel_comp[i]);
            std::mem::swap(&mut self.channel_eq[i], &mut old.channel_eq[i]);
            for (slot, source) in self.channel_reuse[i].iter().enumerate() {
                if let Some(source) = source {
                    self.channel_fx[i][slot] = old.channel_fx[i][*source].take();
                }
            }
            if self.chain_changed[i] {
                self.chain_transition[i] = 1;
            }
            std::mem::swap(&mut self.generators[i], &mut old.generators[i]);
            self.previous[i] = old.previous[i];
            self.previous_mono[i] = old.previous_mono[i];
            self.generator_modes[i] = old.generator_modes[i];
            self.generator_blend[i] = old.generator_blend[i];
            self.generator_level[i] = old.generator_level[i];
            for a in 0..self.buses.len().min(old.buses.len()) - 1 {
                self.previous_sends[i][a] = old.previous_sends[i][a];
            }
        }
        for b in 0..self.buses.len().min(old.buses.len()) {
            self.controls.bus_meters[b].inherit(&old.controls.bus_meters[b]);
            self.controls.meters[b].store(old.controls.meters[b].load(Relaxed), Relaxed);
            std::mem::swap(&mut self.bus_comp[b], &mut old.bus_comp[b]);
            std::mem::swap(&mut self.bus_eq[b], &mut old.bus_eq[b]);
            for (slot, source) in self.bus_reuse[b].iter().enumerate() {
                if let Some(source) = source {
                    self.bus_fx[b][slot] = old.bus_fx[b][*source].take();
                }
            }
            if self.chain_changed[self.channel.len() + b] {
                self.chain_transition[self.channel.len() + b] = 1;
            }
            self.previous_bus[b] = old.previous_bus[b];
            self.previous_bus_mono[b] = old.previous_bus_mono[b];
        }
    }
    /// Interleaved physical inputs (independent of strip count); interleaved physical outputs.
    /// No allocation or locks in our routing code. Effects are third-party code.
    pub fn process(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        frames: usize,
        outputs: usize,
    ) -> bool {
        let _timing = self.controls.performance.measure(frames, self.rate as u32);
        output.fill(0.0);
        if frames > MAX_FRAMES
            || input.len() < frames * self.inputs
            || output.len() < frames * outputs
        {
            return false;
        }
        for bus in &mut self.buses {
            for c in bus {
                c[..frames].fill(0.0);
            }
        }
        if self
            .controls
            .emergency
            .load(std::sync::atomic::Ordering::Acquire)
            || self.controls.failed.load(Relaxed)
        {
            for m in self
                .controls
                .channel_meters
                .iter()
                .chain(self.controls.bus_meters.iter())
            {
                m.silence();
            }
            for c in &self.controls.compressor {
                c.reduction.store(0.0_f32.to_bits(), Relaxed);
            }
            if let Some(tap) = &self.recording {
                tap.capture(
                    &self.record_pre,
                    &self.record_post,
                    &self.buses,
                    frames,
                    true,
                );
            }
            if let Some(hub) = &self.listen {
                hub.publish(&self.buses, frames);
            }
            if let Some(hub) = &self.audio_network {
                hub.publish(&self.buses, frames);
            }
            return true;
        }
        if let Some(tap) = &mut self.diagnostics {
            if tap.probe(input, self.inputs, output, outputs, frames) {
                for m in self
                    .controls
                    .channel_meters
                    .iter()
                    .chain(self.controls.bus_meters.iter())
                {
                    m.silence();
                }
                if let Some(hub) = &self.listen {
                    hub.publish(&self.buses, frames);
                }
                if let Some(hub) = &self.audio_network {
                    hub.publish(&self.buses, frames);
                }
                return true;
            }
            if tap.take_resume() {
                self.controls.resume_fade.store(true, Relaxed);
            }
        }
        if self.controls.resume_fade.swap(false, Relaxed) {
            self.previous.fill(0.0);
            self.previous_bus.fill(0.0);
            for sends in &mut self.previous_sends {
                sends.fill(0.0);
            }
        }
        for b in 0..self.buses.len() {
            self.bus_mono[b] = if self.controls.bus_stereo[b].load(Relaxed) {
                0.0
            } else {
                1.0
            };
        }
        let probe_route = self.diagnostics.as_ref().and_then(|t| t.route());
        for i in 0..self.channel.len() {
            let ch = &mut self.channel[i];
            if probe_route.is_some_and(|(channel, _)| channel != i) {
                for side in 0..2 {
                    ch[side][..frames].fill(0.0);
                    self.keys[i][side][..frames].fill(0.0);
                }
                continue;
            }
            let route = self.controls.input_route[i].load(Relaxed);
            let left_input = (route & 255) as usize;
            let right_input = ((route >> 8) & 255) as usize;
            let mono = if route & (1 << 16) == 0 { 1.0 } else { 0.0 };
            let requested = match self.controls.generator[i].load(Relaxed) {
                1 => Signal::White,
                2 => Signal::Pink,
                3 => Signal::Sine,
                _ => Signal::Off,
            };
            let blend = if requested == Signal::Off { 0.0 } else { 1.0 };
            if requested != Signal::Off {
                self.generator_modes[i] = requested;
            }
            let noise_gain = f32::from_bits(self.controls.generator_gain[i].load(Relaxed));
            let frequency = f32::from_bits(self.controls.frequency[i].load(Relaxed));
            for f in 0..frames {
                let input_at = |index: usize| {
                    let v = if index == 0 {
                        0.0
                    } else {
                        input[f * self.inputs + index - 1]
                    };
                    if v.is_finite() { v } else { 0.0 }
                };
                let mix = ramp(self.generator_blend[i], blend, f, frames);
                let generated_left = if mix > 0.0 {
                    self.generators[i][0].next(self.generator_modes[i], frequency, self.rate)
                } else {
                    0.0
                };
                let generated_right = if mix > 0.0 && (mono < 1.0 || self.previous_mono[i] < 1.0) {
                    self.generators[i][1].next(self.generator_modes[i], frequency, self.rate)
                } else {
                    generated_left
                };
                let level = ramp(self.generator_level[i], noise_gain, f, frames) * mix;
                let left = input_at(left_input) * (1.0 - mix) + generated_left * level;
                let right = input_at(right_input) * (1.0 - mix) + generated_right * level;
                let mono_mix = ramp(self.previous_mono[i], mono, f, frames);
                ch[0][f] = left;
                ch[1][f] = right + (left - right) * mono_mix;
            }
            if probe_route.is_some() {
                if let Some(tap) = &self.diagnostics {
                    for f in 0..frames {
                        ch[0][f] = tap.excitation(f);
                        ch[1][f] = ch[0][f];
                    }
                }
            }
            if self
                .recording
                .as_ref()
                .is_some_and(|tap| tap.active() && tap.wants(i, crate::recording::Point::Pre))
            {
                for side in 0..2 {
                    self.record_pre[i][side][..frames].copy_from_slice(&ch[side][..frames]);
                }
            }
            self.generator_blend[i] = blend;
            self.generator_level[i] = noise_gain;
            let spectral = self.spectrum.as_ref().and_then(|h| h.wants(i));
            if spectral.is_some() {
                for side in 0..2 {
                    self.spectrum_pre[side][..frames].copy_from_slice(&ch[side][..frames]);
                }
            }
            self.channel_eq[i].process(ch, frames, &self.controls.eq[i], self.rate);
            if let Some(generation) = spectral {
                self.spectrum.as_ref().unwrap().capture(
                    i,
                    generation,
                    &self.spectrum_pre,
                    ch,
                    frames,
                );
            }
            for side in 0..2 {
                self.keys[i][side][..frames].copy_from_slice(&ch[side][..frames]);
            }
        }
        for i in 0..self.channel.len() {
            if probe_route.is_some_and(|(channel, _)| channel != i) {
                continue;
            }
            let route = self.controls.input_route[i].load(Relaxed);
            let mono = if route & (1 << 16) == 0 { 1.0 } else { 0.0 };
            let ch = &mut self.channel[i];
            self.channel_comp[i].process(
                ch,
                &self.keys,
                input,
                self.inputs,
                frames,
                &self.controls.compressor[i],
            );
            begin_fx_transition(self.chain_transition[i], ch, &mut self.fx_dry, frames);
            for (slot, p) in self.channel_fx[i].iter_mut().enumerate() {
                let Some(p) = p else {
                    continue;
                };
                self.controls.apply_native(i, slot, &mut **p);
                if !self.controls.bypass[i][slot].load(Relaxed) {
                    p.process(ch, frames);
                }
            }
            finish_fx_transition(&mut self.chain_transition[i], ch, &self.fx_dry, frames);
            // Plugins remain prepared for two lanes. Mono folds their outputs, stereo preserves L/R.
            for f in 0..frames {
                let left = if ch[0][f].is_finite() { ch[0][f] } else { 0.0 };
                let right = if ch[1][f].is_finite() { ch[1][f] } else { 0.0 };
                let folded = (left + right) * 0.5;
                let w = ramp(self.previous_mono[i], mono, f, frames);
                ch[0][f] = left + (folded - left) * w;
                ch[1][f] = right + (folded - right) * w;
            }
            let target = if self.controls.mute[i].load(Relaxed) {
                0.0
            } else {
                f32::from_bits(self.controls.gain[i].load(Relaxed))
            };
            let pan = f32::from_bits(self.controls.pan[i].load(Relaxed));
            // Center unity for dual-mono effects; stereo FX retain their L/R outputs.
            let left = if pan > 0.0 {
                (pan * std::f32::consts::FRAC_PI_2).cos()
            } else {
                1.0
            };
            let right = if pan < 0.0 {
                (-pan * std::f32::consts::FRAC_PI_2).cos()
            } else {
                1.0
            };
            let record_post = self
                .recording
                .as_ref()
                .is_some_and(|tap| tap.active() && tap.wants(i, crate::recording::Point::Post));
            let mut channel_peak = [0.0f32; 2];
            let mut channel_energy = [0.0f64; 2];
            for f in 0..frames {
                let g = ramp(self.previous[i], target, f, frames);
                for (side, pan) in [left, right].into_iter().enumerate() {
                    if record_post {
                        self.record_post[i][side][f] = ch[side][f] * g;
                    }
                    if !ch[side][f].is_finite() {
                        ch[side][f] = 0.0;
                    }
                    let sample = ch[side][f] * g * pan;
                    let sample = if sample.is_finite() { sample } else { 0.0 };
                    channel_peak[side] = channel_peak[side].max(sample.abs());
                    channel_energy[side] += f64::from(sample).powi(2);
                    self.buses[0][side][f] += sample;
                }
            }
            self.controls.channel_meters[i].record(channel_peak, channel_energy, frames, self.rate);
            for a in 0..self.buses.len() - 1 {
                let send = if self.controls.mute[i].load(Relaxed) {
                    0.0
                } else {
                    f32::from_bits(self.controls.sends[i][a].load(Relaxed))
                };
                let pre = self.controls.pre[i][a].load(Relaxed);
                for f in 0..frames {
                    let g = ramp(self.previous_sends[i][a], send, f, frames)
                        * if pre {
                            1.0
                        } else {
                            ramp(self.previous[i], target, f, frames)
                        };
                    let w = ramp(
                        self.previous_bus_mono[a + 1],
                        self.bus_mono[a + 1],
                        f,
                        frames,
                    );
                    let folded = (ch[0][f] + ch[1][f]) * 0.5;
                    self.buses[a + 1][0][f] += (ch[0][f] * left * (1.0 - w) + folded * w) * g;
                    self.buses[a + 1][1][f] += (ch[1][f] * right * (1.0 - w) + folded * w) * g;
                }
                self.previous_sends[i][a] = send;
            }
            self.previous[i] = target;
            self.previous_mono[i] = mono;
        }
        for b in 0..self.buses.len() {
            if probe_route.is_some_and(|(_, bus)| bus != b) {
                continue;
            }
            let bus = &mut self.buses[b];
            let spectral_node = MAX_CHANNELS + b;
            let spectral = self.spectrum.as_ref().and_then(|h| h.wants(spectral_node));
            if spectral.is_some() {
                for side in 0..2 {
                    self.spectrum_pre[side][..frames].copy_from_slice(&bus[side][..frames]);
                }
            }
            self.bus_eq[b].process(
                bus,
                frames,
                &self.controls.eq[self.channel.len() + b],
                self.rate,
            );
            if let Some(generation) = spectral {
                self.spectrum.as_ref().unwrap().capture(
                    spectral_node,
                    generation,
                    &self.spectrum_pre,
                    bus,
                    frames,
                );
            }
            self.bus_comp[b].process(
                bus,
                &self.keys,
                input,
                self.inputs,
                frames,
                &self.controls.compressor[self.channel.len() + b],
            );
            begin_fx_transition(
                self.chain_transition[self.channel.len() + b],
                bus,
                &mut self.fx_dry,
                frames,
            );
            for (slot, p) in self.bus_fx[b].iter_mut().enumerate() {
                let Some(p) = p else {
                    continue;
                };
                self.controls
                    .apply_native(self.channel.len() + b, slot, &mut **p);
                if !self.controls.bypass[self.channel.len() + b][slot].load(Relaxed) {
                    p.process(bus, frames);
                }
            }
            finish_fx_transition(
                &mut self.chain_transition[self.channel.len() + b],
                bus,
                &self.fx_dry,
                frames,
            );
            let target = if self.controls.bus_mute[b].load(Relaxed) {
                0.0
            } else {
                f32::from_bits(self.controls.bus_gain[b].load(Relaxed))
            };
            let mut peak = 0.0f32;
            let mut clipped = false;
            let mut stereo_peak = [0.0f32; 2];
            let mut stereo_energy = [0.0f64; 2];
            for f in 0..frames {
                let g = ramp(self.previous_bus[b], target, f, frames);
                let w = ramp(self.previous_bus_mono[b], self.bus_mono[b], f, frames);
                let left = if bus[0][f].is_finite() {
                    bus[0][f]
                } else {
                    0.0
                };
                let right = if bus[1][f].is_finite() {
                    bus[1][f]
                } else {
                    0.0
                };
                let folded = (left + right) * 0.5;
                bus[0][f] = left + (folded - left) * w;
                bus[1][f] = right + (folded - right) * w;
                for (c, channel) in bus.iter_mut().enumerate() {
                    let sample = channel[f] * g;
                    let sample = if sample.is_finite() { sample } else { 0.0 };
                    peak = peak.max(sample.abs());
                    let delivered = sample.clamp(-1.0, 1.0);
                    stereo_peak[c] = stereo_peak[c].max(delivered.abs());
                    stereo_energy[c] += f64::from(delivered).powi(2);
                    clipped |= sample.abs() > 1.0;
                    channel[f] = sample.clamp(-1.0, 1.0);
                    if let Some(pair) = self.routes[b] {
                        if pair + c < outputs
                            && (c == 0 || self.bus_mono[b] < 1.0 || self.previous_bus_mono[b] < 1.0)
                        {
                            output[f * outputs + pair + c] =
                                channel[f] * if c == 0 { 1.0 } else { 1.0 - w };
                        }
                    }
                }
            }
            if clipped {
                self.controls.clipped.fetch_add(1, Relaxed);
            }
            self.controls.meters[b].fetch_max(peak.to_bits(), Relaxed);
            self.controls.bus_meters[b].record(stereo_peak, stereo_energy, frames, self.rate);
            self.previous_bus[b] = target;
            self.previous_bus_mono[b] = self.bus_mono[b];
        }
        if let Some(tap) = &mut self.diagnostics {
            tap.monitor(input, self.inputs, &self.channel, &self.buses, frames);
        }
        if probe_route.is_some() {
            for bus in &mut self.buses {
                for side in bus {
                    side[..frames].fill(0.0);
                }
            }
        }
        if let Some(tap) = &self.recording {
            tap.capture(
                &self.record_pre,
                &self.record_post,
                &self.buses,
                frames,
                false,
            );
        }
        if let Some(hub) = &self.listen {
            hub.publish(&self.buses, frames);
        }
        if let Some(hub) = &self.audio_network {
            hub.publish(&self.buses, frames);
        }
        true
    }
}
fn ramp(from: f32, to: f32, frame: usize, frames: usize) -> f32 {
    from + (to - from) * ((frame + 1) as f32 / frames.max(1) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (Config, Arc<Controls>) {
        let mut c = Config::default();
        for channel in &mut c.channels {
            channel.mute = true;
        }
        c.channels[0].mute = false;
        c.channels[0].gain_db = -6.0206;
        c.channels[0].sends_db = vec![0.0, -6.0206, -90.0, -90.0];
        c.channels[0].pre = vec![true, false, true, true];
        for (i, b) in c.buses.iter_mut().enumerate() {
            b.gain_db = 0.0;
            b.output = Some(i * 2);
        }
        let controls = Controls::new(&c);
        (c, controls)
    }
    #[test]
    fn real_stereo_inputs_and_mono_auxes_can_switch_format_live() {
        let (mut c, ctl) = setup();
        c.channels.truncate(1);
        c.buses.truncate(3);
        c.channels[0].sends_db = vec![0.0; 2];
        c.channels[0].pre = vec![true; 2];
        c.channels[0].gain_db = 0.0;
        c.channels[0].format = Format::Stereo;
        c.channels[0].input = Some(0);
        c.channels[0].input_right = Some(1);
        c.buses[1].format = Format::Mono;
        c.buses[2].format = Format::Stereo;
        // Sizes changed, so use a graph-sized control set.
        drop(ctl);
        let ctl = Controls::new(&c);
        let mut m = Mixer::new(&c, ctl.clone()).unwrap();
        let mut input = vec![0.0; 8 * 480];
        let mut output = vec![0.0; 10 * 480];
        for f in 0..480 {
            input[f * 8] = 0.2;
            input[f * 8 + 1] = 0.6;
        }
        for _ in 0..3 {
            m.process(&input, &mut output, 480, 10);
        }
        assert!((output[0] - 0.2).abs() < 0.0001 && (output[1] - 0.6).abs() < 0.0001);
        assert!(
            (output[2] - 0.4).abs() < 0.0001 && output[3] == 0.0,
            "mono aux folds L/R and uses only one output"
        );
        assert!((output[4] - 0.2).abs() < 0.0001 && (output[5] - 0.6).abs() < 0.0001);
        c.channels[0].format = Format::Mono;
        c.buses[1].format = Format::Stereo;
        ctl.update(&c);
        for _ in 0..3 {
            m.process(&input, &mut output, 480, 10);
        }
        assert!((output[0] - 0.2).abs() < 0.0001 && (output[1] - 0.2).abs() < 0.0001);
        assert!((output[2] - 0.2).abs() < 0.0001 && (output[3] - 0.2).abs() < 0.0001);
        c.buses[1].format = Format::Mono;
        c.buses[1].output = Some(63);
        c.validate().unwrap();
        c.buses[1].format = Format::Stereo;
        assert!(c.validate().is_err());
    }
    #[test]
    fn legacy_schema_preserves_stereo_auxes_and_schema_two_rejects_inconsistent_sends() {
        let mut v = serde_json::to_value(Config::default()).unwrap();
        v["version"] = serde_json::json!(1);
        for ch in v["channels"].as_array_mut().unwrap() {
            for key in ["format", "input", "input_right", "eq"] {
                ch.as_object_mut().unwrap().remove(key);
            }
        }
        for b in v["buses"].as_array_mut().unwrap() {
            b.as_object_mut().unwrap().remove("format");
            b.as_object_mut().unwrap().remove("eq");
        }
        let mut c: Config = serde_json::from_value(v).unwrap();
        c.normalise();
        c.validate().unwrap();
        assert_eq!(c.version, 2);
        assert!(c.buses.iter().all(|b| b.format == Format::Stereo));
        assert_eq!(c.channels[7].input, Some(7));
        assert!(c.channels[7].input_right.is_none());
        c.channels[0].sends_db.pop();
        assert!(c.validate().is_err());
    }
    #[test]
    fn rms_integrates_power_in_audio_time_and_is_independent_of_block_size() {
        for frames in [64, 480, 960] {
            let m = Meter::new();
            // A constant -20 dBFS signal for exactly 300 ms: 1 - exp(-1) of power.
            for _ in 0..(14400 / frames) {
                m.record([0.1; 2], [0.01 * frames as f64; 2], frames, 48000.0);
            }
            let (_, rms) = m.read();
            let expected = 0.1 * (1.0 - (-1.0f32).exp()).sqrt();
            assert!((rms[0] - expected).abs() < 0.00001);
        }
    }
    #[test]
    fn marked_fader_gains_and_output_meters_match_delivered_audio() {
        let (mut c, ctl) = setup();
        let mut input = vec![0.0; DEFAULT_CHANNELS * 480];
        for f in 0..480 {
            input[f * DEFAULT_CHANNELS] = 0.1;
        }
        let mut output = vec![0.0; 480 * 10];
        for db in [0.0, -6.0, -12.0, -24.0] {
            c.channels[0].gain_db = db;
            ctl.update(&c);
            let mut m = Mixer::new(&c, ctl.clone()).unwrap();
            for _ in 0..300 {
                m.process(&input, &mut output, 480, 10);
            }
            let expected = 0.1 * 10.0f32.powf(db / 20.0);
            let (peak, rms) = ctl.bus_meters[0].read();
            assert!((output[0] - expected).abs() < 0.00001);
            assert!((peak[0] - expected).abs() < 0.00001);
            assert!((rms[0] - expected).abs() < 0.00001);
        }
        c.channels[0].gain_db = 6.0;
        ctl.update(&c);
        let mut m = Mixer::new(&c, ctl.clone()).unwrap();
        input.fill(1.0);
        for _ in 0..300 {
            m.process(&input, &mut output, 480, 10);
        }
        let (peak, rms) = ctl.bus_meters[0].read();
        assert_eq!(output[0], 1.0);
        assert_eq!(peak[0], 1.0);
        assert!((rms[0] - 1.0).abs() < 0.0001);
        assert!(ctl.clipped.load(Relaxed) > 0);
    }
    #[test]
    fn independent_physical_mixes_pre_post_master_and_mute() {
        let (mut c, ctl) = setup();
        let mut m = Mixer::new(&c, ctl.clone()).unwrap();
        let mut input = vec![0.0; 8 * 64];
        for f in 0..64 {
            input[f * 8] = 0.5;
        }
        let mut out = vec![0.0; 10 * 64];
        m.process(&input, &mut out, 64, 10);
        m.process(&input, &mut out, 64, 10);
        assert!((out[0] - 0.25).abs() < 0.0001);
        assert!((out[2] - 0.5).abs() < 0.0001);
        assert!((out[4] - 0.125).abs() < 0.0001);
        c.channels[0].gain_db = -90.0;
        c.buses[0].mute = true;
        ctl.update(&c);
        m.process(&input, &mut out, 64, 10);
        m.process(&input, &mut out, 64, 10);
        assert_eq!(out[0], 0.0);
        assert_eq!(out[4], 0.0);
        assert!((out[2] - 0.5).abs() < 0.0001);
        c.channels[0].mute = true;
        ctl.update(&c);
        m.process(&input, &mut out, 64, 10);
        m.process(&input, &mut out, 64, 10);
        assert!(out.iter().all(|v| *v == 0.0));
    }
    #[test]
    fn emergency_and_oversized_buffers_are_silent() {
        let (c, ctl) = setup();
        let mut m = Mixer::new(&c, ctl.clone()).unwrap();
        let input = vec![1.0; 8 * 64];
        let mut out = vec![3.0; 10 * 64];
        ctl.emergency.store(true, Relaxed);
        assert!(m.process(&input, &mut out, 64, 10));
        assert!(out.iter().all(|v| *v == 0.0));
        assert!(!m.process(&input, &mut out, MAX_FRAMES + 1, 10));
        assert!(out.iter().all(|v| *v == 0.0));
    }
    #[test]
    fn routing_validation_rejects_overlapping_outputs_and_nonfinite_values() {
        let (mut c, _) = setup();
        c.buses[1].output = Some(1);
        assert!(c.validate().is_err());
        c.buses[1].output = Some(2);
        c.channels[0].gain_db = f32::NAN;
        assert!(c.validate().is_err());
    }
    #[test]
    fn stereo_balance_and_clipping_are_bounded() {
        let (mut c, ctl) = setup();
        c.channels[0].pan = -1.0;
        c.channels[0].gain_db = 6.0;
        ctl.update(&c);
        let mut m = Mixer::new(&c, ctl.clone()).unwrap();
        let input = vec![0.9; 8 * 64];
        let mut out = vec![0.0; 640];
        m.process(&input, &mut out, 64, 10);
        m.process(&input, &mut out, 64, 10);
        assert_eq!(out[0], 1.0);
        assert!(out[1].abs() < 0.00001);
        assert!(ctl.clipped.load(Relaxed) > 0);
    }
    #[test]
    #[ignore = "requires MONITOR_VST3_FIXTURE pointing to the built SoundCraft test plugin"]
    fn real_vst3_processes_channel_auxiliary_and_master() {
        let path = std::env::var("MONITOR_VST3_FIXTURE").expect("fixture path");
        let descriptors = crate::fx::load(std::path::Path::new(&path)).unwrap();
        let d = descriptors
            .iter()
            .find(|d| d.name == "Fixture Gain")
            .unwrap();
        let fx = Fx {
            path,
            id: d.id.clone(),
            params: std::collections::BTreeMap::from([("7".into(), 0.5)]),
            ..Fx::default()
        };
        let (mut c, ctl) = setup();
        c.channels[0].gain_db = 0.0;
        c.channels[0].fx = Some(fx.clone());
        c.channels[0].inserts.push(fx.clone());
        c.buses[0].fx = Some(fx.clone());
        c.buses[1].fx = Some(fx);
        ctl.update(&c);
        let mut m = Mixer::new(&c, ctl.clone()).unwrap();
        let mut input = vec![0.0; 8 * 64];
        for f in 0..64 {
            input[f * 8] = 0.5;
        }
        let mut out = vec![0.0; 640];
        m.process(&input, &mut out, 64, 10);
        m.process(&input, &mut out, 64, 10);
        assert!(
            (out[0] - 0.0625).abs() < 0.0001,
            "two channel inserts + master effect"
        );
        assert!(
            (out[2] - 0.0625).abs() < 0.0001,
            "channel + auxiliary effects"
        );
        assert_eq!(m.latencies[0], 6);
        c.channels[0].fx.as_mut().unwrap().bypass = true;
        ctl.update(&c);
        m.process(&input, &mut out, 64, 10);
        assert!(
            (out[0] - 0.125).abs() < 0.0001,
            "live bypass skips only its own slot"
        );
        let old_ptr = &**m.channel_fx[0][0].as_ref().unwrap() as *const dyn Plugin as *const ();
        let mut next = c.clone();
        next.channels.push(c.channels[1].clone());
        let mut replacement =
            Mixer::reconfigure(&next, Controls::new(&next), &c, &m.fx_latencies).unwrap();
        assert!(
            replacement.channel_fx[0].iter().all(Option::is_none),
            "existing VST must not be instantiated again"
        );
        replacement.inherit(&mut m);
        assert_eq!(
            old_ptr,
            &**replacement.channel_fx[0][0].as_ref().unwrap() as *const dyn Plugin as *const ()
        );
        assert_eq!(
            replacement.latencies[next.channels.len()],
            3,
            "master latency uses dynamic channel offset"
        );
        replacement.process(&input, &mut out, 64, 10);
        assert!((out[0] - 0.125).abs() < 0.0001);
    }
    #[test]
    fn two_identical_plugins_reorder_by_instance_identity_without_reinstantiation() {
        let (mut config, _) = setup();
        config.channels[0].inserts = ["first", "second"]
            .map(|instance| Fx {
                id: "native/mda/delay".into(),
                instance: instance.into(),
                ..Fx::default()
            })
            .to_vec();
        let mut mixer = Mixer::new(&config, Controls::new(&config)).unwrap();
        let pointer =
            |p: &Option<Box<dyn Plugin>>| &**p.as_ref().unwrap() as *const dyn Plugin as *const ();
        let a = pointer(&mixer.channel_fx[0][0]);
        let b = pointer(&mixer.channel_fx[0][1]);
        let mut next = config.clone();
        next.channels[0].inserts.swap(0, 1);
        assert!(!config.same_graph(&next));
        let mut replacement =
            Mixer::reconfigure(&next, Controls::new(&next), &config, &mixer.fx_latencies).unwrap();
        crate::test_support::no_heap(|| replacement.inherit(&mut mixer));
        assert_eq!(pointer(&replacement.channel_fx[0][0]), b);
        assert_eq!(pointer(&replacement.channel_fx[0][1]), a);
    }
    #[test]
    fn live_chain_reorder_add_remove_preserves_instances_and_failed_preparation_keeps_audio() {
        let (mut config, _) = setup();
        config.channels[0].inserts = ["delay", "limiter"]
            .map(|name| Fx {
                id: format!("native/mda/{name}"),
                ..Fx::default()
            })
            .to_vec();
        config.channels[1].inserts.push(Fx {
            id: "native/mda/delay".into(),
            ..Fx::default()
        });
        let mut mixer = Mixer::new(&config, Controls::new(&config)).unwrap();
        let pointer =
            |p: &Option<Box<dyn Plugin>>| &**p.as_ref().unwrap() as *const dyn Plugin as *const ();
        let first = pointer(&mixer.channel_fx[0][0]);
        let second = pointer(&mixer.channel_fx[0][1]);
        let other = pointer(&mixer.channel_fx[1][0]);
        let mut next = config.clone();
        next.channels[0].inserts.swap(0, 1);
        next.channels[0].inserts.push(Fx {
            id: "native/mda/dither".into(),
            ..Fx::default()
        });
        let mut replacement =
            Mixer::reconfigure(&next, Controls::new(&next), &config, &mixer.fx_latencies).unwrap();
        let input = vec![0.1; config.inputs as usize * 64];
        let mut output = vec![0.0; 640];
        crate::test_support::no_heap(|| {
            mixer.fade_removed(&replacement);
            assert!(mixer.process(&input, &mut output, 64, 10));
            replacement.inherit(&mut mixer);
            assert!(replacement.process(&input, &mut output, 64, 10));
        });
        assert_eq!(pointer(&replacement.channel_fx[0][0]), second);
        assert_eq!(pointer(&replacement.channel_fx[0][1]), first);
        assert_eq!(pointer(&replacement.channel_fx[1][0]), other);
        assert!(output.iter().all(|v| v.is_finite()));
        let mut removed = next.clone();
        removed.channels[0].inserts.remove(0);
        let mut smaller = Mixer::reconfigure(
            &removed,
            Controls::new(&removed),
            &next,
            &replacement.fx_latencies,
        )
        .unwrap();
        crate::test_support::no_heap(|| {
            replacement.fade_removed(&smaller);
            replacement.process(&input, &mut output, 64, 10);
            smaller.inherit(&mut replacement);
            smaller.process(&input, &mut output, 64, 10);
        });
        assert_eq!(pointer(&smaller.channel_fx[0][0]), first);
        let mut invalid = removed.clone();
        invalid.channels[0].inserts.push(Fx {
            id: "native/not-an-effect".into(),
            ..Fx::default()
        });
        assert!(
            Mixer::reconfigure(
                &invalid,
                Controls::new(&invalid),
                &removed,
                &smaller.fx_latencies
            )
            .is_err()
        );
        crate::test_support::no_heap(|| assert!(smaller.process(&input, &mut output, 64, 10)));
        assert_eq!(pointer(&smaller.channel_fx[0][0]), first);
    }
    #[test]
    fn generator_is_per_channel_and_replaces_hardware_before_sends() {
        let (mut c, ctl) = setup();
        c.channels[0].mute = true;
        c.channels[3].mute = false;
        c.channels[3].gain_db = 0.0;
        c.channels[3].generator = Generator {
            signal: Signal::White,
            level_db: -24.0,
            frequency: 1000.0,
        };
        c.channels[3].sends_db[0] = 0.0;
        ctl.update(&c);
        let mut m = Mixer::new(&c, ctl.clone()).unwrap();
        let input = vec![0.9; DEFAULT_CHANNELS * 1024];
        let mut output = vec![0.0; 10 * 1024];
        m.process(&input, &mut output, 1024, 10);
        m.process(&input, &mut output, 1024, 10);
        assert!(output.iter().any(|v| v.abs() > 0.001));
        assert!(output.iter().all(|v| v.abs() <= gain(-24.0) + 0.00001));
        for i in 0..DEFAULT_CHANNELS {
            let (p, r) = ctl.channel_meters[i].read();
            if i == 3 {
                assert!(p[0] > 0.01 && r[0] > 0.01);
            } else {
                assert_eq!(p, [0.0; 2]);
                assert_eq!(r, [0.0; 2]);
            }
        }
        let (p, _) = ctl.bus_meters[1].read();
        assert!(p[0] > 0.01);
        let (p, _) = ctl.bus_meters[2].read();
        assert_eq!(p, [0.0; 2]);
    }
    #[test]
    fn sine_peak_and_rms_match_known_level_and_pc_selects_a_bus() {
        let (mut c, ctl) = setup();
        c.channels[0].gain_db = 0.0;
        c.channels[0].generator = Generator {
            signal: Signal::Sine,
            level_db: -20.0,
            frequency: 1000.0,
        };
        ctl.update(&c);
        let mut m = Mixer::new(&c, ctl.clone()).unwrap();
        let input = vec![0.0; DEFAULT_CHANNELS * 480];
        let mut output = vec![0.0; 10 * 480];
        for _ in 0..300 {
            m.process(&input, &mut output, 480, 10);
        }
        let (peak, rms) = ctl.channel_meters[0].read();
        assert!((peak[0] - 0.1).abs() < 0.0001);
        assert!((rms[0] - 0.1 / std::f32::consts::SQRT_2).abs() < 0.0001);
        let mut pc = vec![0.0; 480 * 2];
        assert!(m.copy_monitor(&mut pc, 480, 2));
        for f in 0..480 {
            assert!((pc[f * 2] - output[f * 10 + 4]).abs() < 0.00001);
        }
        assert!(m.copy_monitor(&mut pc, 480, 0));
        assert!(pc.iter().any(|v| v.abs() > 0.05));
        ctl.emergency.store(true, Relaxed);
        m.process(&input, &mut output, 480, 10);
        m.copy_monitor(&mut pc, 480, 0);
        assert!(pc.iter().all(|v| *v == 0.0));
        assert_eq!(ctl.channel_meters[0].read().1, [0.0; 2]);
        ctl.resume_fade.store(true, Relaxed);
        ctl.emergency
            .store(false, std::sync::atomic::Ordering::Release);
        assert!(m.process(&input, &mut output, 480, 10));
        assert!(output.iter().any(|v| v.abs() > 0.05));
        m.copy_monitor(&mut pc, 480, 0);
        assert!(pc.iter().any(|v| v.abs() > 0.05));
        assert!(!m.copy_monitor(&mut pc, 480, 99));
        assert!(pc.iter().all(|v| *v == 0.0));
    }
    #[test]
    fn native_eq_on_channel_master_and_auxiliary_changes_delivered_levels() {
        let (mut c, ctl) = setup();
        c.channels[0].gain_db = 0.0;
        c.channels[0].generator = Generator {
            signal: Signal::Sine,
            level_db: -30.0,
            frequency: 1000.0,
        };
        c.channels[0].eq.bands[4].gain_db = 6.0;
        c.buses[0].eq.bands[4].gain_db = 6.0;
        c.buses[1].eq.bands[4].gain_db = -6.0;
        ctl.update(&c);
        let mut m = Mixer::new(&c, ctl).unwrap();
        let input = vec![0.0; 480 * c.inputs as usize];
        let mut output = vec![0.0; 480 * 10];
        for _ in 0..100 {
            assert!(m.process(&input, &mut output, 480, 10));
        }
        let rms = |lane: usize| {
            (output
                .chunks_exact(10)
                .map(|f| f64::from(f[lane]).powi(2))
                .sum::<f64>()
                / 480.0)
                .sqrt()
        };
        let reference = 10.0f64.powf(-30.0 / 20.0) / 2.0f64.sqrt();
        assert!((20.0 * (rms(0) / reference).log10() - 12.0).abs() < 0.02);
        assert!((20.0 * (rms(2) / reference).log10()).abs() < 0.02);
    }
    #[test]
    fn native_parameters_change_audio_live_without_rebuilding_or_heap_work() {
        let (mut c, _) = setup();
        c.channels[0].gain_db = 0.0;
        c.channels[0].generator = Generator {
            signal: Signal::Sine,
            level_db: -30.0,
            frequency: 1000.0,
        };
        c.channels[0].inserts = vec![Fx {
            path: "native:native/freevox8".into(),
            id: "native/freevox8".into(),
            params: std::collections::BTreeMap::from([
                ("Mix".into(), 0.0),
                ("Output gain".into(), 0.0),
            ]),
            ..Fx::default()
        }];
        c.validate().unwrap();
        let ctl = Controls::new(&c);
        let mut m = Mixer::new(&c, ctl.clone()).unwrap();
        let input = vec![0.0; 480 * c.inputs as usize];
        let mut output = vec![0.0; 480 * 10];
        let mut original = 0.0;
        for db in [0.0, -6.0] {
            let mut next = c.clone();
            next.channels[0].inserts[0]
                .params
                .insert("Output gain".into(), db);
            assert!(c.same_graph(&next));
            next.validate().unwrap();
            ctl.update(&next);
            for _ in 0..100 {
                crate::test_support::no_heap(|| m.process(&input, &mut output, 480, 10));
            }
            let rms = (output
                .chunks_exact(10)
                .map(|f| f64::from(f[0]).powi(2))
                .sum::<f64>()
                / 480.0)
                .sqrt();
            if db == 0.0 {
                original = rms;
                assert!(original > 0.01);
            } else {
                assert!((20.0 * (rms / original).log10() + 6.0).abs() < 0.02);
            }
        }
        let mut invalid = c.clone();
        invalid.channels[0].inserts[0]
            .params
            .insert("Output gain".into(), 99.0);
        assert!(invalid.validate().is_err());
        // External VST parameters retain the existing stop-before-edit contract.
        c.channels[0].inserts[0].id = "external-vst".into();
        let mut other = c.clone();
        other.channels[0].inserts[0]
            .params
            .insert("Mix".into(), 0.5);
        assert!(!c.same_graph(&other));
    }
    #[test]
    fn sidechain_keys_are_available_in_the_first_block_regardless_of_channel_order() {
        for (target, source) in [(0, 7), (7, 0)] {
            let (mut c, _) = setup();
            c.inputs = 2;
            for ch in &mut c.channels {
                ch.input = None;
                ch.input_right = None;
            }
            c.channels[target].mute = false;
            c.channels[target].input = Some(0);
            c.channels[target].gain_db = 0.0;
            c.channels[source].mute = true;
            c.channels[source].input = Some(1);
            c.channels[target].compressor = crate::compressor::Compressor {
                bypass: false,
                threshold: -30.0,
                knee: 0.0,
                attack: 0.1,
                source: crate::compressor::Source::Channel(source),
                ..Default::default()
            };
            let ctl = Controls::new(&c);
            let mut m = Mixer::new(&c, ctl.clone()).unwrap();
            let input = (0..64).flat_map(|_| [0.1, 1.0]).collect::<Vec<_>>();
            let mut out = vec![0.0; 640];
            crate::test_support::no_heap(|| m.process(&input, &mut out, 64, 10));
            assert!(out[630] > 0.006 && out[630] < 0.009, "{}", out[630]);
            assert!(f32::from_bits(ctl.compressor[target].reduction.load(Relaxed)) > 22.0);
        }
    }
    #[test]
    fn old_scenes_migrate_accept_eight_and_reject_a_ninth_insert() {
        let mut value = serde_json::to_value(Config::default()).unwrap();
        value.as_object_mut().unwrap().remove("pc_monitor");
        for c in value["channels"].as_array_mut().unwrap() {
            c.as_object_mut().unwrap().remove("inserts");
            c.as_object_mut().unwrap().remove("generator");
        }
        value["channels"][0]["fx"] = serde_json::json!({"path":"old.vst3","id":"old","params":{}});
        let mut c: Config = serde_json::from_value(value).unwrap();
        c.normalise();
        c.validate().unwrap();
        assert_eq!(c.channels[0].generator.signal, Signal::Off);
        assert!(c.channels[0].fx.is_none());
        assert_eq!(c.channels[0].inserts.len(), 1);
        c.channels[0].inserts = (0..INSERTS)
            .map(|i| Fx {
                id: "native/mda/dither".into(),
                instance: format!("limit-{i}"),
                ..Fx::default()
            })
            .collect();
        c.validate().unwrap();
        c.channels[0].inserts.push(Fx {
            id: "native/mda/dither".into(),
            instance: "ninth".into(),
            ..Fx::default()
        });
        assert!(c.validate().is_err());
    }
}
#[cfg(test)]
mod diagnostic_delay_tests {
    use super::*;
    struct Delay {
        ring: [[f32; 37]; 2],
        position: usize,
    }
    impl Plugin for Delay {
        fn info(&self) -> &'static soundcraft_dsp::PluginInfo {
            panic!("metadata unused in this processing fixture")
        }
        fn prepare(&mut self, _: f32, _: usize, _: usize) {}
        fn reset(&mut self) {
            self.ring = [[0.0; 37]; 2];
            self.position = 0;
        }
        fn set_param(&mut self, _: &str, _: f32) -> bool {
            false
        }
        fn param(&self, _: &str) -> Option<f32> {
            None
        }
        fn process(&mut self, io: &mut [Vec<f32>], frames: usize) {
            for f in 0..frames {
                for s in 0..2 {
                    std::mem::swap(&mut self.ring[s][self.position], &mut io[s][f]);
                }
                self.position = (self.position + 1) % 37;
            }
        }
        fn latency(&self) -> usize {
            37
        }
    }
    #[test]
    fn comparator_signal_really_crosses_inserts_and_bypass_removes_their_delay() {
        use crate::diagnostics::{Hub, signal};
        for bypass in [false, true] {
            let mut c = Config::default();
            for ch in &mut c.channels {
                ch.input = None;
                ch.input_right = None;
            }
            c.buses[0].output = Some(0);
            c.buses[0].gain_db = 0.0;
            let control = Controls::new(&c);
            control.bypass[0][0].store(bypass, Relaxed);
            let mut mixer = Mixer::new(&c, control).unwrap();
            mixer.channel_fx[0].push(Some(Box::new(Delay {
                ring: [[0.0; 37]; 2],
                position: 0,
            })));
            let hub = Hub::new();
            mixer.diagnostics = Some(hub.tap());
            hub.route_probe(0, 0, 0, 0, "Insert", serde_json::json!({}))
                .unwrap();
            let input = vec![0.0; 128 * c.inputs as usize];
            let mut output = vec![0.0; 256];
            let mut returned = Vec::new();
            for _ in 0..188 {
                crate::test_support::no_heap(|| {
                    assert!(mixer.process(&input, &mut output, 128, 2))
                });
                returned.extend(output.chunks_exact(2).map(|s| s[0]));
            }
            let result = signal::detect_probe(&returned, false);
            assert_eq!(
                result.samples,
                Some(if bypass { 0 } else { 37 }),
                "{result:?}"
            );
            hub.cancel();
        }
    }
}

#[cfg(test)]
mod spectrum_tests {
    use super::*;
    #[test]
    fn live_eq_spectrum_copy_keeps_the_entire_callback_free_of_rust_allocations() {
        let mut config = Config::default();
        config.normalise();
        config.channels[0].generator.signal = Signal::Sine;
        let hub = crate::spectrum::Hub::new();
        hub.request(0, 3, true).unwrap();
        let mut mixer = Mixer::new(&config, Controls::new(&config)).unwrap();
        mixer.spectrum = Some(hub);
        let input = vec![0.0; 256 * config.inputs as usize];
        let mut output = vec![0.0; 512];
        crate::test_support::no_heap(|| {
            for _ in 0..20 {
                assert!(mixer.process(&input, &mut output, 256, 2));
            }
        });
        assert!(output.iter().all(|x| x.is_finite()));
    }
}
