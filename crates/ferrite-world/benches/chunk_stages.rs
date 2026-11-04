//! Per-stage cost of loading every full chunk of the fixture region:
//! region read (file IO + decompress), NBT parse, and NBT -> `Chunk`.
use criterion::{Criterion, criterion_group, criterion_main};
use ferrite_nbt::{Tag, binary};
use ferrite_world::{RegionFile, RuntimeRegistry, chunk_from_nbt};
use std::hint::black_box;

fn stages(c: &mut Criterion) {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3");
    let registry = RuntimeRegistry::load(base.join("generated")).unwrap();
    let path = base.join("fixture-world/world/dimensions/minecraft/overworld/region/r.0.0.mca");
    let region = RegionFile::open(&path, 0, 0).unwrap();
    let coords: Vec<(i32, i32)> = (0..32)
        .flat_map(|z| (0..32).map(move |x| (x, z)))
        .filter(|&(x, z)| region.read_chunk(x, z).unwrap().is_some())
        .collect();
    let raw: Vec<Vec<u8>> = coords
        .iter()
        .map(|&(x, z)| region.read_chunk(x, z).unwrap().unwrap())
        .collect();
    let tags: Vec<Tag> = raw
        .iter()
        .map(|bytes| binary::read_named(bytes).unwrap().1)
        .collect();
    eprintln!("fixture chunks: {}", coords.len());
    let mut group = c.benchmark_group("chunk_stages");
    group.sample_size(20);
    group.bench_function("region_read_all", |b| {
        b.iter(|| {
            let region = RegionFile::open(&path, 0, 0).unwrap();
            for &(x, z) in &coords {
                black_box(region.read_chunk(x, z).unwrap());
            }
        })
    });
    group.bench_function("nbt_parse_all", |b| {
        b.iter(|| {
            for bytes in &raw {
                black_box(binary::read_named(black_box(bytes)).unwrap());
            }
        })
    });
    group.bench_function("chunk_from_nbt_all", |b| {
        b.iter(|| {
            for tag in &tags {
                black_box(chunk_from_nbt(black_box(tag), -64, 384, &registry).ok());
            }
        })
    });
    group.finish();
}
criterion_group!(benches, stages);
criterion_main!(benches);
