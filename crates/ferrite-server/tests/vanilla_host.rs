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

    // Queries answered by the jar's own biome source and structure manager.
    let biome = generator.biome_at(Dimension::Overworld, 0, 64, 0).unwrap();
    assert!(
        biome.starts_with("minecraft:"),
        "unexpected biome {biome:?}"
    );
    let found = generator
        .locate_biome(Dimension::Overworld, &biome, 0, 0, 512)
        .unwrap()
        .expect("the biome at the origin is within 512 blocks of the origin");
    assert!(found.0.abs() <= 512 && found.2.abs() <= 512, "{found:?}");
    assert!(
        generator
            .locate_biome(Dimension::Overworld, "#minecraft:is_overworld", 0, 0, 512)
            .unwrap()
            .is_some()
    );
    // A tag search must not fail; whether a village is near the origin depends on the seed.
    generator
        .locate_structure(Dimension::Overworld, "#minecraft:village", 0, 0, 100)
        .unwrap();
    assert!(
        generator
            .structure_at(Dimension::Overworld, "minecraft:village_plains", 0, 64, 0)
            .is_ok()
    );
    assert!(
        generator
            .locate_structure(Dimension::Overworld, "minecraft:not_a_structure", 0, 0, 10)
            .is_err()
    );

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

/// Measurement, not a check: `cargo test -p ferrite-server --test vanilla_host measure -- --ignored
/// --nocapture` with `FERRITE_TEST_ACCEPT_EULA=true`. `FERRITE_BENCH_PATCH=true` runs the patched
/// jar; `FERRITE_BENCH_SIDE` (chunks per side, default 12) and `FERRITE_BENCH_THREADS` (default 4)
/// shape the load. Prints start time, throughput, and the host's memory and CPU time.
#[test]
#[ignore]
fn measure() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let defaults = HostSettings::standard(&root);
    assert_eq!(
        std::env::var("FERRITE_TEST_ACCEPT_EULA").as_deref(),
        Ok("true"),
        "set FERRITE_TEST_ACCEPT_EULA=true"
    );
    let env = |name: &str, default: i32| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let patch = std::env::var("FERRITE_BENCH_PATCH").as_deref() == Ok("true");
    let side = env("FERRITE_BENCH_SIDE", 12);
    let threads = env("FERRITE_BENCH_THREADS", 4);
    let registry = RuntimeRegistry::load(root.join("vanilla/26.3/generated")).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let eula = scratch.path().join("eula.txt");
    std::fs::write(
        &eula,
        "eula=true
",
    )
    .unwrap();
    let settings = HostSettings {
        work_dir: scratch.path().join("worldgen"),
        eula,
        heap_mb: env("FERRITE_BENCH_HEAP", 1024) as u32,
        threads: threads as u32,
        seed: "12345".into(),
        patch_rules: patch.then(|| root.join("java/ferrite-worldgen-host/rules/worldgen.rules")),
        // e.g. FERRITE_BENCH_JVM_ARGS="-XX:StartFlightRecording=filename=x.jfr,settings=profile"
        jvm_args: std::env::var("FERRITE_BENCH_JVM_ARGS")
            .map(|args| args.split_whitespace().map(String::from).collect())
            .unwrap_or_default(),
        ..defaults
    };

    let started = std::time::Instant::now();
    let generator = VanillaGenerator::spawn(&settings, Arc::new(registry)).unwrap();
    let start = started.elapsed();

    let coords: Vec<(i32, i32)> = (0..side)
        .flat_map(|x| (0..side).map(move |z| (x, z)))
        .collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let begun = std::time::Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(&(x, z)) = coords.get(index) else {
                        break;
                    };
                    generator.generate(Dimension::Overworld, x, z).unwrap();
                }
            });
        }
    });
    let elapsed = begun.elapsed();

    // Memory and CPU time of the host JVM, read from outside (Windows PowerShell).
    let probe = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "$p = Get-Process -Id {}; \"{{0}} MiB working set, {{1:N1}} s cpu\" -f [int]($p.WorkingSet64/1MB), $p.TotalProcessorTime.TotalSeconds",
                generator.process_id()
            ),
        ])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default();
    // Idle cost: CPU seconds the host burns in 15 s with no requests.
    let cpu = |pid: u32| -> f64 {
        std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!("(Get-Process -Id {pid}).TotalProcessorTime.TotalSeconds"),
            ])
            .output()
            .ok()
            .and_then(|out| String::from_utf8_lossy(&out.stdout).trim().parse().ok())
            .unwrap_or(0.0)
    };
    let before = cpu(generator.process_id());
    std::thread::sleep(std::time::Duration::from_secs(15));
    let idle_cpu = cpu(generator.process_id()) - before;
    // FERRITE_BENCH_HISTO=<file> writes the live-object histogram of the host (needs a JDK).
    if let Ok(file) = std::env::var("FERRITE_BENCH_HISTO") {
        let jcmd = ferrite_jvm::find_java_home(None)
            .map(|java| java.root().join("bin").join("jcmd.exe"))
            .unwrap();
        let out = std::process::Command::new(jcmd)
            .args([&generator.process_id().to_string(), "GC.class_histogram"])
            .output()
            .unwrap();
        std::fs::write(file, out.stdout).unwrap();
    }
    println!("IDLE patch={patch} cpu_in_15s={idle_cpu:.2}s");
    println!(
        "MEASURE patch={patch} side={side} threads={threads} start={:.1}s chunks={} time={:.1}s rate={:.1}/s | {probe}",
        start.as_secs_f64(),
        coords.len(),
        elapsed.as_secs_f64(),
        coords.len() as f64 / elapsed.as_secs_f64()
    );
}

