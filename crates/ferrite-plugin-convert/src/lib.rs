//! Static `.jar` compatibility analyzer and Rust native plugin scaffold generator for Ferrite.

pub mod bytecode;
pub mod classfile;
pub mod compat;
pub mod scaffold;

pub use bytecode::{
    AutoBuildOutcome, AutoConvertPlan, ExtractedHandler, HandlerAction, NativeEventKind,
    analyze_jar_for_auto_convert, auto_build_native_plugin, extract_handlers_from_class,
    generate_standalone_cdylib_rs,
};
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

    #[test]
    fn bytecode_auto_transpiles_and_builds_cdylib_end_to_end() {
        // Construct a valid minimal JVM class containing `onBreak(BlockBreakEvent)` whose
        // bytecode runs `event.setCancelled(true)` and `player.sendMessage("Protected!")`.
        let mut class_bytes = Vec::new();
        class_bytes.extend_from_slice(&0xCAFE_BABEu32.to_be_bytes());
        class_bytes.extend_from_slice(&0u16.to_be_bytes());
        class_bytes.extend_from_slice(&65u16.to_be_bytes());
        // Constant pool entries (1..=14):
        // 1: Utf8 "example/GuardPlugin"
        // 2: Class #1
        // 3: Utf8 "java/lang/Object"
        // 4: Class #3
        // 5: Utf8 "onBreak"
        // 6: Utf8 "(Lorg/bukkit/event/block/BlockBreakEvent;)V"
        // 7: Utf8 "Code"
        // 8: Utf8 "Protected!"
        // 9: String #8
        // 10: Utf8 "setCancelled"
        // 11: Utf8 "(Z)V"
        // 12: NameAndType #10 #11
        // 13: Methodref #2 #12
        // 14: Utf8 "sendMessage"
        // 15: Utf8 "(Ljava/lang/String;)V"
        // 16: NameAndType #14 #15
        // 17: Methodref #2 #16
        let push_utf8 = |buf: &mut Vec<u8>, s: &str| {
            buf.push(1);
            buf.extend_from_slice(&(s.len() as u16).to_be_bytes());
            buf.extend_from_slice(s.as_bytes());
        };
        class_bytes.extend_from_slice(&18u16.to_be_bytes()); // cp_count = 18 (1..=17)
        push_utf8(&mut class_bytes, "example/GuardPlugin"); // 1
        class_bytes.push(7);
        class_bytes.extend_from_slice(&1u16.to_be_bytes()); // 2
        push_utf8(&mut class_bytes, "java/lang/Object"); // 3
        class_bytes.push(7);
        class_bytes.extend_from_slice(&3u16.to_be_bytes()); // 4
        push_utf8(&mut class_bytes, "onBreak"); // 5
        push_utf8(
            &mut class_bytes,
            "(Lorg/bukkit/event/block/BlockBreakEvent;)V",
        ); // 6
        push_utf8(&mut class_bytes, "Code"); // 7
        push_utf8(&mut class_bytes, "Protected!"); // 8
        class_bytes.push(8);
        class_bytes.extend_from_slice(&8u16.to_be_bytes()); // 9: String "Protected!"
        push_utf8(&mut class_bytes, "setCancelled"); // 10
        push_utf8(&mut class_bytes, "(Z)V"); // 11
        class_bytes.push(12);
        class_bytes.extend_from_slice(&10u16.to_be_bytes());
        class_bytes.extend_from_slice(&11u16.to_be_bytes()); // 12
        class_bytes.push(10);
        class_bytes.extend_from_slice(&2u16.to_be_bytes());
        class_bytes.extend_from_slice(&12u16.to_be_bytes()); // 13: Methodref setCancelled
        push_utf8(&mut class_bytes, "sendMessage"); // 14
        push_utf8(&mut class_bytes, "(Ljava/lang/String;)V"); // 15
        class_bytes.push(12);
        class_bytes.extend_from_slice(&14u16.to_be_bytes());
        class_bytes.extend_from_slice(&15u16.to_be_bytes()); // 16
        class_bytes.push(10);
        class_bytes.extend_from_slice(&2u16.to_be_bytes());
        class_bytes.extend_from_slice(&16u16.to_be_bytes()); // 17: Methodref sendMessage

        // access_flags, this_class, super_class, interfaces_count, fields_count
        class_bytes.extend_from_slice(&0x0021u16.to_be_bytes());
        class_bytes.extend_from_slice(&2u16.to_be_bytes());
        class_bytes.extend_from_slice(&4u16.to_be_bytes());
        class_bytes.extend_from_slice(&0u16.to_be_bytes()); // 0 interfaces
        class_bytes.extend_from_slice(&0u16.to_be_bytes()); // 0 fields
        class_bytes.extend_from_slice(&1u16.to_be_bytes()); // 1 method

        // Method: public void onBreak(BlockBreakEvent)
        class_bytes.extend_from_slice(&0x0001u16.to_be_bytes()); // access
        class_bytes.extend_from_slice(&5u16.to_be_bytes()); // name #5
        class_bytes.extend_from_slice(&6u16.to_be_bytes()); // desc #6
        class_bytes.extend_from_slice(&1u16.to_be_bytes()); // 1 attribute (Code)

        // Bytecode: iconst_1; invokevirtual #13 (setCancelled); ldc #9 ("Protected!"); invokevirtual #17 (sendMessage); return
        let code: Vec<u8> = vec![
            0x04, // iconst_1
            0xb6, 0x00, 0x0d, // invokevirtual #13
            0x12, 0x09, // ldc #9
            0xb6, 0x00, 0x11, // invokevirtual #17
            0xb1, // return
        ];
        let mut attr = Vec::new();
        attr.extend_from_slice(&2u16.to_be_bytes()); // max_stack
        attr.extend_from_slice(&2u16.to_be_bytes()); // max_locals
        attr.extend_from_slice(&(code.len() as u32).to_be_bytes());
        attr.extend_from_slice(&code);
        attr.extend_from_slice(&0u16.to_be_bytes()); // exception_table_length
        attr.extend_from_slice(&0u16.to_be_bytes()); // attributes_count
        class_bytes.extend_from_slice(&7u16.to_be_bytes()); // attr name "Code"
        class_bytes.extend_from_slice(&(attr.len() as u32).to_be_bytes());
        class_bytes.extend_from_slice(&attr);

        let dir = tempfile::tempdir().unwrap();
        let jar = dir.path().join("guard.jar");
        {
            let f = std::fs::File::create(&jar).unwrap();
            let mut zip = zip::ZipWriter::new(f);
            let opts = SimpleFileOptions::default();
            zip.start_file("plugin.yml", opts).unwrap();
            zip.write_all(b"name: AutoGuard\nversion: 1.0.0\nmain: example.GuardPlugin\n")
                .unwrap();
            zip.start_file("example/GuardPlugin.class", opts).unwrap();
            zip.write_all(&class_bytes).unwrap();
            zip.finish().unwrap();
        }

        let plan = analyze_jar_for_auto_convert(&jar).unwrap();
        assert!(plan.auto_convertible, "{:?}", plan.fallback_reason);
        assert_eq!(plan.handlers.len(), 1);
        assert_eq!(plan.handlers[0].event, Some(NativeEventKind::BlockBreak));
        assert!(
            plan.handlers[0]
                .actions
                .contains(&HandlerAction::CancelEvent)
        );
        assert!(
            plan.handlers[0]
                .actions
                .contains(&HandlerAction::SendMessage("Protected!".into()))
        );

        let out_native = dir.path().join("native");
        match auto_build_native_plugin(&jar, &out_native).unwrap() {
            AutoBuildOutcome::Built(lib_path) => assert!(lib_path.is_file()),
            AutoBuildOutcome::FallbackToJvm(reason) => {
                panic!("expected cdylib build, got: {reason}")
            }
        }
    }

    #[test]
    fn bytecode_decodes_extended_events_and_player_state_actions() {
        let mut class_bytes = Vec::new();
        class_bytes.extend_from_slice(&0xCAFEBABEu32.to_be_bytes());
        class_bytes.extend_from_slice(&0u16.to_be_bytes());
        class_bytes.extend_from_slice(&61u16.to_be_bytes());
        // 24 constant pool entries -> count = 25
        class_bytes.extend_from_slice(&25u16.to_be_bytes());
        let push_utf8 = |buf: &mut Vec<u8>, s: &str| {
            buf.push(1);
            buf.extend_from_slice(&(s.len() as u16).to_be_bytes());
            buf.extend_from_slice(s.as_bytes());
        };
        push_utf8(&mut class_bytes, "example/KitPlugin"); // 1
        class_bytes.push(7);
        class_bytes.extend_from_slice(&1u16.to_be_bytes()); // 2
        push_utf8(&mut class_bytes, "java/lang/Object"); // 3
        class_bytes.push(7);
        class_bytes.extend_from_slice(&3u16.to_be_bytes()); // 4
        push_utf8(&mut class_bytes, "onCmdPreprocess"); // 5
        push_utf8(
            &mut class_bytes,
            "(Lorg/bukkit/event/player/PlayerCommandPreprocessEvent;)V",
        ); // 6
        push_utf8(&mut class_bytes, "Code"); // 7
        push_utf8(&mut class_bytes, "setHealth"); // 8
        push_utf8(&mut class_bytes, "(D)V"); // 9
        class_bytes.push(12);
        class_bytes.extend_from_slice(&8u16.to_be_bytes());
        class_bytes.extend_from_slice(&9u16.to_be_bytes()); // 10
        class_bytes.push(10);
        class_bytes.extend_from_slice(&2u16.to_be_bytes());
        class_bytes.extend_from_slice(&10u16.to_be_bytes()); // 11: Methodref setHealth
        push_utf8(&mut class_bytes, "setFoodLevel"); // 12
        push_utf8(&mut class_bytes, "(I)V"); // 13
        class_bytes.push(12);
        class_bytes.extend_from_slice(&12u16.to_be_bytes());
        class_bytes.extend_from_slice(&13u16.to_be_bytes()); // 14
        class_bytes.push(10);
        class_bytes.extend_from_slice(&2u16.to_be_bytes());
        class_bytes.extend_from_slice(&14u16.to_be_bytes()); // 15: Methodref setFoodLevel
        push_utf8(&mut class_bytes, "org/bukkit/Material"); // 16
        class_bytes.push(7);
        class_bytes.extend_from_slice(&16u16.to_be_bytes()); // 17: Class Material
        push_utf8(&mut class_bytes, "DIAMOND"); // 18
        push_utf8(&mut class_bytes, "Lorg/bukkit/Material;"); // 19
        class_bytes.push(12);
        class_bytes.extend_from_slice(&18u16.to_be_bytes());
        class_bytes.extend_from_slice(&19u16.to_be_bytes()); // 20
        class_bytes.push(9); // Fieldref
        class_bytes.extend_from_slice(&17u16.to_be_bytes());
        class_bytes.extend_from_slice(&20u16.to_be_bytes()); // 21: Fieldref Material.DIAMOND
        push_utf8(&mut class_bytes, "addItem"); // 22
        push_utf8(
            &mut class_bytes,
            "([Lorg/bukkit/inventory/ItemStack;)Ljava/util/HashMap;",
        ); // 23
        class_bytes.push(12);
        class_bytes.extend_from_slice(&22u16.to_be_bytes());
        class_bytes.extend_from_slice(&23u16.to_be_bytes()); // 24
        // Actually use InvokeMethodref? Let's make 24+1 -> need count = 26 so #25 is Methodref addItem.
        let count_bytes = 26u16.to_be_bytes();
        class_bytes[8] = count_bytes[0];
        class_bytes[9] = count_bytes[1];
        class_bytes.push(10);
        class_bytes.extend_from_slice(&2u16.to_be_bytes());
        class_bytes.extend_from_slice(&24u16.to_be_bytes()); // 25: Methodref addItem

        class_bytes.extend_from_slice(&0x0021u16.to_be_bytes());
        class_bytes.extend_from_slice(&2u16.to_be_bytes());
        class_bytes.extend_from_slice(&4u16.to_be_bytes());
        class_bytes.extend_from_slice(&0u16.to_be_bytes());
        class_bytes.extend_from_slice(&0u16.to_be_bytes());
        class_bytes.extend_from_slice(&1u16.to_be_bytes());

        class_bytes.extend_from_slice(&0x0001u16.to_be_bytes());
        class_bytes.extend_from_slice(&5u16.to_be_bytes());
        class_bytes.extend_from_slice(&6u16.to_be_bytes());
        class_bytes.extend_from_slice(&1u16.to_be_bytes());

        // bipush 20; invokevirtual setHealth; bipush 18; invokevirtual setFoodLevel;
        // getstatic Material.DIAMOND; iconst_3; invokevirtual addItem; return
        let code: Vec<u8> = vec![
            0x10, 20, // bipush 20
            0xb6, 0x00, 0x0b, // invokevirtual #11 (setHealth)
            0x10, 18, // bipush 18
            0xb6, 0x00, 0x0f, // invokevirtual #15 (setFoodLevel)
            0xb2, 0x00, 0x15, // getstatic #21 (Material.DIAMOND)
            0x06, // iconst_3
            0xb6, 0x00, 0x19, // invokevirtual #25 (addItem)
            0xb1, // return
        ];
        let mut attr = Vec::new();
        attr.extend_from_slice(&3u16.to_be_bytes());
        attr.extend_from_slice(&2u16.to_be_bytes());
        attr.extend_from_slice(&(code.len() as u32).to_be_bytes());
        attr.extend_from_slice(&code);
        attr.extend_from_slice(&0u16.to_be_bytes());
        attr.extend_from_slice(&0u16.to_be_bytes());
        class_bytes.extend_from_slice(&7u16.to_be_bytes());
        class_bytes.extend_from_slice(&(attr.len() as u32).to_be_bytes());
        class_bytes.extend_from_slice(&attr);

        let dir = tempfile::tempdir().unwrap();
        let jar = dir.path().join("kit.jar");
        {
            let f = std::fs::File::create(&jar).unwrap();
            let mut zip = zip::ZipWriter::new(f);
            let opts = SimpleFileOptions::default();
            zip.start_file("plugin.yml", opts).unwrap();
            zip.write_all(b"name: KitPlugin\nversion: 1.0.0\nmain: example.KitPlugin\n")
                .unwrap();
            zip.start_file("example/KitPlugin.class", opts).unwrap();
            zip.write_all(&class_bytes).unwrap();
            zip.finish().unwrap();
        }

        let plan = analyze_jar_for_auto_convert(&jar).unwrap();
        assert!(plan.auto_convertible, "{:?}", plan.fallback_reason);
        assert_eq!(plan.handlers[0].event, Some(NativeEventKind::PlayerCommand));
        assert!(
            plan.handlers[0]
                .actions
                .contains(&HandlerAction::SetHealth(20))
        );
        assert!(
            plan.handlers[0]
                .actions
                .contains(&HandlerAction::SetFood(18))
        );
        assert!(plan.handlers[0].actions.contains(&HandlerAction::GiveItem {
            item: "minecraft:diamond".to_owned(),
            amount: 3,
        }));
        let out_native = dir.path().join("native");
        match auto_build_native_plugin(&jar, &out_native).unwrap() {
            AutoBuildOutcome::Built(lib_path) => assert!(lib_path.is_file()),
            AutoBuildOutcome::FallbackToJvm(reason) => {
                panic!("expected cdylib build, got: {reason}")
            }
        }
    }
}
