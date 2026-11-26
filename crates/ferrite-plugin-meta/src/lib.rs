//! Bukkit/Paper plugin metadata for Ferrite.
//!
//! This crate scans a `plugins/` directory of jar files, reads the descriptor
//! each jar carries (`paper-plugin.yml` when present, otherwise `plugin.yml`),
//! validates it against Bukkit's rules, and resolves a Paper-compatible load
//! order. It is deliberately JVM-free: nothing here loads plugin classes, it
//! only describes what a future embedded JVM would have to do.
//!
//! ```no_run
//! let plan = ferrite_plugin_meta::scan_directory(std::path::Path::new("plugins"))?;
//! for plugin in plan.loaded() {
//!     println!("{:?} {} ({})", plugin.threading_mode(), plugin.name(), plugin.meta.load);
//! }
//! for failure in &plan.failed {
//!     println!("{}: {}", failure.source.display(), failure.error);
//! }
//! # Ok::<(), std::io::Error>(())
//! ```

mod descriptor;
mod error;
mod jar;
mod model;
mod order;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub use crate::error::{MetadataError, PluginError, ValidationError};
pub use crate::model::{
    CommandDescription, ManifestFormat, PaperDependencies, PaperDependency, PaperDependencyLoad,
    PaperMeta, PermissionDefault, PermissionDescription, PluginLoadOrder, PluginMeta,
    ThreadingMode,
};
pub use crate::order::{FailedPlugin, LoadPlan, LoadedPlugin, PluginEntry, resolve_load_order};

/// Decode and validate a descriptor that was read from a jar.
pub fn parse_plugin_meta(format: ManifestFormat, yaml: &str) -> Result<PluginMeta, MetadataError> {
    descriptor::parse(format, yaml)
}

/// Read and validate the descriptor of a single plugin jar.
///
/// `paper-plugin.yml` wins when a jar contains both, as it does on Paper.
pub fn read_plugin_meta(path: &Path) -> Result<PluginMeta, PluginError> {
    jar::read_plugin_meta(path)
}

/// Scan `directory` for `.jar` files, validate every descriptor, and resolve
/// the load order.
///
/// Jars are processed in case-insensitive file-name order, so duplicate plugin
/// names are resolved deterministically: the first jar wins, later ones are
/// reported as failures. Only a failure to read the directory itself is
/// propagated; per-jar problems are collected in [`LoadPlan::failed`].
pub fn scan_directory(directory: &Path) -> io::Result<LoadPlan> {
    let mut jars: Vec<(String, PathBuf)> = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        let Some(file_name) = path.file_name() else {
            continue;
        };
        let file_name = file_name.to_string_lossy().to_ascii_lowercase();
        if !file_name.ends_with(".jar") {
            continue;
        }
        jars.push((file_name, path));
    }
    jars.sort();

    let mut entries = Vec::new();
    let mut failed = Vec::new();
    for (_, path) in jars {
        match read_plugin_meta(&path) {
            Ok(meta) => entries.push(PluginEntry { source: path, meta }),
            Err(error) => failed.push(FailedPlugin {
                source: path,
                name: None,
                error,
            }),
        }
    }

    let plan = resolve_load_order(entries);
    let mut all_failed = failed;
    all_failed.extend(plan.failed);
    all_failed.sort_by(|a, b| a.source.cmp(&b.source));

    Ok(LoadPlan {
        startup: plan.startup,
        postworld: plan.postworld,
        failed: all_failed,
    })
}
