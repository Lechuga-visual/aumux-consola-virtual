//! Selective 48 kHz multitrack capture. Only bounded copies/atomics run on the audio thread.
use crate::config::{Config, Format};
use crossbeam_queue::ArrayQueue;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::{BufWriter, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{
            AtomicBool, AtomicU8, AtomicU64,
            Ordering::{Acquire, Relaxed, Release},
        },
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const PACKET_FRAMES: usize = 256;
const POOL: usize = 256;
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Point {
    Pre,
    Post,
}
#[derive(Clone, Deserialize)]
pub struct Selection {
    pub enabled: bool,
    pub point: Point,
}
#[derive(Clone, Serialize)]
pub struct Track {
    pub name: String,
    pub file: String,
    pub kind: String,
    pub index: usize,
    pub channels: usize,
    pub point: Point,
    offset: usize,
}
struct Packet {
    frames: usize,
    samples: Vec<f32>,
}
pub struct Tap {
    pub tracks: Vec<Track>,
    width: usize,
    free: ArrayQueue<Box<Packet>>,
    pending: ArrayQueue<Box<Packet>>,
    accepting: AtomicBool,
    fault: AtomicU8,
    accepted: AtomicU64,
    written: AtomicU64,
    dropped: AtomicU64,
    error: Mutex<String>,
    path: PathBuf,
    layout: Vec<Format>,
    bus_layout: Vec<Format>,
    pre: Vec<bool>,
    post: Vec<bool>,
}
impl Tap {
    fn allocate(tracks: Vec<Track>, path: PathBuf, config: &Config, capacity: usize) -> Arc<Self> {
        let width = tracks.iter().map(|t| t.channels).sum();
        let free = ArrayQueue::new(capacity);
        for _ in 0..capacity {
            let _ = free.push(Box::new(Packet {
                frames: 0,
                samples: vec![0.0; width * PACKET_FRAMES],
            }));
        }
        let pre = (0..config.channels.len())
            .map(|i| {
                tracks
                    .iter()
                    .any(|t| t.kind == "channel" && t.index == i && t.point == Point::Pre)
            })
            .collect();
        let post = (0..config.channels.len())
            .map(|i| {
                tracks
                    .iter()
                    .any(|t| t.kind == "channel" && t.index == i && t.point == Point::Post)
            })
            .collect();
        Arc::new(Self {
            pre,
            post,
            tracks,
            width,
            free,
            pending: ArrayQueue::new(capacity),
            accepting: AtomicBool::new(true),
            fault: AtomicU8::new(0),
            accepted: AtomicU64::new(0),
            written: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            error: Mutex::new(String::new()),
            path,
            layout: config.channels.iter().map(|c| c.format).collect(),
            bus_layout: config.buses.iter().map(|b| b.format).collect(),
        })
    }
    pub fn wants(&self, channel: usize, point: Point) -> bool {
        match point {
            Point::Pre => self.pre.get(channel),
            Point::Post => self.post.get(channel),
        }
        .copied()
        .unwrap_or(false)
    }
    pub fn compatible(&self, config: &Config) -> bool {
        config.sample_rate == 48000
            && self
                .layout
                .iter()
                .copied()
                .eq(config.channels.iter().map(|c| c.format))
            && self
                .bus_layout
                .iter()
                .copied()
                .eq(config.buses.iter().map(|b| b.format))
    }
    pub fn active(&self) -> bool {
        self.accepting.load(Acquire)
    }
    pub fn capture(
        &self,
        pre: &[Vec<Vec<f32>>],
        post: &[Vec<Vec<f32>>],
        buses: &[Vec<Vec<f32>>],
        frames: usize,
        silence: bool,
    ) {
        if !self.active() {
            return;
        }
        for start in (0..frames).step_by(PACKET_FRAMES) {
            let Some(mut packet) = self.free.pop() else {
                self.dropped.fetch_add((frames - start) as u64, Relaxed);
                self.fault.store(1, Release);
                self.accepting.store(false, Release);
                return;
            };
            packet.frames = (frames - start).min(PACKET_FRAMES);
            for track in &self.tracks {
                for frame in 0..packet.frames {
                    for side in 0..track.channels {
                        let sample = if silence {
                            0.0
                        } else if track.kind == "bus" {
                            buses[track.index][side][start + frame]
                        } else if track.point == Point::Pre {
                            pre[track.index][side][start + frame]
                        } else {
                            post[track.index][side][start + frame]
                        };
                        packet.samples[frame * self.width + track.offset + side] =
                            if sample.is_finite() { sample } else { 0.0 };
                    }
                }
            }
            self.accepted.fetch_add(packet.frames as u64, Relaxed);
            if let Err(packet) = self.pending.push(packet) {
                self.dropped.fetch_add(packet.frames as u64, Relaxed);
                self.fault.store(1, Release);
                self.accepting.store(false, Release);
                if let Err(packet) = self.free.push(packet) {
                    std::mem::forget(packet);
                }
                return;
            }
        }
    }
    fn status(&self) -> Value {
        let code = self.fault.load(Acquire);
        let error = match code {1=>"El disco no alcanzó a consumir el audio; se detuvo la grabación para no ocultar pérdidas".into(),_=>self.error.lock().unwrap().clone()};
        json!({"active":self.active(),"session_open":true,"state":if code!=0{"error"}else if self.active(){"recording"}else{"finishing"},"directory":self.path,"sample_rate":48000,"format":"WAV Float32 / RF64","recording_format_version":1,"application_version":env!("CARGO_PKG_VERSION"),"tracks":self.tracks,"accepted_frames":self.accepted.load(Relaxed),"written_frames":self.written.load(Relaxed),"duration_seconds":self.written.load(Relaxed) as f64/48000.0,"dropped_frames":self.dropped.load(Relaxed),"queued_packets":self.pending.len(),"queue_capacity":POOL,"error":error})
    }
    fn fail(&self, message: String) {
        *self.error.lock().unwrap() = message;
        self.fault.store(2, Release);
        self.accepting.store(false, Release);
    }
}
struct Writer {
    file: BufWriter<File>,
    frames: u64,
    channels: usize,
    bytes: Vec<u8>,
}
impl Writer {
    fn new(path: &Path, channels: usize) -> Result<Self, String> {
        let mut file =
            BufWriter::with_capacity(65536, File::create_new(path).map_err(|e| e.to_string())?);
        file.write_all(
            &soundcraft_audio_io::wav_float_header(channels, 48000, 0)
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())?;
        Ok(Self {
            file,
            frames: 0,
            channels,
            bytes: Vec::with_capacity(channels * PACKET_FRAMES * 4),
        })
    }
    fn write(&mut self, packet: &Packet, width: usize, offset: usize) -> Result<(), String> {
        self.bytes.clear();
        for frame in 0..packet.frames {
            for side in 0..self.channels {
                self.bytes.extend_from_slice(
                    &packet.samples[frame * width + offset + side].to_le_bytes(),
                );
            }
        }
        self.file
            .write_all(&self.bytes)
            .map_err(|e| e.to_string())?;
        self.frames += packet.frames as u64;
        Ok(())
    }
    fn checkpoint(&mut self) -> Result<(), String> {
        let header = soundcraft_audio_io::wav_float_header(self.channels, 48000, self.frames)
            .map_err(|e| e.to_string())?;
        self.file
            .seek(SeekFrom::Start(0))
            .and_then(|_| self.file.write_all(&header))
            .and_then(|_| self.file.seek(SeekFrom::End(0)))
            .and_then(|_| self.file.flush())
            .map_err(|e| e.to_string())
    }
}
pub struct Session {
    tap: Arc<Tap>,
    worker: Option<JoinHandle<Value>>,
}
impl Session {
    pub fn start(
        config: &Config,
        directory: Option<&str>,
        selection: &[Selection],
        buses: &[usize],
    ) -> Result<Self, String> {
        if config.sample_rate != 48000 {
            return Err("La grabación requiere 48 kHz".into());
        }
        if selection.len() != config.channels.len() {
            return Err("La selección de grabación no coincide con los canales".into());
        }
        let root = directory
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(default_directory);
        if !root.is_absolute() {
            return Err("Elige una carpeta de grabación con ruta absoluta".into());
        }
        let mut tracks = Vec::new();
        let mut offset = 0;
        for (i, (channel, select)) in config.channels.iter().zip(selection).enumerate() {
            if select.enabled {
                let channels = channel.format.width();
                tracks.push(Track {
                    name: channel.name.clone(),
                    file: format!(
                        "C{:02}_{}_{}.wav",
                        i + 1,
                        safe_name(&channel.name),
                        if select.point == Point::Pre {
                            "PRE"
                        } else {
                            "POST"
                        }
                    ),
                    kind: "channel".into(),
                    index: i,
                    channels,
                    point: select.point,
                    offset,
                });
                offset += channels;
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for &i in buses {
            let Some(bus) = config.buses.get(i) else {
                return Err("Bus de grabación inexistente".into());
            };
            if !seen.insert(i) {
                return Err("Bus de grabación duplicado".into());
            }
            let channels = bus.format.width();
            tracks.push(Track {
                name: bus.name.clone(),
                file: format!("B{:02}_{}_POST.wav", i, safe_name(&bus.name)),
                kind: "bus".into(),
                index: i,
                channels,
                point: Point::Post,
                offset,
            });
            offset += channels;
        }
        if tracks.is_empty() {
            return Err("Selecciona al menos un canal, master o auxiliar para grabar".into());
        }
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis();
        let mut nonce = [0u8; 4];
        getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
        let path = root.join(format!("AuMux-{millis}-{:08x}", u32::from_le_bytes(nonce)));
        fs::create_dir(&path).map_err(|e| e.to_string())?;
        let mut writers = Vec::new();
        for track in &tracks {
            match Writer::new(&path.join(&track.file), track.channels) {
                Ok(writer) => writers.push(writer),
                Err(error) => {
                    let _ = fs::write(
                        path.join("session.json"),
                        json!({"state":"error","error":error,"tracks":tracks}).to_string(),
                    );
                    return Err(error);
                }
            }
        }
        let tap = Tap::allocate(tracks, path, config, POOL);
        fs::write(
            tap.path.join("session.json"),
            serde_json::to_vec_pretty(&tap.status()).unwrap(),
        )
        .map_err(|e| e.to_string())?;
        let capture = tap.clone();
        let worker=thread::Builder::new().name("aumux-record-writer".into()).spawn(move|| {
            let mut checkpoint=Instant::now();let mut last_packet=Instant::now();
            while capture.active() || !capture.pending.is_empty() {
                if let Some(packet)=capture.pending.pop() {
                    last_packet=Instant::now();
                    if capture.fault.load(Acquire)!=2 {
                        for (writer,track) in writers.iter_mut().zip(&capture.tracks) {if let Err(error)=writer.write(&packet,capture.width,track.offset){capture.fail(error);break;}}
                        capture.written.store(writers.iter().map(|w|w.frames).min().unwrap_or(0),Release);
                    }
                    if let Err(packet)=capture.free.push(packet){drop(packet);}
                } else {
                    if capture.active() && last_packet.elapsed()>Duration::from_secs(2){capture.fail("No llegan bloques de audio al grabador desde hace dos segundos".into());}
                    thread::sleep(Duration::from_millis(2));
                }
                if checkpoint.elapsed()>=Duration::from_secs(1){for writer in &mut writers {if let Err(error)=writer.checkpoint(){capture.fail(error);}}checkpoint=Instant::now();}
            }
            for writer in &mut writers {if let Err(error)=writer.checkpoint().and_then(|_|writer.file.get_ref().sync_data().map_err(|e|e.to_string())){capture.fail(error);}}
            let mut report=capture.status();report["session_open"]=json!(false);report["state"]=json!(if capture.fault.load(Acquire)==0{"completed"}else{"error"});
            report["track_frames"]=json!(writers.iter().map(|w|w.frames).collect::<Vec<_>>());
            report["channel_post_note"]=json!("POST: EQ, compresor, FX, fader y mute; antes del paneo. Buses: salida postfader procesada.");
            if let Err(error)=fs::write(capture.path.join("session.json"),serde_json::to_vec_pretty(&report).unwrap()){report["state"]=json!("error");report["error"]=json!(error.to_string());}
            report
        }).map_err(|e|e.to_string())?;
        Ok(Self {
            tap,
            worker: Some(worker),
        })
    }
    pub fn tap(&self) -> Arc<Tap> {
        self.tap.clone()
    }
    pub fn status(&self) -> Value {
        self.tap.status()
    }
    pub fn finish(mut self) -> Value {
        self.tap.accepting.store(false, Release);
        match self.worker.take().unwrap().join() {
            Ok(report) => report,
            Err(_) => {
                json!({"active":false,"session_open":false,"state":"error","error":"El escritor de grabación terminó inesperadamente","directory":self.tap.path})
            }
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.tap.accepting.store(false, Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
pub fn default_directory() -> PathBuf {
    if let Some(root) = std::env::var_os("MONITOR_MIX_DATA_DIR") {
        return PathBuf::from(root).join("Recordings");
    }
    if let Some(root) = std::env::var_os("USERPROFILE") {
        return PathBuf::from(root).join("Music/AuMux");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("AuMux-Recordings")
}
fn safe_name(name: &str) -> String {
    let name: String = name
        .chars()
        .take(64)
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() {
        "Audio".into()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::Fx,
        mixer::{Controls, Mixer},
        test_support::no_heap,
    };
    fn root() -> PathBuf {
        let mut r = [0; 8];
        getrandom::fill(&mut r).unwrap();
        std::env::temp_dir().join(format!("aumux-record-test-{:x}", u64::from_le_bytes(r)))
    }
    #[test]
    fn selected_pre_post_and_master_are_frame_aligned_without_callback_heap_or_disk_io() {
        let root = root();
        let mut config = Config::default();
        config.channels.truncate(3);
        config.inputs = 4;
        config.channels[0].input = Some(0);
        config.channels[0].mute = true;
        config.channels[0].gain_db = -24.0;
        config.channels[0].compressor.bypass = false;
        config.channels[0].compressor.threshold = -30.0;
        config.channels[0].inserts = (0..8)
            .map(|i| Fx {
                id: "native/mda/overdrive".into(),
                instance: format!("record-fx-{i}"),
                ..Fx::default()
            })
            .collect();
        config.channels[1].format = Format::Stereo;
        config.channels[1].input = Some(1);
        config.channels[1].input_right = Some(2);
        config.channels[1].gain_db = -6.0206;
        config.channels[2].mute = true;
        config.buses[0].gain_db = -6.0206;
        let selection = vec![
            Selection {
                enabled: true,
                point: Point::Pre,
            },
            Selection {
                enabled: true,
                point: Point::Post,
            },
            Selection {
                enabled: false,
                point: Point::Pre,
            },
        ];
        let session = Session::start(&config, root.to_str(), &selection, &[0]).unwrap();
        let path = session.tap.path.clone();
        let controls = Controls::new(&config);
        let mut mixer = Mixer::new(&config, controls.clone()).unwrap();
        mixer.attach_recording(Some(session.tap()));
        let input: Vec<f32> = (0..64).flat_map(|_| [0.4, 0.2, -0.6, 0.0]).collect();
        let mut output = vec![0.0; 64 * 64];
        no_heap(|| {
            for _ in 0..20 {
                assert!(mixer.process(&input, &mut output, 64, 64));
            }
        });
        controls.emergency.store(true, Release);
        no_heap(|| {
            assert!(mixer.process(&input, &mut output, 64, 64));
        });
        let report = session.finish();
        assert_eq!(report["state"], "completed");
        assert_eq!(report["written_frames"], 21 * 64);
        assert_eq!(report["tracks"].as_array().unwrap().len(), 3);
        for (i, track) in report["tracks"].as_array().unwrap().iter().enumerate() {
            let bytes = fs::read(path.join(track["file"].as_str().unwrap())).unwrap();
            let (info, audio) = soundcraft_audio_io::decode(&bytes, Some("wav")).unwrap();
            assert_eq!(info.sample_rate, 48000);
            assert_eq!(audio.frames(), 21 * 64);
            let expected = match i {
                0 => vec![0.4],
                1 => vec![0.1, -0.3],
                _ => vec![0.05, -0.15],
            };
            assert_eq!(audio.num_channels(), expected.len());
            for (side, value) in expected.iter().enumerate() {
                assert!(
                    (audio.channels[side][512] - value).abs() < 1e-4,
                    "track {i} side {side}"
                );
                assert!(audio.channels[side][20 * 64..].iter().all(|v| *v == 0.0));
            }
        }
        assert!(
            fs::read_to_string(path.join("session.json"))
                .unwrap()
                .contains("completed")
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn overflow_fails_recording_without_callback_allocation_or_silent_sample_loss() {
        let config = Config::default();
        let track = Track {
            name: "Input".into(),
            file: "input.wav".into(),
            kind: "channel".into(),
            index: 0,
            channels: 1,
            point: Point::Pre,
            offset: 0,
        };
        let tap = Tap::allocate(vec![track], PathBuf::new(), &config, 1);
        let pre = vec![vec![vec![0.5; 512]; 2]];
        no_heap(|| tap.capture(&pre, &[], &[], 512, false));
        assert!(!tap.active());
        assert_eq!(tap.fault.load(Acquire), 1);
        assert_eq!(tap.dropped.load(Relaxed), 256);
    }
    #[test]
    fn float_header_upgrades_to_rf64_and_selected_names_cannot_escape_the_session() {
        assert_eq!(
            &soundcraft_audio_io::wav_float_header(2, 48000, 600_000_000).unwrap()[..4],
            b"RF64"
        );
        assert!(!safe_name("../CON:\\x / 👻").contains('/'));
        let config = Config::default();
        let selection = vec![
            Selection {
                enabled: false,
                point: Point::Pre
            };
            config.channels.len()
        ];
        let root = root();
        assert!(Session::start(&config, root.to_str(), &selection, &[]).is_err());
        assert!(!root.exists());
    }
}
