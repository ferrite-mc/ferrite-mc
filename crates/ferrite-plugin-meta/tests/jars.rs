//! End-to-end tests: build small plugin jars in a tempdir and scan them.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use ferrite_plugin_meta::{
    LoadPlan, ManifestFormat, PaperDependencyLoad, PermissionDefault, PluginLoadOrder,
    ThreadingMode, scan_directory,
};
use tempfile::TempDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// Writes a jar containing a dummy main class plus the given descriptor entries.
fn write_jar(directory: &Path, file_name: &str, descriptors: &[(&str, &str)]) -> PathBuf {
    let path = directory.join(file_name);
    let file = File::create(&path).expect("create jar");
    let mut zip = ZipWriter::new(file);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("com/example/Main.class", stored)
        .expect("start class entry");
    zip.write_all(&[0xCA, 0xFE, 0xBA, 0xBE])
        .expect("write class entry");
    zip.start_file("META-INF/MANIFEST.MF", stored)
        .expect("start manifest entry");
    zip.write_all(b"Manifest-Version: 1.0\n")
        .expect("write manifest entry");
    for (name, contents) in descriptors {
        zip.start_file(*name, deflated)
            .expect("start descriptor entry");
        zip.write_all(contents.as_bytes())
            .expect("write descriptor entry");
    }
    zip.finish().expect("finish jar");
    path
}

fn plugin_jar(directory: &Path, file_name: &str, yaml: &str) -> PathBuf {
    write_jar(directory, file_name, &[("plugin.yml", yaml)])
}

fn postworld_names(plan: &LoadPlan) -> Vec<&str> {
    plan.postworld.iter().map(|plugin| plugin.name()).collect()
}

fn failure_messages(plan: &LoadPlan) -> Vec<String> {
    plan.failed
        .iter()
        .map(|failure| failure.error.to_string())
        .collect()
}

fn position(plan: &LoadPlan, name: &str) -> usize {
    plan.loaded()
        .position(|plugin| plugin.name() == name)
        .expect("plugin should load")
}

fn loaded<'a>(plan: &'a LoadPlan, name: &str) -> &'a ferrite_plugin_meta::LoadedPlugin {
    plan.loaded()
        .find(|plugin| plugin.name() == name)
        .expect("plugin should load")
}

#[test]
fn basic_plugin_yml_is_parsed_from_a_jar() {
    let dir = TempDir::new().unwrap();
    plugin_jar(
        dir.path(),
        "basic.jar",
        r#"
name: Basic
version: 1.2.3
main: com.example.Basic
api-version: 1.21
author: Alice
authors: [Bob]
description: A basic plugin
website: https://example.com
prefix: "[Basic]"
depend: [Lib]
softdepend: [Soft]
loadbefore: [Late]
provides: [BasicAlias]
libraries: [group:artifact:1.0.0]
commands:
  basic:
    description: Basic command
    usage: /basic
    aliases: [b]
    permission: basic.use
permissions:
  basic.use:
    description: Allows /basic
    default: true
    children:
      basic.child: true
"#,
    );
    plugin_jar(
        dir.path(),
        "lib.jar",
        "name: Lib\nversion: '1'\nmain: com.example.Lib\n",
    );
    plugin_jar(
        dir.path(),
        "soft.jar",
        "name: Soft\nversion: '1'\nmain: com.example.Soft\n",
    );
    plugin_jar(
        dir.path(),
        "late.jar",
        "name: Late\nversion: '1'\nmain: com.example.Late\n",
    );

    let plan = scan_directory(dir.path()).unwrap();
    assert!(
        plan.failed.is_empty(),
        "unexpected failures: {:?}",
        plan.failed
    );

    let basic = loaded(&plan, "Basic");
    assert_eq!(basic.meta.version, "1.2.3");
    assert_eq!(basic.meta.main, "com.example.Basic");
    assert_eq!(basic.meta.api_version.as_deref(), Some("1.21"));
    assert_eq!(basic.meta.authors, ["Alice", "Bob"]);
    assert_eq!(basic.meta.description.as_deref(), Some("A basic plugin"));
    assert_eq!(basic.meta.website.as_deref(), Some("https://example.com"));
    assert_eq!(basic.meta.prefix.as_deref(), Some("[Basic]"));
    assert_eq!(basic.meta.depend, ["Lib"]);
    assert_eq!(basic.meta.softdepend, ["Soft"]);
    assert_eq!(basic.meta.loadbefore, ["Late"]);
    assert_eq!(basic.meta.provides, ["BasicAlias"]);
    assert_eq!(basic.meta.libraries, ["group:artifact:1.0.0"]);
    assert_eq!(
        basic.meta.commands["basic"].usage.as_deref(),
        Some("/basic")
    );
    assert_eq!(basic.meta.commands["basic"].aliases, ["b"]);
    assert_eq!(
        basic.meta.commands["basic"].permission.as_deref(),
        Some("basic.use")
    );
    assert_eq!(
        basic.meta.permissions["basic.use"].default,
        PermissionDefault::True
    );
    assert!(basic.meta.permissions["basic.use"].children["basic.child"]);
    assert_eq!(basic.meta.format, ManifestFormat::PluginYml);
    assert!(basic.meta.paper.is_none());
    assert_eq!(basic.threading_mode(), ThreadingMode::LegacyMainThread);

    assert_eq!(postworld_names(&plan), ["Lib", "Soft", "Basic", "Late"]);
}

