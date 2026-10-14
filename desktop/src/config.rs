use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CHANNELS: usize = 8;
pub const AUXES: usize = 4;
pub const BUSES: usize = AUXES + 1;
pub const MAX_FRAMES: usize = 8192;
pub const INSERTS: usize = 4;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct Fx {
    pub path: String,
    pub id: String,
    #[serde(default)]
    pub params: BTreeMap<String, f32>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub bypass: bool,
}
impl Fx {
    fn same_graph(&self, other: &Self) -> bool {
        self.path == other.path && self.id == other.id && self.params == other.params
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
    #[default]
    Off,
    White,
    Pink,
    Sine,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Generator {
    pub signal: Signal,
    pub level_db: f32,
    pub frequency: f32,
}
impl Default for Generator {
    fn default() -> Self {
        Self {
            signal: Signal::Off,
            level_db: -30.0,
            frequency: 1000.0,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Channel {
    pub name: String,
    pub gain_db: f32,
    pub pan: f32,
    pub mute: bool,
    pub sends_db: [f32; AUXES],
    pub pre: [bool; AUXES],
    pub fx: Option<Fx>,
    #[serde(default)]
    pub inserts: Vec<Fx>,
    #[serde(default)]
    pub generator: Generator,
}
impl Channel {
    pub fn effects(&self) -> impl Iterator<Item = &Fx> {
        self.fx.iter().chain(self.inserts.iter())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bus {
    pub name: String,
    pub gain_db: f32,
    pub mute: bool,
    /// Zero-based first physical output, None = no hardware output.
    pub output: Option<usize>,
    pub fx: Option<Fx>,
    #[serde(default)]
    pub inserts: Vec<Fx>,
}
impl Bus {
    pub fn effects(&self) -> impl Iterator<Item = &Fx> {
        self.fx.iter().chain(self.inserts.iter())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub version: u32,
    pub device: Option<u32>,
    pub inputs: u32,
    pub obs_bridge: bool,
    #[serde(default)]
    pub pc_monitor: usize,
    pub sample_rate: u32,
    pub buffer: u32,
    pub channels: [Channel; CHANNELS],
    /// Master followed by the four auxiliaries.
    pub buses: [Bus; BUSES],
}
impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            device: None,
            inputs: 8,
            obs_bridge: false,
            pc_monitor: 0,
            sample_rate: 48000,
            buffer: 256,
            channels: std::array::from_fn(|i| Channel {
                name: format!("Entrada {}", i + 1),
                gain_db: 0.0,
                pan: 0.0,
                mute: false,
                sends_db: [-90.0; AUXES],
                pre: [true; AUXES],
                fx: None,
                inserts: Vec::new(),
                generator: Generator::default(),
            }),
            buses: std::array::from_fn(|i| Bus {
                name: if i == 0 {
                    "Master / OBS".into()
                } else {
                    format!("Monitor {i}")
                },
                gain_db: -12.0,
                mute: false,
                output: None,
                fx: None,
                inserts: Vec::new(),
            }),
        }
    }
}
fn range(value: f32, min: f32, max: f32) -> bool {
    value.is_finite() && (min..=max).contains(&value)
}
impl Config {
    /// Import 0.1 scenes without losing their single insert.
    pub fn normalise(&mut self) {
        for c in &mut self.channels {
            if let Some(fx) = c.fx.take() {
                c.inserts.insert(0, fx);
            }
        }
        for b in &mut self.buses {
            if let Some(fx) = b.fx.take() {
                b.inserts.insert(0, fx);
            }
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 || self.pc_monitor >= BUSES {
            return Err("Unsupported scene version".into());
        }
        if !(1..=8).contains(&self.inputs)
            || ![44100, 48000, 88200, 96000].contains(&self.sample_rate)
            || !(32..=4096).contains(&self.buffer)
        {
            return Err("Invalid sample rate or buffer (32–4096)".into());
        }
        let mut assigned = [false; 64];
        for bus in &self.buses {
            if !range(bus.gain_db, -90.0, 6.0)
                || bus.name.len() > 120
                || bus.effects().count() > INSERTS
            {
                return Err("Invalid bus gain/name".into());
            }
            if let Some(first) = bus.output {
                if first >= 63 || assigned[first] || assigned[first + 1] {
                    return Err("Output pairs must be in 1–64 and must not overlap".into());
                }
                assigned[first] = true;
                assigned[first + 1] = true;
            }
        }
        for c in &self.channels {
            if !range(c.gain_db, -90.0, 6.0)
                || !range(c.pan, -1.0, 1.0)
                || c.sends_db.iter().any(|v| !range(*v, -90.0, 6.0))
                || c.name.len() > 120
                || c.effects().count() > INSERTS
                || !range(c.generator.level_db, -90.0, -6.0)
                || !range(c.generator.frequency, 20.0, 20000.0)
            {
                return Err("Invalid channel controls".into());
            }
        }
        for fx in self
            .channels
            .iter()
            .flat_map(|c| c.effects())
            .chain(self.buses.iter().flat_map(|b| b.effects()))
        {
            if fx.path.len() > 4096
                || fx.id.len() > 128
                || fx.params.len() > 2048
                || fx.name.len() > 240
                || fx
                    .params
                    .iter()
                    .any(|(k, v)| k.len() > 128 || !v.is_finite())
            {
                return Err("Invalid effect configuration".into());
            }
        }
        Ok(())
    }
    pub fn same_graph(&self, other: &Self) -> bool {
        self.device == other.device
            && self.inputs == other.inputs
            && self.sample_rate == other.sample_rate
            && self.buffer == other.buffer
            && self
                .channels
                .iter()
                .zip(&other.channels)
                .all(|(a, b)| same_chain(a.effects(), b.effects()))
            && self
                .buses
                .iter()
                .zip(&other.buses)
                .all(|(a, b)| same_chain(a.effects(), b.effects()) && a.output == b.output)
    }
    pub fn output_channels(&self) -> usize {
        self.buses
            .iter()
            .filter_map(|b| b.output.map(|p| p + 2))
            .max()
            .unwrap_or(2)
    }
}
fn same_chain<'a>(a: impl Iterator<Item = &'a Fx>, b: impl Iterator<Item = &'a Fx>) -> bool {
    let mut a = a;
    let mut b = b;
    loop {
        match (a.next(), b.next()) {
            (None, None) => return true,
            (Some(a), Some(b)) if a.same_graph(b) => {}
            _ => return false,
        }
    }
}
