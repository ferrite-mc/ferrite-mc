//! Tests for the YAML configuration files.

use ferrite_config::yaml::MaybeInt;
use ferrite_config::yaml::bukkit::BukkitConfig;
use ferrite_config::yaml::paper_global::PaperGlobal;
use ferrite_config::yaml::paper_world::WorldSettings;
use ferrite_config::yaml::spigot::SpigotConfig;
use ferrite_config::{ConfigFileKind, ConfigOverrides, FerriteConfig};
use serde_norway::Value;

const PAPER_GLOBAL: &str = include_str!("fixtures/paper-global.yml");
const PAPER_WORLD_DEFAULTS: &str = include_str!("fixtures/paper-world-defaults.yml");
const PAPER_WORLD: &str = include_str!("fixtures/paper-world.yml");

fn setup() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(dir.path().join("config")).expect("config dir");
    std::fs::write(dir.path().join("config/paper-global.yml"), PAPER_GLOBAL).expect("global");
    std::fs::write(
        dir.path().join("config/paper-world-defaults.yml"),
        PAPER_WORLD_DEFAULTS,
    )
    .expect("world defaults");
    std::fs::create_dir_all(dir.path().join("world_nether")).expect("world dir");
    std::fs::write(dir.path().join("world_nether/paper-world.yml"), PAPER_WORLD).expect("world");
    dir
}

/// Assert that every key/value in `expected` also appears in `actual`.
fn assert_value_contains(actual: &Value, expected: &Value, path: &str) {
    match (actual, expected) {
        (Value::Mapping(actual), Value::Mapping(expected)) => {
            for (key, expected_value) in expected {
                let key_name = key.as_str().unwrap_or("<non-string>");
                let actual_value = actual.get(key).unwrap_or_else(|| {
                    panic!("missing key `{path}.{key_name}` in re-emitted YAML")
                });
                assert_value_contains(actual_value, expected_value, &format!("{path}.{key_name}"));
            }
        }
        (Value::Sequence(actual), Value::Sequence(expected)) => {
            assert_eq!(
                actual.len(),
                expected.len(),
                "sequence length differs at `{path}`"
            );
            for (index, (actual_item, expected_item)) in
                actual.iter().zip(expected.iter()).enumerate()
            {
                assert_value_contains(actual_item, expected_item, &format!("{path}[{index}]"));
            }
        }
        (actual, expected) => assert_eq!(actual, expected, "value differs at `{path}`"),
    }
}

#[test]
fn sample_paper_global_parses() {
    let dir = setup();
    let config = FerriteConfig::load(dir.path()).expect("load config");
    let global = config.paper_global();

    assert_eq!(global.chunk_system.gen_parallelism, 2);
    assert_eq!(global.chunk_system.io_threads, 4);
    assert_eq!(global.chunk_system.worker_threads, 8);
    assert!(!global.collisions.enable_player_collisions);
    assert!(global.proxies.proxy_protocol);
    assert!(global.proxies.velocity.enabled);
    assert!(!global.proxies.velocity.online_mode);
    assert_eq!(global.proxies.velocity.secret, "hunter2");
    assert_eq!(global.misc.max_joins_per_tick, 3);
    assert_eq!(global.misc.catchup_ticks, MaybeInt::Default);
    assert!(!global.timings.enabled);
    assert_eq!(global.timings.history_interval, 600);
    // Missing keys fall back to Paper defaults.
    assert_eq!(global.misc.max_tracking_combat_entries, 10240);
    assert!(global.console.enable_brigadier_completions);
}

#[test]
fn sample_paper_global_reemits_equivalent_and_keeps_unknown_keys() {
    let dir = setup();
    let config = FerriteConfig::load(dir.path()).expect("load config");
    let emitted = config
        .to_yaml_with_comments(ConfigFileKind::PaperGlobal)
        .expect("render");

    assert!(
        emitted.contains("# UNVERIFIED"),
        "comments should flag legacy keys"
    );
    assert!(
        emitted.contains("# Shared forwarding secret"),
        "known keys should have comments:\n{emitted}"
    );
    assert!(emitted.contains("plugin-only-key:"));
    assert!(emitted.contains("- alpha"));

    let original: Value = serde_norway::from_str(PAPER_GLOBAL).expect("parse fixture");
    let round_tripped: Value = serde_norway::from_str(&emitted).expect("parse re-emitted");
    assert_value_contains(&round_tripped, &original, "paper-global");

    let reparsed: PaperGlobal =
        serde_norway::from_value(round_tripped).expect("reparse typed config");
    assert_eq!(reparsed.proxies.velocity.secret, "hunter2");
    assert_eq!(reparsed.chunk_system.worker_threads, 8);
    assert_eq!(reparsed.timings.history_interval, 600);
}

