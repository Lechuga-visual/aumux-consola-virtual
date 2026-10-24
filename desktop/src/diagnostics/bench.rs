use crate::config::Config;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
pub struct Job {
    pub original: Config,
    pub buffers: Vec<u32>,
    pub index: usize,
    pub mode: String,
    pub seconds: u64,
    pub began: Option<Instant>,
}
impl Job {
    pub fn parse(v: &Value, c: &Config) -> Result<Self, String> {
        let a = v["buffers"]
            .as_array()
            .ok_or("Lista de buffers requerida")?;
        if a.is_empty() || a.len() > 10 {
            return Err("Usa entre 1 y 10 buffers".into());
        }
        let mut buffers = Vec::new();
        for n in a {
            let n = n.as_u64().ok_or("Buffer inválido")?;
            if !(32..=8192).contains(&n) {
                return Err("Buffers entre 32 y 8192 muestras".into());
            }
            buffers.push(n as u32);
        }
        let seconds = v["seconds"]
            .as_u64()
            .filter(|s| (1..=600).contains(s))
            .ok_or("Duración entre 1 y 600 segundos")?;
        let mode = v["mode"]
            .as_str()
            .filter(|m| ["asio", "system", "demo"].contains(m))
            .ok_or("Modo de prueba inválido")?
            .to_owned();
        Ok(Self {
            original: c.clone(),
            buffers,
            index: 0,
            mode,
            seconds,
            began: None,
        })
    }
    pub fn status(&self) -> Value {
        json!({"mode":self.mode,"index":self.index,"buffers":self.buffers,"seconds":self.seconds,"elapsed_seconds":self.began.map(|t|t.elapsed().as_secs_f64()),"hardware":self.mode!="demo"})
    }
    pub fn due(&self) -> bool {
        self.began
            .is_some_and(|t| t.elapsed() >= Duration::from_secs(self.seconds))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_workload_before_touching_audio() {
        let c = Config::default();
        assert!(Job::parse(&json!({"buffers":[64,128],"seconds":1,"mode":"demo"}), &c).is_ok());
        for v in [
            json!({"buffers":[],"seconds":1,"mode":"demo"}),
            json!({"buffers":[999999],"seconds":1,"mode":"demo"}),
            json!({"buffers":[64],"seconds":0,"mode":"demo"}),
        ] {
            assert!(Job::parse(&v, &c).is_err());
        }
    }
}