#[test]
fn paper_plugin_yml_is_parsed_and_takes_precedence() {
    let dir = TempDir::new().unwrap();
    write_jar(
        dir.path(),
        "paper.jar",
        &[
            (
                "plugin.yml",
                "name: Ignored\nversion: '1'\nmain: com.example.Ignored\n",
            ),
            (
                "paper-plugin.yml",
                r#"
name: PaperPlugin
version: 2.0
main: com.example.PaperMain
api-version: '1.21'
bootstrapper: com.example.Boot
loader: com.example.Loader
has-open-classloader: true
folia-supported: true
dependencies:
  server:
    Other:
      load: BEFORE
      required: true
      join-classpath: true
    Optional:
      required: false
  bootstrap:
    BootDep:
      load: AFTER
"#,
            ),
        ],
    );
    plugin_jar(
        dir.path(),
        "other.jar",
        "name: Other\nversion: '1'\nmain: com.example.Other\n",
    );

    let plan = scan_directory(dir.path()).unwrap();
    assert!(
        plan.failed.is_empty(),
        "unexpected failures: {:?}",
        plan.failed
    );

    let paper_plugin = loaded(&plan, "PaperPlugin");
    assert_eq!(paper_plugin.meta.format, ManifestFormat::PaperPluginYml);
    assert_eq!(paper_plugin.threading_mode(), ThreadingMode::RegionAware);
    let paper = paper_plugin.meta.paper.as_ref().expect("paper metadata");
    assert_eq!(paper.bootstrapper.as_deref(), Some("com.example.Boot"));
    assert_eq!(paper.loader.as_deref(), Some("com.example.Loader"));
    assert!(paper.has_open_classloader);
    assert_eq!(
        paper.dependencies.server["Other"].load,
        PaperDependencyLoad::Before
    );
    assert!(paper.dependencies.server["Other"].required);
    assert!(paper.dependencies.server["Other"].join_classpath);
    assert!(!paper.dependencies.server["Optional"].required);
    assert_eq!(
        paper.dependencies.bootstrap["BootDep"].load,
        PaperDependencyLoad::After
    );

    // `Other` is a BEFORE dependency, so it must load first.
    assert_eq!(postworld_names(&plan), ["Other", "PaperPlugin"]);
}

#[test]
fn missing_hard_dependency_fails_plugin_and_its_dependents() {
    let dir = TempDir::new().unwrap();
    plugin_jar(
        dir.path(),
        "01-broken.jar",
        "name: Broken\nversion: '1'\nmain: com.example.Broken\ndepend: [Ghost]\n",
    );
    plugin_jar(
        dir.path(),
        "02-dependent.jar",
        "name: Dependent\nversion: '1'\nmain: com.example.Dependent\ndepend: [Broken]\n",
    );
    plugin_jar(
        dir.path(),
        "03-fine.jar",
        "name: Fine\nversion: '1'\nmain: com.example.Fine\n",
    );

    let plan = scan_directory(dir.path()).unwrap();
    assert_eq!(postworld_names(&plan), ["Fine"]);

    let messages = failure_messages(&plan);
    assert_eq!(messages.len(), 2, "unexpected failures: {messages:?}");
    assert!(
        messages
            .iter()
            .any(|message| message.contains("Missing") || message.contains("Ghost")),
        "missing dependency should be reported: {messages:?}"
    );
    assert!(
        messages.iter().any(|message| message
            .contains("Unknown/missing dependency plugins: [Ghost]")
            && message.contains("Broken")),
        "missing dependency message should carry the plugin and dependency: {messages:?}"
    );
    assert!(
        messages.iter().any(|message| message
            .contains("Unknown/missing dependency plugins: [Broken]")
            && message.contains("Dependent")),
        "dependents of a skipped plugin should fail too: {messages:?}"
    );
}

