//! Stereo-linked peak compressor. All keys are taps upstream of every compressor.
use serde::{Deserialize, Serialize};
use std::sync::atomic::{
    AtomicBool, AtomicU32,
    Ordering::{Acquire, Relaxed, Release},
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "index", rename_all = "snake_case")]
pub enum Source {
    #[default]
    Internal,
    Channel(usize),
    Input(usize),
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Compressor {
    pub bypass: bool,
    pub threshold: f32,
    pub ratio: f32,
    pub attack: f32,
    pub release: f32,
    pub knee: f32,
    pub makeup: f32,
    pub mix: f32,
    pub highpass: f32,
    pub source: Source,
}
impl Default for Compressor {
    fn default() -> Self {
        Self {
            bypass: true,
            threshold: -18.0,
            ratio: 4.0,
            attack: 10.0,
            release: 120.0,
            knee: 6.0,
            makeup: 0.0,
            mix: 1.0,
            highpass: 0.0,
            source: Source::Internal,
        }
    }
}
impl Compressor {
    fn values(self) -> [f32; 8] {
        [
            self.threshold,
            self.ratio,
            self.attack,
            self.release,
            self.knee,
            self.makeup,
            self.mix,
            self.highpass,
        ]
    }
    pub fn valid(self, channels: usize, inputs: usize) -> bool {
        let ranges = [
            (-60.0, 0.0),
            (1.0, 20.0),
            (0.1, 200.0),
            (10.0, 2000.0),
            (0.0, 24.0),
            (-12.0, 24.0),
            (0.0, 1.0),
            (0.0, 1000.0),
        ];
        self.values()
            .iter()
            .zip(ranges)
            .all(|(v, (a, b))| v.is_finite() && (a..=b).contains(v))
            && match self.source {
                Source::Internal => true,
                Source::Channel(i) => i < channels,
                Source::Input(i) => i < inputs,
            }
    }
    pub fn reduction(self, db: f32) -> f32 {
        let x = db - self.threshold;
        let factor = 1.0 - 1.0 / self.ratio;
        if self.knee > 0.0 && x.abs() < self.knee / 2.0 {
            factor * (x + self.knee / 2.0).powi(2) / (2.0 * self.knee)
        } else {
            factor * x.max(0.0)
        }
    }
}
pub struct Control {
    revision: AtomicU32,
    values: [AtomicU32; 8],
    source: AtomicU32,
    bypass: AtomicBool,
    pub reduction: AtomicU32,
}
impl Control {
    pub fn new(c: Compressor) -> Self {
        let s = Self {
            revision: AtomicU32::new(0),
            values: std::array::from_fn(|_| AtomicU32::new(0)),
            source: AtomicU32::new(0),
            bypass: AtomicBool::new(true),
            reduction: AtomicU32::new(0),
        };
        s.update(c);
        s
    }
    pub fn update(&self, c: Compressor) {
        self.revision
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        for (a, v) in self.values.iter().zip(c.values()) {
            a.store(v.to_bits(), Relaxed);
        }
        self.source.store(
            match c.source {
                Source::Internal => 0,
                Source::Channel(i) => 0x10000 | i as u32,
                Source::Input(i) => 0x20000 | i as u32,
            },
            Relaxed,
        );
        self.bypass.store(c.bypass, Relaxed);
        self.revision.fetch_add(1, Release);
    }
    fn read(&self) -> Option<Compressor> {
        let before = self.revision.load(Acquire);
        if before & 1 != 0 {
            return None;
        }
        let v = self
            .values
            .each_ref()
            .map(|a| f32::from_bits(a.load(Relaxed)));
        let route = self.source.load(Relaxed);
        let c = Compressor {
            threshold: v[0],
            ratio: v[1],
            attack: v[2],
            release: v[3],
            knee: v[4],
            makeup: v[5],
            mix: v[6],
            highpass: v[7],
            bypass: self.bypass.load(Relaxed),
            source: match route >> 16 {
                1 => Source::Channel((route & 65535) as usize),
                2 => Source::Input((route & 65535) as usize),
                _ => Source::Internal,
            },
        };
        std::sync::atomic::fence(Acquire);
        (before == self.revision.load(Relaxed)).then_some(c)
    }
}
pub struct Processor {
    spec: Compressor,
    gr: f32,
    wet: f32,
    hp_in: [f32; 2],
    hp_out: [f32; 2],
}
impl Processor {
    pub fn new(spec: Compressor) -> Self {
        Self {
            spec,
            gr: 0.0,
            wet: if spec.bypass { 0.0 } else { 1.0 },
            hp_in: [0.0; 2],
            hp_out: [0.0; 2],
        }
    }
    pub fn process(
        &mut self,
        audio: &mut [Vec<f32>],
        keys: &[Vec<Vec<f32>>],
        physical: &[f32],
        stride: usize,
        frames: usize,
        control: &Control,
    ) {
        if let Some(spec) = control.read() {
            self.spec = spec;
        }
        let c = self.spec;
        if c.bypass && self.wet == 0.0 {
            self.gr = 0.0;
            control.reduction.store(0, Relaxed);
            return;
        }
        let attack = (-1.0 / (48.0 * c.attack)).exp();
        let release = (-1.0 / (48.0 * c.release)).exp();
        let hp = (-2.0 * std::f32::consts::PI * c.highpass / 48000.0).exp();
        let makeup = 10.0f32.powf(c.makeup / 20.0);
        let target = if c.bypass { 0.0 } else { 1.0 };
        for f in 0..frames {
            let mut peak = 0.0f32;
            for side in 0..2 {
                let x = match c.source {
                    Source::Internal => audio[side][f],
                    Source::Channel(i) => keys.get(i).map_or(0.0, |k| k[side][f]),
                    Source::Input(i) => {
                        if i < stride {
                            physical[f * stride + i]
                        } else {
                            0.0
                        }
                    }
                };
                let x = if x.is_finite() { x } else { 0.0 };
                let filtered = if c.highpass > 0.0 {
                    hp * (self.hp_out[side] + x - self.hp_in[side])
                } else {
                    x
                };
                self.hp_in[side] = x;
                self.hp_out[side] = filtered;
                peak = peak.max(filtered.abs());
            }
            let wanted = c.reduction(20.0 * peak.max(1e-12).log10());
            let coefficient = if wanted > self.gr { attack } else { release };
            self.gr = wanted + (self.gr - wanted) * coefficient;
            let gain = 10.0f32.powf(-self.gr / 20.0) * makeup;
            let wet =
                (self.wet + (target - self.wet) * (f + 1) as f32 / frames.max(1) as f32) * c.mix;
            for lane in audio.iter_mut().take(2) {
                let dry = lane[f];
                lane[f] = dry + (dry * gain - dry) * wet;
            }
        }
        self.wet = target;
        control
            .reduction
            .store((if c.bypass { 0.0 } else { self.gr }).to_bits(), Relaxed);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn threshold_ratio_and_knee_follow_the_transfer_curve() {
        let c = Compressor {
            knee: 0.0,
            ..Compressor::default()
        };
        assert_eq!(c.reduction(-30.0), 0.0);
        assert!((c.reduction(-6.0) - 9.0).abs() < 1e-6);
        let c = Compressor { knee: 6.0, ..c };
        assert!((c.reduction(-18.0) - 0.5625).abs() < 1e-6);
        assert_eq!(c.reduction(-22.0), 0.0);
    }
    #[test]
    fn linked_external_key_ducks_both_lanes_and_releases_without_heap() {
        let spec = Compressor {
            bypass: false,
            threshold: -20.0,
            knee: 0.0,
            attack: 0.1,
            release: 10.0,
            source: Source::Channel(0),
            ..Compressor::default()
        };
        let control = Control::new(spec);
        let mut p = Processor::new(spec);
        let mut a = vec![vec![0.1; 480]; 2];
        let mut keys = vec![vec![vec![1.0; 480], vec![-1.0; 480]]];
        for _ in 0..10 {
            a.iter_mut().for_each(|v| v.fill(0.1));
            crate::test_support::no_heap(|| p.process(&mut a, &keys, &[], 0, 480, &control));
        }
        assert!((20.0 * (a[0][479] / 0.1).log10() + 15.0).abs() < 0.01);
        assert_eq!(a[0], a[1]);
        keys[0].iter_mut().for_each(|v| v.fill(0.0));
        for _ in 0..100 {
            a.iter_mut().for_each(|v| v.fill(0.1));
            p.process(&mut a, &keys, &[], 0, 480, &control);
        }
        assert!((a[0][479] - 0.1).abs() < 1e-6);
    }
}
