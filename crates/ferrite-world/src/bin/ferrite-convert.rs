//! Standalone converter between Vanilla Anvil (`.mca`) worlds and Ferrite `.fworld` (`FWRD` v3) blobs.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

use ferrite_nbt::{Tag, binary};
use ferrite_world::{BlobCompression, FworldColdStore, RegionCompression, RegionFile};
use flate2::read::GzDecoder;

const USAGE: &str = "\
Ferrite World Converter (Anvil .mca <-> .fworld FWRD v3)

USAGE:
  ferrite-convert anvil-to-fworld <WORLD_DIR> <OUT_FWORLD> [--compression <lz4|zlib|gzip|none>]
  ferrite-convert fworld-to-anvil <IN_FWORLD> <WORLD_DIR>  [--compression <zlib|lz4|gzip|none>]

NOTES:
  - Automatically converts Overworld (region/), Nether (DIM-1/region/ <-> <stem>.nether.fworld),
    and The End (DIM1/region/ <-> <stem>.end.fworld) when present.
  - Defaults: anvil-to-fworld uses lz4; fworld-to-anvil uses zlib (vanilla compatible).";

#[derive(Clone, Copy)]
enum DimKind {
    Overworld,
    Nether,
    End,
}

impl DimKind {
    fn region_subdir(self) -> &'static str {
        match self {
            Self::Overworld => "region",
            Self::Nether => "DIM-1/region",
            Self::End => "DIM1/region",
        }
    }

    fn region_subdir_26_3(self) -> &'static str {
        match self {
            Self::Overworld => "dimensions/minecraft/overworld/region",
            Self::Nether => "dimensions/minecraft/the_nether/region",
            Self::End => "dimensions/minecraft/the_end/region",
        }
    }

    fn find_region_dir(self, world_dir: &Path) -> Option<PathBuf> {
        let modern = world_dir.join(self.region_subdir_26_3());
        if modern.is_dir() {
            return Some(modern);
        }
        let legacy = world_dir.join(self.region_subdir());
        legacy.is_dir().then_some(legacy)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Overworld => "overworld",
            Self::Nether => "the_nether",
            Self::End => "the_end",
        }
    }

    fn fworld_path(self, base: &Path) -> PathBuf {
        let suffix = match self {
            Self::Overworld => return base.to_path_buf(),
            Self::Nether => "nether",
            Self::End => "end",
        };
        let raw = base.to_string_lossy();
        if let Some(stem) = raw.strip_suffix(".fworld") {
            PathBuf::from(format!("{stem}.{suffix}.fworld"))
        } else {
            PathBuf::from(format!("{raw}.{suffix}.fworld"))
        }
    }
}

fn parse_blob_compression(s: &str) -> Result<BlobCompression, String> {
    match s.to_ascii_lowercase().as_str() {
        "lz4" => Ok(BlobCompression::Lz4),
        "zlib" => Ok(BlobCompression::Zlib),
        "gzip" => Ok(BlobCompression::Gzip),
        "none" => Ok(BlobCompression::None),
        _ => Err(format!(
            "invalid compression '{s}' (expected lz4|zlib|gzip|none)"
        )),
    }
}

fn parse_region_compression(s: &str) -> Result<RegionCompression, String> {
    match s.to_ascii_lowercase().as_str() {
        "zlib" => Ok(RegionCompression::Zlib),
        "lz4" => Ok(RegionCompression::Lz4),
        "gzip" => Ok(RegionCompression::Gzip),
        "none" => Ok(RegionCompression::None),
        _ => Err(format!(
            "invalid compression '{s}' (expected zlib|lz4|gzip|none)"
        )),
    }
}

fn parse_mca_coords(name: &str) -> Option<(i32, i32)> {
    let rest = name.strip_prefix("r.")?.strip_suffix(".mca")?;
    let (rx, rz) = rest.split_once('.')?;
    Some((rx.parse().ok()?, rz.parse().ok()?))
}

fn read_level_spawn(world_dir: &Path) -> [i32; 3] {
    let Ok(bytes) = std::fs::read(world_dir.join("level.dat")) else {
        return [0, 64, 0];
    };
    let mut decoder = GzDecoder::new(bytes.as_slice());
    let mut raw = Vec::new();
    let Ok(_) = decoder.read_to_end(&mut raw) else {
        return [0, 64, 0];
    };
    let Ok((_, Tag::Compound(root))) = binary::read_named(&raw) else {
        return [0, 64, 0];
    };
    let Some(Tag::Compound(data)) = root.get("Data") else {
        return [0, 64, 0];
    };
    if let (Some(Tag::Int(x)), Some(Tag::Int(y)), Some(Tag::Int(z))) =
        (data.get("SpawnX"), data.get("SpawnY"), data.get("SpawnZ"))
    {
        return [*x, *y, *z];
    }
    if let Some(Tag::Compound(spawn)) = data.get("spawn")
        && let Some(Tag::IntArray(pos)) = spawn.get("pos")
        && pos.len() == 3
    {
        return [pos[0], pos[1], pos[2]];
    }
    [0, 64, 0]
}

