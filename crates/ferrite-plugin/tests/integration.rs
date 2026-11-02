use ferrite_plugin::{
    BlockPosition, BlockSnapshot, ColumnHeight, CommandSender, EntityTask, GameMode, ItemSnapshot,
    OfflinePlayerSnapshot, PlayerCommand, PlayerLocation, PlayerSnapshot, PluginConfig, PluginHost,
    RegionContext, RegionTask, ServerCommand, ServerSnapshot, WorldCommand, WorldSnapshot,
};

#[derive(Default)]
struct FakeRegions {
    regions: std::sync::Mutex<Vec<RegionTask>>,
    entities: std::sync::Mutex<Vec<EntityTask>>,
}
impl RegionContext for FakeRegions {
    fn is_owned_by_current_region(&self, _: uuid::Uuid, x: i32, z: i32) -> bool {
        z == 8 && (-4..=-3).contains(&x)
    }
    fn schedule_on_region(&self, task: RegionTask) {
        self.regions.lock().unwrap().push(task);
    }
    fn schedule_on_entity(&self, task: EntityTask) {
        self.entities.lock().unwrap().push(task);
    }
    fn retire_entity(&self, _: uuid::Uuid) {}
}
use std::{path::PathBuf, process::Command};

#[test]
fn plugin_lifecycle() {
    let java_home = match ferrite_jvm::find_java_home(None) {
        Ok(home) => home.root().to_owned(),
        Err(error) => {
            eprintln!("Java 25 unavailable; skipping plugin integration test: {error}");
            return;
        }
    };
    // Build a clean path: `cmd.exe` mishandles a cwd that mixes `\`, `/` and `..`.
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crate lives in <repo>/crates/<name>")
        .to_owned();
    let project = repo_root.join("java").join("ferrite-api-impl");
    let mut command = if cfg!(windows) {
        let mut c = Command::new("cmd");
        // `cmd` may not search the cwd (NoDefaultCurrentDirectoryInExePath), so be explicit.
        c.args(["/C", ".\\gradlew.bat", "build"]);
        c
    } else {
        let mut c = Command::new("./gradlew");
        c.arg("build");
        c
    };
    let result = command
        .current_dir(&project)
        .env("JAVA_HOME", &java_home)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "gradle build failed in {}:
{}
{}",
        project.display(),
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(
        repo_root
            .join("java")
            .join("test-plugin")
            .join("build/libs/test-plugin.jar"),
        dir.path().join("test.jar"),
    )
    .unwrap();
    std::fs::copy(
        repo_root
            .join("java")
            .join("test-plugin")
            .join("build/libs/test-plugin-legacy.jar"),
        dir.path().join("legacy.jar"),
    )
    .unwrap();
    std::fs::copy(
        project.join("../test-dependency/build/libs/test-dependency.jar"),
        dir.path().join("dependency.jar"),
    )
    .unwrap();
    let called = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = called.clone();
    let regions = std::sync::Arc::new(FakeRegions::default());
    let messages = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured_messages = messages.clone();
    let world_commands = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured_world_commands = world_commands.clone();
    let server_commands = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured_server_commands = server_commands.clone();
    let cfg = PluginConfig {
        server_handler: Some(std::sync::Arc::new(move |command| {
            captured_server_commands.lock().unwrap().push(command)
        })),
        world_handler: Some(std::sync::Arc::new(move |command| {
            captured_world_commands.lock().unwrap().push(command)
        })),
        player_handler: Some(std::sync::Arc::new(move |command| {
            captured_messages.lock().unwrap().push(command);
        })),

        region_context: Some(regions.clone()),
        server_snapshot: ServerSnapshot {
            motd: "Integration MOTD".into(),
            max_players: 42,
            port: 25570,
            view_distance: 8,
            whitelist: true,
            online_mode: false,
            banned_ips: vec!["192.0.2.7".parse().unwrap()],
            bind_address: "127.0.0.1".into(),
        },
        data_dir: project.join("../../vanilla/26.3/generated"),
        world_view: None,
        world: None,
        command_handler: Some(std::sync::Arc::new(move |_, line| {
            captured.lock().unwrap().push(line.to_owned());
            true
        })),
        bridge_jar: project.join("build/libs/ferrite-api-impl-all.jar"),
        java_home: Some(java_home),
    };
    let host = PluginHost::start(dir.path(), cfg).unwrap();
    assert!(host.is_plugin_enabled("FerriteTest").unwrap());
    assert!(!host.is_plugin_enabled("Missing").unwrap());
    let world_id = uuid::Uuid::new_v4();
    host.register_world(&WorldSnapshot {
        id: world_id,
        name: "probe_world".into(),
        time: 6000,
        spawn: BlockPosition { x: 0, y: 64, z: 0 },
        heights: vec![ColumnHeight { x: 1, z: 2, y: 64 }],
        blocks: vec![BlockSnapshot {
            position: BlockPosition { x: 1, y: 64, z: 2 },
            material: "minecraft:stone".into(),
        }],
    })
    .unwrap();
    let offline_id = uuid::Uuid::from_u128(44);
    host.register_offline_player(&OfflinePlayerSnapshot {
        id: offline_id,
        name: Some("KnownOffline".into()),
        operator: true,
    })
    .unwrap();
    host.register_offline_player(&OfflinePlayerSnapshot {
        id: uuid::Uuid::from_u128(45),
        name: None,
        operator: false,
    })
    .unwrap();
    let player_id = uuid::Uuid::new_v4();
    host.register_player(&PlayerSnapshot {
        id: player_id,
        name: "ProbePlayer".into(),
        operator: true,
        health: 16.0,
        food: 14,
        mode: GameMode::Creative,
        inventory: vec![Some(ItemSnapshot {
            material: "minecraft:stone".into(),
            amount: 3,
        })],
        location: PlayerLocation {
            world: world_id,
            x: 1.0,
            y: 65.0,
            z: 2.0,
            yaw: 0.0,
            pitch: 0.0,
        },
    })
    .unwrap();
    assert!(
        host.dispatch_command(CommandSender::Player(player_id), "ferrite-test hello")
            .unwrap()
    );
    assert!(
        host.logs()
            .iter()
            .any(|line| line.contains("player snapshot 16.0 14 CREATIVE"))
    );
    assert!(
        host.logs()
            .iter()
            .any(|line| line.contains("offline snapshot true"))
    );
    assert!(
        host.logs()
            .iter()
            .any(|line| line.contains("world snapshot probe_world STONE"))
    );
    assert!(world_commands.lock().unwrap().iter().any(
        |command| matches!(command, WorldCommand::SetBlock { world, .. } if *world == world_id)
    ));
    assert!(
        server_commands
            .lock()
            .unwrap()
            .iter()
            .any(|command| matches!(command, ServerCommand::SetWhitelist { enabled: false }))
    );
    assert!(messages.lock().unwrap().iter().any(|command| matches!(command, PlayerCommand::DisplayName { player, .. } if *player == player_id)));
    assert!(messages.lock().unwrap().iter().any(
        |command| matches!(command, PlayerCommand::Title { player, .. } if *player == player_id)
    ));
    assert!(messages.lock().unwrap().iter().any(
        |command| matches!(command, PlayerCommand::Kick { player, .. } if *player == player_id)
    ));
    assert!(messages.lock().unwrap().iter().any(
        |command| matches!(command, PlayerCommand::PlaySound { player, .. } if *player == player_id)
    ));
    assert!(server_commands.lock().unwrap().iter().any(|command| matches!(command, ServerCommand::BanIp { address, .. } if *address == "192.0.2.2".parse::<std::net::IpAddr>().unwrap())));
    assert!(messages.lock().unwrap().iter().any(|command| matches!(command, PlayerCommand::SetInventorySlot { player, slot: 0, .. } if *player == player_id)));
    assert!(messages.lock().unwrap().iter().any(
        |command| matches!(command, PlayerCommand::Teleport { player, .. } if *player == player_id)
    ));
    assert!(messages.lock().unwrap().iter().any(|command| matches!(command, PlayerCommand::SetOperator { player, operator: false } if *player == player_id)));
    assert!(messages.lock().unwrap().iter().any(|command| matches!(command, PlayerCommand::SetHealth { player, health } if *player == player_id && *health == 10.0)));
    assert!(messages.lock().unwrap().iter().any(|command| matches!(command, PlayerCommand::SetFood { player, food } if *player == player_id && *food == 9)));
    assert!(messages.lock().unwrap().iter().any(|command| matches!(command, PlayerCommand::SetGameMode { player, mode: GameMode::Adventure } if *player == player_id)));
    assert!(messages.lock().unwrap().iter().any(
        |command| matches!(command, PlayerCommand::Message { player, .. } if *player == player_id)
    ));
    assert!(
        host.logs()
            .iter()
            .any(|line| line.contains("FerriteTest settings Integration MOTD 42 25570 8 true"))
    );
    assert!(
        !host
            .dispatch_command(
                CommandSender::Player(uuid::Uuid::new_v4()),
                "ferrite-test hello"
            )
            .unwrap()
    );
    assert!(
        host.dispatch_command(CommandSender::Console, "ferrite-test hello")
            .unwrap()
    );
    assert!(host.logs().iter().any(|line| line.contains("hello hello")));
    assert_eq!(
        &*called.lock().unwrap(),
        &["bridge-command", "bridge-command"]
    );
    for _ in 0..3 {
        host.tick().unwrap();
    }
    assert_eq!(
        host.logs()
            .iter()
            .filter(|line| line.contains("FerriteTest tick"))
            .count(),
        6
    );
    host.route_ready_tasks().unwrap();
    let entities = std::mem::take(&mut *regions.entities.lock().unwrap());
    assert_eq!(entities.len(), 2);
    assert!(entities.iter().any(|task| task.region_aware));
    assert!(entities.iter().any(|task| !task.region_aware));
    assert!(
        entities
            .iter()
            .all(|task| task.entity == "00000000-0000-0000-0000-000000000002")
    );
    assert!(
        !host
            .logs()
            .iter()
            .any(|line| line.contains("FerriteTest entity"))
    );
    let mut forged_entity = entities[0].clone();
    forged_entity.entity = "forged".into();
    assert!(host.run_entity_task(forged_entity).is_err());
    for task in entities {
        host.run_entity_task(task.clone()).unwrap();
        assert!(host.run_entity_task(task).is_err());
    }
    assert_eq!(
        host.logs()
            .iter()
            .filter(|line| line.contains("FerriteTest entity"))
            .count(),
        2
    );
    let tasks = std::mem::take(&mut *regions.regions.lock().unwrap());
    assert_eq!(tasks.len(), 2);
    assert!(tasks.iter().any(|task| task.region_aware));
    assert!(tasks.iter().any(|task| !task.region_aware));
    assert!(
        tasks
            .iter()
            .all(|task| (task.chunk_x, task.chunk_z) == (-4, 8)
                && task.world == "00000000-0000-0000-0000-000000000001")
    );
    assert!(
        !host
            .logs()
            .iter()
            .any(|line| line.contains("FerriteTest region"))
    );
    let mut forged_region = tasks[0].clone();
    forged_region.chunk_x += 1;
    assert!(host.run_region_task(forged_region).is_err());
    for task in tasks {
        host.run_region_task(task.clone()).unwrap();
        assert!(host.run_region_task(task).is_err());
    }
    assert_eq!(
        host.logs()
            .iter()
            .filter(|line| line.contains("FerriteTest region"))
            .count(),
        2
    );
    assert_eq!(
        host.logs()
            .iter()
            .filter(|line| line.contains("FerriteTest owned"))
            .count(),
        2
    );
    assert_eq!(
        host.logs()
            .iter()
            .filter(|line| line.contains("FerriteTest range owned"))
            .count(),
        2
    );
    host.shutdown().unwrap();
    assert!(
        host.logs()
            .iter()
            .any(|line| line.contains("FerriteTest disabled"))
    );
    assert!(!host.drain_logs().is_empty());
    assert!(host.logs().is_empty());
}