#[test]
fn soft_dependency_orders_but_does_not_require_presence() {
    let dir = TempDir::new().unwrap();
    plugin_jar(
        dir.path(),
        "alpha.jar",
        "name: Alpha\nversion: '1'\nmain: com.example.Alpha\nsoftdepend: [Zulu, Ghost]\n",
    );
    plugin_jar(
        dir.path(),
        "zulu.jar",
        "name: Zulu\nversion: '1'\nmain: com.example.Zulu\n",
    );

    let plan = scan_directory(dir.path()).unwrap();
    assert!(
        plan.failed.is_empty(),
        "unexpected failures: {:?}",
        plan.failed
    );
    assert_eq!(postworld_names(&plan), ["Zulu", "Alpha"]);
}

#[test]
fn loadbefore_orders_plugin_before_target() {
    let dir = TempDir::new().unwrap();
    plugin_jar(
        dir.path(),
        "aardvark.jar",
        "name: Aardvark\nversion: '1'\nmain: com.example.Aardvark\n",
    );
    plugin_jar(
        dir.path(),
        "zebra.jar",
        "name: Zebra\nversion: '1'\nmain: com.example.Zebra\nloadbefore: [Aardvark]\n",
    );

    let plan = scan_directory(dir.path()).unwrap();
    assert!(
        plan.failed.is_empty(),
        "unexpected failures: {:?}",
        plan.failed
    );
    assert_eq!(postworld_names(&plan), ["Zebra", "Aardvark"]);
}

#[test]
fn provides_aliases_satisfy_hard_dependencies() {
    let dir = TempDir::new().unwrap();
    plugin_jar(
        dir.path(),
        "consumer.jar",
        "name: Consumer\nversion: '1'\nmain: com.example.Consumer\ndepend: [ALIAS]\n",
    );
    plugin_jar(
        dir.path(),
        "provider.jar",
        "name: Provider\nversion: '1'\nmain: com.example.Provider\nprovides: [Alias]\n",
    );

    let plan = scan_directory(dir.path()).unwrap();
    assert!(
        plan.failed.is_empty(),
        "unexpected failures: {:?}",
        plan.failed
    );
    assert_eq!(postworld_names(&plan), ["Provider", "Consumer"]);
}

#[test]
fn cycles_are_reported_paper_style_and_skipped() {
    let dir = TempDir::new().unwrap();
    plugin_jar(
        dir.path(),
        "alpha.jar",
        "name: Alpha\nversion: '1'\nmain: com.example.Alpha\ndepend: [Bravo]\n",
    );
    plugin_jar(
        dir.path(),
        "bravo.jar",
        "name: Bravo\nversion: '1'\nmain: com.example.Bravo\ndepend: [Charlie]\n",
    );
    plugin_jar(
        dir.path(),
        "charlie.jar",
        "name: Charlie\nversion: '1'\nmain: com.example.Charlie\ndepend: [Alpha]\n",
    );
    plugin_jar(
        dir.path(),
        "delta.jar",
        "name: Delta\nversion: '1'\nmain: com.example.Delta\n",
    );
    plugin_jar(
        dir.path(),
        "echo.jar",
        "name: Echo\nversion: '1'\nmain: com.example.Echo\ndepend: [Alpha]\n",
    );

    let plan = scan_directory(dir.path()).unwrap();
    assert_eq!(postworld_names(&plan), ["Delta"]);

    let messages = failure_messages(&plan);
    assert_eq!(messages.len(), 4, "unexpected failures: {messages:?}");
    let cycle_messages: Vec<&String> = messages
        .iter()
        .filter(|message| message.starts_with("Circular plugin loading detected: "))
        .collect();
    assert_eq!(cycle_messages.len(), 3, "unexpected failures: {messages:?}");
    for message in cycle_messages {
        assert!(
            message.contains(" -> "),
            "cycle chain should use arrows: {message}"
        );
        assert!(
            message.contains("Alpha") && message.contains("Bravo") && message.contains("Charlie")
        );
    }
    assert!(
        messages.iter().any(|message| message
            .contains("Unknown/missing dependency plugins: [Alpha]")
            && message.contains("Echo")),
        "dependents of cycle members should fail too: {messages:?}"
    );
}

#[test]
fn duplicate_names_keep_the_first_plugin() {
    let dir = TempDir::new().unwrap();
    plugin_jar(
        dir.path(),
        "01-first.jar",
        "name: Dupe\nversion: '1'\nmain: com.example.First\n",
    );
    plugin_jar(
        dir.path(),
        "02-second.jar",
        "name: DUPE\nversion: '2'\nmain: com.example.Second\n",
    );

    let plan = scan_directory(dir.path()).unwrap();
    assert_eq!(postworld_names(&plan), ["Dupe"]);
    assert_eq!(loaded(&plan, "Dupe").meta.version, "1");
    assert_eq!(plan.failed.len(), 1);
    let message = plan.failed[0].error.to_string();
    assert!(
        message.contains("ambiguous plugin name 'DUPE'"),
        "{message}"
    );
    assert!(message.contains("01-first.jar"), "{message}");
}

