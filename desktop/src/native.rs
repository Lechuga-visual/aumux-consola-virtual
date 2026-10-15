use crate::{bridge::Bridge, config::*, graph::Exchange, mixer::Mixer};
use serde::Serialize;
use std::{
    ffi::{CStr, c_char, c_void},
    ptr::NonNull,
    sync::{Arc, atomic::Ordering::Relaxed},
};

#[repr(C)]
struct RawDevice {
    id: u32,
    inputs: u32,
    outputs: u32,
    name: [c_char; 512],
}
#[derive(Serialize)]
pub struct Device {
    pub id: u32,
    pub inputs: u32,
    pub outputs: u32,
    pub name: String,
}
type Callback = unsafe extern "C" fn(*mut f32, *const f32, u32, u32, *mut c_void) -> i32;
unsafe extern "C" {
    fn mm_create(system: bool) -> *mut c_void;
    fn mm_destroy(handle: *mut c_void);
    fn mm_close(handle: *mut c_void);
    fn mm_error(handle: *mut c_void) -> *const c_char;
    fn mm_enumerate(handle: *mut c_void) -> i32;
    fn mm_device(handle: *mut c_void, index: u32, result: *mut RawDevice) -> bool;
    fn mm_open(
        handle: *mut c_void,
        device: u32,
        inputs: u32,
        outputs: u32,
        rate: u32,
        frames: *mut u32,
        callback: Callback,
        user: *mut c_void,
    ) -> i32;
    fn mm_start(handle: *mut c_void) -> i32;
    fn mm_open_system(
        handle: *mut c_void,
        rate: u32,
        frames: *mut u32,
        callback: Callback,
        user: *mut c_void,
    ) -> i32;
}
struct State {
    mixer: Mixer,
    inputs: usize,
    input_stride: usize,
    outputs: usize,
    rate: u32,
    scratch: Vec<f32>,
    bridge: Arc<Bridge>,
    system: bool,
    exchange: Arc<Exchange>,
}
pub struct Audio {
    handle: NonNull<c_void>,
    state: Option<Box<State>>,
    pub frames: u32,
    pub outputs: usize,
    exchange: Arc<Exchange>,
}
impl Audio {
    pub fn new(system: bool) -> Result<Self, String> {
        let handle = NonNull::new(unsafe { mm_create(system) })
            .ok_or("Cannot initialise the audio backend")?;
        Ok(Self {
            handle,
            state: None,
            frames: 0,
            outputs: 0,
            exchange: Arc::new(Exchange::new()),
        })
    }
    fn error(&self) -> String {
        unsafe { CStr::from_ptr(mm_error(self.handle.as_ptr())) }
            .to_string_lossy()
            .into_owned()
    }
    pub fn devices(&mut self) -> Result<Vec<Device>, String> {
        let count = unsafe { mm_enumerate(self.handle.as_ptr()) };
        if count < 0 {
            return Err(self.error());
        }
        let mut out = Vec::new();
        for i in 0..count as u32 {
            let mut d = RawDevice {
                id: 0,
                inputs: 0,
                outputs: 0,
                name: [0; 512],
            };
            if unsafe { mm_device(self.handle.as_ptr(), i, &mut d) } {
                out.push(Device {
                    id: d.id,
                    inputs: d.inputs,
                    outputs: d.outputs,
                    name: unsafe { CStr::from_ptr(d.name.as_ptr()) }
                        .to_string_lossy()
                        .into_owned(),
                });
            }
        }
        Ok(out)
    }
    pub fn stop(&mut self) {
        // RtAudio stops and joins callbacks before we drop their state on this control thread.
        unsafe { mm_close(self.handle.as_ptr()) };
        self.state = None;
        self.exchange.clear(); // All retired graphs are freed on the control thread.
    }
    pub fn replace_mixer(&mut self, mixer: Mixer) -> Result<(), String> {
        if let Err(error) = self.exchange.replace(Box::new(mixer)) {
            self.stop();
            return Err(error);
        }
        Ok(())
    }
    pub fn start(
        &mut self,
        c: &Config,
        mixer: Mixer,
        bridge: Arc<Bridge>,
        system: bool,
    ) -> Result<(), String> {
        self.stop();
        let id = if system {
            0
        } else {
            c.device.ok_or("Choose an ASIO device")?
        };
        let outputs = if system {
            2
        } else {
            let device = self
                .devices()?
                .into_iter()
                .find(|d| d.id == id)
                .ok_or("ASIO device disappeared")?;
            (device.outputs as usize).min(MAX_IO)
        };
        if !system && c.output_channels() > outputs {
            return Err("Las salidas asignadas exceden las disponibles en este dispositivo".into());
        }
        self.outputs = outputs;
        let mut state = Box::new(State {
            mixer,
            inputs: if system { 0 } else { c.inputs as usize },
            outputs,
            input_stride: c.inputs as usize,
            exchange: self.exchange.clone(),
            rate: c.sample_rate,
            scratch: vec![0.0; MAX_FRAMES * c.inputs as usize],
            bridge,
            system,
        });
        let mut frames = c.buffer;
        let opened = if system {
            unsafe {
                mm_open_system(
                    self.handle.as_ptr(),
                    c.sample_rate,
                    &mut frames,
                    callback,
                    (&mut *state as *mut State).cast(),
                )
            }
        } else {
            unsafe {
                mm_open(
                    self.handle.as_ptr(),
                    id,
                    c.inputs,
                    outputs as u32,
                    c.sample_rate,
                    &mut frames,
                    callback,
                    (&mut *state as *mut State).cast(),
                )
            }
        };
        if opened != 0 {
            unsafe { mm_close(self.handle.as_ptr()) };
            return Err(self.error());
        }
        if frames as usize > MAX_FRAMES {
            unsafe { mm_close(self.handle.as_ptr()) };
            return Err("Driver buffer exceeds 8192 frames".into());
        }
        self.state = Some(state);
        self.frames = frames;
        if unsafe { mm_start(self.handle.as_ptr()) } != 0 {
            let e = self.error();
            self.stop();
            return Err(e);
        }
        Ok(())
    }
}
impl Drop for Audio {
    fn drop(&mut self) {
        self.stop();
        unsafe { mm_destroy(self.handle.as_ptr()) };
    }
}

