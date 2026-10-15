//! Read-only LAN PCM monitor. The callback only copies into bounded, preallocated queues.
use crossbeam_queue::ArrayQueue;
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::{
        Arc, Mutex,
        atomic::{
            AtomicBool, AtomicU16, AtomicU64, AtomicUsize,
            Ordering::{Acquire, Relaxed, Release},
        },
    },
    thread,
    time::{Duration, Instant},
};
const CLIENTS: usize = 8;
const FRAMES: usize = 480;
struct Packet {
    samples: [f32; FRAMES * 2],
    frames: usize,
    generation: u64,
}
struct Slot {
    active: AtomicBool,
    bus: AtomicUsize,
    generation: AtomicU64,
    free: ArrayQueue<Box<Packet>>,
    ready: ArrayQueue<Box<Packet>>,
    dropped: AtomicU64,
}
struct Runtime {
    alive: Arc<AtomicBool>,
    thread: thread::JoinHandle<()>,
}
pub struct Hub {
    pub gate: crate::access::Gate,
    latency: Mutex<Value>,
    enabled: AtomicBool,
    running: AtomicBool,
    port: AtomicU16,
    key: String,
    public_host: Mutex<String>,
    slots: Vec<Slot>,
    names: Mutex<Vec<(String, String)>>,
    runtime: Mutex<Option<Runtime>>,
    connections: AtomicUsize,
}
impl Hub {
    pub fn new() -> Result<Arc<Self>, String> {
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|e| e.to_string())?;
        let key = random.iter().map(|b| format!("{b:02x}")).collect();
        let slots = (0..CLIENTS)
            .map(|_| {
                let slot = Slot {
                    active: AtomicBool::new(false),
                    bus: AtomicUsize::new(0),
                    generation: AtomicU64::new(0),
                    free: ArrayQueue::new(32),
                    ready: ArrayQueue::new(32),
                    dropped: AtomicU64::new(0),
                };
                for _ in 0..32 {
                    slot.free
                        .push(Box::new(Packet {
                            samples: [0.0; FRAMES * 2],
                            frames: 0,
                            generation: 0,
                        }))
                        .ok();
                }
                slot
            })
            .collect();
        Ok(Arc::new(Self {
            gate: crate::access::Gate::new(),
            latency: Mutex::new(Value::Null),
            enabled: AtomicBool::new(false),
            running: AtomicBool::new(false),
            port: AtomicU16::new(0),
            key,
            public_host: Mutex::new(String::new()),
            slots,
            names: Mutex::new(vec![]),
            runtime: Mutex::new(None),
            connections: AtomicUsize::new(0),
        }))
    }
    pub fn configure(&self, c: &crate::config::Config) {
        *self.names.lock().unwrap() = c
            .buses
            .iter()
            .map(|b| {
                (
                    b.name.clone(),
                    if b.format == crate::config::Format::Mono {
                        "mono".into()
                    } else {
                        "stereo".into()
                    },
                )
            })
            .collect();
    }
    pub fn running(&self, value: bool) {
        self.running.store(value, Release);
    }
    pub fn stop(&self) {
        self.gate.revoke();
        self.enabled.store(false, Release);
        if let Some(r) = self.runtime.lock().unwrap().take() {
            r.alive.store(false, Release);
            let _ = r.thread.join();
        }
    }
    pub fn start(self: &Arc<Self>, port: u16) -> Result<Value, String> {
        self.stop();
        let host = std::env::var("MONITOR_LISTEN_HOST").unwrap_or_else(|_| "0.0.0.0".into());
        let server = TcpListener::bind((host.as_str(), port))
            .map_err(|e| crate::web_http::bind_error("la escucha web", &host, port, e))?;
        server.set_nonblocking(true).map_err(|e| e.to_string())?;
        self.port.store(
            server.local_addr().map_err(|e| e.to_string())?.port(),
            Relaxed,
        );
        let alive = Arc::new(AtomicBool::new(true));
        let flag = alive.clone();
        let hub = self.clone();
        self.enabled.store(true, Release);
        let task = thread::Builder::new()
            .name("monitor-lan".into())
            .spawn(move || {
                while flag.load(Acquire) {
                    match server.accept() {
                        Ok((stream, _)) => {
                            if hub.connections.fetch_add(1, Relaxed) >= 16 {
                                hub.connections.fetch_sub(1, Relaxed);
                                continue;
                            }
                            let h = hub.clone();
                            let a = flag.clone();
                            if thread::Builder::new()
                                .name("monitor-listener".into())
                                .spawn(move || {
                                    let _guard = Connection(h.clone());
                                    let _ = h.serve(stream, a);
                                })
                                .is_err()
                            {
                                hub.connections.fetch_sub(1, Relaxed);
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5))
                        }
                        Err(_) => break,
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        *self.runtime.lock().unwrap() = Some(Runtime {
            alive,
            thread: task,
        });
        Ok(self.status())
    }
    pub fn set_host(&self, host: &str) -> Result<(), String> {
        *self.public_host.lock().unwrap() = share_host(host)?;
        Ok(())
    }
    pub fn set_latency(&self, value: Value) {
        *self.latency.lock().unwrap() = value;
    }
    pub fn status(&self) -> Value {
        let host = self.public_host.lock().unwrap();
        let ip = if host.is_empty() {
            local_ip()
        } else {
            host.clone()
        };
        let ip = url_host(&ip);
        let port = self.port.load(Relaxed);
        json!({"enabled":self.enabled.load(Acquire),"port":port,"host":*host,"access":self.gate.status(),"generic_url":format!("http://{ip}:{port}/"),"url":if self.gate.status()["mode"]=="disabled" {format!("http://{ip}:{port}/?key={}",self.key)}else{format!("http://{ip}:{port}/")},"clients":self.slots.iter().filter(|s|s.active.load(Relaxed)).count(),"dropped":self.slots.iter().map(|s|s.dropped.load(Relaxed)).sum::<u64>()})
    }
    pub fn publish(&self, buses: &[Vec<Vec<f32>>], frames: usize) {
        if !self.enabled.load(Acquire) {
            return;
        }
        for slot in &self.slots {
            if !slot.active.load(Acquire) {
                continue;
            }
            let generation = slot.generation.load(Acquire);
            let bus = slot.bus.load(Relaxed);
            for first in (0..frames).step_by(FRAMES) {
                let n = (frames - first).min(FRAMES);
                let Some(mut packet) = slot.free.pop() else {
                    slot.dropped.fetch_add(1, Relaxed);
                    continue;
                };
                packet.frames = n;
                packet.generation = generation;
                for f in 0..n {
                    for side in 0..2 {
                        packet.samples[f * 2 + side] =
                            buses.get(bus).map_or(0.0, |b| b[side][first + f]);
                    }
                }
                if let Err(p) = slot.ready.push(packet) {
                    slot.dropped.fetch_add(1, Relaxed);
                    recycle(slot, p);
                }
            }
        }
    }
    fn serve(
        self: &Arc<Self>,
        mut stream: TcpStream,
        alive: Arc<AtomicBool>,
    ) -> std::io::Result<()> {
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        stream.set_nodelay(true)?;
        let request = match crate::web_http::headers(&mut stream) {
            Ok(v) => v,
            Err(e) => return crate::web_http::request_error(&mut stream, e),
        };
        let text = String::from_utf8_lossy(&request);
        let mut line = text.lines().next().unwrap_or("").split_whitespace();
        let method = line.next();
        let headers: Vec<_> = text
            .lines()
            .skip(1)
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim(), v.trim()))
            .collect();
        let header = |name: &str| {
            headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| *v)
        };
        let cookie = crate::access::cookie_value(header("Cookie").unwrap_or(""), "mm_listen");
        let uri = line.next().unwrap_or("");
        if method == Some("POST") && uri == "/login/listen" {
            if header("Origin")
                .is_some_and(|o| Some(o.strip_prefix("http://").unwrap_or("")) != header("Host"))
            {
                return response(
                    &mut stream,
                    403,
                    "application/json",
                    b"{\"error\":\"Origin denied\"}",
                );
            }
            let n = header("Content-Length")
                .and_then(|n| n.parse::<usize>().ok())
                .filter(|n| *n <= 2048);
            if header("Content-Type") != Some("application/json")
                || header("Transfer-Encoding").is_some()
                || n.is_none()
            {
                return response(
                    &mut stream,
                    400,
                    "application/json",
                    b"{\"error\":\"JSON required\"}",
                );
            }
            let mut body = vec![0; n.unwrap()];
            if let Err(error) = stream.read_exact(&mut body) {
                return crate::web_http::request_error(&mut stream, error);
            }
            return crate::access::login_http(&self.gate, &mut stream, "listen", &body);
        }
        if method != Some("GET") {
            return response(&mut stream, 405, "text/plain", b"Read-only monitor");
        }
        let (path, query) = uri.split_once('?').unwrap_or((uri, ""));
        if path == "/brand-logo.png" {
            return response(
                &mut stream,
                200,
                "image/png",
                include_bytes!("../assets/aumux-logo.png"),
            );
        }
        let query: Vec<_> = query.split('&').filter_map(|p| p.split_once('=')).collect();
        let key_access = query.iter().any(|(k, v)| *k == "key" && *v == self.key)
            && self.gate.status()["mode"] == "disabled";
        if !key_access && !self.gate.authorized(cookie) {
            if path == "/" && self.gate.status()["mode"] == "password" {
                return response(
                    &mut stream,
                    200,
                    "text/html; charset=utf-8",
                    crate::access::login_page("listen").as_bytes(),
                );
            }
            return response(
                &mut stream,
                403,
                "application/json",
                b"{\"error\":\"Autenticacion requerida\"}",
            );
        }
        match path {
            "/" => response(
                &mut stream,
                200,
                "text/html; charset=utf-8",
                include_str!("../web/listen.html")
                    .replace("__KEY__", if key_access { &self.key } else { "" })
                    .as_bytes(),
            ),
            "/status" => {
                let names = self.names.lock().unwrap();
                let data = json!({"running":self.running.load(Acquire),"buses":names.iter().enumerate().map(|(i,(name,format))|json!({"index":i,"name":name,"format":format})).collect::<Vec<_>>(),"sample_rate":48000,"latency":*self.latency.lock().unwrap(),"queue_ms_upper_bound":self.slots.iter().filter(|s|s.active.load(Relaxed)).map(|s|s.ready.len() as f64*10.0).fold(0.0_f64,f64::max)});
                response(
                    &mut stream,
                    200,
                    "application/json",
                    data.to_string().as_bytes(),
                )
            }
            "/stream" => {
                let bus = query
                    .iter()
                    .find(|(k, _)| *k == "bus")
                    .and_then(|(_, v)| v.parse::<usize>().ok());
                let Some(bus) = bus.filter(|b| *b < self.names.lock().unwrap().len()) else {
                    return response(&mut stream, 400, "text/plain", b"Unknown mix");
                };
                let Some(slot) = self.slots.iter().find(|s| {
                    s.active
                        .compare_exchange(false, true, std::sync::atomic::Ordering::AcqRel, Relaxed)
                        .is_ok()
                }) else {
                    return response(
                        &mut stream,
                        503,
                        "text/plain",
                        b"Eight listeners already connected",
                    );
                };
                let _guard = Subscription(slot);
                while let Some(p) = slot.ready.pop() {
                    recycle(slot, p);
                }
                slot.bus.store(bus, Relaxed);
                let generation = slot.generation.fetch_add(1, Release) + 1;
                stream.write_all(b"HTTP/1.0 200 OK\r\nContent-Type: application/octet-stream\r\nCache-Control: no-store\r\nConnection: close\r\nX-Sample-Rate: 48000\r\nX-Channels: 2\r\n\r\n")?;
                let mut bytes = [0u8; FRAMES * 8];
                let mut last = Instant::now();
                while alive.load(Acquire)
                    && ((key_access && self.gate.status()["mode"] == "disabled")
                        || self.gate.authorized(cookie))
                {
                    while slot.ready.len() > 12 {
                        if let Some(p) = slot.ready.pop() {
                            recycle(slot, p);
                            slot.dropped.fetch_add(1, Relaxed);
                        }
                    }
                    if let Some(p) = slot.ready.pop() {
                        if p.generation != generation {
                            recycle(slot, p);
                            continue;
                        }
                        let count = p.frames * 8;
                        for (i, v) in p.samples[..p.frames * 2].iter().enumerate() {
                            bytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
                        }
                        recycle(slot, p);
                        stream.write_all(&bytes[..count])?;
                        last = Instant::now();
                    } else if last.elapsed() > Duration::from_millis(30) {
                        bytes.fill(0);
                        stream.write_all(&bytes)?;
                        last = Instant::now();
                    } else {
                        thread::sleep(Duration::from_millis(2));
                    }
                }
                Ok(())
            }
            _ => response(&mut stream, 404, "text/plain", b"Not found"),
        }
    }
}
struct Connection(Arc<Hub>);
impl Drop for Connection {
    fn drop(&mut self) {
        self.0.connections.fetch_sub(1, Relaxed);
    }
}
struct Subscription<'a>(&'a Slot);
impl Drop for Subscription<'_> {
    fn drop(&mut self) {
        self.0.active.store(false, Release);
    }
}
fn recycle(s: &Slot, p: Box<Packet>) {
    if let Err(p) = s.free.push(p) {
        std::mem::forget(p);
    }
}
fn response(s: &mut TcpStream, status: u16, kind: &str, body: &[u8]) -> std::io::Result<()> {
    write!(
        s,
        "HTTP/1.0 {status} Response\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    s.write_all(body)
}
pub fn local_ip() -> String {
    if let Ok(host) = std::env::var("MONITOR_LISTEN_HOST") {
        if host != "0.0.0.0" {
            return host;
        }
    }
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("192.168.1.1:9")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audio_publish_is_bounded_allocation_free_and_keeps_mixes_separate() {
        let hub = Hub::new().unwrap();
        hub.enabled.store(true, Release);
        let buses = vec![vec![vec![0.1; 8192]; 2], vec![vec![0.25; 8192]; 2]];
        for (i, s) in hub.slots.iter().take(2).enumerate() {
            s.bus.store(i, Relaxed);
            s.active.store(true, Release);
        }
        crate::test_support::no_heap(|| hub.publish(&buses, 8192));
        for (i, s) in hub.slots.iter().take(2).enumerate() {
            assert_eq!(s.ready.len(), 18);
            let mut count = 0;
            while let Some(p) = s.ready.pop() {
                assert!(
                    p.samples[..p.frames * 2]
                        .iter()
                        .all(|v| *v == if i == 0 { 0.1 } else { 0.25 })
                );
                count += p.frames;
                recycle(s, p);
            }
            assert_eq!(count, 8192);
            assert_eq!(s.free.len(), 32);
        }
        for _ in 0..10 {
            crate::test_support::no_heap(|| hub.publish(&buses, 8192));
        }
        assert!(hub.slots[0].dropped.load(Relaxed) > 0);
        assert_eq!(hub.slots[0].ready.len(), 32);
    }
}

/// Address to advertise in links. Binding stays on local network interfaces, including Tailscale.
pub fn share_host(value: &str) -> Result<String, String> {
    let host = value.trim();
    if host.is_empty() {
        return Ok(String::new());
    }
    let plain = host
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(host);
    if let Ok(ip) = plain.parse::<std::net::IpAddr>() {
        return Ok(ip.to_string());
    }
    if host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return Ok(host.into());
    }
    Err("Usa solo una IP o nombre DNS, sin protocolo, puerto ni ruta".into())
}
pub fn url_host(host: &str) -> String {
    if host.contains(':') {
        format!("[{host}]")
    } else {
        host.into()
    }
}
