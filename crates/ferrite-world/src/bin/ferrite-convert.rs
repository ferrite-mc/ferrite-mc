//! Standalone converter between Vanilla Anvil (`.mca`) worlds and Ferrite `.fworld` (`FWRD` v3) blobs.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

use ferrite_nbt::{Tag, binary};
use ferrite_world::{BlobCompression, FworldColdStore, RegionCompression, RegionFile};
use flate2::read::GzDecoder;

const USAGE: &str = "\
Ferrite World Converter & Maintenance Tool (Anvil .mca <-> .fworld FWRD v3)

USAGE:
  ferrite-convert anvil-to-fworld <WORLD_DIR> <OUT_FWORLD> [--compression <lz4|zlib|gzip|none>]
  ferrite-convert fworld-to-anvil <IN_FWORLD> <WORLD_DIR>  [--compression <zlib|lz4|gzip|none>]
  ferrite-convert snapshot        <IN_FWORLD> <BACKUP_DIR> [--keep <N>]
  ferrite-convert verify          <IN_FWORLD>
  ferrite-convert recompress      <IN_FWORLD> <OUT_FWORLD> [--compression <lz4|zlib|gzip|none>]

NOTES:
  - Automatically handles Overworld (region/), Nether (DIM-1/region/ <-> <stem>.nether.fworld),
    and The End (DIM1/region/ <-> <stem>.end.fworld) when present.
  - Defaults: anvil-to-fworld/recompress use lz4; fworld-to-anvil uses zlib; snapshot keeps 5.";

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

fn snapshot_fworld(in_base: &Path, backup_dir: &Path, keep: usize) -> Result<(), String> {
    let mut snapped = 0usize;
    let stem = in_base
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("world");

    for dim in [DimKind::Overworld, DimKind::Nether, DimKind::End] {
        let in_path = dim.fworld_path(in_base);
        if !in_path.is_file() {
            continue;
        }
        let store = FworldColdStore::read_file(&in_path, BlobCompression::None)
            .map_err(|e| format!("reading {}: {e}", in_path.display()))?;
        let prefix = match dim {
            DimKind::Overworld => stem.to_owned(),
            DimKind::Nether => format!("{stem}.nether"),
            DimKind::End => format!("{stem}.end"),
        };
        let written = store
            .write_snapshot_to_dir(backup_dir, &prefix, keep)
            .map_err(|e| format!("writing snapshot for {}: {e}", in_path.display()))?;
        println!(
            "[{}] snapshot ({} chunks, keep={}) -> {}",
            dim.label(),
            store.chunks.len(),
            keep,
            written.display()
        );
        snapped += 1;
    }

    if snapped == 0 {
        return Err(format!("no .fworld file found at {}", in_base.display()));
    }
    Ok(())
}

fn verify_fworld(in_base: &Path) -> Result<(), String> {
    let mut verified_dims = 0usize;

    for dim in [DimKind::Overworld, DimKind::Nether, DimKind::End] {
        let in_path = dim.fworld_path(in_base);
        if !in_path.is_file() {
            continue;
        }
        let t0 = Instant::now();
        let store = FworldColdStore::read_file(&in_path, BlobCompression::None)
            .map_err(|e| format!("reading {}: {e}", in_path.display()))?;
        let mut total_bytes = 0usize;
        for &(cx, cz) in store.chunks.keys() {
            let raw = store
                .get_raw(cx, cz)
                .map_err(|e| format!("corrupt chunk ({cx}, {cz}) in {}: {e}", in_path.display()))?
                .ok_or_else(|| format!("missing chunk ({cx}, {cz}) in {}", in_path.display()))?;
            total_bytes += raw.len();
        }
        println!(
            "[{}] OK: {} chunks ({:.2} MiB uncompressed) verified in {:.2}s",
            dim.label(),
            store.chunks.len(),
            total_bytes as f64 / (1024.0 * 1024.0),
            t0.elapsed().as_secs_f64()
        );
        verified_dims += 1;
    }

    if verified_dims == 0 {
        return Err(format!("no .fworld file found at {}", in_base.display()));
    }
    Ok(())
}

