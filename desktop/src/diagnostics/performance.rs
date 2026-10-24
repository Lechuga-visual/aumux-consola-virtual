//! Monotonic DSP timings: bounded atomics only, never a history on the audio thread.
use serde_json::{Value, json};
use std::{
    sync::atomic::{AtomicU64, Ordering::Relaxed},
    time::Instant,
};
#[derive(Default)]
pub struct Performance {
    blocks: AtomicU64,
    nanos: AtomicU64,
    maximum: AtomicU64,
    latest: AtomicU64,
    overruns: AtomicU64,
}
pub struct Timer<'a> {
    p: &'a Performance,
    start: Instant,
    budget: u64,
}
impl Performance {
    pub fn measure(&self, frames: usize, rate: u32) -> Timer<'_> {
        Timer {
            p: self,
            start: Instant::now(),
            budget: frames as u64 * 1_000_000_000 / u64::from(rate.max(1)),
        }
    }
    pub fn snapshot(&self) -> Value {
        let n = self.blocks.load(Relaxed);
        let last = self.latest.load(Relaxed);
        json!({"scope":"mixer_dsp", "blocks":n, "total_ns":self.nanos.load(Relaxed),
            "last_ms": last as f64 / 1e6,
            "max_ms":self.maximum.load(Relaxed).max(last) as f64/1e6,
            "mean_ms":if n == 0 {0.0} else {self.nanos.load(Relaxed) as f64 / n as f64 /1e6},
            "over_budget_blocks":self.overruns.load(Relaxed)})
    }
}
impl Drop for Timer<'_> {
    fn drop(&mut self) {
        let n = self.start.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        self.p.latest.store(n, Relaxed);
        self.p.maximum.fetch_max(n, Relaxed);
        self.p.nanos.fetch_add(n, Relaxed);
        self.p.blocks.fetch_add(1, Relaxed);
        if n > self.budget {
            self.p.overruns.fetch_add(1, Relaxed);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn times_real_work_with_no_heap_and_does_not_reset_on_read() {
        let p = Performance::default();
        crate::test_support::no_heap(|| {
            let _timer = p.measure(256, 48000);
        });
        let v = p.snapshot();
        assert_eq!(v["blocks"], 1);
        assert!(v["max_ms"].as_f64().unwrap() >= 0.0);
        assert_eq!(v, p.snapshot());
    }
}
