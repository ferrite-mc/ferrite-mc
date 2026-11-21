use ferrite_light::{
    LightBlockView, LightEngine, LightJob, LightKind, LightPool, LightPoolConfig,
    chunk_neighbourhood,
};
use ferrite_nbt::{
    Compound, List, Tag,
    binary::{self, Compression},
};
use std::{collections::HashMap, fs, path::Path, sync::Arc};

/// The compared chunks are the force-loaded inner 16x16 block of the fixture.
/// Light spreads at most 15 blocks, and the light engine only reads the 3x3
/// chunk neighbourhood, but requiring a larger full + lit halo keeps the
/// compared chunks away from the fixture's generated edge: vanilla may store
/// light for a chunk before a later neighbour feature (for example a spruce
/// overhanging from the next chunk) has placed its blocks.
const COMPARE_HALO: i32 = 3;
const COMPARE_MIN: i32 = 0;
const COMPARE_MAX: i32 = 15;
/// The buildable section range of the overworld fixture.
const MIN_SECTION: i32 = -4;
const MAX_SECTION: i32 = 19;
/// Every block in the buildable range, top to bottom.
const COMPARE_Y: usize = (MAX_SECTION - MIN_SECTION + 1) as usize * 16;

#[derive(Default)]
struct Chunk {
    sections: HashMap<i32, Section>,
}
struct Section {
    states: Vec<u16>,
    sky: Option<Vec<i8>>,
    block: Option<Vec<i8>>,
}
struct Fixture {
    chunks: HashMap<(i32, i32), Arc<Chunk>>,
}
impl LightBlockView for Fixture {
    fn state(&self, x: i32, y: i32, z: i32) -> u16 {
        self.chunks
            .get(&(x.div_euclid(16), z.div_euclid(16)))
            .and_then(|c| c.sections.get(&y.div_euclid(16)))
            .map_or(0, |s| {
                s.states[(y.rem_euclid(16) as usize) * 256
                    + (z.rem_euclid(16) as usize) * 16
                    + x.rem_euclid(16) as usize]
            })
    }
    fn is_chunk_loaded(&self, x: i32, z: i32) -> bool {
        self.chunks.contains_key(&(x, z))
    }
}
fn compound(tag: &Tag) -> &Compound {
    if let Tag::Compound(v) = tag {
        v
    } else {
        panic!("expected compound")
    }
}
fn read_root(bytes: &[u8], offset: usize) -> Tag {
    let pos = offset * 4096;
    let len = u32::from_be_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
    let compression = match bytes[pos + 4] {
        1 => Compression::Gzip,
        2 => Compression::Zlib,
        3 => Compression::None,
        n => panic!("unsupported compression {n}"),
    };
    binary::read_compressed(&bytes[pos + 5..pos + 4 + len], compression)
        .unwrap()
        .1
}
/// A chunk is ready once generation finished (`minecraft:full`) and vanilla
/// computed and stored its light (`isLightOn`).
fn is_ready(root: &Compound) -> bool {
    matches!(root.get("isLightOn"), Some(Tag::Byte(1)))
        && matches!(root.get("Status"), Some(Tag::String(s)) if s == "minecraft:full")
}
fn parse_chunk(root: &Compound) -> Chunk {
    let Some(Tag::List(List::Compound(sections))) = root.get("sections") else {
        panic!("missing sections")
    };
    let mut out = Chunk::default();
    for section in sections {
        let Some(Tag::Byte(y)) = section.get("Y") else {
            continue;
        };
        let mut states = vec![0u16; 4096];
        if let Some(Tag::Compound(blocks)) = section.get("block_states") {
            let palette: Vec<u16> = match blocks.get("palette") {
                Some(Tag::List(List::String(names))) => names
                    .iter()
                    .map(|name| {
                        ferrite_data::blocks::default_state(name)
                            .unwrap_or_else(|| panic!("unknown {name}"))
                    })
                    .collect(),
                Some(Tag::List(List::Compound(entries))) => entries
                    .iter()
                    .map(|entry| {
                        let Some(Tag::String(name)) = entry.get("id").or_else(|| entry.get(""))
                        else {
                            panic!("missing block name {entry:?}")
                        };
                        let props: Vec<_> = match entry.get("properties") {
                            Some(Tag::Compound(p)) => p
                                .iter()
                                .map(|(k, v)| match v {
                                    Tag::String(v) => (k.as_str(), v.as_str()),
                                    _ => panic!("invalid property"),
                                })
                                .collect(),
                            _ => Vec::new(),
                        };
                        ferrite_data::blocks::state_id(name, &props)
                            .unwrap_or_else(|| panic!("unknown {name} {props:?}"))
                    })
                    .collect(),
                _ => panic!("missing palette"),
            };
            if palette.len() == 1 {
                states.fill(palette[0]);
            } else {
                let Some(Tag::LongArray(data)) = blocks.get("data") else {
                    panic!("missing packed states")
                };
                let bits = (usize::BITS - (palette.len() - 1).leading_zeros()).max(4) as usize;
                let per = 64 / bits;
                let mask = (1u64 << bits) - 1;
                for (i, state) in states.iter_mut().enumerate() {
                    *state =
                        palette[((data[i / per] as u64 >> ((i % per) * bits)) & mask) as usize];
                }
            }
        }
        let byte_array = |name| match section.get(name) {
            Some(Tag::ByteArray(v)) => Some(v.clone()),
            _ => None,
        };
        out.sections.insert(
            i32::from(*y),
            Section {
                states,
                sky: byte_array("SkyLight"),
                block: byte_array("BlockLight"),
            },
        );
    }
    out
}
fn region_coords(path: &Path) -> Option<(i32, i32)> {
    let stem = path.file_stem()?.to_str()?;
    let (x, z) = stem.strip_prefix("r.")?.split_once('.')?;
    Some((x.parse().ok()?, z.parse().ok()?))
}

