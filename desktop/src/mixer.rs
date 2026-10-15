use crate::config::*;
use soundcraft_dsp::Plugin;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed},
};

pub struct Meter {
    peak: [AtomicU32; 2],
    rms: [AtomicU32; 2],
}
impl Meter {
    fn new() -> Self {
        Self {
            peak: std::array::from_fn(|_| AtomicU32::new(0)),
            rms: std::array::from_fn(|_| AtomicU32::new(0)),
        }
    }
    pub fn read(&self) -> ([f32; 2], [f32; 2]) {
        (
            std::array::from_fn(|i| f32::from_bits(self.peak[i].swap(0, Relaxed))),
            std::array::from_fn(|i| f32::from_bits(self.rms[i].load(Relaxed))),
        )
    }
    fn record(&self, peak: [f32; 2], energy: [f64; 2], frames: usize, rate: f32) {
        for i in 0..2 {
            self.peak[i].fetch_max(peak[i].to_bits(), Relaxed);
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
            self.rms[i].store(old.rms[i].load(Relaxed), Relaxed);
        }
    }
    fn silence(&self) {
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
    eq: Vec<crate::eq::Control>,
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
    pub fn update(&self, c: &Config) {
        self.pc_monitor.store(c.pc_monitor as u32, Relaxed);
        for (i, ch) in c.channels.iter().enumerate() {
            self.eq[i].update(&ch.eq);
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
                self.sends[i][a].store(gain(ch.sends_db[a]).to_bits(), Relaxed);
                self.pre[i][a].store(ch.pre[a], Relaxed);
            }
        }
        for (i, b) in c.buses.iter().enumerate() {
            self.eq[c.channels.len() + i].update(&b.eq);
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
    pub controls: Arc<Controls>,
    channel: Vec<Vec<Vec<f32>>>,
    pub buses: Vec<Vec<Vec<f32>>>,
    channel_eq: Vec<crate::eq::Processor>,
    bus_eq: Vec<crate::eq::Processor>,
    channel_fx: Vec<Vec<Box<dyn Plugin>>>,
    bus_fx: Vec<Vec<Box<dyn Plugin>>>,
    routes: Vec<Option<usize>>,
    previous: Vec<f32>,
    previous_sends: Vec<Vec<f32>>,
    previous_bus: Vec<f32>,
    pub latencies: Vec<usize>,
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
        latencies: &[usize],
    ) -> Result<Self, String> {
        if !old.same_device(c) || !old.same_existing_fx(c) {
            return Err("Detén el motor para cambiar dispositivo, frecuencia, buffer o cadenas VST3 existentes".into());
        }
        Self::prepare(c, controls, Some((old, latencies)))
    }
    fn prepare(
        c: &Config,
        controls: Arc<Controls>,
        old: Option<(&Config, &[usize])>,
    ) -> Result<Self, String> {
        c.validate()?;
        let channels = c.channels.len();
        let buses = c.buses.len();
        let mut channel_fx: Vec<Vec<Box<dyn Plugin>>> = (0..channels).map(|_| Vec::new()).collect();
        let mut bus_fx: Vec<Vec<Box<dyn Plugin>>> = (0..buses).map(|_| Vec::new()).collect();
        let mut latencies = vec![0; channels + buses];
        for i in 0..channels {
            if let Some((previous, old_latencies)) = old {
                if i < previous.channels.len() {
                    latencies[i] = old_latencies[i];
                    continue;
                }
            }
            for fx in c.channels[i].effects() {
                channel_fx[i].push(prepare_fx(fx, c.sample_rate)?);
            }
            latencies[i] = channel_fx[i].iter().map(|p| p.latency()).sum();
        }
        for i in 0..buses {
            if let Some((previous, old_latencies)) = old {
                if i < previous.buses.len() {
                    latencies[channels + i] = old_latencies[previous.channels.len() + i];
                    continue;
                }
            }
            for fx in c.buses[i].effects() {
                bus_fx[i].push(prepare_fx(fx, c.sample_rate)?);
            }
            latencies[channels + i] = bus_fx[i].iter().map(|p| p.latency()).sum();
        }
        Ok(Self {
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
    pub fn fade_removed(&self, next: &Self) {
        for i in next.channel.len()..self.channel.len() {
            self.controls.mute[i].store(true, Relaxed);
        }
        for b in next.buses.len()..self.buses.len() {
            self.controls.bus_mute[b].store(true, Relaxed);
        }
    }
    /// Audio-thread handover: swaps ownership, never allocates, destroys or prepares plugins.
    pub fn inherit(&mut self, old: &mut Self) {
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
            std::mem::swap(&mut self.channel_eq[i], &mut old.channel_eq[i]);
            std::mem::swap(&mut self.channel_fx[i], &mut old.channel_fx[i]);
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
            std::mem::swap(&mut self.bus_eq[b], &mut old.bus_eq[b]);
            std::mem::swap(&mut self.bus_fx[b], &mut old.bus_fx[b]);
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
            return true;
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
        for i in 0..self.channel.len() {
            let ch = &mut self.channel[i];
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
            self.generator_blend[i] = blend;
            self.generator_level[i] = noise_gain;
            self.channel_eq[i].process(ch, frames, &self.controls.eq[i], self.rate);
            for (slot, p) in self.channel_fx[i].iter_mut().enumerate() {
                self.controls.apply_native(i, slot, &mut **p);
                if !self.controls.bypass[i][slot].load(Relaxed) {
                    p.process(ch, frames);
                }
            }
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
            let mut channel_peak = [0.0f32; 2];
            let mut channel_energy = [0.0f64; 2];
            for f in 0..frames {
                let g = ramp(self.previous[i], target, f, frames);
                for (side, pan) in [left, right].into_iter().enumerate() {
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
            let bus = &mut self.buses[b];
            self.bus_eq[b].process(
                bus,
                frames,
                &self.controls.eq[self.channel.len() + b],
                self.rate,
            );
            for (slot, p) in self.bus_fx[b].iter_mut().enumerate() {
                self.controls
                    .apply_native(self.channel.len() + b, slot, &mut **p);
                if !self.controls.bypass[self.channel.len() + b][slot].load(Relaxed) {
                    p.process(bus, frames);
                }
            }
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
        let old_ptr = &*m.channel_fx[0][0] as *const dyn Plugin as *const ();
        let mut next = c.clone();
        next.channels.push(c.channels[1].clone());
        let mut replacement =
            Mixer::reconfigure(&next, Controls::new(&next), &c, &m.latencies).unwrap();
        assert!(
            replacement.channel_fx[0].is_empty(),
            "existing VST must not be instantiated again"
        );
        replacement.inherit(&mut m);
        assert_eq!(
            old_ptr,
            &*replacement.channel_fx[0][0] as *const dyn Plugin as *const ()
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
    fn old_scenes_migrate_and_reject_a_fifth_insert() {
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
        c.channels[0].inserts = vec![Fx::default(); 5];
        assert!(c.validate().is_err());
    }
}
