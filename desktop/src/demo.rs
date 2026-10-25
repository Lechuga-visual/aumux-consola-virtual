//! The hardware-free engine owns its render thread, independent of HTTP/plugin preparation.
use crate::{bridge::Bridge, config::*, graph::Exchange, mixer::Mixer};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering::Relaxed},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
pub struct Demo {
    exchange: Arc<Exchange>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Mixer>>,
}
impl Demo {
    pub fn start(mut mixer: Mixer, bridge: Arc<Bridge>, config: &Config) -> Result<Self, String> {
        let exchange = Arc::new(Exchange::new());
        let stop = Arc::new(AtomicBool::new(false));
        let worker_exchange = exchange.clone();
        let worker_stop = stop.clone();
        let frames = config.buffer as usize;
        let inputs = config.inputs as usize;
        let rate = config.sample_rate;
        let thread = thread::Builder::new()
            .name("aumux-demo-audio".into())
            .spawn(move || {
                let input = vec![0.0; frames * inputs];
                let mut output = vec![0.0; frames * MAX_IO];
                let period = Duration::from_secs_f64(frames as f64 / rate as f64);
                let mut next = Instant::now();
                while !worker_stop.load(Relaxed) {
                    let replacement = worker_exchange.begin(&mut mixer);
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        if !mixer.process(&input, &mut output, frames, MAX_IO) {
                            return false;
                        }
                        bridge.push(&mixer.buses[0], frames, rate);
                        true
                    }));
                    let success = matches!(result, Ok(true));
                    if let Some(replacement) = replacement {
                        worker_exchange.finish(&mut mixer, replacement, success);
                    }
                    if !success {
                        mixer.controls.failed.store(true, Relaxed);
                    }
                    next += period;
                    if next + Duration::from_millis(100) < Instant::now() {
                        next = Instant::now();
                    }
                    while !worker_stop.load(Relaxed) {
                        let Some(wait) = next.checked_duration_since(Instant::now()) else {
                            break;
                        };
                        thread::park_timeout(wait);
                    }
                }
                mixer // Joined and destroyed on the control thread, outside the render loop.
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            exchange,
            stop,
            thread: Some(thread),
        })
    }
    pub fn replace_mixer(&self, mixer: Mixer) -> Result<(), String> {
        self.exchange.replace(Box::new(mixer))
    }
}
impl Drop for Demo {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
        self.exchange.clear();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::mixer::Controls;
    #[test]
    fn renders_while_control_thread_is_busy_and_swaps_without_stopping() {
        let mut config = Config::default();
        config.buffer = 64;
        let controls = Controls::new(&config);
        let mixer = Mixer::new(&config, controls.clone()).unwrap();
        let latencies = mixer.fx_latencies.clone();
        let demo = Demo::start(mixer, Bridge::start().unwrap(), &config).unwrap();
        thread::sleep(Duration::from_millis(40));
        let before = controls.performance.snapshot()["blocks"].as_u64().unwrap();
        thread::sleep(Duration::from_millis(40)); // Simulates slow plugin work on the control thread.
        assert!(controls.performance.snapshot()["blocks"].as_u64().unwrap() > before);
        let mut next = config.clone();
        next.channels[0].inserts.push(Fx {
            id: "native/mda/limiter".into(),
            ..Fx::default()
        });
        let next_controls = Controls::new(&next);
        let mixer = Mixer::reconfigure(&next, next_controls.clone(), &config, &latencies).unwrap();
        demo.replace_mixer(mixer).unwrap();
        assert!(
            next_controls.performance.snapshot()["blocks"]
                .as_u64()
                .unwrap()
                > before
        );
    }
}
