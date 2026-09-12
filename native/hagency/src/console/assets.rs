//! Startup captures actual original files; HTTP never resolves a filesystem path.
use super::Error;
use cap_fs_ext::DirExt;
use cap_std::{ambient_authority, fs::Dir};
use hagency_files::{Limits, RelativeFile, Snapshot, Workspace};
use hyper::body::Bytes;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    assets: Vec<Entry>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    size: usize,
    sha256: String,
    mime: String,
}
pub(super) struct Asset {
    pub(super) bytes: Bytes,
    pub(super) mime: String,
    _proof: Snapshot,
}
pub(super) struct Assets {
    values: BTreeMap<String, Asset>,
    _manifest: Snapshot,
}

fn root(path: &Path) -> Result<Dir, Error> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|_| Error::Assets)?
            .join(path)
    };
    let mut anchor = PathBuf::new();
    let mut parts = Vec::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir if parts.is_empty() => {
                anchor.push(component.as_os_str())
            }
            Component::Normal(name) => parts.push(name),
            Component::CurDir => {}
            _ => return Err(Error::Assets),
        }
    }
    if !anchor.is_absolute() || parts.is_empty() {
        return Err(Error::Assets);
    }
    let mut dir = Dir::open_ambient_dir(anchor, ambient_authority()).map_err(|_| Error::Assets)?;
    for name in parts {
        dir = dir.open_dir_nofollow(name).map_err(|_| Error::Assets)?;
    }
    // Existing helper validates owner and private permissions from the actual handle.
    hagency_store::private::check_handle(
        &dir.try_clone().map_err(|_| Error::Assets)?.into_std_file(),
    )
    .map_err(|_| Error::Assets)?;
    Ok(dir)
}
fn snapshot(dir: &Dir, path: &str, limit: usize) -> Result<Snapshot, Error> {
    Workspace::from_directory(
        dir.try_clone().map_err(|_| Error::Assets)?,
        Limits::new(limit.max(1), 1).map_err(|_| Error::Assets)?,
    )
    .map_err(|_| Error::Assets)?
    .snapshot(&RelativeFile::new(path).map_err(|_| Error::Assets)?)
    .map_err(|_| Error::Assets)
}
fn mime(path: &str) -> Option<&'static str> {
    if matches!(
        path,
        "usage/index.html" | "resources/index.html" | "resources/new/index.html"
    ) {
        return Some("text/html; charset=utf-8");
    }
    if !path.starts_with("_next/static/")
        || path.len() > 512
        || path
            .split('/')
            .any(|v| v.is_empty() || v == "." || v == "..")
        || !path
            .bytes()
            .all(|v| v.is_ascii_alphanumeric() || b"/_-.".contains(&v))
    {
        return None;
    }
    match path.rsplit('.').next()? {
        "js" => Some("text/javascript; charset=utf-8"),
        "css" => Some("text/css; charset=utf-8"),
        "woff2" => Some("font/woff2"),
        "woff" => Some("font/woff"),
        _ => None,
    }
}
impl Assets {
    pub(super) fn load(path: &Path) -> Result<Self, Error> {
        let dir = root(path)?;
        let manifest = snapshot(&dir, "manifest.json", 128 * 1024)?;
        let input: Manifest =
            serde_json::from_slice(manifest.bytes()).map_err(|_| Error::Assets)?;
        if input.version != 1 || input.assets.is_empty() || input.assets.len() > 512 {
            return Err(Error::Assets);
        }
        let mut total = 0usize;
        let mut values = BTreeMap::new();
        for entry in input.assets {
            let expected = mime(&entry.path).ok_or(Error::Assets)?;
            if entry.mime != expected || entry.size > 4 * 1024 * 1024 || entry.sha256.len() != 64 {
                return Err(Error::Assets);
            }
            total = total
                .checked_add(entry.size)
                .filter(|n| *n <= 32 * 1024 * 1024)
                .ok_or(Error::Assets)?;
            let proof = snapshot(&dir, &entry.path, entry.size)?;
            let digest: String = proof.digest().iter().map(|b| format!("{b:02x}")).collect();
            if proof.len() != entry.size || digest != entry.sha256 {
                return Err(Error::Assets);
            }
            let key = if entry.path == "usage/index.html" {
                "/console/usage/".into()
            } else if entry.path == "resources/new/index.html" {
                "/console/resources/new/".into()
            } else if entry.path == "resources/index.html" {
                "/console/resources/".into()
            } else {
                format!("/console/{}", entry.path)
            };
            let asset = Asset {
                bytes: Bytes::copy_from_slice(proof.bytes()),
                mime: entry.mime,
                _proof: proof,
            };
            if values.insert(key, asset).is_some() {
                return Err(Error::Assets);
            }
        }
        if !values.contains_key("/console/usage/") {
            return Err(Error::Assets);
        }
        Ok(Self {
            values,
            _manifest: manifest,
        })
    }
    pub(super) fn get(&self, path: &str) -> Option<&Asset> {
        self.values.get(if path == "/console/usage" {
            "/console/usage/"
        } else if path == "/console/resources/new" {
            "/console/resources/new/"
        } else if path == "/console/resources" {
            "/console/resources/"
        } else {
            path
        })
    }
}