fn chunk_offset(bytes: &[u8], x: usize, z: usize) -> usize {
    let i = (x + z * 32) * 4;
    ((bytes[i] as usize) << 16) | ((bytes[i + 1] as usize) << 8) | bytes[i + 2] as usize
}

#[test]
fn compare_fixture_light_when_available() {
    let region_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vanilla/26.3/fixture-world/world/dimensions/minecraft/overworld/region");
    let fixture = region_dir.join("r.0.0.mca");
    if !fixture.exists() {
        eprintln!(
            "SKIP vanilla light differential: {} absent",
            fixture.display()
        );
        return;
    }

    let mut regions = HashMap::new();
    for entry in fs::read_dir(&region_dir).expect("read fixture region directory") {
        let path = entry.expect("read fixture region entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("mca") {
            continue;
        }
        let Some((rx, rz)) = region_coords(&path) else {
            continue;
        };
        let bytes = fs::read(&path).expect("read fixture region");
        assert!(
            bytes.len() >= 8192,
            "truncated region header: {}",
            path.display()
        );
        regions.insert((rx, rz), bytes);
    }
    assert!(!regions.is_empty(), "fixture has no region files");

    // Parse every ready chunk once: each one is reused as the neighbour of up
    // to nine comparison centers.
    let mut chunks: HashMap<(i32, i32), Arc<Chunk>> = HashMap::new();
    for (&(rx, rz), bytes) in &regions {
        for z in 0..32 {
            for x in 0..32 {
                let offset = chunk_offset(bytes, x, z);
                if offset == 0 {
                    continue;
                }
                let root = read_root(bytes, offset);
                let root = compound(&root);
                if is_ready(root) {
                    let pos = (rx * 32 + x as i32, rz * 32 + z as i32);
                    chunks.insert(pos, Arc::new(parse_chunk(root)));
                }
            }
        }
    }

    let halo = COMPARE_HALO;
    let candidates: Vec<_> = (COMPARE_MIN..=COMPARE_MAX)
        .flat_map(|z| (COMPARE_MIN..=COMPARE_MAX).map(move |x| (x, z)))
        .filter(|&(x, z)| {
            (z - halo..=z + halo)
                .all(|nz| (x - halo..=x + halo).all(|nx| chunks.contains_key(&(nx, nz))))
        })
        .collect();
    eprintln!(
        "vanilla light differential: {} comparable chunk(s) with a {}x{} full + lit halo, {} full lit chunks in {} region file(s)",
        candidates.len(),
        halo * 2 + 1,
        halo * 2 + 1,
        chunks.len(),
        regions.len()
    );
    assert_eq!(
        candidates.len(),
        ((COMPARE_MAX - COMPARE_MIN + 1) * (COMPARE_MAX - COMPARE_MIN + 1)) as usize,
        "fixture margin is incomplete; rerun tools/make_fixture_world.py"
    );
    assert!(
        candidates.contains(&(0, 7)),
        "spawn chunk (0, 7) must stay in the comparable set"
    );

    // The pool reads only each job's 3x3 chunk neighbourhood and every
    // comparable centre has a full 7x7 halo, so one world holding the loaded
    // fixture serves the whole batch. Each worker count runs once over all
    // centres, then every snapshot is compared against the serial engine.
    let pool_world = Fixture {
        chunks: chunks.clone(),
    };
    let jobs: Vec<_> = candidates
        .iter()
        .map(|&(x, z)| LightJob::new(x, z))
        .collect();
    for job in &jobs {
        for (x, z) in chunk_neighbourhood(job.center_x, job.center_z) {
            assert!(
                chunks.contains_key(&(x, z)),
                "centre {:?} needs loaded chunk {x},{z}",
                job.center()
            );
        }
    }
    let mut pooled = Vec::new();
    for workers in [1usize, 2, 4, 8] {
        let pool = LightPool::new(LightPoolConfig::new(
            workers,
            MIN_SECTION,
            MAX_SECTION,
            true,
        ));
        let snapshots = pool
            .relight(&pool_world, &jobs)
            .unwrap_or_else(|error| panic!("{workers}-worker pool failed: {error}"));
        assert_eq!(snapshots.len(), jobs.len());
        for (job, snapshot) in jobs.iter().zip(&snapshots) {
            assert_eq!(snapshot.center(), job.center());
            assert_eq!(
                pool.snapshot(job.center_x, job.center_z).as_deref(),
                Some(snapshot.as_ref()),
                "published {workers}-worker snapshot for {:?}",
                job.center()
            );
        }
        eprintln!(
            "pool differential: {workers} worker(s) produced {} snapshots",
            snapshots.len()
        );
        pooled.push((workers, snapshots));
    }

    let mut total_mismatch = [0usize; 2];
    let mut total_compared = [0usize; 2];
    let mut pool_compared = 0usize;
    for (center_index, &(cx, cz)) in candidates.iter().enumerate() {
        let mut neighborhood = HashMap::new();
        for z in cz - halo..=cz + halo {
            for x in cx - halo..=cx + halo {
                let chunk = chunks
                    .get(&(x, z))
                    .unwrap_or_else(|| panic!("chunk {x},{z} missing for center {cx},{cz}"));
                neighborhood.insert((x, z), Arc::clone(chunk));
            }
        }
        let world = Fixture {
            chunks: neighborhood,
        };
        let mut engine = LightEngine::new(cx, cz, MIN_SECTION, MAX_SECTION, true);
        engine.relight(&world);
        let mut mismatch = [0usize; 2];
        let mut compared = [0usize; 2];
        for (&sy, section) in &world.chunks[&(cx, cz)].sections {
            for (kind, array, slot) in [
                (LightKind::Block, &section.block, 0),
                (LightKind::Sky, &section.sky, 1),
            ] {
                let Some(array) = array else {
                    continue;
                };
                assert_eq!(array.len(), 2048);
                for i in 0..4096 {
                    let x = cx * 16 + (i % 16) as i32;
                    let z = cz * 16 + ((i / 16) % 16) as i32;
                    let y = sy * 16 + (i / 256) as i32;
                    if let Some(actual) = engine.get(kind, x, y, z) {
                        let expected = ((array[i / 2] as u8) >> ((i % 2) * 4)) & 15;
                        compared[slot] += 1;
                        if actual != expected && total_mismatch[slot] + mismatch[slot] < 20 {
                            eprintln!(
                                "{kind:?} mismatch {x} {y} {z}: actual {actual} expected {expected} state {:?}",
                                ferrite_data::blocks::state(world.state(x, y, z))
                            );
                        }
                        mismatch[slot] += usize::from(actual != expected);
                    }
                }
            }
        }
        // Every pooled snapshot for this centre must equal the serial engine
        // block for block, for both light kinds and at every worker count.
        for (workers, snapshots) in &pooled {
            let snapshot = &snapshots[center_index];
            for kind in [LightKind::Block, LightKind::Sky] {
                for y in MIN_SECTION * 16..=(MAX_SECTION + 1) * 16 - 1 {
                    for local_z in 0..16 {
                        for local_x in 0..16 {
                            let x = cx * 16 + local_x;
                            let z = cz * 16 + local_z;
                            assert_eq!(
                                snapshot.get(kind, x, y, z),
                                engine.get(kind, x, y, z),
                                "{workers}-worker pool {kind:?} at {x},{y},{z} in chunk {cx},{cz}"
                            );
                            pool_compared += 1;
                        }
                    }
                }
            }
        }
        for slot in 0..2 {
            total_mismatch[slot] += mismatch[slot];
            total_compared[slot] += compared[slot];
        }
    }
    assert_eq!(
        pool_compared,
        candidates.len() * pooled.len() * 2 * COMPARE_Y * 16 * 16,
        "every pooled snapshot must be compared over every block and both light kinds"
    );
    eprintln!("pool differential total: {pool_compared} block/kind comparisons, 0 mismatches");
    eprintln!(
        "vanilla differential total: block {}/{} ({:.5}%), sky {}/{} ({:.5}%)",
        total_mismatch[0],
        total_compared[0],
        total_mismatch[0] as f64 * 100.0 / total_compared[0] as f64,
        total_mismatch[1],
        total_compared[1],
        total_mismatch[1] as f64 * 100.0 / total_compared[1] as f64
    );
    assert!(
        total_compared[0] > 0 && total_compared[1] > 0,
        "fixture has no stored light arrays"
    );
    assert_eq!(
        total_mismatch,
        [0, 0],
        "vanilla light mismatch; rerun tools/make_fixture_world.py to regenerate the fixture"
    );
}
