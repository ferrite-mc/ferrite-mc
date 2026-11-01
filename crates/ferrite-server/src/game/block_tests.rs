//! Bot-session tests for the block interactions: they drive a real connection through
//! login/configuration and assert the `block_update` packets a tracking client receives.

use super::World;
use super::anvil::AnvilChunkSource;
use super::integration::Bot;
use ferrite_net::{ServerConfig, serve};
use ferrite_protocol::{
    Decode, VarInt,
    packets::play::{
        BlockUpdate, CreativeSlot, ItemStack, MovePosition, MoveRotation, SignUpdate, UseItemOn,
    },
    types::BlockPos,
};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;

fn block(name: &str, props: &[(&str, &str)]) -> u32 {
    u32::from(ferrite_data::blocks::state_id(name, props).unwrap())
}

fn air() -> u32 {
    u32::from(ferrite_data::blocks::default_state("air").unwrap())
}

fn prop(state: u32, key: &str) -> Option<String> {
    u16::try_from(state)
        .ok()
        .and_then(ferrite_data::blocks::state)
        .and_then(|(_, props)| {
            props
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_owned())
        })
}

async fn fixture() -> Option<(Arc<AnvilChunkSource>, PathBuf)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3");
    let fixture = root.join("fixture-world/world/dimensions/minecraft/overworld/region/r.0.0.mca");
    if !fixture.exists() {
        return None;
    }
    let temp = std::env::temp_dir().join(format!("ferrite-blocks-{}", uuid::Uuid::new_v4()));
    let dir = temp.join("dimensions/minecraft/overworld/region");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(fixture, dir.join("r.0.0.mca")).unwrap();
    let registry = ferrite_world::RuntimeRegistry::load(root.join("generated")).unwrap();
    let source = AnvilChunkSource::new(temp.clone(), registry);
    loop {
        if let Some(result) = source.get(0, 0, 0) {
            result.unwrap();
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    Some((source, temp))
}

/// Clears a 13x13 area above the fixture world's floor.
fn flatten(source: &AnvilChunkSource) {
    let stone = u32::from(ferrite_data::blocks::default_state("stone").unwrap());
    for x in 0..=12 {
        for z in 0..=12 {
            source.set_block(x, 63, z, stone).unwrap();
            for y in 64..=66 {
                source.set_block(x, y, z, air()).unwrap();
            }
        }
    }
}

async fn look_down_at(bot: &mut Bot, x: f64, y: f64, z: f64) {
    bot.send(&MovePosition { x, y, z, flags: 1 }).await;
    bot.send(&MoveRotation {
        yaw: 0.0,
        pitch: 90.0,
        flags: 1,
    })
    .await;
}

async fn use_at(bot: &mut Bot, x: i32, y: i32, z: i32, sequence: i32) {
    bot.send(&UseItemOn {
        hand: VarInt(0),
        location: BlockPos::new(x, y, z).pack(),
        face: VarInt(1),
        cursor_x: 0.5,
        cursor_y: 1.0,
        cursor_z: 0.5,
        inside: false,
        world_border_hit: false,
        sequence: VarInt(sequence),
    })
    .await;
}

async fn wait_block(bot: &mut Bot, at: (i32, i32, i32)) -> u32 {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let body = bot.until(8).await;
            let update = BlockUpdate::decode(&mut body.as_slice()).unwrap();
            let pos = ferrite_protocol::types::BlockPos::unpack(update.location);
            if (pos.x, pos.y, pos.z) == at {
                return u32::try_from(update.block_state.0).unwrap_or(0);
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no block update at {at:?}"))
}

/// Vanilla `FlintAndSteelItem`, `DoorBlock`, `LeverBlock` and `ButtonBlock`, all through a
/// tracking bot on the fixture world.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fire_door_lever_and_button_reach_tracking_bot() {
    let Some((source, temp)) = fixture().await else {
        return;
    };
    flatten(&source);
    source.set_block(0, 64, 0, block("stone", &[])).unwrap();
    source
        .set_block(
            2,
            64,
            2,
            block("oak_trapdoor", &[("half", "bottom"), ("facing", "north")]),
        )
        .unwrap();
    source
        .set_block(4, 64, 4, block("lever", &[("face", "floor")]))
        .unwrap();
    source
        .set_block(6, 64, 6, block("oak_button", &[("face", "floor")]))
        .unwrap();
    let world = World::start_with(Some(source.clone()), [0, 65, 0]);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(
        listener,
        ServerConfig {
            online_mode: false,
            view_distance: 2,
            game: Some(world.sender.clone()),
            ..Default::default()
        },
    ));
    let mut bot = Bot::join(addr, "Interactor").await;
    bot.until(46).await;
    let flint =
        ferrite_data::registries::id("minecraft:item", "minecraft:flint_and_steel").unwrap();
    bot.send(&CreativeSlot {
        slot: 36,
        item: ItemStack {
            count: 1,
            item_id: flint,
        },
    })
    .await;
    look_down_at(&mut bot, 0.5, 65.0, 0.5).await;
    use_at(&mut bot, 0, 64, 0, 1).await;
    let fire = wait_block(&mut bot, (0, 65, 0)).await;
    assert_eq!(
        fire,
        u32::from(ferrite_data::blocks::default_state("fire").unwrap())
    );
    // A door with only its lower half is clickable from above.
    look_down_at(&mut bot, 2.5, 65.0, 2.5).await;
    use_at(&mut bot, 2, 64, 2, 2).await;
    let lower = wait_block(&mut bot, (2, 64, 2)).await;
    assert_eq!(prop(lower, "open").as_deref(), Some("true"));
    look_down_at(&mut bot, 4.5, 65.0, 4.5).await;
    use_at(&mut bot, 4, 64, 4, 3).await;
    let lever = wait_block(&mut bot, (4, 64, 4)).await;
    assert_eq!(prop(lever, "powered").as_deref(), Some("true"));
    look_down_at(&mut bot, 6.5, 65.0, 6.5).await;
    use_at(&mut bot, 6, 64, 6, 4).await;
    let pressed = wait_block(&mut bot, (6, 64, 6)).await;
    assert_eq!(prop(pressed, "powered").as_deref(), Some("true"));
    // The use also sends a state snapshot; wait for the scheduled release itself.
    let released = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let state = wait_block(&mut bot, (6, 64, 6)).await;
            if prop(state, "powered").as_deref() == Some("false") {
                break state;
            }
        }
    })
    .await
    .expect("the button never released");
    assert_eq!(prop(released, "powered").as_deref(), Some("false"));
    drop(bot);
    world.shutdown().await.unwrap();
    server.abort();
    let _ = std::fs::remove_dir_all(temp);
}

