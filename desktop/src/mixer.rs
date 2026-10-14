use crate::config::*;
use soundcraft_dsp::Plugin;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed},
};

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
        });
        s.update(c);
        s
    }
    pub fn update(&self, c: &Config) {
        for (i, ch) in c.channels.iter().enumerate() {
            self.gain[i].store(gain(ch.gain_db).to_bits(), Relaxed);
            self.pan[i].store(ch.pan.to_bits(), Relaxed);
            self.mute[i].store(ch.mute, Relaxed);
            for a in 0..AUXES {
                self.sends[i][a].store(gain(ch.sends_db[a]).to_bits(), Relaxed);
                self.pre[i][a].store(ch.pre[a], Relaxed);
            }
        }
        for (i, b) in c.buses.iter().enumerate() {
            self.bus_gain[i].store(gain(b.gain_db).to_bits(), Relaxed);
            self.bus_mute[i].store(b.mute, Relaxed);
        }
    }
}

pub struct Mixer {
    pub controls: Arc<Controls>,
    channel: [Vec<Vec<f32>>; CHANNELS],
    pub buses: [Vec<Vec<f32>>; BUSES],
    channel_fx: [Option<Box<dyn Plugin>>; CHANNELS],
    bus_fx: [Option<Box<dyn Plugin>>; BUSES],
    routes: [Option<usize>; BUSES],
    previous: [f32; CHANNELS],
    previous_sends: [[f32; AUXES]; CHANNELS],
    previous_bus: [f32; BUSES],
    pub latencies: [usize; CHANNELS + BUSES],
}
fn prepare_fx(fx: &Option<Fx>, rate: u32) -> Result<Option<Box<dyn Plugin>>, String> {
    let Some(spec) = fx else {
        return Ok(None);
    };
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
    Ok(Some(p))
}
impl Mixer {
    /// Called on the control thread; all allocations and plugin creation happen here.
    pub fn new(c: &Config, controls: Arc<Controls>) -> Result<Self, String> {
        c.validate()?;
        let mut channel_fx = std::array::from_fn(|_| None);
        let mut bus_fx = std::array::from_fn(|_| None);
        let mut latencies = [0; CHANNELS + BUSES];
        for i in 0..CHANNELS {
            channel_fx[i] = prepare_fx(&c.channels[i].fx, c.sample_rate)?;
            latencies[i] = channel_fx[i].as_ref().map_or(0, |p| p.latency());
        }
        for i in 0..BUSES {
            bus_fx[i] = prepare_fx(&c.buses[i].fx, c.sample_rate)?;
            latencies[CHANNELS + i] = bus_fx[i].as_ref().map_or(0, |p| p.latency());
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
            return true;
        }
        for i in 0..CHANNELS {
            let ch = &mut self.channel[i];
            for f in 0..frames {
                let v = input[f * CHANNELS + i];
                ch[0][f] = if v.is_finite() { v } else { 0.0 };
                ch[1][f] = ch[0][f];
            }
            if let Some(p) = &mut self.channel_fx[i] {
                p.process(ch, frames);
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
            for f in 0..frames {
                let g = ramp(self.previous[i], target, f, frames);
                self.buses[0][0][f] += ch[0][f] * g * left;
                self.buses[0][1][f] += ch[1][f] * g * right;
            }
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
            if let Some(p) = &mut self.bus_fx[b] {
                p.process(bus, frames);
            }
            let target = if self.controls.bus_mute[b].load(Relaxed) {
                0.0
            } else {
                f32::from_bits(self.controls.bus_gain[b].load(Relaxed))
            };
            let mut peak = 0.0f32;
            let mut clipped = false;
            for f in 0..frames {
                let g = ramp(self.previous_bus[b], target, f, frames);
                for (c, channel) in bus.iter_mut().enumerate() {
                    let sample = channel[f] * g;
                    let sample = if sample.is_finite() { sample } else { 0.0 };
                    peak = peak.max(sample.abs());
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
        };
        let (mut c, ctl) = setup();
        c.channels[0].gain_db = 0.0;
        c.channels[0].fx = Some(fx.clone());
        c.buses[0].fx = Some(fx.clone());
        c.buses[1].fx = Some(fx);
        ctl.update(&c);
        let mut m = Mixer::new(&c, ctl).unwrap();
        let mut input = vec![0.0; 8 * 64];
        for f in 0..64 {
            input[f * 8] = 0.5;
        }
        let mut out = vec![0.0; 640];
        m.process(&input, &mut out, 64, 10);
        m.process(&input, &mut out, 64, 10);
        assert!((out[0] - 0.125).abs() < 0.0001, "channel + master effects");
        assert!(
            (out[2] - 0.125).abs() < 0.0001,
            "channel + auxiliary effects"
        );
        assert_eq!(m.latencies[0], 3);
    }
}
