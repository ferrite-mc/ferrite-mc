//! Static `.jar` compatibility analyzer and Rust native plugin scaffold generator for Ferrite.

pub mod classfile;
pub mod compat;
pub mod scaffold;

pub use compat::{CompatReport, CompatVerdict, ConvertError, inspect_jar};
pub use scaffold::generate_scaffold;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn build_synthetic_class(class_refs: &[&str]) -> Vec<u8> {
        // Minimal valid JVM class file with UTF8 + Class entries in the constant pool.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0xCAFE_BABEu32.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes()); // minor
        bytes.extend_from_slice(&65u16.to_be_bytes()); // major (Java 21)
        let cp_count = 1 + (class_refs.len() as u16) * 2;
        bytes.extend_from_slice(&cp_count.to_be_bytes());
        for (i, cls) in class_refs.iter().enumerate() {
            let utf8_idx = 1 + (i as u16) * 2;
            bytes.push(1); // CONSTANT_Utf8
            bytes.extend_from_slice(&(cls.len() as u16).to_be_bytes());
            bytes.extend_from_slice(cls.as_bytes());
            bytes.push(7); // CONSTANT_Class
            bytes.extend_from_slice(&utf8_idx.to_be_bytes());
        }
        bytes
    }

    fn write_test_jar(
        path: &std::path::Path,
        plugin_yml: &str,
        config_yml: Option<&str>,
        class_refs: &[&str],
    ) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = SimpleFileOptions::default();
        zip.start_file("plugin.yml", opts).unwrap();
        zip.write_all(plugin_yml.as_bytes()).unwrap();
        if let Some(cfg) = config_yml {
            zip.start_file("config.yml", opts).unwrap();
            zip.write_all(cfg.as_bytes()).unwrap();
        }
        zip.start_file("example/DemoPlugin.class", opts).unwrap();
        zip.write_all(&build_synthetic_class(class_refs)).unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn inspects_ready_plugin_and_generates_rust_scaffold() {
        let dir = tempfile::tempdir().unwrap();
        let jar = dir.path().join("demo.jar");
        write_test_jar(
            &jar,
            "name: DemoWarp\nversion: 1.2.0\nmain: example.DemoPlugin\napi-version: '1.21'\nfolia-supported: true\ncommands:\n  warp:\n    aliases: [w]\n",
            Some("greeting: hello\nmax-warps: 10\nenabled: true\n"),
            &[
                "org/bukkit/plugin/java/JavaPlugin",
                "org/bukkit/event/player/PlayerJoinEvent",
                "io/papermc/paper/threadedregions/scheduler/RegionScheduler",
            ],
        );

        let (meta, config, report) = inspect_jar(&jar).unwrap();
        assert_eq!(report.plugin_name, "DemoWarp");
        assert!(report.folia_supported);
        assert_eq!(report.verdict, CompatVerdict::ReadyForJvmHost);
        assert!(
            report
                .detected_events
                .contains(&"org.bukkit.event.player.PlayerJoinEvent".to_owned())
        );
        assert!(report.blockers.is_empty());

        let out = dir.path().join("out-crate");
        generate_scaffold(&meta, config.as_deref(), &report, &out).unwrap();
        let lib_rs = std::fs::read_to_string(out.join("src/lib.rs")).unwrap();
        assert!(lib_rs.contains("pub struct DemoWarpPlugin"));
        assert!(lib_rs.contains("pub max_warps: i64"));
        assert!(lib_rs.contains("pub fn on_player_join(&self, _player: Uuid, _name: &str)"));
        assert!(lib_rs.contains("pub extern \"C\" fn ferrite_plugin_entry()"));
        assert!(lib_rs.contains("fn scaffold_smoke_test()"));
        assert!(lib_rs.contains("\"warp\" | \"w\""));
        assert!(out.join("COMPAT_REPORT.json").is_file());
        assert!(out.join("PORTING_GUIDE.md").is_file());
        let cargo_toml = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
        assert!(cargo_toml.contains("edition = \"2024\""));
        let report_json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(out.join("COMPAT_REPORT.json")).unwrap())
                .unwrap();
        assert_eq!(report_json["plugin_name"], "DemoWarp");
    }

    #[test]
    fn generates_folia_scheduler_hints_and_permission_constants() {
        let dir = tempfile::tempdir().unwrap();
        let jar = dir.path().join("legacy.jar");
        write_test_jar(
            &jar,
            "name: LegacySched\nversion: 1.0.0\nmain: example.LegacySched\ncommands:\n  ping:\n    permission: legacy.ping\n",
            None,
            &[
                "org/bukkit/plugin/java/JavaPlugin",
                "org/bukkit/scheduler/BukkitScheduler",
                "org/bukkit/event/block/BlockBreakEvent",
            ],
        );
        let (meta, config, report) = inspect_jar(&jar).unwrap();
        let out = dir.path().join("legacy-out");
        generate_scaffold(&meta, config.as_deref(), &report, &out).unwrap();
        let lib_rs = std::fs::read_to_string(out.join("src/lib.rs")).unwrap();
        let guide = std::fs::read_to_string(out.join("PORTING_GUIDE.md")).unwrap();
        assert!(lib_rs.contains("pub mod permissions"));
        assert!(lib_rs.contains("pub const LEGACY_PING: &str = \"legacy.ping\";"));
        assert!(lib_rs.contains("pub fn on_block_break("));
        assert!(guide.contains("Folia Regionized Scheduler Migration Hints"));
    }

    #[test]
    fn flags_netty_and_protocol_blockers_for_rust_porting() {
        let dir = tempfile::tempdir().unwrap();
        let jar = dir.path().join("blocker.jar");
        write_test_jar(
            &jar,
            "name: PacketHack\nversion: 0.1.0\nmain: example.PacketHack\napi-version: '1.21'\n",
            None,
            &[
                "org/bukkit/plugin/java/JavaPlugin",
                "io/netty/channel/ChannelPipeline",
                "net/minecraft/network/protocol/Packet",
            ],
        );

        let (_, _, report) = inspect_jar(&jar).unwrap();
        assert_eq!(report.verdict, CompatVerdict::RecommendRustPort);
        assert_eq!(report.blockers.len(), 2);
    }
}
