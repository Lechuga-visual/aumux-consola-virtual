//! Cached process telemetry. Sampling belongs to the control loop, never audio.
use serde::Serialize;
use std::time::{Duration, Instant};

const SAMPLE_PERIOD: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, Serialize)]
pub struct Metrics {
    /// Process CPU time divided by wall time and logical processor count.
    pub cpu_percent: Option<f64>,
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
}

impl Telemetry {
    pub fn new() -> Self {
        let raw = read_process();
        Self {
            latest: Metrics {
                cpu_percent: None,
                ram_bytes: raw.resident_bytes,
                logical_processors: logical_processors(),
                sample_interval_ms: None,
                scope: "process",
                cpu_normalization: "all_logical_processors",
                ram_kind: "resident_working_set",
            },
            last_sample: Instant::now(),
            previous_cpu: raw.cpu_nanos,
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
