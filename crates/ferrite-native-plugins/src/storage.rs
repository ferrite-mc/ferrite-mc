//! Atomic JSON persistence helpers shared across engines.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::{PluginError, Result};

/// Serializes `value` as pretty JSON and writes it to `path` atomically via a sibling temp file.
pub(crate) fn save_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| PluginError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let json = serde_json::to_string_pretty(value).map_err(|source| PluginError::Json {
        path: path.to_path_buf(),
        source,
    })?;

    let tmp_path = temp_sibling_path(path);
    let write_res = (|| -> io::Result<()> {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(json.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        Ok(())
    })();

    if let Err(source) = write_res {
        let _ = fs::remove_file(&tmp_path);
        return Err(PluginError::Io {
            path: tmp_path,
            source,
        });
    }

    if let Err(source) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(PluginError::Io {
            path: path.to_path_buf(),
            source,
        });
    }

    Ok(())
}

/// Reads and deserializes JSON from `path`.
pub(crate) fn load_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = fs::read_to_string(path).map_err(|source| PluginError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&text).map_err(|source| PluginError::Json {
        path: path.to_path_buf(),
        source,
    })
}

/// Reads and deserializes JSON from `path`, returning `T::default()` if the file does not exist.
pub(crate) fn load_json_or_default<T: DeserializeOwned + Default>(path: &Path) -> Result<T> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|source| PluginError::Json {
            path: path.to_path_buf(),
            source,
        }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(T::default()),
        Err(source) => Err(PluginError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn temp_sibling_path(path: &Path) -> PathBuf {
    let suffix = format!(".{}.tmp", std::process::id());
    match path.file_name() {
        Some(name) => {
            let mut os = name.to_os_string();
            os.push(suffix);
            path.with_file_name(os)
        }
        None => path.with_extension("tmp"),
    }
}
