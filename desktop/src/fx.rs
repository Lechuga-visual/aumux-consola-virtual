use soundcraft_vst3_host::Vst3Descriptor;
use std::path::Path;

/// Load only the selected bundle. Discovery never executes installed plugin code.
pub fn load(path: &Path) -> Result<Vec<Vst3Descriptor>, String> {
    if !soundcraft_vst3_host::scan::is_vst3_path(path) {
        return Err("Select a .vst3 file or bundle folder".into());
    }
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let descriptors = soundcraft_vst3_host::load_bundle(&path).map_err(|e| e.to_string())?;
    if let Some(known) = soundcraft_vst3_host::cached() {
        for d in &descriptors {
            if known.iter().any(|k| k.id == d.id && k.path != path) {
                return Err("This VST3 class was already loaded from another path. Restart the app to switch versions.".into());
            }
        }
    }
    soundcraft_vst3_host::add_search_dir(&path);
    Ok(descriptors)
}

/// Enumerate installed bundles without loading DLLs or instantiating plugins.
pub fn installed(extra: Option<&str>) -> serde_json::Value {
    let mut roots = soundcraft_vst3_host::scan::default_search_paths();
    if let Some(extra) = extra.filter(|p| !p.trim().is_empty()) {
        roots.push(extra.into());
    }
    let plugins: Vec<_> = soundcraft_vst3_host::scan::find_bundles(&roots)
        .into_iter()
        .filter(|p| soundcraft_vst3_host::scan::bundle_binary(p).is_some())
        .map(|p| {
            serde_json::json!({
                "name": p.file_stem().unwrap_or_default().to_string_lossy(),
                "path": p.to_string_lossy(),
            })
        })
        .collect();
    serde_json::json!({"roots": roots, "plugins": plugins})
}