fn recompress_fworld(
    in_base: &Path,
    out_base: &Path,
    compression: BlobCompression,
) -> Result<(), String> {
    let mut converted_dims = 0usize;

    for dim in [DimKind::Overworld, DimKind::Nether, DimKind::End] {
        let in_path = dim.fworld_path(in_base);
        if !in_path.is_file() {
            continue;
        }
        let t0 = Instant::now();
        let src = FworldColdStore::read_file(&in_path, BlobCompression::None)
            .map_err(|e| format!("reading {}: {e}", in_path.display()))?;
        let mut entries = Vec::with_capacity(src.chunks.len());
        for &(cx, cz) in src.chunks.keys() {
            if let Some(raw) = src
                .get_raw(cx, cz)
                .map_err(|e| format!("decoding chunk ({cx}, {cz}): {e}"))?
            {
                entries.push(((cx, cz), raw));
            }
        }
        let mut dst = FworldColdStore::new(src.spawn, compression);
        dst.insert_raw_batch(&entries)
            .map_err(|e| format!("recompressing {} chunks: {e}", dim.label()))?;
        let out_path = dim.fworld_path(out_base);
        if let Some(parent) = out_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        dst.write_file_atomic(&out_path)
            .map_err(|e| format!("writing {}: {e}", out_path.display()))?;
        let size = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
        println!(
            "[{}] recompressed {} chunks ({compression:?}) -> {} ({:.2} MiB) in {:.2}s",
            dim.label(),
            entries.len(),
            out_path.display(),
            size as f64 / (1024.0 * 1024.0),
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
    let mut keep_arg: Option<usize> = None;
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
            "--keep" => {
                i += 1;
                let raw = args
                    .get(i)
                    .ok_or_else(|| "missing value after --keep".to_owned())?;
                keep_arg = Some(
                    raw.parse::<usize>()
                        .map_err(|_| format!("invalid --keep value '{raw}'"))?,
                );
            }
            v if v.starts_with("--keep=") => {
                let raw = v.trim_start_matches("--keep=");
                keep_arg = Some(
                    raw.parse::<usize>()
                        .map_err(|_| format!("invalid --keep value '{raw}'"))?,
                );
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown flag '{other}'\n\n{USAGE}"));
            }
            pos => positional.push(pos),
        }
        i += 1;
    }

    match mode {
        "verify" => {
            if positional.len() != 1 {
                return Err(format!("expected <IN_FWORLD>\n\n{USAGE}"));
            }
            verify_fworld(Path::new(positional[0]))
        }
        "snapshot" => {
            if positional.len() != 2 {
                return Err(format!("expected <IN_FWORLD> <BACKUP_DIR>\n\n{USAGE}"));
            }
            snapshot_fworld(
                Path::new(positional[0]),
                Path::new(positional[1]),
                keep_arg.unwrap_or(5),
            )
        }
        "recompress" => {
            if positional.len() != 2 {
                return Err(format!("expected <IN_FWORLD> <OUT_FWORLD>\n\n{USAGE}"));
            }
            let comp = compression_arg
                .map(parse_blob_compression)
                .transpose()?
                .unwrap_or(BlobCompression::Lz4);
            recompress_fworld(Path::new(positional[0]), Path::new(positional[1]), comp)
        }
        "anvil-to-fworld" => {
            if positional.len() != 2 {
                return Err(format!("expected <SRC> and <DST>\n\n{USAGE}"));
            }
            let comp = compression_arg
                .map(parse_blob_compression)
                .transpose()?
                .unwrap_or(BlobCompression::Lz4);
            convert_anvil_to_fworld(Path::new(positional[0]), Path::new(positional[1]), comp)
        }
        "fworld-to-anvil" => {
            if positional.len() != 2 {
                return Err(format!("expected <SRC> and <DST>\n\n{USAGE}"));
            }
            let comp = compression_arg
                .map(parse_region_compression)
                .transpose()?
                .unwrap_or(RegionCompression::Zlib);
            convert_fworld_to_anvil(Path::new(positional[0]), Path::new(positional[1]), comp)
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

    #[test]
    fn snapshot_verify_and_recompress_subcommands_work() {
        let dir = std::env::temp_dir().join(format!(
            "ferrite_convert_maint_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let src_fworld = dir.join("world.fworld");
        let recompressed = dir.join("recompressed.fworld");
        let backups = dir.join("backups");

        let mut store = FworldColdStore::new([1, 64, 2], BlobCompression::Lz4);
        store.insert_raw(0, 0, b"chunk-zero-payload").unwrap();
        store.insert_raw(1, -2, b"chunk-one-payload").unwrap();
        store.write_file_atomic(&src_fworld).unwrap();

        // verify passes on valid file
        run(&[
            "verify".to_owned(),
            src_fworld.to_string_lossy().into_owned(),
        ])
        .unwrap();

        // snapshot with rotation (--keep 2) keeps at most 2 snapshots
        for _ in 0..3 {
            run(&[
                "snapshot".to_owned(),
                src_fworld.to_string_lossy().into_owned(),
                backups.to_string_lossy().into_owned(),
                "--keep".to_owned(),
                "2".to_owned(),
            ])
            .unwrap();
        }
        let snap_count = std::fs::read_dir(&backups)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "fworld"))
            .count();
        assert_eq!(snap_count, 2);

        // recompress lz4 -> zlib preserves exact bytes and verifies cleanly
        run(&[
            "recompress".to_owned(),
            src_fworld.to_string_lossy().into_owned(),
            recompressed.to_string_lossy().into_owned(),
            "--compression=zlib".to_owned(),
        ])
        .unwrap();
        run(&[
            "verify".to_owned(),
            recompressed.to_string_lossy().into_owned(),
        ])
        .unwrap();
        let reloaded = FworldColdStore::read_file(&recompressed, BlobCompression::None).unwrap();
        assert_eq!(
            reloaded.get_raw(0, 0).unwrap().as_deref(),
            Some(b"chunk-zero-payload".as_slice())
        );
        assert_eq!(
            reloaded.get_raw(1, -2).unwrap().as_deref(),
            Some(b"chunk-one-payload".as_slice())
        );

        // verify rejects corrupted payload
        let mut bytes = std::fs::read(&recompressed).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        std::fs::write(&recompressed, &bytes).unwrap();
        assert!(
            run(&[
                "verify".to_owned(),
                recompressed.to_string_lossy().into_owned(),
            ])
            .is_err()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
