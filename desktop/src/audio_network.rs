//! AUM1 network audio. Audio only copies into bounded queues; DNS/socket work is off-thread.
use crossbeam_queue::ArrayQueue;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket},
    sync::{
        Arc, Mutex,
        atomic::{
            AtomicBool, AtomicU64,
            Ordering::{Acquire, Relaxed, Release},
        },
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const MAX_FLOWS: usize = 8;
pub const FRAMES: usize = 128;
pub const HEADER: usize = 80;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Route {
    pub enabled: bool,
    pub bus: usize,
    pub stream: String,
    pub bind: String,
    pub port: u16,
    pub destinations: Vec<String>,
}
impl Default for Route {
    fn default() -> Self {
        Self {
            enabled: false,
            bus: 0,
            stream: "AUMUX-MAIN".into(),
            bind: "0.0.0.0".into(),
            port: 17843,
            destinations: vec!["127.0.0.1".into()],
        }
    }
}
pub fn validate(routes: &[Route], buses: usize) -> Result<(), String> {
    if routes.len() > MAX_FLOWS {
        return Err("Máximo ocho flujos de audio de red".into());
    }
    let mut names = std::collections::HashSet::new();
    for r in routes {
        if r.stream.is_empty()
            || r.stream.len() > 31
            || !r
                .stream
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || r.bind.parse::<Ipv4Addr>().is_err()
            || r.port < 1024
            || r.destinations.is_empty()
            || r.destinations.len() > 8
            || r.destinations.iter().any(|d| {
                d.len() > 253
                    || d.is_empty()
                    || !d
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            })
            || (r.enabled && (r.bus >= buses || !names.insert(&r.stream)))
        {
            return Err("Flujo inválido: nombre único ASCII de 1–31 caracteres, bus existente, IPv4 local, puerto 1024–65535 y 1–8 destinos IP/MagicDNS".into());
        }
    }
    Ok(())
}
struct Packet {
    generation: u64,
    sequence: u32,
    sample: u64,
    frames: usize,
    audio: [[f32; 2]; FRAMES],
}
struct Ready {
    session: u64,
    last_send: Option<Instant>,
    generation: u64,
    socket: UdpSocket,
    targets: Vec<SocketAddr>,
    name: [u8; 32],
}
struct Slot {
    enabled: AtomicBool,
    publication: AtomicU64,
    generation: AtomicU64,
    sequence: AtomicU64,
    samples: AtomicU64,
    sent: AtomicU64,
    dropped: AtomicU64,
    config: Mutex<Option<Route>>,
    ready: Mutex<Option<Ready>>,
    error: Mutex<String>,
    queue: ArrayQueue<Packet>,
    notify: std::sync::mpsc::SyncSender<()>,
}
impl Slot {
    fn new() -> Arc<Self> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let slot = Arc::new(Self {
            enabled: AtomicBool::new(false),
            publication: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            sequence: AtomicU64::new(0),
            samples: AtomicU64::new(0),
            sent: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            config: Mutex::new(None),
            ready: Mutex::new(None),
            error: Mutex::new(String::new()),
            queue: ArrayQueue::new(16),
            notify: tx,
        });
        let weak = Arc::downgrade(&slot);
        std::thread::spawn(move || {
            while rx.recv().is_ok() {
                let Some(slot) = weak.upgrade() else {
                    break;
                };
                if slot.enabled.load(Acquire) {
                    continue;
                }
                let generation = slot.generation.load(Acquire);
                let next = slot.config.lock().unwrap().clone();
                let Some(route) = next.filter(|r| r.enabled) else {
                    continue;
                };
                let result = (|| -> Result<Ready, String> {
                    let socket =
                        UdpSocket::bind((route.bind.as_str(), 0)).map_err(|e| e.to_string())?;
                    socket.set_nonblocking(true).map_err(|e| e.to_string())?;
                    socket.set_broadcast(true).map_err(|e| e.to_string())?;
                    let mut targets = Vec::new();
                    for host in &route.destinations {
                        let target = (host.as_str(), route.port)
                            .to_socket_addrs()
                            .map_err(|e| e.to_string())?
                            .find(SocketAddr::is_ipv4)
                            .ok_or("Destino sin dirección IPv4")?;
                        if !targets.contains(&target) {
                            targets.push(target);
                        }
                    }
                    let mut name = [0; 32];
                    name[..route.stream.len()].copy_from_slice(route.stream.as_bytes());
                    Ok(Ready {
                        session: 0,
                        last_send: None,
                        generation,
                        socket,
                        targets,
                        name,
                    })
                })();
                let mut ready = slot.ready.lock().unwrap();
                if slot.generation.load(Acquire) != generation {
                    continue;
                }
                match result {
                    Ok(r) => {
                        *ready = Some(r);
                        *slot.error.lock().unwrap() = String::new();
                        slot.enabled.store(true, Release);
                        slot.publication
                            .store((generation << 6) | route.bus as u64, Release);
                    }
                    Err(e) => {
                        *slot.error.lock().unwrap() = e;
                    }
                }
            }
        });
        slot
    }
}
pub struct Hub {
    slots: [Arc<Slot>; MAX_FLOWS],
    epoch: u64,
}
impl Hub {
    pub fn new() -> Arc<Self> {
        let hub = Arc::new(Self {
            epoch: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64
                ^ u64::from(std::process::id()),
            slots: std::array::from_fn(|_| Slot::new()),
        });
        let weak = Arc::downgrade(&hub);
        std::thread::spawn(move || {
            let mut bytes = [0_u8; HEADER + FRAMES * 8];
            loop {
                let Some(hub) = weak.upgrade() else {
                    break;
                };
                for slot in &hub.slots {
                    let mut ready = slot.ready.lock().unwrap();
                    if let Some(r) = ready.as_mut() {
                        for _ in 0..16 {
                            let Some(p) = slot.queue.pop() else {
                                break;
                            };
                            if p.generation != r.generation || !slot.enabled.load(Acquire) {
                                continue;
                            }
                            if r.last_send
                                .is_some_and(|t| t.elapsed() > Duration::from_millis(20))
                            {
                                r.session = r.session.wrapping_add(1);
                            }
                            r.last_send = Some(Instant::now());
                            bytes.fill(0);
                            bytes[..4].copy_from_slice(b"AUM1");
                            bytes[4] = 1;
                            bytes[5] = 2;
                            bytes[6..8].copy_from_slice(&(HEADER as u16).to_le_bytes());
                            bytes[8..16].copy_from_slice(
                                &(hub.epoch ^ (p.generation << 32) ^ r.session).to_le_bytes(),
                            );
                            bytes[16..20].copy_from_slice(&p.sequence.to_le_bytes());
                            bytes[20..24].copy_from_slice(&48000_u32.to_le_bytes());
                            bytes[24..26].copy_from_slice(&(p.frames as u16).to_le_bytes());
                            bytes[32..40].copy_from_slice(&p.sample.to_le_bytes());
                            bytes[40..72].copy_from_slice(&r.name);
                            bytes[72..80].copy_from_slice(&hub.epoch.to_le_bytes());
                            for f in 0..p.frames {
                                for c in 0..2 {
                                    let start = HEADER + f * 8 + c * 4;
                                    bytes[start..start + 4]
                                        .copy_from_slice(&p.audio[f][c].to_le_bytes());
                                }
                            }
                            for dest in &r.targets {
                                if slot.generation.load(Acquire) != p.generation {
                                    break;
                                }
                                match r.socket.send_to(&bytes[..HEADER + p.frames * 8], dest) {
                                    Ok(_) => {
                                        slot.sent.fetch_add(1, Relaxed);
                                    }
                                    Err(_) => {
                                        slot.dropped.fetch_add(1, Relaxed);
                                    }
                                }
                            }
                        }
                    }
                }
                drop(hub);
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        hub
    }
    pub fn configure(&self, routes: &[Route]) {
        for (i, slot) in self.slots.iter().enumerate() {
            let next = routes.get(i).cloned();
            let mut current = slot.config.lock().unwrap();
            if *current == next {
                if !slot.enabled.load(Acquire) && next.as_ref().is_some_and(|r| r.enabled) {
                    let _ = slot.notify.try_send(());
                }
                continue;
            }
            {
                // Publish generation and bus as one atomic snapshot. The resolver
                // holds the same control-side lock when committing a ready socket.
                let mut ready = slot.ready.lock().unwrap();
                slot.publication.store(0, Release);
                slot.enabled.store(false, Release);
                slot.generation
                    .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                *ready = None;
            }
            *current = next.clone();
            while slot.queue.pop().is_some() {}
            *slot.error.lock().unwrap() = String::new();
            let Some(_route) = next.filter(|r| r.enabled) else {
                continue;
            };

            *slot.error.lock().unwrap() = "Preparando destino…".into();
            let _ = slot.notify.try_send(());
        }
    }
    /// The mixer calls this at block boundaries. No socket, DNS, mutex, allocation or destruction here.
    pub fn publish(&self, buses: &[Vec<Vec<f32>>], frames: usize) {
        for slot in &self.slots {
            let publication = slot.publication.load(Acquire);
            if publication == 0 {
                continue;
            }
            let generation = publication >> 6;
            let Some(bus) = buses.get((publication & 63) as usize) else {
                continue;
            };
            for start in (0..frames).step_by(FRAMES) {
                let n = (frames - start).min(FRAMES);
                let mut p = Packet {
                    generation,
                    frames: n,
                    sequence: slot.sequence.fetch_add(1, Relaxed) as u32,
                    sample: slot.samples.fetch_add(n as u64, Relaxed),
                    audio: [[0.0; 2]; FRAMES],
                };
                for f in 0..n {
                    p.audio[f] = [bus[0][start + f], bus[1][start + f]];
                }
                if slot.queue.force_push(p).is_some() {
                    slot.dropped.fetch_add(1, Relaxed);
                }
            }
        }
    }
    pub fn status(&self) -> Value {
        json!(self.slots.iter().filter_map(|s| {
            let config = s.config.lock().unwrap().clone()?;
            Some(json!({"stream":config.stream,"enabled":s.enabled.load(Acquire),"sent":s.sent.load(Relaxed),
                "dropped":s.dropped.load(Relaxed),"queued":s.queue.len(),"error":s.error.lock().unwrap().clone()}))
        }).collect::<Vec<_>>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn named_flow_is_bounded_and_routes_correct_bus_without_callback_heap() {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let hub = Hub::new();
        let routes = vec![Route {
            enabled: true,
            bus: 1,
            stream: "IEM-VOX".into(),
            port: socket.local_addr().unwrap().port(),
            ..Default::default()
        }];
        validate(&routes, 2).unwrap();
        hub.configure(&routes);
        for _ in 0..100 {
            if hub.slots[0].enabled.load(Acquire) {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(hub.slots[0].enabled.load(Acquire));
        let buses = vec![vec![vec![0.1; 256]; 2], vec![vec![0.25; 256]; 2]];
        crate::test_support::no_heap(|| hub.publish(&buses, 256));
        let mut bytes = [0; 1500];
        for offset in [0_u64, 128] {
            let (n, _) = socket.recv_from(&mut bytes).unwrap();
            assert_eq!(n, HEADER + 128 * 8);
            assert!(n < 1200);
            assert_eq!(&bytes[..4], b"AUM1");
            assert_eq!(&bytes[40..47], b"IEM-VOX");
            assert_eq!(
                u64::from_le_bytes(bytes[32..40].try_into().unwrap()),
                offset
            );
            assert_eq!(f32::from_le_bytes(bytes[80..84].try_into().unwrap()), 0.25);
        }
        // Ordinary fader updates must not restart/resynchronize an unchanged flow.
        let generation = hub.slots[0].generation.load(Acquire);
        hub.configure(&routes);
        assert_eq!(hub.slots[0].generation.load(Acquire), generation);
        hub.configure(&[]);
        assert!(!hub.slots[0].enabled.load(Acquire));
    }
    #[test]
    fn live_route_changes_never_relabel_audio_from_another_bus() {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let hub = Hub::new();
        let stop = Arc::new(AtomicBool::new(false));
        let producer = {
            let hub = hub.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                let buses = vec![vec![vec![0.125; 128]; 2], vec![vec![0.5; 128]; 2]];
                while !stop.load(Acquire) {
                    hub.publish(&buses, 128);
                    std::thread::sleep(Duration::from_micros(100));
                }
            })
        };
        for i in 0..40 {
            hub.configure(&[Route {
                enabled: true,
                bus: i % 2,
                stream: if i % 2 == 0 { "FIRST" } else { "SECOND" }.into(),
                port: socket.local_addr().unwrap().port(),
                ..Default::default()
            }]);
            std::thread::sleep(Duration::from_millis(2));
        }
        stop.store(true, Release);
        producer.join().unwrap();
        let mut bytes = [0; 1500];
        let mut count = 0;
        while let Ok((_, _)) = socket.recv_from(&mut bytes) {
            let value = f32::from_le_bytes(bytes[80..84].try_into().unwrap());
            assert_eq!(
                value,
                if &bytes[40..45] == b"FIRST" {
                    0.125
                } else {
                    0.5
                }
            );
            count += 1;
        }
        assert!(
            count > 5,
            "must inspect real packets during the reconfiguration race"
        );
    }
    #[test]
    fn rejects_ambiguous_flows_bad_bind_and_injection() {
        let mut r = Route {
            enabled: true,
            ..Default::default()
        };
        assert!(validate(&[r.clone(), r.clone()], 1).is_err());
        r.destinations = vec!["127.0.0.1:42".into()];
        assert!(validate(&[r.clone()], 1).is_err());
        r.destinations = vec!["studio.tailnet.ts.net".into()];
        assert!(validate(&[r.clone()], 1).is_ok());
        r.stream = "../unsafe".into();
        assert!(validate(&[r], 1).is_err());
    }
}