/// The patched jar must produce the same terrain as the unmodified jar (within the jar's own
/// run-to-run variation).
#[test]
fn patched_jar_generates_the_same_terrain() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let defaults = HostSettings::standard(&root);
    if std::env::var("FERRITE_TEST_ACCEPT_EULA").as_deref() != Ok("true")
        || !defaults.server_jar.exists()
        || !defaults.host_jar.exists()
    {
        eprintln!("skipping patched jar test: jar, host or FERRITE_TEST_ACCEPT_EULA missing");
        return;
    }
    let Ok(registry) = RuntimeRegistry::load(root.join("vanilla/26.3/generated")) else {
        eprintln!("skipping patched jar test: generated vanilla data missing");
        return;
    };
    let registry = Arc::new(registry);
    let scratch = tempfile::tempdir().unwrap();
    let eula = scratch.path().join("eula.txt");
    std::fs::write(&eula, "eula=true\n").unwrap();
    let make = |name: &str, patched: bool| {
        let settings = HostSettings {
            work_dir: scratch.path().join(name),
            eula: eula.clone(),
            heap_mb: 768,
            threads: 2,
            seed: "424242".into(),
            patch_rules: patched
                .then(|| root.join("java/ferrite-worldgen-host/rules/worldgen.rules")),
            ..defaults.clone()
        };
        VanillaGenerator::spawn(&settings, registry.clone()).unwrap()
    };
    let plain = make("plain", false);
    // FERRITE_TEST_CONTROL=true compares two unmodified hosts: it shows how much the jar's own
    // output varies between runs, which is the noise floor for this comparison.
    let control = std::env::var("FERRITE_TEST_CONTROL").as_deref() == Ok("true");
    let patched = make("patched", !control);
    // The jar's own output varies from run to run (a control comparison of two unmodified hosts
    // differs too), so compare how much differs, not whether anything does.
    let (mut total, mut differing) = (0u64, 0u64);
    for dimension in [Dimension::Overworld, Dimension::Nether] {
        for (cx, cz) in [(0, 0), (5, -3), (-9, 12)] {
            let a = plain.generate(dimension, cx, cz).unwrap().chunk;
            let b = patched.generate(dimension, cx, cz).unwrap().chunk;
            for x in 0..16u8 {
                for z in 0..16u8 {
                    for y in a.min_y()..a.min_y() + a.height_blocks() as i32 {
                        total += 1;
                        differing += u64::from(a.block(x, y, z) != b.block(x, y, z));
                    }
                }
            }
        }
    }
    assert!(
        differing * 200 < total,
        "{differing} of {total} blocks differ between the unmodified and the patched jar"
    );
}

/// `/place feature|structure|jigsaw` run vanilla's own placement in the host and return the
/// blocks it changed.
#[test]
fn places_features_and_structures_through_the_host() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let defaults = HostSettings::standard(&root);
    if std::env::var("FERRITE_TEST_ACCEPT_EULA").as_deref() != Ok("true")
        || !defaults.server_jar.exists()
        || !defaults.host_jar.exists()
    {
        eprintln!("skipping place test: jar, host or FERRITE_TEST_ACCEPT_EULA missing");
        return;
    }
    let Ok(registry) = RuntimeRegistry::load(root.join("vanilla/26.3/generated")) else {
        return;
    };
    let scratch = tempfile::tempdir().unwrap();
    let eula = scratch.path().join("eula.txt");
    std::fs::write(&eula, "eula=true\n").unwrap();
    let settings = HostSettings {
        work_dir: scratch.path().join("worldgen"),
        eula,
        heap_mb: 1024,
        threads: 2,
        seed: "12345".into(),
        ..defaults
    };
    let generator = VanillaGenerator::spawn(&settings, Arc::new(registry)).unwrap();
    let height = generator.spawn_height(Dimension::Overworld, 0, 0).unwrap();
    let tree = generator
        .place(
            Dimension::Overworld,
            &format!("feature;minecraft:oak;0;{height};0"),
        )
        .unwrap();
    assert!(
        tree.len() > 10,
        "an oak changes more than {} blocks",
        tree.len()
    );
    let village = generator.place(
        Dimension::Overworld,
        &format!("structure;minecraft:village_plains;0;{height};0"),
    );
    eprintln!("village: {:?}", village.as_ref().map(Vec::len));
    let missing = generator.place(
        Dimension::Overworld,
        "feature;minecraft:not_a_feature;0;70;0",
    );
    assert!(missing.is_err());
}