#[test]
fn world_defaults_and_per_world_fallback() {
    let dir = setup();
    let config = FerriteConfig::load(dir.path()).expect("load config");

    let overworld = config.world_settings("world");
    assert!(!overworld.anticheat.anti_xray.enabled);
    assert_eq!(overworld.anticheat.anti_xray.engine_mode, 2);
    assert_eq!(
        overworld.anticheat.anti_xray.hidden_blocks,
        ["copper_ore", "chest"]
    );
    assert_eq!(overworld.collisions.max_entity_collisions, 6);
    assert_eq!(overworld.chunks.delay_chunk_unloads_by, "25m");
    assert_eq!(overworld.chunks.auto_save_interval, MaybeInt::Value(1200));
    assert!(overworld.hopper.disable_move_event);
    assert_eq!(overworld.environment.void_damage_amount, MaybeInt::Disabled);
    assert_eq!(
        overworld.environment.nether_ceiling_void_damage_height,
        MaybeInt::Value(200)
    );
    assert_eq!(
        overworld.entities.spawning.duplicate_uuid.mode,
        ferrite_config::yaml::paper_world::DuplicateUuidMode::Delete
    );
    assert!(
        !overworld
            .entities
            .mob_effects
            .immune_to_wither_effect
            .wither
    );
    assert!(
        overworld
            .entities
            .mob_effects
            .immune_to_wither_effect
            .wither_skeleton
    );
    assert_eq!(overworld.tick_rates.mob_spawner, 2);

    let nether = config.world_settings("world_nether");
    assert!(nether.anticheat.anti_xray.enabled, "world value should win");
    assert!(!nether.hopper.cooldown_when_full, "world value should win");
    assert!(
        nether.hopper.disable_move_event,
        "unset value should fall back to world defaults"
    );
    assert_eq!(nether.collisions.max_entity_collisions, 6);
    assert_eq!(nether.chunks.delay_chunk_unloads_by, "25m");

    // An unknown world has no override file and gets the defaults.
    let unknown = config.world_settings("world_the_end");
    assert!(!unknown.anticheat.anti_xray.enabled);
    assert_eq!(unknown.anticheat.anti_xray.engine_mode, 2);
}

#[test]
fn broken_world_file_falls_back_and_reports() {
    let dir = setup();
    std::fs::create_dir_all(dir.path().join("broken")).expect("world dir");
    std::fs::write(
        dir.path().join("broken/paper-world.yml"),
        "anticheat:\n  anti-xray:\n    engine-mode: not-a-number\n",
    )
    .expect("write broken file");

    let config = FerriteConfig::load(dir.path()).expect("load config");
    assert!(config.try_world_settings("broken").is_err());
    let fallback = config.world_settings("broken");
    assert_eq!(fallback, *config.paper_world_defaults());
}

#[test]
fn unknown_paper_keys_survive_rewrite() {
    let dir = setup();
    let config = FerriteConfig::load(dir.path()).expect("load config");
    config.save().expect("save");

    let global =
        std::fs::read_to_string(dir.path().join("config/paper-global.yml")).expect("read global");
    assert!(global.contains("plugin-only-key:"));
    assert!(global.contains("- alpha"));

    let defaults = std::fs::read_to_string(dir.path().join("config/paper-world-defaults.yml"))
        .expect("read world defaults");
    assert!(defaults.contains("unknown-world-key: keep-me"));

    let reloaded = FerriteConfig::load(dir.path()).expect("reload");
    assert_eq!(
        reloaded
            .paper_world_defaults()
            .chunks
            .max_auto_save_chunks_per_tick,
        12
    );
    assert_eq!(reloaded.paper_global().chunk_system.worker_threads, 8);
}

#[test]
fn bukkit_and_spigot_defaults_reemit_equivalent() {
    let config = FerriteConfig::defaults("server").expect("defaults");

    let bukkit_text = config
        .to_yaml_with_comments(ConfigFileKind::Bukkit)
        .expect("bukkit yaml");
    let bukkit_value: Value = serde_norway::from_str(&bukkit_text).expect("parse bukkit");
    let bukkit: BukkitConfig =
        serde_norway::from_value(bukkit_value.clone()).expect("typed bukkit");
    assert_eq!(bukkit, *config.bukkit());
    assert_eq!(
        bukkit_value,
        serde_norway::to_value(config.bukkit()).expect("value")
    );
    assert_eq!(bukkit.spawn_limits.monsters, 70);
    assert_eq!(bukkit.ticks_per.autosave, 6000);
    assert_eq!(
        bukkit.settings.deprecated_verbose,
        ferrite_config::yaml::bukkit::DeprecatedVerbose::Keyword("default".to_string())
    );

    let spigot_text = config
        .to_yaml_with_comments(ConfigFileKind::Spigot)
        .expect("spigot yaml");
    let spigot_value: Value = serde_norway::from_str(&spigot_text).expect("parse spigot");
    let spigot: SpigotConfig =
        serde_norway::from_value(spigot_value.clone()).expect("typed spigot");
    assert_eq!(spigot, *config.spigot());
    assert_eq!(
        spigot_value,
        serde_norway::to_value(config.spigot()).expect("value")
    );
    assert_eq!(spigot.settings.netty_threads, 4);
    assert_eq!(
        spigot.world_settings["default"]
            .entity_activation_range
            .monsters,
        24
    );
}

