//! Cached process telemetry. Sampling belongs to the control loop, never audio.
use serde::Serialize;
use std::time::{Duration, Instant};

const SAMPLE_PERIOD: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, Serialize)]
pub struct Metrics {
    /// Process CPU time divided by wall time and logical processor count.
    pub cpu_percent: Option<f64>,
    pub peak_cpu_percent: Option<f64>,
    pub peak_ram_bytes: Option<u64>,
    pub io_read_bytes_per_second: Option<f64>,
    pub io_write_bytes_per_second: Option<f64>,
    pub peak_io_read_bytes_per_second: Option<f64>,
    pub peak_io_write_bytes_per_second: Option<f64>,
    pub io_scope: &'static str,
    /// Resident working set; excludes the separate browser process.
    pub ram_bytes: Option<u64>,
    pub logical_processors: usize,
    pub sample_interval_ms: Option<u64>,
    pub scope: &'static str,
    pub cpu_normalization: &'static str,
    pub ram_kind: &'static str,
}

struct Raw {
    cpu_nanos: Option<u64>,
    resident_bytes: Option<u64>,
}

pub struct Telemetry {
    latest: Metrics,
    last_sample: Instant,
    previous_cpu: Option<u64>,
    previous_io: Option<(u64, u64)>,
}

impl Telemetry {
    pub fn new() -> Self {
        let raw = read_process();
        Self {
            latest: Metrics {
                cpu_percent: None,
                peak_cpu_percent: None,
                peak_ram_bytes: raw.resident_bytes,
                io_read_bytes_per_second: None,
                io_write_bytes_per_second: None,
                peak_io_read_bytes_per_second: None,
                peak_io_write_bytes_per_second: None,
                io_scope: "process_io_not_disk_busy_percent",
                ram_bytes: raw.resident_bytes,
                logical_processors: logical_processors(),
                sample_interval_ms: None,
                scope: "process",
                cpu_normalization: "all_logical_processors",
                ram_kind: "resident_working_set",
            },
            last_sample: Instant::now(),
            previous_cpu: raw.cpu_nanos,
            previous_io: process_io(),
        }
    }

    pub fn cached(&self) -> &Metrics {
        &self.latest
    }

    /// Returns whether a fresh sample was taken. Status polling only reads cached().
    pub fn refresh_if_due(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_sample);
        if elapsed < SAMPLE_PERIOD {
            return false;
        }
        let raw = read_process();
        self.latest.cpu_percent = normalized_cpu(
            self.previous_cpu,
            raw.cpu_nanos,
            elapsed,
            self.latest.logical_processors,
        );
        self.latest.peak_cpu_percent =
            max_option(self.latest.peak_cpu_percent, self.latest.cpu_percent);
        self.latest.peak_ram_bytes = match (self.latest.peak_ram_bytes, raw.resident_bytes) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        let current = process_io();
        let rate = |a: u64, b: u64| b.checked_sub(a).map(|n| n as f64 / elapsed.as_secs_f64());
        self.latest.io_read_bytes_per_second = self
            .previous_io
            .zip(current)
            .and_then(|(a, b)| rate(a.0, b.0));
        self.latest.io_write_bytes_per_second = self
            .previous_io
            .zip(current)
            .and_then(|(a, b)| rate(a.1, b.1));
        self.latest.peak_io_read_bytes_per_second = max_option(
            self.latest.peak_io_read_bytes_per_second,
            self.latest.io_read_bytes_per_second,
        );
        self.latest.peak_io_write_bytes_per_second = max_option(
            self.latest.peak_io_write_bytes_per_second,
            self.latest.io_write_bytes_per_second,
        );
        self.previous_io = current;
        self.latest.ram_bytes = raw.resident_bytes;
        self.latest.sample_interval_ms = Some(elapsed.as_millis().min(u64::MAX as u128) as u64);
        self.previous_cpu = raw.cpu_nanos;
        self.last_sample = now;
        true
    }
}

fn normalized_cpu(
    previous: Option<u64>,
    current: Option<u64>,
    elapsed: Duration,
    processors: usize,
) -> Option<f64> {
    let nanos = current?.checked_sub(previous?)?;
    if elapsed.is_zero() || processors == 0 {
        return None;
    }
    Some((100.0 * nanos as f64 / elapsed.as_nanos() as f64 / processors as f64).clamp(0.0, 100.0))
}

#[cfg(windows)]
unsafe extern "C" {
    fn mm_process_metrics(cpu_100ns: *mut u64, resident_bytes: *mut u64) -> u32;
    fn mm_logical_processors() -> u32;
}

#[cfg(windows)]
fn logical_processors() -> usize {
    unsafe { mm_logical_processors().max(1) as usize }
}

#[cfg(not(windows))]
fn logical_processors() -> usize {
    std::thread::available_parallelism().map_or(1, usize::from)
}

#[cfg(windows)]
fn read_process() -> Raw {
    let mut cpu = 0;
    let mut ram = 0;
    let available = unsafe { mm_process_metrics(&mut cpu, &mut ram) };
    Raw {
        cpu_nanos: (available & 1 != 0).then(|| cpu.saturating_mul(100)),
        resident_bytes: (available & 2 != 0).then_some(ram),
    }
}

