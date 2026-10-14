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
    fn record(&self, peak: [f32; 2], energy: [f64; 2], frames: usize) {
        for i in 0..2 {
            self.peak[i].fetch_max(peak[i].to_bits(), Relaxed);
            self.rms[i].store(
                ((energy[i] / frames.max(1) as f64).sqrt() as f32).to_bits(),
                Relaxed,
            );
        }
    }
    fn silence(&self) {
        for x in &self.rms {
            x.store(0, Relaxed);
        }
    }
}

pub struct Controls {
    gain: [AtomicU32; CHANNELS],
    pan: [AtomicU32; CHANNELS],
    mute: [AtomicBool; CHANNELS],
    sends: [[AtomicU32; AUXES]; CHANNELS],
    pre: [[AtomicBool; AUXES]; CHANNELS],
    bus_gain: [AtomicU32; BUSES],
    bus_mute: [AtomicBool; BUSES],
    pub emergency: AtomicBool,
    pub failed: AtomicBool,
    pub xruns: AtomicU64,
    pub clipped: AtomicU64,
    pub meters: [AtomicU32; BUSES],
    pub channel_meters: [Meter; CHANNELS],
    pub bus_meters: [Meter; BUSES],
    pub pc_monitor: AtomicU32,
    generator: [AtomicU32; CHANNELS],
    generator_gain: [AtomicU32; CHANNELS],
    frequency: [AtomicU32; CHANNELS],
    bypass: [[AtomicBool; INSERTS]; CHANNELS + BUSES],
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
        let s = Arc::new(Self {
            gain: std::array::from_fn(|_| AtomicU32::new(0)),
            pan: std::array::from_fn(|_| AtomicU32::new(0)),
            mute: std::array::from_fn(|_| AtomicBool::new(false)),
            sends: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU32::new(0))),
            pre: std::array::from_fn(|_| std::array::from_fn(|_| AtomicBool::new(true))),
            bus_gain: std::array::from_fn(|_| AtomicU32::new(0)),
            bus_mute: std::array::from_fn(|_| AtomicBool::new(false)),
            emergency: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            xruns: AtomicU64::new(0),
            clipped: AtomicU64::new(0),
            meters: std::array::from_fn(|_| AtomicU32::new(0)),
            channel_meters: std::array::from_fn(|_| Meter::new()),
            bus_meters: std::array::from_fn(|_| Meter::new()),
            pc_monitor: AtomicU32::new(0),
            generator: std::array::from_fn(|_| AtomicU32::new(0)),
            generator_gain: std::array::from_fn(|_| AtomicU32::new(0)),
            frequency: std::array::from_fn(|_| AtomicU32::new(0)),
            bypass: std::array::from_fn(|_| std::array::from_fn(|_| AtomicBool::new(false))),
        });
        s.update(c);
        s
    }
    pub fn update(&self, c: &Config) {
        self.pc_monitor.store(c.pc_monitor as u32, Relaxed);
        for (i, ch) in c.channels.iter().enumerate() {
            self.gain[i].store(gain(ch.gain_db).to_bits(), Relaxed);
            self.pan[i].store(ch.pan.to_bits(), Relaxed);
            self.mute[i].store(ch.mute, Relaxed);
            self.generator[i].store(ch.generator.signal as u32, Relaxed);
            self.generator_gain[i].store(gain(ch.generator.level_db).to_bits(), Relaxed);
            self.frequency[i].store(ch.generator.frequency.to_bits(), Relaxed);
            for (slot, fx) in ch.effects().take(INSERTS).enumerate() {
                self.bypass[i][slot].store(fx.bypass, Relaxed);
            }
            for a in 0..AUXES {
                self.sends[i][a].store(gain(ch.sends_db[a]).to_bits(), Relaxed);
                self.pre[i][a].store(ch.pre[a], Relaxed);
            }
        }
        for (i, b) in c.buses.iter().enumerate() {
            self.bus_gain[i].store(gain(b.gain_db).to_bits(), Relaxed);
            self.bus_mute[i].store(b.mute, Relaxed);
            for (slot, fx) in b.effects().take(INSERTS).enumerate() {
                self.bypass[CHANNELS + i][slot].store(fx.bypass, Relaxed);
            }
        }
    }
}