fn convert_anvil_to_fworld(
    world_dir: &Path,
    out_base: &Path,
    compression: BlobCompression,
) -> Result<(), String> {
    let spawn = read_level_spawn(world_dir);
    let mut converted_dims = 0usize;

    for dim in [DimKind::Overworld, DimKind::Nether, DimKind::End] {
        let Some(reg_dir) = dim.find_region_dir(world_dir) else {
            continue;
        };
        let t0 = Instant::now();
        let mut entries: Vec<((i32, i32), Vec<u8>)> = Vec::new();
        let dir_entries = std::fs::read_dir(&reg_dir)
            .map_err(|e| format!("reading {}: {e}", reg_dir.display()))?;
        for entry in dir_entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some((rx, rz)) = parse_mca_coords(name) else {
                continue;
            };
            let region = RegionFile::open(&path, rx, rz)
                .map_err(|e| format!("opening {}: {e}", path.display()))?;
            let coords = region
                .present_chunks()
                .map_err(|e| format!("scanning {}: {e}", path.display()))?;
            for (cx, cz) in coords {
                if let Some(raw) = region.read_chunk(cx, cz).map_err(|e| {
                    format!("reading chunk ({cx}, {cz}) from {}: {e}", path.display())
                })? {
                    entries.push(((cx, cz), raw));
                }
            }
        }
        if entries.is_empty() && !matches!(dim, DimKind::Overworld) {
            continue;
        }
        let mut store = FworldColdStore::new(spawn, compression);
        store
            .insert_raw_batch(&entries)
            .map_err(|e| format!("compressing {} chunks: {e}", dim.label()))?;
        let out_path = dim.fworld_path(out_base);
        if let Some(parent) = out_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        store
            .write_file_atomic(&out_path)
            .map_err(|e| format!("writing {}: {e}", out_path.display()))?;
        let size = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
        println!(
            "[{}] {} chunks -> {} ({:.2} MiB) in {:.2}s",
            dim.label(),
            entries.len(),
            out_path.display(),
            size as f64 / (1024.0 * 1024.0),
            t0.elapsed().as_secs_f64()
        );
        converted_dims += 1;
    }

    if converted_dims == 0 {
        return Err(format!(
            "no Anvil region directories found under {}",
            world_dir.display()
        ));
    }
    Ok(())
}

