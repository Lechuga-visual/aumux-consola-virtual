use soundcraft_vst3_host::Vst3Descriptor;
use std::{io::Read, path::Path};

// Optional VST3 sidecar metadata: enumeration must never load a plugin DLL.
fn module_classes(value: &serde_json::Value) -> Vec<serde_json::Value> {
    let vendor = value["Factory Info"]["Vendor"].as_str().unwrap_or("");
    value["Classes"].as_array().into_iter().flatten().take(512)
        .filter_map(|class| {
            if class["Category"].as_str()? != "Audio Module Class" { return None; }
            let cid = class["CID"].as_str()?;
            if cid.len() != 32 || !cid.bytes().all(|b| b.is_ascii_hexdigit()) { return None; }
            let subs: Vec<String> = match &class["Sub Categories"] {
                serde_json::Value::String(s) => s.split('|').map(str::trim).filter(|s| !s.is_empty()).take(32).map(|s|s.chars().take(128).collect()).collect(),
                serde_json::Value::Array(a) => a.iter().take(32).filter_map(|s|s.as_str()).map(|s|s.chars().take(128).collect()).collect(),
                _ => Vec::new(),
            };
            // Same convention as the VST3 host: Fx takes precedence over Instrument.
            if subs.iter().any(|s|s == "Instrument") && !subs.iter().any(|s|s == "Fx") { return None; }
            Some(serde_json::json!({
                "id":format!("vst3:{}",cid.to_ascii_uppercase()),
                "name":class["Name"].as_str()?.chars().take(256).collect::<String>(),
                "vendor":class["Vendor"].as_str().unwrap_or(vendor).chars().take(256).collect::<String>(),
                "sub_categories":subs,
            }))
        }).collect()
}

fn bundle_classes(path: &Path) -> Vec<serde_json::Value> {
    for relative in [
        "Contents/Resources/moduleinfo.json",
        "Contents/moduleinfo.json",
    ] {
        let Ok(file) = std::fs::File::open(path.join(relative)) else {
            continue;
        };
        let mut bytes = Vec::new();
        if file.take(1_048_577).read_to_end(&mut bytes).is_err() || bytes.len() > 1_048_576 {
            continue;
        }
        if let Ok(value) = serde_json::from_slice(&bytes) {
            return module_classes(&value);
        }
    }
    Vec::new()
}

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
                "classes": bundle_classes(&p),
            })
        })
        .collect();
    serde_json::json!({"roots": roots, "plugins": plugins})
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn sidecar_metadata_excludes_controllers_instruments_and_invalid_ids() {
        let cid = "abcdef0123456789abcdef0123456789";
        let classes = module_classes(
            &json!({"Factory Info":{"Vendor":"Fixture maker"},"Classes":[
                {"Category":"Audio Module Class","CID":cid,"Name":"Compressor","Sub Categories":"Fx|Dynamics|Compressor"},
                {"Category":"Audio Module Class","CID":cid,"Name":"Synth","Sub Categories":["Instrument","Synth"]},
                {"Category":"Component Controller Class","CID":cid,"Name":"Controller"},
                {"Category":"Audio Module Class","CID":"invalid","Name":"Bad"}
            ]}),
        );
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0]["vendor"], "Fixture maker");
        assert_eq!(classes[0]["id"], format!("vst3:{}", cid.to_uppercase()));
        assert_eq!(
            classes[0]["sub_categories"],
            json!(["Fx", "Dynamics", "Compressor"])
        );
        assert!(module_classes(&json!({"Classes":null})).is_empty());
    }
}
