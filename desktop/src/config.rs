use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DEFAULT_CHANNELS: usize = 8;
pub const DEFAULT_AUXES: usize = 4;
// Policy limits, not fixed graph sizes. All graph storage follows scene lengths.
pub const MAX_CHANNELS: usize = 64;
pub const MAX_AUXES: usize = 32;
pub const MAX_IO: usize = 64;
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
        self.path == other.path
            && self.id == other.id
            && (self.id.starts_with("native/") || self.params == other.params)
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
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    #[default]
    Mono,
    Stereo,
}
impl Format {
    pub fn width(self) -> usize {
        if self == Self::Stereo { 2 } else { 1 }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Channel {
    #[serde(default)]
    pub eq: crate::eq::Eq,
    pub name: String,
    pub gain_db: f32,
    pub pan: f32,
    pub mute: bool,
    #[serde(default)]
    pub format: Format,
    /// Physical input indices are independent of the strip count/format. None = silence.
    #[serde(default)]
    pub input: Option<usize>,
    #[serde(default)]
    pub input_right: Option<usize>,
    pub sends_db: Vec<f32>,
    pub pre: Vec<bool>,
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
    #[serde(default)]
    pub eq: crate::eq::Eq,
    pub name: String,
    pub gain_db: f32,
    pub mute: bool,
    #[serde(default)]
    pub format: Format,
    /// First physical output, None = no hardware output. Mono reserves one, stereo two.
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
    pub channels: Vec<Channel>,
    /// Stereo master followed by zero or more mono/stereo auxiliaries.
    pub buses: Vec<Bus>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            version: 2,
            device: None,
            inputs: 8,
            obs_bridge: false,
            pc_monitor: 0,
            sample_rate: 48000,
            buffer: 256,
            channels: (0..DEFAULT_CHANNELS)
                .map(|i| Channel {
                    eq: crate::eq::Eq::default(),
                    name: format!("Entrada {}", i + 1),
                    gain_db: 0.0,
                    pan: 0.0,
                    mute: false,
                    format: Format::Mono,
                    input: Some(i),
                    input_right: (i + 1 < 8).then_some(i + 1),
                    sends_db: vec![-90.0; DEFAULT_AUXES],
                    pre: vec![true; DEFAULT_AUXES],
                    fx: None,
                    inserts: Vec::new(),
                    generator: Generator::default(),
                })
                .collect(),
            buses: (0..=DEFAULT_AUXES)
                .map(|i| Bus {
                    eq: crate::eq::Eq::default(),
                    name: if i == 0 {
                        "Master / OBS".into()
                    } else {
                        format!("Monitor {i}")
                    },
                    gain_db: -12.0,
                    mute: false,
                    format: if i == 0 { Format::Stereo } else { Format::Mono },
                    output: None,
                    fx: None,
                    inserts: Vec::new(),
                })
                .collect(),
        }
    }
}
fn range(value: f32, min: f32, max: f32) -> bool {
    value.is_finite() && (min..=max).contains(&value)
}
impl Config {
    /// Import 0.1 scenes without losing their single insert.
    pub fn normalise(&mut self) {
        if self.version == 1 {
            // Version 1 had one physical input per strip and stereo auxiliaries.
            for (i, c) in self.channels.iter_mut().enumerate() {
                c.format = Format::Mono;
                c.input = (i < self.inputs as usize).then_some(i);
                c.input_right = (i + 1 < self.inputs as usize).then_some(i + 1);
            }
            for b in &mut self.buses {
                b.format = Format::Stereo;
            }
            self.version = 2;
        }
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
        if self.version != 2
            || self.channels.is_empty()
            || self.channels.len() > MAX_CHANNELS
            || self.buses.is_empty()
            || self.buses.len() > MAX_AUXES + 1
            || self.pc_monitor >= self.buses.len()
            || self.buses[0].format != Format::Stereo
        {
            return Err(format!(
                "Escena inválida: 1–{MAX_CHANNELS} canales, 0–{MAX_AUXES} auxiliares y master estéreo"
            ));
        }
        if self.inputs as usize > MAX_IO
            || self.sample_rate != 48000
            || !(32..=4096).contains(&self.buffer)
        {
            return Err("Invalid sample rate or buffer (32–4096)".into());
        }
        let mut assigned = [false; MAX_IO];
        for bus in &self.buses {
            if !bus.eq.valid()
                || !range(bus.gain_db, -90.0, 6.0)
                || bus.name.len() > 120
                || bus.effects().count() > INSERTS
            {
                return Err("Invalid bus gain/name".into());
            }
            if let Some(first) = bus.output {
                let width = bus.format.width();
                if first >= MAX_IO
                    || first + width > MAX_IO
                    || assigned[first..first + width].iter().any(|v| *v)
                {
                    return Err(format!(
                        "Salida de '{}': rango fuera de 1–{MAX_IO} o superpuesto con otro bus; mono usa una salida y estéreo dos",
                        bus.name
                    ));
                }
                assigned[first..first + width].fill(true);
            }
        }
        for c in &self.channels {
            if !c.eq.valid()
                || c.sends_db.len() != self.buses.len() - 1
                || c.pre.len() != self.buses.len() - 1
                || [c.input, c.input_right]
                    .iter()
                    .flatten()
                    .any(|i| *i >= self.inputs as usize)
                || !range(c.gain_db, -90.0, 6.0)
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
            if fx.id.starts_with("native/") {
                let info = crate::native_fx::registry()
                    .iter()
                    .find(|p| p.id == fx.id)
                    .ok_or("Efecto nativo desconocido")?;
                for (id, value) in &fx.params {
                    let p = info
                        .params
                        .iter()
                        .find(|p| p.id == id)
                        .ok_or("Parámetro nativo desconocido")?;
                    if !range(*value, p.min, p.max) {
                        return Err(format!("Parámetro nativo fuera de rango: {id}"));
                    }
                }
            }
        }
        Ok(())
    }
    pub fn same_device(&self, other: &Self) -> bool {
        self.device == other.device
            && self.inputs == other.inputs
            && self.sample_rate == other.sample_rate
            && self.buffer == other.buffer
    }
    /// Resize keeps existing VST instances. Editing their chains still requires stopping.
    pub fn same_existing_fx(&self, other: &Self) -> bool {
        self.channels
            .iter()
            .zip(&other.channels)
            .all(|(a, b)| same_chain(a.effects(), b.effects()))
            && self
                .buses
                .iter()
                .zip(&other.buses)
                .all(|(a, b)| same_chain(a.effects(), b.effects()))
    }
    pub fn same_graph(&self, other: &Self) -> bool {
        self.channels.len() == other.channels.len()
            && self.buses.len() == other.buses.len()
            && self.device == other.device
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
            .filter_map(|b| b.output.map(|p| p + b.format.width()))
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
