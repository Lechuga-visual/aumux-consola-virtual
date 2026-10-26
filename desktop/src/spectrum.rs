//! EQ-only spectral taps. The callback copies bounded blocks; FFTs run on a worker.
use crate::config::{MAX_AUXES, MAX_CHANNELS};
use crate::diagnostics::signal::{Complex, fft};
use crossbeam_queue::ArrayQueue;
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{
        AtomicU32, AtomicU64,
        Ordering::{Acquire, Relaxed, Release},
    },
};
use std::time::{Duration, Instant};
pub const WINDOW: usize = 4096;
pub const BINS: usize = 256;
const CHUNK: usize = 256;
const LEASE: u64 = 2500;
const MAX_ACTIVE: usize = 8;
struct Block {
    generation: u64,
    sequence: u64,
    frames: usize,
    samples: [[f32; CHUNK]; 4], // PRE L/R, POST L/R; never sum opposite polarities.
}
struct Slot {
    queue: ArrayQueue<Block>,
    until: AtomicU64,
    generation: AtomicU64,
    sequence: AtomicU64,
    mask: AtomicU32,
    dropped: AtomicU64,
    snapshot: Mutex<Option<Snapshot>>,
}
impl Slot {
    fn new() -> Self {
        Self {
            queue: ArrayQueue::new(64),
            until: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            sequence: AtomicU64::new(0),
            mask: AtomicU32::new(0),
            dropped: AtomicU64::new(0),
            snapshot: Mutex::new(None),
        }
    }
}
#[derive(Clone, Serialize)]
struct Snapshot {
    generation: u64,
    sequence: u64,
    timestamp_ms: u64,
    pre: Vec<f32>,
    post: Vec<f32>,
}
pub struct Hub {
    slots: Vec<OnceLock<Slot>>,
    began: Instant,
    clock: AtomicU64,
}
impl Hub {
    pub fn new() -> Arc<Self> {
        let hub = Arc::new(Self {
            slots: (0..MAX_CHANNELS + MAX_AUXES + 1)
                .map(|_| OnceLock::new())
                .collect(),
            began: Instant::now(),
            clock: AtomicU64::new(1),
        });
        let weak = Arc::downgrade(&hub);
        std::thread::Builder::new()
            .name("aumux-eq-spectrum".into())
            .spawn(move || {
                let mut states: Vec<Option<State>> =
                    (0..MAX_CHANNELS + MAX_AUXES + 1).map(|_| None).collect();
                loop {
                    let Some(hub) = weak.upgrade() else { break };
                    let now = hub.now();
                    hub.clock.store(now, Release);
                    for (i, entry) in hub.slots.iter().enumerate() {
                        let Some(slot) = entry.get() else { continue };
                        if slot.until.load(Acquire) <= now {
                            while slot.queue.pop().is_some() {}
                            states[i] = None;
                            continue;
                        }
                        let generation = slot.generation.load(Acquire);
                        if states[i]
                            .as_ref()
                            .is_none_or(|s| s.generation != generation)
                        {
                            states[i] = Some(State::new(generation));
                        }
                        let state = states[i].as_mut().unwrap();
                        while let Some(block) = slot.queue.pop() {
                            if block.generation == generation {
                                state.push(&block);
                            }
                        }
                        if state.valid >= WINDOW && state.since >= 2048 {
                            state.since = 0;
                            let mask = slot.mask.load(Relaxed);
                            let pre = if mask & 1 != 0 {
                                state.spectrum(0)
                            } else {
                                vec![]
                            };
                            let post = if mask & 2 != 0 {
                                state.spectrum(2)
                            } else {
                                vec![]
                            };
                            *slot.snapshot.lock().unwrap() = Some(Snapshot {
                                generation,
                                sequence: state.sequence,
                                timestamp_ms: now,
                                pre,
                                post,
                            });
                        }
                    }
                    drop(hub);
                    std::thread::sleep(Duration::from_millis(10));
                }
            })
            .expect("spectrum worker");
        hub
    }
    fn now(&self) -> u64 {
        self.began.elapsed().as_millis() as u64 + 1
    }
    pub fn request(&self, node: usize, mask: u32, running: bool) -> Result<Value, String> {
        let entry = self.slots.get(node).ok_or("Fuente espectral inválida")?;
        let now = self.now();
        let slot = entry.get_or_init(Slot::new);
        if running {
            if slot.until.load(Acquire) <= now {
                if self
                    .slots
                    .iter()
                    .filter_map(OnceLock::get)
                    .filter(|s| s.until.load(Acquire) > now)
                    .count()
                    >= MAX_ACTIVE
                {
                    return Err("Máximo ocho tiras con analizador simultáneo; cierra otro EQ o apaga su analizador".into());
                }
                slot.generation.fetch_add(1, Release);
                slot.mask.store(mask & 3, Relaxed);
                *slot.snapshot.lock().unwrap() = None;
            } else {
                slot.mask.fetch_or(mask & 3, Relaxed);
            }
            slot.until.store(now + LEASE, Release);
        }
        let snapshot = slot
            .snapshot
            .lock()
            .unwrap()
            .clone()
            .filter(|s| s.generation == slot.generation.load(Acquire));
        Ok(
            json!({"running":running,"now_ms":now,"window_frames":WINDOW,"window_ms":WINDOW as f64/48.0,"bin_hz":48000.0/WINDOW as f64,"bins":BINS,"dropped_blocks":slot.dropped.load(Relaxed),"data":if running {snapshot} else {None}}),
        )
    }
    pub fn reset(&self) {
        for slot in self.slots.iter().filter_map(OnceLock::get) {
            slot.until.store(0, Release);
            slot.generation.fetch_add(1, Release);
            *slot.snapshot.lock().unwrap() = None;
        }
    }
    pub fn wants(&self, node: usize) -> Option<u64> {
        let slot = self.slots.get(node)?.get()?;
        (slot.until.load(Acquire) > self.clock.load(Acquire)).then(|| slot.generation.load(Acquire))
    }
    pub fn capture(
        &self,
        node: usize,
        generation: u64,
        pre: &[Vec<f32>; 2],
        post: &[Vec<f32>],
        frames: usize,
    ) {
        let Some(slot) = self.slots.get(node).and_then(OnceLock::get) else {
            return;
        };
        for start in (0..frames).step_by(CHUNK) {
            let n = (frames - start).min(CHUNK);
            let mut block = Block {
                generation,
                sequence: slot.sequence.fetch_add(1, Relaxed),
                frames: n,
                samples: [[0.0; CHUNK]; 4],
            };
            for side in 0..2 {
                block.samples[side][..n].copy_from_slice(&pre[side][start..start + n]);
                block.samples[2 + side][..n].copy_from_slice(&post[side][start..start + n]);
            }
            if slot.queue.push(block).is_err() {
                slot.dropped.fetch_add(1, Relaxed);
            }
        }
    }
}
struct State {
    generation: u64,
    sequence: u64,
    first: bool,
    cursor: usize,
    valid: usize,
    since: usize,
    samples: [Vec<f32>; 4],
}
impl State {
    fn new(generation: u64) -> Self {
        Self {
            generation,
            sequence: 0,
            first: true,
            cursor: 0,
            valid: 0,
            since: 0,
            samples: std::array::from_fn(|_| vec![0.0; WINDOW]),
        }
    }
    fn push(&mut self, block: &Block) {
        if !self.first && block.sequence != self.sequence.wrapping_add(1) {
            self.valid = 0;
            self.since = 0;
        }
        self.first = false;
        self.sequence = block.sequence;
        for f in 0..block.frames {
            for side in 0..4 {
                let x = block.samples[side][f];
                self.samples[side][self.cursor] = if x.is_finite() { x } else { 0.0 };
            }
            self.cursor = (self.cursor + 1) % WINDOW;
        }
        self.valid = (self.valid + block.frames).min(WINDOW);
        self.since += block.frames;
    }
    fn spectrum(&self, side: usize) -> Vec<f32> {
        let mut lanes: [Vec<Complex>; 2] =
            std::array::from_fn(|_| vec![Complex::default(); WINDOW]);
        let mut sum = 0.0;
        for i in 0..WINDOW {
            let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / (WINDOW - 1) as f64).cos();
            sum += w;
            for c in 0..2 {
                lanes[c][i].re = f64::from(self.samples[side + c][(self.cursor + i) % WINDOW]) * w;
            }
        }
        for lane in &mut lanes {
            fft(lane, false);
        }
        // Peak amplitude calibration: a bin-centred 0 dBFS sine reads 0 dBFS.
        let power = (0..WINDOW / 2)
            .map(|i| {
                let norm = |v: Complex| v.re * v.re + v.im * v.im;
                (norm(lanes[0][i]) + norm(lanes[1][i])) * 2.0 / (sum * sum)
            })
            .collect::<Vec<_>>();
        (0..BINS)
            .map(|i| {
                let lo_hz = 20.0 * 1000f64.powf((i as f64 - 0.5) / (BINS - 1) as f64);
                let hi_hz = 20.0 * 1000f64.powf((i as f64 + 0.5) / (BINS - 1) as f64);
                let lo = (lo_hz * WINDOW as f64 / 48000.0)
                    .round()
                    .clamp(1.0, (WINDOW / 2 - 1) as f64) as usize;
                let hi = (hi_hz * WINDOW as f64 / 48000.0)
                    .round()
                    .clamp(lo as f64, (WINDOW / 2 - 1) as f64) as usize;
                let p = power[lo..=hi].iter().copied().fold(0.0f64, f64::max);
                (10.0 * p.max(1e-12).log10()).clamp(-120.0, 24.0) as f32
            })
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn calibrated_stereo_power_preserves_opposite_polarities_and_post_level() {
        let mut state = State::new(1);
        for start in (0..WINDOW).step_by(CHUNK) {
            let mut b = Block {
                generation: 1,
                sequence: (start / CHUNK) as u64,
                frames: CHUNK,
                samples: [[0.0; CHUNK]; 4],
            };
            for f in 0..CHUNK {
                let x = 0.1
                    * (std::f64::consts::TAU * (start + f) as f64 * 64.0 / WINDOW as f64).sin()
                        as f32;
                b.samples[0][f] = x;
                b.samples[1][f] = -x;
                b.samples[2][f] = x * 0.5;
                b.samples[3][f] = -x * 0.5;
            }
            state.push(&b);
        }
        let max = |v: Vec<f32>| v.into_iter().fold(-120.0f32, f32::max);
        assert!((max(state.spectrum(0)) + 20.0).abs() < 0.01);
        assert!((max(state.spectrum(2)) + 26.0206).abs() < 0.01);
        let gap = Block {
            generation: 1,
            sequence: 1000,
            frames: CHUNK,
            samples: [[0.0; CHUNK]; 4],
        };
        state.push(&gap);
        assert_eq!(state.valid, CHUNK);
    }
    #[test]
    fn callback_taps_are_bounded_and_do_not_allocate_or_change_audio() {
        let hub = Hub {
            slots: vec![OnceLock::from(Slot::new())],
            began: Instant::now(),
            clock: AtomicU64::new(1),
        };
        hub.request(0, 3, true).unwrap();
        let generation = hub.wants(0).unwrap();
        let pre = std::array::from_fn(|_| vec![0.1; 8192]);
        let post = vec![vec![0.05; 8192]; 2];
        crate::test_support::no_heap(|| {
            for _ in 0..4 {
                hub.capture(0, generation, &pre, &post, 8192);
            }
        });
        assert_eq!(post[0][0], 0.05);
        assert!(hub.slots[0].get().unwrap().dropped.load(Relaxed) > 0);
        hub.reset();
        assert!(hub.wants(0).is_none());
    }
}