unsafe extern "C" fn callback(
    out: *mut f32,
    input: *const f32,
    frames: u32,
    status: u32,
    opaque: *mut c_void,
) -> i32 {
    // Pointers and buffer lengths are supplied by RtAudio for this open stream.
    let s = unsafe { &mut *opaque.cast::<State>() };
    let frames = frames as usize;
    if out.is_null() {
        return 2;
    }
    let output = unsafe { std::slice::from_raw_parts_mut(out, frames * s.outputs) };
    output.fill(0.0);
    if frames > MAX_FRAMES {
        s.mixer.controls.failed.store(true, Relaxed);
        return 2;
    }
    if status != 0 {
        s.mixer.controls.xruns.fetch_add(1, Relaxed);
    }
    s.scratch[..frames * s.input_stride].fill(0.0);
    if !input.is_null() && s.inputs > 0 {
        let input = unsafe { std::slice::from_raw_parts(input, frames * s.inputs) };
        s.scratch[..input.len()].copy_from_slice(input);
    }
    let replacement = s.exchange.begin(&s.mixer);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !s.mixer.process(&s.scratch, output, frames, s.outputs) {
            return false;
        }
        if s.system {
            let selected = s.mixer.controls.pc_monitor.load(Relaxed) as usize;
            if !s.mixer.copy_monitor(output, frames, selected) {
                return false;
            }
        }
        s.bridge.push(&s.mixer.buses[0], frames, s.rate);
        true
    }));
    if let Some(next) = replacement {
        s.exchange
            .finish(&mut s.mixer, next, matches!(result, Ok(true)));
    }
    if !matches!(result, Ok(true)) {
        output.fill(0.0);
        s.mixer.controls.failed.store(true, Relaxed);
        return 2;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    #[test]
    fn actual_callback_adopts_larger_and_smaller_graphs_without_heap_work_or_stream_restart() {
        let mut c = Config::default();
        c.inputs = 2;
        for ch in &mut c.channels {
            ch.input = None;
            ch.input_right = None;
        }
        c.channels[0].generator.signal = Signal::Sine;
        let controls = crate::mixer::Controls::new(&c);
        let mixer = Mixer::new(&c, controls).unwrap();
        let exchange = Arc::new(Exchange::new());
        let mut state = Box::new(State {
            mixer,
            inputs: 2,
            input_stride: 2,
            outputs: 2,
            rate: 48000,
            scratch: vec![0.0; 128],
            bridge: Bridge::start().unwrap(),
            system: true,
            exchange: exchange.clone(),
        });
        let done = Arc::new(AtomicBool::new(false));
        let stop = done.clone();
        let audio = std::thread::spawn(move || {
            let mut output = vec![0.0; 128];
            let input = vec![0.0; 128];
            let mut blocks = 0;
            while !stop.load(Relaxed) {
                let result = crate::test_support::no_heap(|| unsafe {
                    callback(
                        output.as_mut_ptr(),
                        input.as_ptr(),
                        64,
                        0,
                        (&mut *state as *mut State).cast(),
                    )
                });
                assert_eq!(result, 0);
                assert!(output.iter().all(|v| v.is_finite()));
                blocks += 1;
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            (state, blocks)
        });
        let mut latencies = vec![0; c.channels.len() + c.buses.len()];
        for (channels, auxes) in [(16, 9), (MAX_CHANNELS, MAX_AUXES), (1, 0), (8, 4)] {
            let mut next = c.clone();
            next.channels.resize(channels, c.channels[0].clone());
            next.buses.resize(auxes + 1, c.buses[0].clone());
            for b in next.buses.iter_mut().skip(1) {
                b.format = Format::Mono;
                b.output = None;
            }
            for ch in &mut next.channels {
                ch.sends_db.resize(auxes, -90.0);
                ch.pre.resize(auxes, true);
                ch.send_mute.resize(auxes, false);
            }
            let mixer =
                Mixer::reconfigure(&next, crate::mixer::Controls::new(&next), &c, &latencies)
                    .unwrap();
            latencies = mixer.latencies.clone();
            exchange.replace(Box::new(mixer)).unwrap();
            c = next;
        }
        done.store(true, Relaxed);
        let (state, blocks) = audio.join().unwrap();
        assert!(blocks >= 4);
        assert_eq!(state.mixer.controls.channel_meters.len(), 8);
        assert_eq!(state.mixer.buses.len(), 5);
    }
}
