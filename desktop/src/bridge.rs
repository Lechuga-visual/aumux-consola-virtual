use crossbeam_queue::ArrayQueue;
use std::{
    net::UdpSocket,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const FRAMES: usize = 256;
struct Packet {
    frames: usize,
    rate: u32,
    audio: [[f32; 2]; FRAMES],
}
pub struct Bridge {
    queue: ArrayQueue<Packet>,
    pub enabled: AtomicBool,
    pub dropped: AtomicU64,
    sent: AtomicU64,
    last_ack: AtomicU64,
}
impl Bridge {
    pub fn start() -> Result<Arc<Self>, String> {
        let socket = UdpSocket::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        socket.set_nonblocking(true).map_err(|e| e.to_string())?;
        let bridge = Arc::new(Self {
            queue: ArrayQueue::new(8),
            enabled: AtomicBool::new(false),
            dropped: AtomicU64::new(0),
            sent: AtomicU64::new(0),
            last_ack: AtomicU64::new(0),
        });
        let weak = Arc::downgrade(&bridge);
        std::thread::spawn(move || {
            let mut sequence = 0u32;
            let mut bytes = [0u8; 16 + FRAMES * 8];
            loop {
                let Some(b) = weak.upgrade() else {
                    break;
                };
                while let Some(p) = b.queue.pop() {
                    if !b.enabled.load(Relaxed) {
                        continue;
                    }
                    bytes[..4].copy_from_slice(b"MMX1");
                    bytes[4..8].copy_from_slice(&sequence.to_le_bytes());
                    sequence = sequence.wrapping_add(1);
                    bytes[8..12].copy_from_slice(&p.rate.to_le_bytes());
                    bytes[12..16].copy_from_slice(&(p.frames as u32).to_le_bytes());
                    for f in 0..p.frames {
                        for c in 0..2 {
                            bytes[16 + f * 8 + c * 4..20 + f * 8 + c * 4]
                                .copy_from_slice(&p.audio[f][c].to_le_bytes());
                        }
                    }
                    if socket
                        .send_to(&bytes[..16 + p.frames * 8], "127.0.0.1:17840")
                        .is_err()
                    {
                        b.dropped.fetch_add(1, Relaxed);
                    } else {
                        b.sent.fetch_add(1, Relaxed);
                    }
                }
                let mut ack = [0u8; 16];
                while let Ok((size, from)) = socket.recv_from(&mut ack) {
                    if from == "127.0.0.1:17840".parse::<std::net::SocketAddr>().unwrap()
                        && size == 8
                        && &ack[..4] == b"MMXA"
                    {
                        b.last_ack.store(now_ms(), Relaxed);
                    }
                }
                drop(b);
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        Ok(bridge)
    }
    pub fn status(&self) -> serde_json::Value {
        let ack = self.last_ack.load(Relaxed);
        serde_json::json!({"sent": self.sent.load(Relaxed),
            "receiver_active": self.enabled.load(Relaxed) && ack != 0 && now_ms().saturating_sub(ack) < 1500,
            "ack_age_ms": if ack == 0 { None } else { Some(now_ms().saturating_sub(ack)) }})
    }
    /// Bounded copying only; no socket operations on the audio callback.
    pub fn push(&self, planar: &[Vec<f32>], frames: usize, rate: u32) {
        if !self.enabled.load(Relaxed) {
            return;
        }
        for start in (0..frames).step_by(FRAMES) {
            let n = (frames - start).min(FRAMES);
            let mut p = Packet {
                frames: n,
                rate,
                audio: [[0.0; 2]; FRAMES],
            };
            for f in 0..n {
                p.audio[f] = [planar[0][start + f], planar[1][start + f]];
            }
            if self.queue.force_push(p).is_some() {
                self.dropped.fetch_add(1, Relaxed);
            }
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