/// Vanilla `SignBlock` editing and `BucketItem`/`Cauldron` interactions.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sign_and_bucket_interactions_reach_tracking_bot() {
    let Some((source, temp)) = fixture().await else {
        return;
    };
    flatten(&source);
    source.set_block(8, 64, 8, block("oak_sign", &[])).unwrap();
    source
        .set_block(
            9,
            64,
            0,
            u32::from(ferrite_data::blocks::default_state("water").unwrap()),
        )
        .unwrap();
    source
        .set_block(
            10,
            64,
            0,
            u32::from(ferrite_data::blocks::default_state("cauldron").unwrap()),
        )
        .unwrap();
    let world = World::start_with(Some(source.clone()), [0, 65, 0]);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(
        listener,
        ServerConfig {
            online_mode: false,
            view_distance: 2,
            game: Some(world.sender.clone()),
            ..Default::default()
        },
    ));
    let mut bot = Bot::join(addr, "Writer").await;
    bot.until(46).await;
    look_down_at(&mut bot, 8.5, 65.0, 8.5).await;
    use_at(&mut bot, 8, 64, 8, 1).await;
    bot.until(61).await;
    bot.send(&SignUpdate {
        location: BlockPos::new(8, 64, 8).pack(),
        is_front_text: true,
        lines: ["Hi".into(), "there".into(), "".into(), "".into()],
    })
    .await;
    let messages = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if let Some(entity) = source.block_entity(8, 64, 8)
                && let Some(messages) = super::blocks::sign_messages(&entity, true)
            {
                break messages;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("sign text was never stored");
    assert_eq!(messages, ["Hi", "there", "", ""]);
    let bucket = ferrite_data::registries::id("minecraft:item", "minecraft:bucket").unwrap();
    bot.send(&CreativeSlot {
        slot: 36,
        item: ItemStack {
            count: 1,
            item_id: bucket,
        },
    })
    .await;
    look_down_at(&mut bot, 9.5, 65.0, 0.5).await;
    use_at(&mut bot, 9, 64, 0, 2).await;
    assert_eq!(wait_block(&mut bot, (9, 64, 0)).await, air());
    let water_bucket =
        ferrite_data::registries::id("minecraft:item", "minecraft:water_bucket").unwrap();
    bot.send(&CreativeSlot {
        slot: 36,
        item: ItemStack {
            count: 1,
            item_id: water_bucket,
        },
    })
    .await;
    look_down_at(&mut bot, 10.5, 65.0, 0.5).await;
    use_at(&mut bot, 10, 64, 0, 3).await;
    let filled = wait_block(&mut bot, (10, 64, 0)).await;
    assert_eq!(
        ferrite_data::blocks::state(u16::try_from(filled).unwrap()).map(|(name, props)| (
            name,
            props.iter().find(|(k, _)| *k == "level").map(|(_, v)| *v)
        )),
        Some(("minecraft:water_cauldron", Some("3")))
    );
    drop(bot);
    world.shutdown().await.unwrap();
    server.abort();
    let _ = std::fs::remove_dir_all(temp);
}

