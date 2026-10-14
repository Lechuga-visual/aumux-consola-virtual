use soundcraft_vst3_host::Vst3Descriptor;
use std::path::Path;

/// Inspect and register only the requested bundle, not every installed plugin.
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
