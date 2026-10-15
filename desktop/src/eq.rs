//! Eight-band native FreeEQ8 minimum-phase core, with block-smoothed parameters.
use serde::{Deserialize, Serialize};
use std::sync::atomic::{
    AtomicBool, AtomicU32,
    Ordering::{Acquire, Relaxed, Release},
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Band {
    #[serde(default)]
    pub kind: u32,
    pub frequency: f32,
    pub gain_db: f32,
    pub q: f32,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Eq {
    pub bypass: bool,
    pub bands: [Band; 8],
}
impl Default for Eq {
    fn default() -> Self {
        Self {
            bypass: false,
            bands: [50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 6000.0, 12000.0].map(|frequency| {
                Band {
                    kind: 0,
                    frequency,
                    gain_db: 0.0,
                    q: 0.707,
                }
            }),
        }
    }
}
impl Eq {
    pub fn valid(&self) -> bool {
        self.bands.iter().all(|b| {
            b.kind <= 5
                && b.frequency.is_finite()
                && (20.0..=20000.0).contains(&b.frequency)
                && b.gain_db.is_finite()
                && (-18.0..=18.0).contains(&b.gain_db)
                && b.q.is_finite()
                && (0.1..=18.0).contains(&b.q)
        })
    }
}
pub struct Control {
    revision: AtomicU32,
    bypass: AtomicBool,
    bands: [[AtomicU32; 4]; 8],
}
impl Control {
    pub fn new(eq: &Eq) -> Self {
        let control = Self {
            revision: AtomicU32::new(0),
            bypass: AtomicBool::new(false),
            bands: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU32::new(0))),
        };
        control.update(eq);
        control
    }
    pub fn update(&self, eq: &Eq) {
        self.revision
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.bypass.store(eq.bypass, Relaxed);
        for (a, b) in self.bands.iter().zip(eq.bands) {
            for (a, v) in a
                .iter()
                .zip([b.frequency, b.gain_db, b.q, f32::from_bits(b.kind)])
            {
                a.store(v.to_bits(), Relaxed);
            }
        }
        self.revision.fetch_add(1, Release);
    }
    fn read(&self) -> Option<Eq> {
        let before = self.revision.load(Acquire);
        if before & 1 != 0 {
            return None;
        }
        let eq = Eq {
            bypass: self.bypass.load(Relaxed),
            bands: std::array::from_fn(|i| {
                let v = self.bands[i]
                    .each_ref()
                    .map(|a| f32::from_bits(a.load(Relaxed)));
                Band {
                    kind: v[3].to_bits(),
                    frequency: v[0],
                    gain_db: v[1],
                    q: v[2],
                }
            }),
        };
        // Never spin on the callback: keep the previous snapshot if control is updating.
        std::sync::atomic::fence(Acquire);
        (before == self.revision.load(Relaxed) && eq.valid()).then_some(eq)
    }
}

#[repr(C)]
struct RawBand {
    frequency: f32,
    gain_db: f32,
    q: f32,
    kind: u32,
}
unsafe extern "C" {
    fn mm_eq_create() -> *mut std::ffi::c_void;
    fn mm_eq_destroy(p: *mut std::ffi::c_void);
    fn mm_eq_process(
        p: *mut std::ffi::c_void,
        l: *mut f32,
        r: *mut f32,
        n: u32,
        bypass: bool,
        bands: *const RawBand,
    );
}
pub struct Processor {
    handle: std::ptr::NonNull<std::ffi::c_void>,
    spec: Eq,
}
unsafe impl Send for Processor {}
impl Processor {
    pub fn new(eq: Eq, _rate: f32) -> Self {
        Self {
            handle: std::ptr::NonNull::new(unsafe { mm_eq_create() })
                .expect("native EQ allocation"),
            spec: eq,
        }
    }
    pub fn process(
        &mut self,
        audio: &mut [Vec<f32>],
        frames: usize,
        control: &Control,
        _rate: f32,
    ) {
        if let Some(eq) = control.read() {
            self.spec = eq;
        }
        let bands = self.spec.bands.map(|b| RawBand {
            frequency: b.frequency,
            gain_db: b.gain_db,
            q: b.q,
            kind: b.kind,
        });
        unsafe {
            mm_eq_process(
                self.handle.as_ptr(),
                audio[0].as_mut_ptr(),
                audio[1].as_mut_ptr(),
                frames as u32,
                self.spec.bypass,
                bands.as_ptr(),
            );
        }
    }
}
impl Drop for Processor {
    fn drop(&mut self) {
        unsafe { mm_eq_destroy(self.handle.as_ptr()) }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parametric_eq_has_expected_gain_at_center_and_live_bypass_is_unity() {
        let rate = 48000.0;
        let mut eq = Eq::default();
        eq.bands[1] = Band {
            kind: 0,
            frequency: 1000.0,
            gain_db: 6.0,
            q: 1.0,
        };
        let control = Control::new(&eq);
        let mut p = Processor::new(eq, rate);
        let mut audio = vec![vec![0.0; 480]; 2];
        let mut phase = 0usize;
        for bypass in [false, true, false] {
            eq.bypass = bypass;
            control.update(&eq);
            let mut energy = 0.0f64;
            for block in 0..200 {
                for f in 0..480 {
                    let x =
                        0.1 * (2.0 * std::f64::consts::PI * (phase + f) as f64 / 48.0).sin() as f32;
                    audio[0][f] = x;
                    audio[1][f] = x;
                }
                phase += 480;
                p.process(&mut audio, 480, &control, rate);
                if block >= 100 {
                    energy += audio[0].iter().map(|v| f64::from(*v).powi(2)).sum::<f64>();
                }
            }
            let rms = (energy / 48000.0).sqrt();
            let gain = 20.0 * (rms / (0.1 / 2.0f64.sqrt())).log10();
            assert!(
                (gain - if bypass { 0.0 } else { 6.0 }).abs() < 0.01,
                "{gain}"
            );
        }
    }
}
