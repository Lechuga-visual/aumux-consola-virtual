//! User presets are stored outside scenes, with atomic replacement and bounded input.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preset {
    pub plugin_id: String,
    pub name: String,
    pub params: BTreeMap<String, f32>,
}
impl Preset {
    fn valid(&self) -> bool {
        !self.plugin_id.is_empty()
            && self.plugin_id.len() <= 128
            && !self.name.trim().is_empty()
            && self.name.len() <= 160
            && self.params.len() <= 2048
            && self
                .params
                .iter()
                .all(|(k, v)| !k.is_empty() && k.len() <= 128 && v.is_finite())
    }
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
enum Command {
    Save { preset: Preset },
    Delete { plugin_id: String, name: String },
}
fn path() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("MONITOR_MIX_DATA_DIR") {
        return Ok(PathBuf::from(p).join("presets.json"));
    }
    let folder = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")))
    };
    folder
        .map(|p| p.join("MonitorMix/presets.json"))
        .ok_or("No se encontró una carpeta de datos para los presets".into())
}
fn read(p: &Path) -> Result<Vec<Preset>, String> {
    let bytes = match fs::read(p) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(format!("No se pudieron leer los presets: {e}")),
    };
    if bytes.len() > 2097152 {
        return Err("El archivo de presets excede 2 MiB".into());
    }
    let values: Vec<Preset> =
        serde_json::from_slice(&bytes).map_err(|e| format!("Archivo de presets inválido: {e}"))?;
    if values.len() > 256 || values.iter().any(|v| !v.valid()) {
        return Err("Archivo de presets fuera de rango".into());
    }
    Ok(values)
}
fn apply(p: &Path, command: Command) -> Result<Vec<Preset>, String> {
    let mut values = read(p)?;
    match command {
        Command::Save { mut preset } => {
            preset.name = preset.name.trim().to_string();
            if !preset.valid() {
                return Err("Nombre o parámetros del preset inválidos".into());
            }
            values.retain(|v| v.plugin_id != preset.plugin_id || v.name != preset.name);
            values.push(preset);
        }
        Command::Delete { plugin_id, name } => {
            values.retain(|v| v.plugin_id != plugin_id || v.name != name)
        }
    }
    if values.len() > 256 {
        return Err("Máximo 256 presets de usuario".into());
    }
    values.sort_by(|a, b| (&a.plugin_id, &a.name).cmp(&(&b.plugin_id, &b.name)));
    let bytes = serde_json::to_vec_pretty(&values).map_err(|e| e.to_string())?;
    if bytes.len() > 2097152 {
        return Err("Máximo 2 MiB de presets".into());
    }
    fs::create_dir_all(p.parent().ok_or("Ruta de presets inválida")?).map_err(|e| e.to_string())?;
    let temporary = p.with_extension("json.tmp");
    fs::write(&temporary, bytes).map_err(|e| format!("No se pudo guardar el preset: {e}"))?;
    fs::rename(temporary, p)
        .map_err(|e| format!("No se pudo reemplazar el archivo de presets: {e}"))?;
    Ok(values)
}
pub fn load() -> Result<Vec<Preset>, String> {
    read(&path()?)
}
pub fn command(body: &str) -> Result<serde_json::Value, String> {
    let command = serde_json::from_str(body).map_err(|e| format!("Preset inválido: {e}"))?;
    Ok(serde_json::json!(apply(&path()?, command)?))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_persist_replace_delete_and_never_interpret_names_as_paths() {
        let folder =
            std::env::temp_dir().join(format!("monitor-presets-test-{}", std::process::id()));
        let p = folder.join("presets.json");
        let preset = Preset {
            plugin_id: "native/freevox8".into(),
            name: "../../Voz estudio".into(),
            params: BTreeMap::from([("Mix".into(), 0.5)]),
        };
        apply(
            &p,
            Command::Save {
                preset: preset.clone(),
            },
        )
        .unwrap();
        assert_eq!(read(&p).unwrap()[0].params["Mix"], 0.5);
        let mut changed = preset.clone();
        changed.params.insert("Mix".into(), 0.8);
        apply(&p, Command::Save { preset: changed }).unwrap();
        assert_eq!(read(&p).unwrap().len(), 1);
        assert_eq!(read(&p).unwrap()[0].params["Mix"], 0.8);
        apply(
            &p,
            Command::Delete {
                plugin_id: preset.plugin_id,
                name: preset.name,
            },
        )
        .unwrap();
        assert!(read(&p).unwrap().is_empty());
        fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn invalid_preset_does_not_replace_saved_data() {
        let folder =
            std::env::temp_dir().join(format!("monitor-presets-invalid-{}", std::process::id()));
        let p = folder.join("presets.json");
        let good = Preset {
            plugin_id: "test".into(),
            name: "A".into(),
            params: BTreeMap::new(),
        };
        apply(
            &p,
            Command::Save {
                preset: good.clone(),
            },
        )
        .unwrap();
        let mut bad = good;
        bad.params.insert("value".into(), f32::NAN);
        assert!(apply(&p, Command::Save { preset: bad }).is_err());
        assert_eq!(read(&p).unwrap().len(), 1);
        fs::remove_dir_all(folder).unwrap();
    }
}