pub struct Mixer {
    pub controls: Arc<Controls>,
    channel: [Vec<Vec<f32>>; CHANNELS],
    pub buses: [Vec<Vec<f32>>; BUSES],
    channel_fx: [Vec<Box<dyn Plugin>>; CHANNELS],
    bus_fx: [Vec<Box<dyn Plugin>>; BUSES],
    routes: [Option<usize>; BUSES],
    previous: [f32; CHANNELS],
    previous_sends: [[f32; AUXES]; CHANNELS],
    previous_bus: [f32; BUSES],
    pub latencies: [usize; CHANNELS + BUSES],
    generators: [crate::generator::Generator; CHANNELS],
    generator_modes: [Signal; CHANNELS],
    generator_blend: [f32; CHANNELS],
    generator_level: [f32; CHANNELS],
    rate: f32,
}
fn prepare_fx(spec: &Fx, rate: u32) -> Result<Box<dyn Plugin>, String> {
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
        if bus >= BUSES || frames > MAX_FRAMES || output.len() < frames * 2 {
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
        c.validate()?;
        let mut channel_fx: [Vec<Box<dyn Plugin>>; CHANNELS] = std::array::from_fn(|_| Vec::new());
        let mut bus_fx: [Vec<Box<dyn Plugin>>; BUSES] = std::array::from_fn(|_| Vec::new());
        let mut latencies = [0; CHANNELS + BUSES];
        for i in 0..CHANNELS {
            for fx in c.channels[i].effects() {
                channel_fx[i].push(prepare_fx(fx, c.sample_rate)?);
            }
            latencies[i] = channel_fx[i].iter().map(|p| p.latency()).sum();
        }
        for i in 0..BUSES {
            for fx in c.buses[i].effects() {
                bus_fx[i].push(prepare_fx(fx, c.sample_rate)?);
            }
            latencies[CHANNELS + i] = bus_fx[i].iter().map(|p| p.latency()).sum();
        }
        Ok(Self {
            controls,
            channel: std::array::from_fn(|_| vec![vec![0.0; MAX_FRAMES]; 2]),
            buses: std::array::from_fn(|_| vec![vec![0.0; MAX_FRAMES]; 2]),
            channel_fx,
            bus_fx,
            routes: std::array::from_fn(|i| c.buses[i].output),
            previous: [0.0; CHANNELS],
            previous_sends: [[0.0; AUXES]; CHANNELS],
            previous_bus: [0.0; BUSES],
            latencies,
            generators: std::array::from_fn(crate::generator::Generator::new),
            generator_modes: [Signal::White; CHANNELS],
            generator_blend: [0.0; CHANNELS],
            generator_level: [0.0; CHANNELS],
            rate: c.sample_rate as f32,
        })
    }
    /// Interleaved eight-channel input; interleaved physical output.
    /// No allocation or locks in our routing code. Effects are third-party code.
    pub fn process(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        frames: usize,
        outputs: usize,
    ) -> bool {
        output.fill(0.0);
        if frames > MAX_FRAMES || input.len() < frames * CHANNELS || output.len() < frames * outputs
        {
            return false;
        }
        for bus in &mut self.buses {
            for c in bus {
                c[..frames].fill(0.0);
            }
        }
        if self.controls.emergency.load(Relaxed) || self.controls.failed.load(Relaxed) {
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
        for i in 0..CHANNELS {
            let ch = &mut self.channel[i];
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
                let dry = input[f * CHANNELS + i];
                let dry = if dry.is_finite() { dry } else { 0.0 };
                let mix = ramp(self.generator_blend[i], blend, f, frames);
                let generated = if mix > 0.0 {
                    self.generators[i].next(self.generator_modes[i], frequency, self.rate)
                } else {
                    0.0
                };
                let v = dry * (1.0 - mix)
                    + generated * ramp(self.generator_level[i], noise_gain, f, frames) * mix;
                ch[0][f] = if v.is_finite() { v } else { 0.0 };
                ch[1][f] = ch[0][f];
            }
            self.generator_blend[i] = blend;
            self.generator_level[i] = noise_gain;
            for (slot, p) in self.channel_fx[i].iter_mut().enumerate() {
                if !self.controls.bypass[i][slot].load(Relaxed) {
                    p.process(ch, frames);
                }
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
            self.controls.channel_meters[i].record(channel_peak, channel_energy, frames);
            for a in 0..AUXES {
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
                    self.buses[a + 1][0][f] += ch[0][f] * g * left;
                    self.buses[a + 1][1][f] += ch[1][f] * g * right;
                }
                self.previous_sends[i][a] = send;
            }
            self.previous[i] = target;
        }
        for b in 0..BUSES {
            let bus = &mut self.buses[b];
            for (slot, p) in self.bus_fx[b].iter_mut().enumerate() {
                if !self.controls.bypass[CHANNELS + b][slot].load(Relaxed) {
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
                for (c, channel) in bus.iter_mut().enumerate() {
                    let sample = channel[f] * g;
                    let sample = if sample.is_finite() { sample } else { 0.0 };
                    peak = peak.max(sample.abs());
                    stereo_peak[c] = stereo_peak[c].max(sample.abs());
                    stereo_energy[c] += f64::from(sample).powi(2);
                    clipped |= sample.abs() > 1.0;
                    channel[f] = sample.clamp(-1.0, 1.0);
                    if let Some(pair) = self.routes[b] {
                        if pair + c < outputs {
                            output[f * outputs + pair + c] = channel[f];
                        }
                    }
                }
            }
            if clipped {
                self.controls.clipped.fetch_add(1, Relaxed);
            }
            self.controls.meters[b].fetch_max(peak.to_bits(), Relaxed);
            self.controls.bus_meters[b].record(stereo_peak, stereo_energy, frames);
            self.previous_bus[b] = target;
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
        c.channels[0].sends_db = [0.0, -6.0206, -90.0, -90.0];
        c.channels[0].pre = [true, false, true, true];
        for (i, b) in c.buses.iter_mut().enumerate() {
            b.gain_db = 0.0;
            b.output = Some(i * 2);
        }
        let controls = Controls::new(&c);
        (c, controls)
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
        let input = vec![0.9; CHANNELS * 1024];
        let mut output = vec![0.0; 10 * 1024];
        m.process(&input, &mut output, 1024, 10);
        m.process(&input, &mut output, 1024, 10);
        assert!(output.iter().any(|v| v.abs() > 0.001));
        assert!(output.iter().all(|v| v.abs() <= gain(-24.0) + 0.00001));
        for i in 0..CHANNELS {
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
        let input = vec![0.0; CHANNELS * 480];
        let mut output = vec![0.0; 10 * 480];
        m.process(&input, &mut output, 480, 10);
        m.process(&input, &mut output, 480, 10);
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
        assert!(!m.copy_monitor(&mut pc, 480, 99));
        assert!(pc.iter().all(|v| *v == 0.0));
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