#[test]
fn paper_defaults_reemit_equivalent() {
    let config = FerriteConfig::defaults("server").expect("defaults");
    for kind in [
        ConfigFileKind::PaperGlobal,
        ConfigFileKind::PaperWorldDefaults,
    ] {
        let text = config.to_yaml_with_comments(kind).expect("render");
        let value: Value = serde_norway::from_str(&text).expect("parse");
        let expected = match kind {
            ConfigFileKind::PaperGlobal => {
                serde_norway::to_value(config.paper_global()).expect("value")
            }
            ConfigFileKind::PaperWorldDefaults => {
                serde_norway::to_value(config.paper_world_defaults()).expect("value")
            }
            _ => unreachable!(),
        };
        assert_eq!(value, expected, "{} did not round-trip", kind.file_name());
    }
}

#[test]
fn save_defaults_writes_every_file_and_reloads() {
    let dir = tempfile::tempdir().expect("temp dir");
    let config = FerriteConfig::save_defaults(dir.path()).expect("save defaults");

    for relative in [
        "server.properties",
        "bukkit.yml",
        "spigot.yml",
        "config/paper-global.yml",
        "config/paper-world-defaults.yml",
    ] {
        let path = dir.path().join(relative);
        assert!(path.is_file(), "missing generated file {relative}");
    }

    let server = std::fs::read_to_string(dir.path().join("server.properties")).expect("read");
    assert!(server.contains("# Port the server listens on."));
    assert!(server.contains("server-port=25565"));

    let reloaded = FerriteConfig::load(dir.path()).expect("reload defaults");
    assert_eq!(reloaded.server_properties(), config.server_properties());
    assert_eq!(reloaded.bukkit(), config.bukkit());
    assert_eq!(reloaded.spigot(), config.spigot());
    assert_eq!(reloaded.paper_global(), config.paper_global());
    assert_eq!(
        reloaded.paper_world_defaults(),
        config.paper_world_defaults()
    );
}

#[test]
fn cli_overrides_apply_everywhere() {
    let dir = setup();
    let overrides = ConfigOverrides::from_cli_args([
        "--set",
        "server.motd=Overridden",
        "--set=paper-global.proxies.velocity.enabled=true",
        "--set",
        "paper-world-defaults.anticheat.anti-xray.enabled=true",
        "--set",
        "world.world_nether.hopper.disable-move-event=false",
        "--set",
        "bukkit.ticks-per.autosave=1200",
        "--set",
        "spigot.settings.netty-threads=2",
        "unrelated-argument",
    ])
    .expect("parse cli overrides");

    let config = FerriteConfig::load_with_overrides(dir.path(), &overrides).expect("load");
    assert_eq!(config.server_properties().motd, "Overridden");
    assert!(config.paper_global().proxies.velocity.enabled);
    assert_eq!(config.bukkit().ticks_per.autosave, 1200);
    assert_eq!(config.spigot().settings.netty_threads, 2);
    assert!(config.world_settings("world").anticheat.anti_xray.enabled);
    let nether = config.world_settings("world_nether");
    assert!(
        !nether.hopper.disable_move_event,
        "world override should be applied after the world file"
    );
    assert!(nether.anticheat.anti_xray.enabled);
}

#[test]
fn environment_overrides_apply() {
    let dir = setup();
    let overrides = ConfigOverrides::from_env_vars([
        ("FERRITE_SERVER_MAX_PLAYERS", "50"),
        (
            "FERRITE_CONFIG_OVERRIDES",
            "paper-global.packet-limiter.all-packets.max-packet-rate=250.5;\
             world.world.hopper.disable-move-event=false",
        ),
    ])
    .expect("parse env overrides");

    let config = FerriteConfig::load_with_overrides(dir.path(), &overrides).expect("load");
    assert_eq!(config.server_properties().max_players, 50);
    assert_eq!(
        config
            .paper_global()
            .packet_limiter
            .all_packets
            .max_packet_rate,
        250.5
    );
    assert!(!config.world_settings("world").hopper.disable_move_event);
    assert!(config.world_settings("world").hopper.cooldown_when_full);
}

