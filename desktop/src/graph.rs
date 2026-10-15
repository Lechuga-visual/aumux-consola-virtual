//! One producer (control) / one consumer (audio). Ownership returns to control for destruction.
use crate::mixer::Mixer;
use crossbeam_queue::ArrayQueue;
use std::{
    sync::atomic::{
        AtomicBool,
        Ordering::{Acquire, Release},
    },
    time::{Duration, Instant},
};

pub struct Exchange {
    pending: ArrayQueue<Box<Mixer>>,
    retired: ArrayQueue<Box<Mixer>>,
    applied: AtomicBool,
}
impl Exchange {
    pub fn new() -> Self {
        Self {
            pending: ArrayQueue::new(1),
            retired: ArrayQueue::new(1),
            applied: AtomicBool::new(false),
        }
    }
    /// Only the control thread calls this. Audio keeps running while it awaits the boundary.
    pub fn replace(&self, next: Box<Mixer>) -> Result<(), String> {
        self.applied.store(false, Release);
        self.pending
            .push(next)
            .map_err(|_| "Ya hay un cambio de estructura pendiente".to_string())?;
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if let Some(retired) = self.retired.pop() {
                drop(retired);
                return if self.applied.load(Acquire) {
                    Ok(())
                } else {
                    Err("El motor no pudo activar la nueva estructura".into())
                };
            }
            if Instant::now() > deadline {
                return Err(
                    "El dispositivo no confirmó el cambio de estructura; motor detenido".into(),
                );
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    pub fn begin(&self, old: &Mixer) -> Option<Box<Mixer>> {
        if !self.retired.is_empty() {
            return None;
        }
        let next = self.pending.pop()?;
        old.fade_removed(&next);
        Some(next)
    }
    pub fn finish(&self, old: &mut Mixer, mut next: Box<Mixer>, success: bool) {
        if success {
            next.inherit(old);
            std::mem::swap(old, &mut *next);
        }
        self.applied.store(success, Release);
        // One request at a time; begin() only consumes with an empty retirement slot.
        // Even an invariant failure must never destroy a graph on the callback.
        if let Err(graph) = self.retired.push(next) {
            std::mem::forget(graph);
        }
    }
    pub fn clear(&self) {
        while let Some(graph) = self.pending.pop() {
            drop(graph);
        }
        while let Some(graph) = self.retired.pop() {
            drop(graph);
        }
    }
}
