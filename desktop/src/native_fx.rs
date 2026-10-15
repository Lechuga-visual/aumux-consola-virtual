//! Built-in native DSP. No VST module/GUI/runtime discovery is required for these inserts.
use soundcraft_dsp::{Category, ParamInfo, Plugin, PluginInfo, Taper, Unit};
use std::{
    ffi::{CStr, c_char, c_void},
    ptr::NonNull,
    sync::OnceLock,
};
#[repr(C)]
struct RawParam {
    id: *const c_char,
    name: *const c_char,
    min: f32,
    max: f32,
    value: f32,
}
unsafe extern "C" {
    fn mm_fx_count() -> i32;
    fn mm_fx_name(i: i32) -> *const c_char;
    fn mm_fx_create(i: i32) -> *mut c_void;
    fn mm_fx_destroy(p: *mut c_void);
    fn mm_fx_process(p: *mut c_void, l: *mut f32, r: *mut f32, n: u32);
    fn mm_fx_params(p: *mut c_void) -> i32;
    fn mm_fx_param(p: *mut c_void, i: i32, out: *mut RawParam) -> bool;
    fn mm_fx_set(p: *mut c_void, i: i32, value: f32);
}
fn text(p: *const c_char) -> String {
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}
fn permanent(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}
pub fn registry() -> &'static [PluginInfo] {
    static INFO: OnceLock<Vec<PluginInfo>> = OnceLock::new();
    INFO.get_or_init(|| {
        (0..unsafe { mm_fx_count() })
            .map(|i| {
                let ptr =
                    NonNull::new(unsafe { mm_fx_create(i) }).expect("native insert allocation");
                let name = text(unsafe { mm_fx_name(i) });
                let params: Vec<_> = (0..unsafe { mm_fx_params(ptr.as_ptr()) })
                    .map(|p| {
                        let mut raw = RawParam {
                            id: std::ptr::null(),
                            name: std::ptr::null(),
                            min: 0.0,
                            max: 0.0,
                            value: 0.0,
                        };
                        assert!(unsafe { mm_fx_param(ptr.as_ptr(), p, &mut raw) });
                        ParamInfo {
                            id: permanent(text(raw.id)),
                            name: permanent(text(raw.name)),
                            min: raw.min,
                            max: raw.max,
                            default: raw.value,
                            unit: Unit::None,
                            taper: Taper::Linear,
                            choices: &[],
                        }
                    })
                    .collect();
                unsafe { mm_fx_destroy(ptr.as_ptr()) };
                let id = if i == 20 {
                    "native/freevox8".into()
                } else {
                    format!(
                        "native/mda/{}",
                        name.trim_start_matches("MDA ").to_lowercase()
                    )
                };
                PluginInfo {
                    id: permanent(id),
                    name: permanent(name),
                    short_name: "Native",
                    category: Category::Other,
                    params: Box::leak(params.into_boxed_slice()),
                    is_instrument: false,
                    audiosuite: false,
                }
            })
            .collect()
    })
}
struct Native {
    handle: NonNull<c_void>,
    info: &'static PluginInfo,
}
// A single audio callback owns each instance. Parameter edits happen stopped.
unsafe impl Send for Native {}
impl Drop for Native {
    fn drop(&mut self) {
        unsafe { mm_fx_destroy(self.handle.as_ptr()) }
    }
}
impl Plugin for Native {
    fn info(&self) -> &'static PluginInfo {
        self.info
    }
    fn prepare(&mut self, _: f32, _: usize, _: usize) {}
    fn reset(&mut self) {}
    fn set_param(&mut self, id: &str, value: f32) -> bool {
        let Some((index, p)) = self
            .info
            .params
            .iter()
            .enumerate()
            .find(|(_, p)| p.id == id)
        else {
            return false;
        };
        if !value.is_finite() || value < p.min || value > p.max {
            return false;
        }
        unsafe { mm_fx_set(self.handle.as_ptr(), index as i32, value) };
        true
    }
    fn param(&self, id: &str) -> Option<f32> {
        let index = self.info.params.iter().position(|p| p.id == id)?;
        let mut p = RawParam {
            id: std::ptr::null(),
            name: std::ptr::null(),
            min: 0.0,
            max: 0.0,
            value: 0.0,
        };
        unsafe { mm_fx_param(self.handle.as_ptr(), index as i32, &mut p) };
        Some(p.value)
    }
    fn process(&mut self, io: &mut [Vec<f32>], frames: usize) {
        unsafe {
            mm_fx_process(
                self.handle.as_ptr(),
                io[0].as_mut_ptr(),
                io[1].as_mut_ptr(),
                frames as u32,
            );
        }
    }
}
pub fn instantiate(id: &str) -> Result<Box<dyn Plugin>, String> {
    let Some((i, info)) = registry().iter().enumerate().find(|(_, p)| p.id == id) else {
        return Err("Efecto nativo desconocido".into());
    };
    let handle = NonNull::new(unsafe { mm_fx_create(i as i32) })
        .ok_or("No se pudo crear el efecto nativo")?;
    Ok(Box::new(Native { handle, info }))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_native_inserts_process_finite_stereo_and_parameters_without_rust_heap_work() {
        assert_eq!(registry().len(), 21);
        for info in registry() {
            let mut plugin = instantiate(info.id).unwrap();
            let mut audio = vec![vec![0.0; 480]; 2];
            for (i, v) in audio[0].iter_mut().enumerate() {
                *v = (i as f32 * 0.1).sin() * 0.02;
            }
            audio[1] = audio[0].clone();
            for _ in 0..20 {
                crate::test_support::no_heap(|| plugin.process(&mut audio, 480));
                assert!(
                    audio.iter().flatten().all(|v| v.is_finite()),
                    "{}",
                    info.name
                );
            }
            for p in info.params {
                assert!(
                    plugin.set_param(p.id, p.default),
                    "{} {} default={} range={}..{}",
                    info.name,
                    p.id,
                    p.default,
                    p.min,
                    p.max
                );
                assert!(plugin.param(p.id).is_some());
            }
        }
    }
}
