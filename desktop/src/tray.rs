//! Optional Windows notification icon and single normal desktop instance.
#[cfg(windows)]
unsafe extern "C" {
    fn mm_tray_claim() -> i32;
    fn mm_tray_start() -> i32;
    fn mm_tray_poll() -> u32;
    fn mm_tray_update(
        ip: *const std::ffi::c_char,
        latency: *const std::ffi::c_char,
        cpu: *const std::ffi::c_char,
        ram: *const std::ffi::c_char,
    );
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
            _ => Err("No se pudo registrar la instancia de AuMux".into()),
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
    /// Updates a UI-only snapshot; the audio callback never touches this state.
    pub fn update(
        &self,
        local_ip: Option<&str>,
        latency: &serde_json::Value,
        metrics: &crate::telemetry::Metrics,
    ) {
        let ip = format!("IP local: {}", local_ip.unwrap_or("sin conexión LAN"));
        let latency = if latency["running"] != true {
            "Latencia: audio detenido".into()
        } else if let Some(ms) = latency["driver_ms"].as_f64() {
            format!("Latencia del driver (declarada): {ms:.2} ms")
        } else if let Some(ms) = latency["block_ms"].as_f64() {
            format!("Latencia de bloque: {ms:.2} ms")
        } else {
            "Latencia: sin dato".into()
        };
        let cpu = metrics.cpu_percent.map_or_else(
            || "CPU AuMux: calculando…".into(),
            |value| format!("CPU AuMux: {value:.1} %"),
        );
        let ram = metrics.ram_bytes.map_or_else(
            || "RAM AuMux: sin dato".into(),
            |bytes| format!("RAM AuMux: {:.1} MiB", bytes as f64 / 1_048_576.0),
        );
        if let (Ok(ip), Ok(latency), Ok(cpu), Ok(ram)) = (
            std::ffi::CString::new(ip),
            std::ffi::CString::new(latency),
            std::ffi::CString::new(cpu),
            std::ffi::CString::new(ram),
        ) {
            unsafe { mm_tray_update(ip.as_ptr(), latency.as_ptr(), cpu.as_ptr(), ram.as_ptr()) };
        }
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
