//! Optional Windows notification icon and single normal desktop instance.
#[cfg(windows)]
unsafe extern "C" {
    fn mm_tray_claim() -> i32;
    fn mm_tray_start() -> i32;
    fn mm_tray_poll() -> u32;
    fn mm_tray_stop();
    fn mm_desktop_open_url(url: *const std::ffi::c_char);
    fn mm_desktop_error(error: *const std::ffi::c_char);
}
#[cfg(windows)]
pub struct Tray;
#[cfg(windows)]
impl Tray {
    pub fn claim() -> Result<Option<Self>, String> {
        match unsafe { mm_tray_claim() } {
            0 => Ok(None),
            1 => Ok(Some(Self)),
            _ => Err("No se pudo registrar la instancia de Monitor Mix".into()),
        }
    }
    pub fn start(&self) -> Result<(), String> {
        if unsafe { mm_tray_start() } == 1 {
            Ok(())
        } else {
            Err("No se pudo crear el icono de notificación".into())
        }
    }
    pub fn poll(&self) -> u32 {
        unsafe { mm_tray_poll() }
    }
}
#[cfg(windows)]
impl Drop for Tray {
    fn drop(&mut self) {
        unsafe { mm_tray_stop() };
    }
}
#[cfg(windows)]
pub fn error(message: &str) {
    if let Ok(text) = std::ffi::CString::new(message) {
        unsafe { mm_desktop_error(text.as_ptr()) };
    }
}

#[cfg(windows)]
pub fn open_url(url: &str) {
    if let Ok(url) = std::ffi::CString::new(url) {
        unsafe { mm_desktop_open_url(url.as_ptr()) };
    }
}
