use ferrite_world::{RegionCompression, RegionFile};
#[test]
fn region_roundtrip_and_reuse_sectors() {
    let dir = std::env::temp_dir().join(format!("ferrite-region-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("r.0.0.mca");
    let _ = std::fs::remove_file(&path);
    let region = RegionFile::open(&path, 0, 0).unwrap();
    for compression in [
        RegionCompression::Gzip,
        RegionCompression::Zlib,
        RegionCompression::None,
        RegionCompression::Lz4,
    ] {
        let data = vec![42; 9000];
        region.write_chunk(1, 2, &data, compression).unwrap();
        assert_eq!(region.read_chunk(1, 2).unwrap(), Some(data));
    }
    let large: Vec<_> = (0..1_100_000).map(|i| (i % 251) as u8).collect();
    region
        .write_chunk(3, 4, &large, RegionCompression::None)
        .unwrap();
    assert!(dir.join("c.3.4.mcc").exists());
    assert_eq!(region.read_chunk(3, 4).unwrap(), Some(large));
    region
        .write_chunk(3, 4, b"small", RegionCompression::Zlib)
        .unwrap();
    assert_eq!(region.read_chunk(3, 4).unwrap(), Some(b"small".to_vec()));
    assert!(!dir.join("c.3.4.mcc").exists());
    drop(region);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn concurrent_handles_share_file_lock_and_reuse_space() {
    let dir =
        std::env::temp_dir().join(format!("ferrite-region-concurrent-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("r.0.0.mca");
    let _ = std::fs::remove_file(&path);
    let mut workers = Vec::new();
    for x in 0..8 {
        let path = path.clone();
        workers.push(std::thread::spawn(move || {
            let region = RegionFile::open(path, 0, 0).unwrap();
            for n in 0..20 {
                let data = vec![n as u8; 5000];
                region
                    .write_chunk(x, 0, &data, RegionCompression::None)
                    .unwrap();
                assert_eq!(region.read_chunk(x, 0).unwrap(), Some(data));
            }
        }));
    }
    for worker in workers {
        worker.join().unwrap();
    }
    let region = RegionFile::open(&path, 0, 0).unwrap();
    for x in 0..8 {
        assert_eq!(region.read_chunk(x, 0).unwrap(), Some(vec![19; 5000]));
    }
    assert!(std::fs::metadata(path).unwrap().len() < 32 * 4096);
    drop(region);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn write_compressed_batch_roundtrips_and_cleans_sidecars() {
    let dir = std::env::temp_dir().join(format!("ferrite-region-batch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("r.0.0.mca");
    let _ = std::fs::remove_file(&path);
    let region = RegionFile::open(&path, 0, 0).unwrap();

    let large: Vec<_> = (0..1_100_000).map(|i| (i % 251) as u8).collect();
    region
        .write_chunk(2, 3, &large, RegionCompression::None)
        .unwrap();
    assert!(dir.join("c.2.3.mcc").exists());

    let mut batch = Vec::new();
    for i in 0..8 {
        let raw = vec![(i * 13 + 7) as u8; 6000];
        let comp = RegionFile::compress_chunk(&raw, RegionCompression::Lz4).unwrap();
        batch.push((i, 3, comp));
    }
    region
        .write_compressed_batch(&batch, RegionCompression::Lz4)
        .unwrap();
    assert!(!dir.join("c.2.3.mcc").exists());

    drop(region);
    let reopened = RegionFile::open(&path, 0, 0).unwrap();
    for i in 0..8 {
        assert_eq!(
            reopened.read_chunk(i, 3).unwrap(),
            Some(vec![(i * 13 + 7) as u8; 6000])
        );
    }
    drop(reopened);
    std::fs::remove_dir_all(dir).unwrap();
}