fn convert_fworld_to_anvil(
    in_base: &Path,
    world_dir: &Path,
    compression: RegionCompression,
) -> Result<(), String> {
    let mut converted_dims = 0usize;

    for dim in [DimKind::Overworld, DimKind::Nether, DimKind::End] {
        let in_path = dim.fworld_path(in_base);
        if !in_path.is_file() {
            continue;
        }
        let t0 = Instant::now();
        let store = FworldColdStore::read_file(&in_path, BlobCompression::None)
            .map_err(|e| format!("reading {}: {e}", in_path.display()))?;
        let reg_dir = world_dir.join(dim.region_subdir());
        std::fs::create_dir_all(&reg_dir)
            .map_err(|e| format!("creating {}: {e}", reg_dir.display()))?;

        let mut by_region: HashMap<(i32, i32), Vec<(i32, i32)>> = HashMap::new();
        for &(cx, cz) in store.chunks.keys() {
            by_region
                .entry((cx.div_euclid(32), cz.div_euclid(32)))
                .or_default()
                .push((cx, cz));
        }

        let mut total_chunks = 0usize;
        for ((rx, rz), coords) in by_region {
            let mca_path = reg_dir.join(format!("r.{rx}.{rz}.mca"));
            let region = RegionFile::open(&mca_path, rx, rz)
                .map_err(|e| format!("opening {}: {e}", mca_path.display()))?;
            for (cx, cz) in coords {
                if let Some(raw) = store
                    .get_raw(cx, cz)
                    .map_err(|e| format!("decoding chunk ({cx}, {cz}): {e}"))?
                {
                    region.write_chunk(cx, cz, &raw, compression).map_err(|e| {
                        format!("writing chunk ({cx}, {cz}) to {}: {e}", mca_path.display())
                    })?;
                    total_chunks += 1;
                }
            }
        }
        println!(
            "[{}] {} chunks -> {} in {:.2}s",
            dim.label(),
            total_chunks,
            reg_dir.display(),
            t0.elapsed().as_secs_f64()
        );
        converted_dims += 1;
    }

    if converted_dims == 0 {
        return Err(format!("no .fworld file found at {}", in_base.display()));
    }
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("ferrite-convert {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let mode = args[0].as_str();
    let mut positional: Vec<&str> = Vec::new();
    let mut compression_arg: Option<&str> = None;
    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--compression" => {
                i += 1;
                compression_arg = Some(
                    args.get(i)
                        .ok_or_else(|| "missing value after --compression".to_owned())?
                        .as_str(),
                );
            }
            v if v.starts_with("--compression=") => {
                compression_arg = Some(v.trim_start_matches("--compression="));
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown flag '{other}'\n\n{USAGE}"));
            }
            pos => positional.push(pos),
        }
        i += 1;
    }

    if positional.len() != 2 {
        return Err(format!("expected <SRC> and <DST>\n\n{USAGE}"));
    }
    let src = Path::new(positional[0]);
    let dst = Path::new(positional[1]);

    match mode {
        "anvil-to-fworld" => {
            let comp = compression_arg
                .map(parse_blob_compression)
                .transpose()?
                .unwrap_or(BlobCompression::Lz4);
            convert_anvil_to_fworld(src, dst, comp)
        }
        "fworld-to-anvil" => {
            let comp = compression_arg
                .map(parse_region_compression)
                .transpose()?
                .unwrap_or(RegionCompression::Zlib);
            convert_fworld_to_anvil(src, dst, comp)
        }
        other => Err(format!("unknown subcommand '{other}'\n\n{USAGE}")),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(err) = run(&args) {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mca_filenames_and_dimension_fworld_paths() {
        assert_eq!(parse_mca_coords("r.0.0.mca"), Some((0, 0)));
        assert_eq!(parse_mca_coords("r.-3.12.mca"), Some((-3, 12)));
        assert_eq!(parse_mca_coords("c.0.0.mcc"), None);

        let base = Path::new("/tmp/world.fworld");
        assert_eq!(DimKind::Overworld.fworld_path(base), base);
        assert_eq!(
            DimKind::Nether.fworld_path(base),
            PathBuf::from("/tmp/world.nether.fworld")
        );
        assert_eq!(
            DimKind::End.fworld_path(base),
            PathBuf::from("/tmp/world.end.fworld")
        );
    }

    #[test]
    fn roundtrips_anvil_and_fworld_across_dimensions() {
        let dir = std::env::temp_dir().join(format!(
            "ferrite_convert_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let src_world = dir.join("src");
        let dst_world = dir.join("dst");
        let fworld_out = dir.join("converted.fworld");

        let ow_reg = src_world.join("region");
        let nether_reg = src_world.join("DIM-1/region");
        std::fs::create_dir_all(&ow_reg).unwrap();
        std::fs::create_dir_all(&nether_reg).unwrap();

        let ow_bytes = b"sample-overworld-chunk-nbt".to_vec();
        let nether_bytes = b"sample-nether-chunk-nbt".to_vec();

        RegionFile::open(ow_reg.join("r.0.0.mca"), 0, 0)
            .unwrap()
            .write_chunk(3, 5, &ow_bytes, RegionCompression::Zlib)
            .unwrap();
        RegionFile::open(nether_reg.join("r.-1.0.mca"), -1, 0)
            .unwrap()
            .write_chunk(-2, 4, &nether_bytes, RegionCompression::Zlib)
            .unwrap();

        convert_anvil_to_fworld(&src_world, &fworld_out, BlobCompression::Lz4).unwrap();
        assert!(fworld_out.is_file());
        assert!(dir.join("converted.nether.fworld").is_file());

        convert_fworld_to_anvil(&fworld_out, &dst_world, RegionCompression::Zlib).unwrap();
        let got_ow = RegionFile::open(dst_world.join("region/r.0.0.mca"), 0, 0)
            .unwrap()
            .read_chunk(3, 5)
            .unwrap()
            .unwrap();
        let got_nether = RegionFile::open(dst_world.join("DIM-1/region/r.-1.0.mca"), -1, 0)
            .unwrap()
            .read_chunk(-2, 4)
            .unwrap()
            .unwrap();
        assert_eq!(got_ow, ow_bytes);
        assert_eq!(got_nether, nether_bytes);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
