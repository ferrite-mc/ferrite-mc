//! Reading plugin descriptors out of `.jar` (zip) files.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use zip::ZipArchive;

use crate::descriptor;
use crate::error::PluginError;
use crate::model::{ManifestFormat, PluginMeta};

const MAX_DESCRIPTOR_BYTES: u64 = 1024 * 1024;

/// Read and validate the descriptor of a single plugin jar.
///
/// `paper-plugin.yml` wins when a jar contains both, as it does on Paper.
pub(crate) fn read_plugin_meta(path: &Path) -> Result<PluginMeta, PluginError> {
    let file = File::open(path).map_err(|source| PluginError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    let mut archive = ZipArchive::new(file).map_err(|source| PluginError::Zip {
        path: path.to_path_buf(),
        source,
    })?;

    let format = if archive
        .by_name(ManifestFormat::PaperPluginYml.file_name())
        .is_ok()
    {
        ManifestFormat::PaperPluginYml
    } else if archive
        .by_name(ManifestFormat::PluginYml.file_name())
        .is_ok()
    {
        ManifestFormat::PluginYml
    } else {
        return Err(PluginError::MissingManifest {
            path: path.to_path_buf(),
        });
    };

    let entry = format.file_name();
    let descriptor = archive.by_name(entry).map_err(|source| PluginError::Zip {
        path: path.to_path_buf(),
        source,
    })?;
    if descriptor.size() > MAX_DESCRIPTOR_BYTES
        || descriptor.compressed_size() > MAX_DESCRIPTOR_BYTES
    {
        return Err(PluginError::DescriptorTooLarge {
            path: path.to_path_buf(),
            entry,
            max_bytes: MAX_DESCRIPTOR_BYTES,
        });
    }

    let mut bytes = Vec::new();
    descriptor
        .take(MAX_DESCRIPTOR_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| PluginError::Entry {
            path: path.to_path_buf(),
            entry,
            source,
        })?;
    if bytes.len() as u64 > MAX_DESCRIPTOR_BYTES {
        return Err(PluginError::DescriptorTooLarge {
            path: path.to_path_buf(),
            entry,
            max_bytes: MAX_DESCRIPTOR_BYTES,
        });
    }

    let yaml = String::from_utf8(bytes).map_err(|_| PluginError::NotUtf8 {
        path: path.to_path_buf(),
        entry,
    })?;

    descriptor::parse(format, &yaml).map_err(|source| PluginError::Metadata {
        path: path.to_path_buf(),
        entry,
        source,
    })
}