#[test]
fn bad_overrides_are_rejected() {
    let dir = setup();
    let config = FerriteConfig::defaults(dir.path()).expect("defaults");

    let unknown_key =
        ConfigOverrides::from_cli_args(["--set", "paper-global.this.key.does.not.exist=1"])
            .expect("syntax is valid");
    assert!(config.clone().apply_overrides(&unknown_key).is_err());

    let wrong_type =
        ConfigOverrides::from_cli_args(["--set", "paper-global.proxies.proxy-protocol=yes"])
            .expect("syntax is valid");
    assert!(config.clone().apply_overrides(&wrong_type).is_err());

    let unknown_server_key = ConfigOverrides::from_cli_args(["--set", "server.not-a-real-key=1"])
        .expect("syntax is valid");
    assert!(config.clone().apply_overrides(&unknown_server_key).is_err());

    let unknown_section =
        ConfigOverrides::from_cli_args(["--set", "not-a-section.foo=1"]).expect_err("section");
    assert!(matches!(
        unknown_section,
        ferrite_config::ConfigError::Override { .. }
    ));
}

#[test]
fn config_version_migration_hoists_legacy_key() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(dir.path().join("config")).expect("config dir");
    std::fs::write(
        dir.path().join("config/paper-global.yml"),
        "config-version: 0\ncommands:\n  time-command-affects-all-worlds: true\n",
    )
    .expect("write legacy file");

    let config = FerriteConfig::load(dir.path()).expect("load");
    assert_eq!(config.paper_global().config_version, 1);
    assert!(config.paper_global().time.affects_all_worlds);
    assert!(
        !config
            .paper_global()
            .commands
            .time_command_affects_all_worlds
    );
}

#[test]
fn newer_config_version_is_not_rewritten() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(dir.path().join("config")).expect("config dir");
    std::fs::write(
        dir.path().join("config/paper-global.yml"),
        "_version_: 99\nchunk-system:\n  io-threads: 2\n",
    )
    .expect("write file");

    let config = FerriteConfig::load(dir.path()).expect("load");
    assert_eq!(config.paper_global().config_version, 99);
    assert_eq!(config.paper_global().chunk_system.io_threads, 2);
    config.save().expect("save");

    let written =
        std::fs::read_to_string(dir.path().join("config/paper-global.yml")).expect("read");
    let value: Value = serde_norway::from_str(&written).expect("parse");
    assert_eq!(value.get("_version_").and_then(Value::as_u64), Some(99));
}

#[test]
fn world_render_includes_comments() {
    let dir = setup();
    let config = FerriteConfig::load(dir.path()).expect("load");
    let rendered = config
        .world_to_yaml_with_comments("world_nether")
        .expect("render world");
    assert!(rendered.contains("# Paper per-world overrides."));
    assert!(rendered.contains("anti-xray:"));
    assert!(rendered.contains("enabled: true"));
    let value: Value = serde_norway::from_str(&rendered).expect("parse world render");
    let settings: WorldSettings = serde_norway::from_value(value).expect("typed world");
    assert!(settings.anticheat.anti_xray.enabled);
    assert!(!settings.hopper.cooldown_when_full);
}

#[test]
fn unsupported_settings_default_to_papers_values() {
    let dir = tempfile::tempdir().expect("temp dir");
    let config = FerriteConfig::load(dir.path()).expect("load config");
    let unsupported = &config.paper_global().unsupported_settings;
    assert!(!unsupported.allow_headless_pistons);
    assert!(!unsupported.allow_permanent_block_break_exploits);
    assert!(!unsupported.allow_piston_duplication);
    assert!(!unsupported.allow_unsafe_end_portal_teleportation);
    assert!(unsupported.perform_username_validation);
}

#[test]
fn unsupported_settings_are_read_from_paper_global_yml() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(dir.path().join("config")).expect("config dir");
    std::fs::write(
        dir.path().join("config/paper-global.yml"),
        "unsupported-settings:\n  allow-headless-pistons: true\n  \
         allow-permanent-block-break-exploits: true\n  allow-piston-duplication: true\n  \
         allow-unsafe-end-portal-teleportation: true\n  perform-username-validation: false\n",
    )
    .expect("global");
    let config = FerriteConfig::load(dir.path()).expect("load config");
    let unsupported = &config.paper_global().unsupported_settings;
    assert!(unsupported.allow_headless_pistons);
    assert!(unsupported.allow_permanent_block_break_exploits);
    assert!(unsupported.allow_piston_duplication);
    assert!(unsupported.allow_unsafe_end_portal_teleportation);
    assert!(!unsupported.perform_username_validation);
    // Settings that were not written keep Paper's defaults.
    assert!(unsupported.update_equipment_on_player_actions);
}