#[cfg(target_os = "linux")]
fn read_process() -> Raw {
    unsafe extern "C" {
        fn sysconf(name: std::ffi::c_int) -> std::ffi::c_long;
    }
    // Linux _SC_CLK_TCK; process stat reports utime/stime in these units.
    let ticks_per_second = unsafe { sysconf(2) };
    let cpu_nanos = std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|s| process_ticks(&s))
        .filter(|_| ticks_per_second > 0)
        .map(|ticks| ((ticks as u128 * 1_000_000_000) / ticks_per_second as u128) as u64);
    let resident_bytes = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| resident_bytes(&s));
    Raw {
        cpu_nanos,
        resident_bytes,
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn read_process() -> Raw {
    Raw {
        cpu_nanos: None,
        resident_bytes: None,
    }
}

#[cfg(any(target_os = "linux", test))]
fn process_ticks(stat: &str) -> Option<u64> {
    // comm may itself contain spaces or parentheses; the final ')' ends field 2.
    let (_, remaining) = stat.rsplit_once(')')?;
    let mut fields = remaining.split_whitespace();
    let user: u64 = fields.nth(11)?.parse().ok()?;
    let system: u64 = fields.next()?.parse().ok()?;
    user.checked_add(system)
}

#[cfg(any(target_os = "linux", test))]
fn resident_bytes(status: &str) -> Option<u64> {
    let line = status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))?;
    let mut fields = line.split_whitespace();
    let kib: u64 = fields.next()?.parse().ok()?;
    (fields.next()? == "kB")
        .then(|| kib.checked_mul(1024))
        .flatten()
}

/// Prefer the chosen LAN interface, while keeping a Tailscale address separate.
#[cfg(any(windows, test))]
pub fn local_ipv4(preferred: &str, addresses: &serde_json::Value) -> Option<String> {
    let eligible = |entry: &&serde_json::Value| {
        let ip = entry["ip"]
            .as_str()
            .and_then(|s| s.parse::<std::net::Ipv4Addr>().ok());
        ip.is_some_and(|ip| {
            let b = ip.octets();
            !ip.is_loopback()
                && !ip.is_unspecified()
                && !ip.is_link_local()
                && !(b[0] == 100 && (64..=127).contains(&b[1]))
                && entry["tailscale_candidate"] != true
        })
    };
    let interfaces = addresses["interfaces"].as_array()?;
    let chosen = interfaces
        .iter()
        .filter(eligible)
        .find(|entry| entry["ip"].as_str() == Some(preferred))
        .or_else(|| {
            interfaces.iter().filter(eligible).find(|entry| {
                entry["ip"]
                    .as_str()
                    .and_then(|s| s.parse::<std::net::Ipv4Addr>().ok())
                    .is_some_and(|ip| ip.is_private())
            })
        })
        .or_else(|| interfaces.iter().filter(eligible).next())?;
    chosen["ip"].as_str().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_normalizes_process_time_and_rejects_invalid_samples() {
        assert_eq!(
            normalized_cpu(Some(0), Some(1_000_000_000), Duration::from_secs(1), 4),
            Some(25.0)
        );
        assert_eq!(
            normalized_cpu(Some(0), Some(8_000_000_000), Duration::from_secs(1), 4),
            Some(100.0)
        );
        assert_eq!(
            normalized_cpu(Some(2), Some(1), Duration::from_secs(1), 4),
            None
        );
        assert_eq!(
            normalized_cpu(None, Some(1), Duration::from_secs(1), 4),
            None
        );
        assert_eq!(normalized_cpu(Some(0), Some(1), Duration::ZERO, 4), None);
    }

    #[test]
    fn proc_parser_handles_spaces_parentheses_and_resident_units() {
        assert_eq!(
            process_ticks("42 (AuMux ) worker) S 0 0 0 0 0 0 0 0 0 0 120 30 0 0"),
            Some(150)
        );
        assert_eq!(
            resident_bytes("Name:\tAuMux\nVmRSS:\t 2048 kB\n"),
            Some(2_097_152)
        );
        assert_eq!(resident_bytes("VmRSS: 2048 bytes"), None);
        assert_eq!(process_ticks("broken"), None);
    }

    #[test]
    fn repeated_status_reads_do_not_resample_the_process() {
        let mut telemetry = Telemetry::new();
        let before = serde_json::to_value(telemetry.cached()).unwrap();
        assert!(!telemetry.refresh_if_due());
        assert_eq!(serde_json::to_value(telemetry.cached()).unwrap(), before);
        assert_eq!(before["scope"], "process");
        assert_eq!(before["ram_kind"], "resident_working_set");
    }

    #[test]
    fn tray_local_ip_prefers_selected_lan_and_excludes_tailnet_and_loopback() {
        let addresses = serde_json::json!({"interfaces":[
            {"ip":"127.0.0.1"}, {"ip":"100.90.1.2","tailscale_candidate":true},
            {"ip":"192.168.1.20"}, {"ip":"10.10.0.4"}, {"ip":"169.254.1.3"}
        ]});
        assert_eq!(
            local_ipv4("10.10.0.4", &addresses).as_deref(),
            Some("10.10.0.4")
        );
        assert_eq!(
            local_ipv4("100.90.1.2", &addresses).as_deref(),
            Some("192.168.1.20")
        );
        assert_eq!(local_ipv4("", &serde_json::json!({"interfaces":[]})), None);
    }
}

fn max_option(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}
#[cfg(windows)]
fn process_io() -> Option<(u64, u64)> {
    unsafe extern "C" {
        fn mm_process_io(read: *mut u64, write: *mut u64) -> bool;
    }
    let (mut read, mut write) = (0, 0);
    unsafe { mm_process_io(&mut read, &mut write) }.then_some((read, write))
}
#[cfg(target_os = "linux")]
fn process_io() -> Option<(u64, u64)> {
    let s = std::fs::read_to_string("/proc/self/io").ok()?;
    let value = |name: &str| {
        s.lines()
            .find_map(|l| l.strip_prefix(name))
            .and_then(|v| v.trim().parse().ok())
    };
    Some((value("read_bytes:")?, value("write_bytes:")?))
}
#[cfg(not(any(windows, target_os = "linux")))]
fn process_io() -> Option<(u64, u64)> {
    None
}
