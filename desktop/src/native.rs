use crate::{bridge::Bridge, config::*, mixer::Mixer};
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
    outputs: usize,
    rate: u32,
    scratch: Vec<f32>,
    bridge: Arc<Bridge>,
    system: bool,
}
pub struct Audio {
    handle: NonNull<c_void>,
    state: Option<Box<State>>,
    pub frames: u32,
}
impl Audio {
    pub fn new(system: bool) -> Result<Self, String> {
        let handle = NonNull::new(unsafe { mm_create(system) })
            .ok_or("Cannot initialise the audio backend")?;
        Ok(Self {
            handle,
            state: None,
            frames: 0,
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
        let mut state = Box::new(State {
            mixer,
            inputs: if system { 0 } else { c.inputs as usize },
            outputs: if system { 2 } else { c.output_channels() },
            rate: c.sample_rate,
            scratch: vec![0.0; MAX_FRAMES * CHANNELS],
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
                    c.output_channels() as u32,
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
    s.scratch[..frames * CHANNELS].fill(0.0);
    if !input.is_null() {
        let input = unsafe { std::slice::from_raw_parts(input, frames * s.inputs) };
        for f in 0..frames {
            for c in 0..s.inputs {
                s.scratch[f * CHANNELS + c] = input[f * s.inputs + c];
            }
        }
    }
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
    if !matches!(result, Ok(true)) {
        output.fill(0.0);
        s.mixer.controls.failed.store(true, Relaxed);
        return 2;
    }
    0
}