#[test]
fn folia_supported_selects_region_aware_threading() {
    let dir = TempDir::new().unwrap();
    plugin_jar(
        dir.path(),
        "folia.jar",
        "name: Folia\nversion: '1'\nmain: com.example.Folia\nfolia-supported: true\n",
    );
    plugin_jar(
        dir.path(),
        "legacy.jar",
        "name: Legacy\nversion: '1'\nmain: com.example.Legacy\n",
    );

    let plan = scan_directory(dir.path()).unwrap();
    assert_eq!(
        loaded(&plan, "Folia").threading_mode(),
        ThreadingMode::RegionAware
    );
    assert_eq!(
        loaded(&plan, "Legacy").threading_mode(),
        ThreadingMode::LegacyMainThread
    );
}

#[test]
fn startup_plugins_are_split_from_postworld_plugins() {
    let dir = TempDir::new().unwrap();
    plugin_jar(
        dir.path(),
        "startup.jar",
        "name: Startup\nversion: '1'\nmain: com.example.Startup\nload: STARTUP\n",
    );
    plugin_jar(
        dir.path(),
        "postworld.jar",
        "name: Postworld\nversion: '1'\nmain: com.example.Postworld\ndepend: [Startup]\n",
    );

    let plan = scan_directory(dir.path()).unwrap();
    assert!(
        plan.failed.is_empty(),
        "unexpected failures: {:?}",
        plan.failed
    );
    assert_eq!(
        plan.startup
            .iter()
            .map(|plugin| plugin.name())
            .collect::<Vec<_>>(),
        ["Startup"]
    );
    assert_eq!(postworld_names(&plan), ["Postworld"]);
    assert_eq!(loaded(&plan, "Startup").meta.load, PluginLoadOrder::Startup);
    assert!(position(&plan, "Startup") < position(&plan, "Postworld"));
}

#[test]
fn invalid_descriptors_are_reported_per_jar() {
    let dir = TempDir::new().unwrap();
    plugin_jar(
        dir.path(),
        "bad-name.jar",
        "name: Bad/Name\nversion: '1'\nmain: com.example.Bad\n",
    );
    plugin_jar(dir.path(), "no-main.jar", "name: NoMain\nversion: '1'\n");
    // A plain jar with no descriptor at all.
    write_jar(dir.path(), "no-descriptor.jar", &[]);
    plugin_jar(
        dir.path(),
        "good.jar",
        "name: Good\nversion: '1'\nmain: com.example.Good\n",
    );

    let plan = scan_directory(dir.path()).unwrap();
    assert_eq!(postworld_names(&plan), ["Good"]);
    let messages = failure_messages(&plan);
    assert_eq!(messages.len(), 3, "unexpected failures: {messages:?}");
    assert!(
        messages
            .iter()
            .any(|message| message.contains("name 'Bad/Name' contains invalid characters")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("main is not defined")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("contains neither plugin.yml nor paper-plugin.yml")),
        "{messages:?}"
    );
}

#[test]
fn oversized_compressed_plugin_descriptor_is_rejected() {
    let dir = TempDir::new().unwrap();
    let oversized = format!(
        "name: Oversized\nversion: '1'\nmain: com.example.Oversized\n#{}",
        "x".repeat(1024 * 1024),
    );
    plugin_jar(dir.path(), "oversized.jar", &oversized);

    let mut state = 0x1234_5678_u32;
    let noisy_comment: String = (0..2 * 1024 * 1024)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            char::from(b'a' + ((state >> 24) % 26) as u8)
        })
        .collect();
    let oversized_compressed = format!(
        "name: OversizedCompressed\nversion: '1'\nmain: com.example.OversizedCompressed\n#{noisy_comment}"
    );
    plugin_jar(
        dir.path(),
        "oversized-compressed.jar",
        &oversized_compressed,
    );

    let plan = scan_directory(dir.path()).unwrap();

    assert!(postworld_names(&plan).is_empty());
    assert_eq!(plan.failed.len(), 2);
    for failure in &plan.failed {
        assert!(
            failure
                .error
                .to_string()
                .contains("exceeds the maximum size of 1048576 bytes"),
            "unexpected error: {}",
            failure.error
        );
    }
}
