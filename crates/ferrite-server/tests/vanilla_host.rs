//! End to end: start the vanilla world generation host and generate chunks through it.
//!
//! Skipped unless everything it needs is present: the downloaded server jar
//! (`python tools/fetch_vanilla.py`), the built host (`python tools/build_worldgen_host.py`) and
//! `FERRITE_TEST_ACCEPT_EULA=true`. The test never accepts the Mojang EULA by itself; setting that
//! variable is the developer accepting it (https://aka.ms/MinecraftEULA).
use ferrite_server::game::{ChunkGenerator, Dimension, HostSettings, VanillaGenerator};
use ferrite_world::RuntimeRegistry;
use std::{path::Path, sync::Arc};

#[test]
fn generates_chunks_through_the_host() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let settings = HostSettings::standard(&root);
    if std::env::var("FERRITE_TEST_ACCEPT_EULA").as_deref() != Ok("true")
        || !settings.server_jar.exists()
        || !settings.host_jar.exists()
    {
        eprintln!("skipping vanilla host test: jar, host or FERRITE_TEST_ACCEPT_EULA missing");
        return;
    }
    let Ok(registry) = RuntimeRegistry::load(root.join("vanilla/26.3/generated")) else {
        eprintln!("skipping vanilla host test: generated vanilla data missing");
        return;
    };

    let scratch = tempfile::tempdir().unwrap();
    let eula = scratch.path().join("eula.txt");
    std::fs::write(&eula, "eula=true\n").unwrap();
    let settings = HostSettings {
        work_dir: scratch.path().join("worldgen"),
        eula,
        heap_mb: 768,
        threads: 2,
        seed: "12345".into(),
        ..settings
    };

    let generator = VanillaGenerator::spawn(&settings, Arc::new(registry)).unwrap();
    for (x, z) in [(0, 0), (1, 0), (-3, 5)] {
        let generated = generator.generate(Dimension::Overworld, x, z).unwrap();
        assert_eq!(generated.chunk.min_y(), -64);
        assert_eq!(generated.chunk.height_blocks(), 384);
    }
    let height = generator.spawn_height(Dimension::Overworld, 0, 0).unwrap();
    assert!(
        (40..320).contains(&height),
        "implausible surface height {height}"
    );
    assert!(generator.generate(Dimension::Nether, 0, 0).is_ok());

    // The jar's own game listener must be bound to loopback only, never all interfaces.
    let log = std::fs::read_to_string(scratch.path().join("worldgen/host.log")).unwrap();
    assert!(
        log.contains("127.0.0.1:"),
        "vanilla did not report a loopback bind:\n{log}"
    );
    assert!(
        !log.contains("0.0.0.0") && !log.contains("*:"),
        "vanilla listens on every interface:\n{log}"
    );
}

#[test]
fn refuses_to_start_without_an_accepted_eula() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let scratch = tempfile::tempdir().unwrap();
    let settings = HostSettings {
        work_dir: scratch.path().join("worldgen"),
        eula: scratch.path().join("eula.txt"),
        ..HostSettings::standard(&root)
    };
    let Ok(registry) = RuntimeRegistry::load(root.join("vanilla/26.3/generated")) else {
        return;
    };
    let error = VanillaGenerator::spawn(&settings, Arc::new(registry))
        .err()
        .expect("must refuse");
    assert!(error.to_string().contains("EULA"), "{error}");
    assert!(
        std::fs::read_to_string(&settings.eula)
            .unwrap()
            .contains("eula=false")
    );
}
