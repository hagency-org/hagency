use super::{PathFlavor, keys_within, present, text};
use crate::canonical;
use serde_json::{Map, Value, json};

pub(super) fn derive(value: &Value, flavor: PathFlavor) -> Option<(Value, String)> {
    if !keys_within(value, &["network", "fileSystem"]) {
        return None;
    }
    let mut result = Map::new();
    let mut descriptions = Vec::new();
    if let Some(network) = present(value, "network") {
        if !keys_within(network, &["enabled"]) {
            return None;
        }
        let enabled = network.get("enabled")?.as_bool()?;
        result.insert("network".into(), json!({"enabled":enabled}));
        if enabled {
            descriptions.push("Network access: all destinations (not limited to a domain)".into());
        }
    }
    if let Some(fs) = present(value, "fileSystem") {
        if !keys_within(fs, &["read", "write", "entries", "globScanMaxDepth"])
            || present(fs, "globScanMaxDepth").is_some()
        {
            return None;
        }
        let mut normalized = Map::new();
        for mode in ["read", "write"] {
            if let Some(paths) = present(fs, mode) {
                let paths = paths.as_array()?;
                if paths.len() > 64 {
                    return None;
                }
                let mut paths = paths
                    .iter()
                    .map(|v| flavor.normalize(text(v)?))
                    .collect::<Option<Vec<_>>>()?;
                paths.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
                paths.dedup();
                for path in &paths {
                    descriptions.push(format!("{mode}: {path}"));
                }
                normalized.insert(mode.into(), json!(paths));
            }
        }
        if let Some(entries) = present(fs, "entries") {
            let entries = entries.as_array()?;
            if entries.len() > 64 {
                return None;
            }
            let mut normalized_entries = Vec::new();
            for entry in entries {
                if !keys_within(entry, &["access", "path"]) {
                    return None;
                }
                let access = entry.get("access")?.as_str()?;
                let path = entry.get("path")?;
                if !["read", "write"].contains(&access)
                    || !keys_within(path, &["type", "path"])
                    || path.get("type")? != "path"
                {
                    return None;
                }
                let path = flavor.normalize(text(path.get("path")?)?)?;
                descriptions.push(format!("{access}: {path}"));
                let entry = json!({"access":access,"path":{"type":"path","path":path}});
                let key = canonical::encode(&entry).ok()?;
                normalized_entries.push((key, entry));
            }
            normalized_entries.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
            normalized.insert(
                "entries".into(),
                Value::Array(normalized_entries.into_iter().map(|(_, v)| v).collect()),
            );
        }
        result.insert("fileSystem".into(), Value::Object(normalized));
    }
    if descriptions.is_empty() {
        return None;
    }
    Some((Value::Object(result), descriptions.join("\n")))
}
