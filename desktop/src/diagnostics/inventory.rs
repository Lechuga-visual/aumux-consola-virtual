//! Slow OS/driver discovery is cached on a worker, never on audio or HTTP threads.
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
pub struct Inventory {
    value: Arc<Mutex<Value>>,
}
impl Inventory {
    pub fn new() -> Self {
        let value = Arc::new(Mutex::new(
            json!({"state":"collecting","architecture":std::env::consts::ARCH}),
        ));
        let result = value.clone();
        std::thread::Builder::new()
            .name("aumux-inventory".into())
            .spawn(move || {
                *result.lock().unwrap() = collect();
            })
            .expect("inventory worker");
        Self { value }
    }
    pub fn snapshot(&self) -> Value {
        self.value.lock().unwrap().clone()
    }
}
#[cfg(windows)]
fn collect() -> Value {
    use std::{
        os::windows::process::CommandExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    // Fixed script: no user strings, serial numbers, accounts or network configuration.
    let script = r#"$ErrorActionPreference='Stop'; [Console]::OutputEncoding=[System.Text.UTF8Encoding]::new(); $o=Get-CimInstance Win32_OperatingSystem; $c=Get-CimInstance Win32_ComputerSystem; $p=Get-CimInstance Win32_Processor | Select-Object -First 1; @{state='complete';os=@{name=$o.Caption;version=$o.Version;build=$o.BuildNumber};computer=@{manufacturer=$c.Manufacturer;model=$c.Model;ram_total_bytes=$c.TotalPhysicalMemory;cpu=$p.Name};audio_drivers=@(Get-CimInstance Win32_PnPSignedDriver | Where-Object {$_.DeviceClass -eq 'MEDIA'} | Select-Object -First 32 DeviceName,Manufacturer,DriverVersion,DriverDate,InfName);disks=@(Get-CimInstance Win32_LogicalDisk -Filter 'DriveType=3' | Select-Object -First 16 DeviceID,Size,FreeSpace)} | ConvertTo-Json -Depth 6 -Compress"#;
    let spawn = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut child) = spawn {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(25))
                }
                _ => {
                    let _ = child.kill();
                    break;
                }
            }
        }
        if let Ok(out) = child.wait_with_output() {
            if out.status.success() && out.stdout.len() < 1_048_576 {
                if let Ok(mut v) = serde_json::from_slice::<Value>(&out.stdout) {
                    v["architecture"] = json!(std::env::consts::ARCH);
                    return v;
                }
            }
        }
    }
    json!({"state":"unavailable","os":std::env::consts::OS,"architecture":std::env::consts::ARCH,"note":"Windows no devolvió el inventario CIM. Completa las versiones manualmente."})
}
#[cfg(not(windows))]
fn collect() -> Value {
    let os = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                .map(|v| v.trim_matches('"').to_owned())
        });
    let cpu = std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|s| {
        s.lines().find_map(|l| {
            l.strip_prefix("model name")
                .and_then(|v| v.split_once(':'))
                .map(|(_, v)| v.trim().to_owned())
        })
    });
    let ram = std::fs::read_to_string("/proc/meminfo").ok().and_then(|s| {
        s.lines()
            .find_map(|l| l.strip_prefix("MemTotal:"))
            .and_then(|v| v.split_whitespace().next())
            .and_then(|v| v.parse::<u64>().ok())
            .map(|v| v * 1024)
    });
    json!({"state":"complete","os":os.unwrap_or_else(||std::env::consts::OS.into()),"architecture":std::env::consts::ARCH,"cpu":cpu,"ram_total_bytes":ram,"audio_driver_note":"Las versiones del hardware/controlador deben verificarse en el equipo de Windows."})
}
