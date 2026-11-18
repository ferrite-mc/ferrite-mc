//! Bot-session tests for the block interactions: they drive a real connection through
//! login/configuration and assert the `block_update` packets a tracking client receives.

use super::World;
use super::anvil::AnvilChunkSource;
use super::integration::Bot;
use ferrite_net::{ServerConfig, serve};
use ferrite_protocol::{
    Decode, Packet, VarInt,
    packets::play::{
        BlockUpdate, ContainerSlot, CreativeSlot, GameEvent, ItemStack, MovePosition, MoveRotation,
        SignUpdate, UseItemOn,
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
    let mut dimensions = super::dimension::DimensionRegistry::with_source(
        super::Dimension::Overworld,
        std::sync::Arc::clone(&source),
        [1, 65, 1],
    );
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

fn item_id(name: &str) -> i32 {
    ferrite_data::registries::id("minecraft:item", name).unwrap()
}

async fn memory_world_for(
    gamemode: i32,
    op_name: &str,
) -> (
    Arc<AnvilChunkSource>,
    World,
    std::net::SocketAddr,
    tokio::task::JoinHandle<anyhow::Result<()>>,
) {
    let source = AnvilChunkSource::new_memory();
    let world = World::start_configured(
        Some(source.clone()),
        [0, 65, 0],
        super::WorldSettings {
            gamemode,
            ops: vec![super::Operator {
                uuid: ferrite_net::offline_uuid(op_name),
                level: 4,
            }],
            ..Default::default()
        },
    );
    loop {
        if let Some(result) = source.get(0, 0, 0) {
            result.unwrap();
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let stone = block("stone", &[]);
    for x in 0..=8 {
        for z in 0..=2 {
            source.set_block(x, 63, z, stone).unwrap();
            for y in 64..=66 {
                source.set_block(x, y, z, air()).unwrap();
            }
        }
    }
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
    (source, world, addr, server)
}

async fn wait_block_and_slot(
    bot: &mut Bot,
    at: (i32, i32, i32),
    expected_block: u32,
    slot_index: i16,
    expected_item: i32,
    expected_count: i32,
) {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        let mut saw_block = false;
        let mut saw_slot = false;
        while !(saw_block && saw_slot) {
            let (id, body) = bot.next_play().await;
            if id == BlockUpdate::ID {
                let update = BlockUpdate::decode(&mut body.as_slice()).unwrap();
                let pos = BlockPos::unpack(update.location);
                if (pos.x, pos.y, pos.z) == at && update.block_state.0 as u32 == expected_block {
                    saw_block = true;
                }
            } else if id == ContainerSlot::<ferrite_item::ItemStack>::ID {
                let slot =
                    ContainerSlot::<ferrite_item::ItemStack>::decode(&mut body.as_slice()).unwrap();
                if slot.window == VarInt(0)
                    && slot.slot == slot_index
                    && i32::from(slot.item.item) == expected_item
                    && slot.item.count == expected_count
                {
                    saw_slot = true;
                }
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!("timed out waiting for block {expected_block} at {at:?} and slot {slot_index}")
    });
}

async fn wait_slot(bot: &mut Bot, slot_index: i16, expected_item: i32, expected_count: i32) {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let body = bot
                .until(ContainerSlot::<ferrite_item::ItemStack>::ID)
                .await;
            let slot =
                ContainerSlot::<ferrite_item::ItemStack>::decode(&mut body.as_slice()).unwrap();
            if slot.window == VarInt(0)
                && slot.slot == slot_index
                && i32::from(slot.item.item) == expected_item
                && slot.item.count == expected_count
            {
                break;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for slot {slot_index} item {expected_item}"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn powder_snow_bucket_placement_and_pickup_in_survival() {
    let (source, world, addr, server) = memory_world_for(0, "SnowBot").await;
    source
        .set_block(2, 64, 0, block("short_grass", &[]))
        .unwrap();

    let mut bot = Bot::join(addr, "SnowBot").await;
    bot.until(46).await;

    let powder_bucket = item_id("minecraft:powder_snow_bucket");
    let empty_bucket = item_id("minecraft:bucket");
    let powder_snow = block("powder_snow", &[]);

    world
        .console_command("give SnowBot powder_snow_bucket 1".into())
        .await
        .unwrap();
    wait_slot(&mut bot, 36, powder_bucket, 1).await;

    // Clicking a non-replaceable stone face places powder snow adjacent and returns an empty bucket.
    look_down_at(&mut bot, 1.5, 65.0, 0.5).await;
    use_at(&mut bot, 1, 63, 0, 1).await;
    wait_block_and_slot(&mut bot, (1, 64, 0), powder_snow, 36, empty_bucket, 1).await;

    // Empty bucket picks up the placed powder snow and returns a powder snow bucket.
    use_at(&mut bot, 1, 64, 0, 2).await;
    wait_block_and_slot(&mut bot, (1, 64, 0), air(), 36, powder_bucket, 1).await;

    // Clicking a replaceable block replaces it directly instead of placing above it.
    look_down_at(&mut bot, 2.5, 65.0, 0.5).await;
    use_at(&mut bot, 2, 64, 0, 3).await;
    wait_block_and_slot(&mut bot, (2, 64, 0), powder_snow, 36, empty_bucket, 1).await;
    assert_eq!(source.block(2, 65, 0), Some(air()));

    // Picking up powder snow with stacked empty buckets consumes one bucket and adds the filled bucket.
    world
        .console_command("give SnowBot bucket 1".into())
        .await
        .unwrap();
    wait_slot(&mut bot, 36, empty_bucket, 2).await;
    use_at(&mut bot, 2, 64, 0, 4).await;
    wait_block_and_slot(&mut bot, (2, 64, 0), air(), 36, empty_bucket, 1).await;
    wait_slot(&mut bot, 37, powder_bucket, 1).await;

    drop(bot);
    world.shutdown().await.unwrap();
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn powder_snow_bucket_creative_keeps_bucket_and_respects_blocked_target() {
    let (source, world, addr, server) = memory_world_for(1, "CreativeSnow").await;
    source.set_block(1, 64, 0, block("stone", &[])).unwrap();

    let mut bot = Bot::join(addr, "CreativeSnow").await;
    bot.until(46).await;

    let powder_bucket = item_id("minecraft:powder_snow_bucket");
    let powder_snow = block("powder_snow", &[]);
    bot.send(&CreativeSlot {
        slot: 36,
        item: ItemStack {
            count: 1,
            item_id: powder_bucket,
        },
    })
    .await;

    // Blocked target: stone at (1, 64, 0) is not replaceable, so clicking (1, 63, 0) does not place.
    look_down_at(&mut bot, 1.5, 65.0, 0.5).await;
    use_at(&mut bot, 1, 63, 0, 1).await;

    // Open target: places powder snow at (2, 64, 0) and creative keeps the powder snow bucket.
    look_down_at(&mut bot, 2.5, 65.0, 0.5).await;
    use_at(&mut bot, 2, 63, 0, 2).await;
    assert_eq!(wait_block(&mut bot, (2, 64, 0)).await, powder_snow);
    assert_eq!(source.block(1, 64, 0), Some(block("stone", &[])));

    bot.raw(7, "clear @s powder_snow_bucket 0").await;
    bot.chat_containing("Found 1 matching item").await;
    bot.raw(7, "clear @s bucket 0").await;
    bot.chat_containing("No items were found").await;

    drop(bot);
    world.shutdown().await.unwrap();
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn powder_snow_cauldron_interactions_with_bot() {
    let (source, world, addr, server) = memory_world_for(0, "CauldronBot").await;
    let empty_cauldron = block("cauldron", &[]);
    let partial_powder = block("powder_snow_cauldron", &[("level", "2")]);
    let full_powder = block("powder_snow_cauldron", &[("level", "3")]);
    let full_water = block("water_cauldron", &[("level", "3")]);

    source.set_block(1, 64, 0, empty_cauldron).unwrap();
    source.set_block(2, 64, 0, partial_powder).unwrap();
    source.set_block(3, 64, 0, full_powder).unwrap();
    source.set_block(4, 64, 0, empty_cauldron).unwrap();
    source.set_block(5, 64, 0, full_water).unwrap();

    let mut bot = Bot::join(addr, "CauldronBot").await;
    bot.until(46).await;

    let powder_bucket = item_id("minecraft:powder_snow_bucket");
    let empty_bucket = item_id("minecraft:bucket");
    let water_bucket = item_id("minecraft:water_bucket");

    world
        .console_command("give CauldronBot powder_snow_bucket 1".into())
        .await
        .unwrap();
    wait_slot(&mut bot, 36, powder_bucket, 1).await;

    // Powder snow bucket fills an empty cauldron to level=3 and returns an empty bucket.
    look_down_at(&mut bot, 1.5, 65.0, 0.5).await;
    use_at(&mut bot, 1, 64, 0, 1).await;
    wait_block_and_slot(&mut bot, (1, 64, 0), full_powder, 36, empty_bucket, 1).await;

    // Empty bucket on a partial powder snow cauldron (level=2) leaves both cauldron and bucket unchanged.
    look_down_at(&mut bot, 2.5, 65.0, 0.5).await;
    use_at(&mut bot, 2, 64, 0, 2).await;

    // Empty bucket on a full powder snow cauldron (level=3) restores empty cauldron and gives powder_snow_bucket.
    look_down_at(&mut bot, 3.5, 65.0, 0.5).await;
    use_at(&mut bot, 3, 64, 0, 3).await;
    wait_block_and_slot(&mut bot, (3, 64, 0), empty_cauldron, 36, powder_bucket, 1).await;
    assert_eq!(source.block(2, 64, 0), Some(partial_powder));

    // Existing water cauldron fill and empty behavior stays intact.
    world
        .console_command("give CauldronBot water_bucket 1".into())
        .await
        .unwrap();
    wait_slot(&mut bot, 37, water_bucket, 1).await;
    bot.raw(
        7,
        "item replace entity @s hotbar.0 with minecraft:water_bucket 1",
    )
    .await;
    wait_slot(&mut bot, 36, water_bucket, 1).await;
    look_down_at(&mut bot, 4.5, 65.0, 0.5).await;
    use_at(&mut bot, 4, 64, 0, 4).await;
    wait_block_and_slot(&mut bot, (4, 64, 0), full_water, 36, empty_bucket, 1).await;

    look_down_at(&mut bot, 5.5, 65.0, 0.5).await;
    use_at(&mut bot, 5, 64, 0, 5).await;
    wait_block_and_slot(&mut bot, (5, 64, 0), empty_cauldron, 36, water_bucket, 1).await;

    // Creative mode keeps the powder snow bucket when filling a cauldron.
    bot.raw(7, "gamemode creative").await;
    loop {
        let body = bot.until(GameEvent::ID).await;
        let event = GameEvent::decode(&mut body.as_slice()).unwrap();
        if event.event == 3 && event.value == 1.0 {
            break;
        }
    }
    bot.send(&CreativeSlot {
        slot: 36,
        item: ItemStack {
            count: 1,
            item_id: powder_bucket,
        },
    })
    .await;
    look_down_at(&mut bot, 3.5, 65.0, 0.5).await;
    use_at(&mut bot, 3, 64, 0, 6).await;
    assert_eq!(wait_block(&mut bot, (3, 64, 0)).await, full_powder);
    bot.raw(7, "clear @s powder_snow_bucket 0").await;
    bot.chat_containing("Found 1 matching item").await;

    // Creative mode empties a full powder snow cauldron and adds powder_snow_bucket to inventory.
    bot.send(&CreativeSlot {
        slot: 36,
        item: ItemStack {
            count: 1,
            item_id: empty_bucket,
        },
    })
    .await;
    bot.raw(7, "clear @s powder_snow_bucket 0").await;
    bot.chat_containing("No items were found").await;
    use_at(&mut bot, 3, 64, 0, 7).await;
    assert_eq!(wait_block(&mut bot, (3, 64, 0)).await, empty_cauldron);
    bot.raw(7, "clear @s powder_snow_bucket 0").await;
    bot.chat_containing("Found 1 matching item").await;

    drop(bot);
    world.shutdown().await.unwrap();
    server.abort();
}

/// Waterlogging slabs and fences with a water bucket and picking up source water with an empty
/// bucket, in both creative and survival modes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn water_and_empty_buckets_waterlog_slab_and_fence() {
    use super::integration::{
        inventory_of, join_with_inventory, op_world, plain, set_mode, wait_for_block_ack,
    };

    let source = AnvilChunkSource::new_memory();
    let (world, addr, server) = op_world(Some(source.clone()), 1, &["Waterlogger"]).await;
    loop {
        if let Some(result) = source.get(0, 0, 0) {
            result.unwrap();
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let stone = u32::from(ferrite_data::blocks::default_state("stone").unwrap());
    for x in 0..=11 {
        for z in 0..=3 {
            source.set_block(x, 63, z, stone).unwrap();
            for y in 64..=66 {
                source.set_block(x, y, z, air()).unwrap();
            }
        }
    }

    let dry_slab = block("stone_slab", &[("type", "top"), ("waterlogged", "false")]);
    let wet_slab = block("stone_slab", &[("type", "top"), ("waterlogged", "true")]);
    let dry_fence = block(
        "oak_fence",
        &[
            ("east", "true"),
            ("north", "false"),
            ("south", "false"),
            ("west", "false"),
            ("waterlogged", "false"),
        ],
    );
    let wet_fence = block(
        "oak_fence",
        &[
            ("east", "true"),
            ("north", "false"),
            ("south", "false"),
            ("west", "false"),
            ("waterlogged", "true"),
        ],
    );
    let double_slab = block(
        "stone_slab",
        &[("type", "double"), ("waterlogged", "false")],
    );
    let flowing_water = block("water", &[("level", "1")]);

    source.set_block(1, 64, 1, dry_slab).unwrap();
    source.set_block(3, 64, 1, dry_fence).unwrap();
    source.set_block(5, 64, 1, double_slab).unwrap();
    source.set_block(7, 64, 1, flowing_water).unwrap();
    // Neighbor block to keep flowing water from being a single-cell test distraction.
    source.set_block(7, 64, 2, stone).unwrap();
    source.set_block(9, 64, 1, wet_slab).unwrap();

    let mut bot = join_with_inventory(addr, "Waterlogger").await;
    bot.until(46).await;

    // 1. Creative water bucket on dry top slab: waterlogs slab, keeps water_bucket, no water above.
    bot.send(&CreativeSlot {
        slot: 36,
        item: plain("water_bucket", 1),
    })
    .await;
    look_down_at(&mut bot, 1.5, 65.0, 1.5).await;
    use_at(&mut bot, 1, 64, 1, 1).await;
    assert_eq!(wait_block(&mut bot, (1, 64, 1)).await, wet_slab);
    wait_for_block_ack(&mut bot, 1).await;
    assert_eq!(source.block(1, 65, 1), Some(air()));
    assert_eq!(inventory_of(&mut bot).await[36], plain("water_bucket", 1));

    // 2. Survival empty bucket on waterlogged slab: picks up water, preserves type=top, gives water_bucket.
    bot.send(&CreativeSlot {
        slot: 36,
        item: plain("bucket", 1),
    })
    .await;
    set_mode(&mut bot, "survival", 0).await;
    use_at(&mut bot, 1, 64, 1, 2).await;
    assert_eq!(wait_block(&mut bot, (1, 64, 1)).await, dry_slab);
    wait_for_block_ack(&mut bot, 2).await;
    assert_eq!(inventory_of(&mut bot).await[36], plain("water_bucket", 1));

    // 3. Survival water bucket on dry slab: waterlogs slab, gives empty bucket, no water above.
    use_at(&mut bot, 1, 64, 1, 3).await;
    assert_eq!(wait_block(&mut bot, (1, 64, 1)).await, wet_slab);
    wait_for_block_ack(&mut bot, 3).await;
    assert_eq!(source.block(1, 65, 1), Some(air()));
    assert_eq!(inventory_of(&mut bot).await[36], plain("bucket", 1));

    // 4. Survival water bucket on already-waterlogged slab: places water on clicked face instead.
    set_mode(&mut bot, "creative", 1).await;
    bot.send(&CreativeSlot {
        slot: 36,
        item: plain("water_bucket", 1),
    })
    .await;
    set_mode(&mut bot, "survival", 0).await;
    use_at(&mut bot, 1, 64, 1, 4).await;
    let placed_above = wait_block(&mut bot, (1, 65, 1)).await;
    assert_eq!(
        placed_above,
        u32::from(ferrite_data::blocks::default_state("water").unwrap())
    );
    wait_for_block_ack(&mut bot, 4).await;
    assert_eq!(source.block(1, 64, 1), Some(wet_slab));
    assert_eq!(inventory_of(&mut bot).await[36], plain("bucket", 1));

    // 5. Survival water bucket on dry fence: preserves east=true, gives empty bucket, no water above.
    set_mode(&mut bot, "creative", 1).await;
    bot.send(&CreativeSlot {
        slot: 36,
        item: plain("water_bucket", 1),
    })
    .await;
    set_mode(&mut bot, "survival", 0).await;
    look_down_at(&mut bot, 3.5, 65.5, 1.5).await;
    use_at(&mut bot, 3, 64, 1, 5).await;
    assert_eq!(wait_block(&mut bot, (3, 64, 1)).await, wet_fence);
    wait_for_block_ack(&mut bot, 5).await;
    assert_eq!(source.block(3, 65, 1), Some(air()));
    assert_eq!(inventory_of(&mut bot).await[36], plain("bucket", 1));

    // 6. Survival empty bucket (stack of 2) on waterlogged fence: decrements stack and adds water_bucket.
    set_mode(&mut bot, "creative", 1).await;
    bot.send(&CreativeSlot {
        slot: 36,
        item: plain("bucket", 2),
    })
    .await;
    set_mode(&mut bot, "survival", 0).await;
    use_at(&mut bot, 3, 64, 1, 6).await;
    assert_eq!(wait_block(&mut bot, (3, 64, 1)).await, dry_fence);
    wait_for_block_ack(&mut bot, 6).await;
    let inv = inventory_of(&mut bot).await;
    assert_eq!(inv[36], plain("bucket", 1));
    assert!(inv.iter().any(|s| *s == plain("water_bucket", 1)));

    // 7. Water bucket on double slab does not waterlog it; places water above instead.
    set_mode(&mut bot, "creative", 1).await;
    bot.send(&CreativeSlot {
        slot: 36,
        item: plain("water_bucket", 1),
    })
    .await;
    look_down_at(&mut bot, 5.5, 65.0, 1.5).await;
    use_at(&mut bot, 5, 64, 1, 7).await;
    assert_eq!(
        wait_block(&mut bot, (5, 65, 1)).await,
        u32::from(ferrite_data::blocks::default_state("water").unwrap())
    );
    wait_for_block_ack(&mut bot, 7).await;
    assert_eq!(source.block(5, 64, 1), Some(double_slab));

    // 8. Empty bucket on flowing water must not pick it up.
    bot.send(&CreativeSlot {
        slot: 36,
        item: plain("bucket", 1),
    })
    .await;
    set_mode(&mut bot, "survival", 0).await;
    look_down_at(&mut bot, 7.5, 65.0, 1.5).await;
    use_at(&mut bot, 7, 64, 1, 8).await;
    wait_for_block_ack(&mut bot, 8).await;
    assert_eq!(source.block(7, 64, 1), Some(flowing_water));
    assert_eq!(inventory_of(&mut bot).await[36], plain("bucket", 1));

    // 9. Creative empty bucket on waterlogged slab: clears waterlogged, keeps bucket, gives water_bucket.
    set_mode(&mut bot, "creative", 1).await;
    // Remove any remaining water_bucket from earlier steps across hotbar/main slots.
    for (i, stack) in inventory_of(&mut bot).await.iter().enumerate() {
        if *stack == plain("water_bucket", 1) {
            bot.send(&CreativeSlot {
                slot: i as i16,
                item: ItemStack {
                    count: 0,
                    item_id: 0,
                },
            })
            .await;
        }
    }
    bot.send(&CreativeSlot {
        slot: 36,
        item: plain("bucket", 1),
    })
    .await;
    look_down_at(&mut bot, 9.5, 65.0, 1.5).await;
    use_at(&mut bot, 9, 64, 1, 9).await;
    assert_eq!(wait_block(&mut bot, (9, 64, 1)).await, dry_slab);
    wait_for_block_ack(&mut bot, 9).await;
    let inv = inventory_of(&mut bot).await;
    assert_eq!(inv[36], plain("bucket", 1));
    assert!(inv.iter().any(|s| *s == plain("water_bucket", 1)));

    drop(bot);
    world.shutdown().await.unwrap();
    server.abort();
}