/// Vanilla `BedBlock.explodeBed`: using a bed in the nether removes both halves and explodes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nether_bed_explodes_for_tracking_bot() {
    let source = AnvilChunkSource::new_memory();
    // Beds only explode where `bedWorks` is false, and each dimension owns its own chunk
    // source now, so the bed must live in the nether source and the player must spawn on it.
    let mut dimensions =
        super::dimension::DimensionRegistry::with_overworld(std::sync::Arc::clone(&source), [1, 65, 1]);
    std::sync::Arc::get_mut(&mut dimensions)
        .unwrap()
        .set_spawn(super::Dimension::Nether, [1, 65, 1]);
    let world = World::start_configured(
        Some(source.clone()),
        [1, 65, 1],
        super::WorldSettings {
            dimension: super::Dimension::Nether,
            dimensions: Some(dimensions),
            ..Default::default()
        },
    );
    let source = world.dimensions().source(super::Dimension::Nether).clone();
    loop {
        if let Some(result) = source.get(0, 0, 0) {
            result.unwrap();
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let stone = u32::from(ferrite_data::blocks::default_state("stone").unwrap());
    for x in 0..=3 {
        for z in 0..=3 {
            source.set_block(x, 63, z, stone).unwrap();
            for y in 64..=66 {
                source.set_block(x, y, z, air()).unwrap();
            }
        }
    }
    source
        .set_block(
            1,
            64,
            1,
            block("red_bed", &[("part", "foot"), ("facing", "north")]),
        )
        .unwrap();
    source
        .set_block(
            1,
            64,
            0,
            block("red_bed", &[("part", "head"), ("facing", "north")]),
        )
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(
        listener,
        ServerConfig {
            online_mode: false,
            view_distance: 2,
            game: Some(world.sender.clone()),
            ..Default::default()
        },
    ));
    let mut bot = Bot::join(addr, "Sleeper").await;
    bot.until(46).await;
    look_down_at(&mut bot, 1.5, 65.0, 1.5).await;
    use_at(&mut bot, 1, 64, 1, 1).await;
    assert_eq!(wait_block(&mut bot, (1, 64, 1)).await, air());
    assert_eq!(source.block(1, 64, 0), Some(air()));
    drop(bot);
    world.shutdown().await.unwrap();
    server.abort();
}
