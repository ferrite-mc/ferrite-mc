//! Vanilla block interactions without items: `DoorBlock`, `TrapDoorBlock`, `FenceGateBlock`,
//! `LeverBlock`, `ButtonBlock`, `BedBlock` and `SignBlock`, plus bucket/`Cauldron` item use.
//!
//! These run inside the region that owns the player's chunk; every write goes through the
//! player's `change_block`, so tracking clients see the usual `block_update` and the region's
//! neighbor-update pass sees the position.

use super::region::Player;
use ferrite_data::{blocks, tags};
use ferrite_item::ItemStack;
use ferrite_net::game::packet;
use ferrite_protocol::{
    packets::play::{OpenSignEditor, SignUpdate},
    types::BlockPos,
};
use std::collections::HashSet;

fn name(state: u16) -> Option<&'static str> {
    blocks::state(state).map(|(name, _)| name)
}

fn short(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

pub fn property(state: u16, key: &str) -> Option<&'static str> {
    blocks::state(state)?
        .1
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| *value)
}

/// A copy of `state` with the named properties replaced.
pub fn with_props(state: u16, changes: &[(&str, &str)]) -> Option<u16> {
    let (name, props) = blocks::state(state)?;
    let mut properties = props.to_vec();
    for (key, value) in changes {
        match properties.iter_mut().find(|(name, _)| name == key) {
            Some((_, old)) => *old = value,
            None => properties.push((key, value)),
        }
    }
    blocks::state_id(name, &properties)
}

fn state_at(player: &Player, pos: [i32; 3]) -> Option<u16> {
    player
        .source
        .as_ref()?
        .block(pos[0], pos[1], pos[2])
        .and_then(|state| u16::try_from(state).ok())
}

fn change(player: &mut Player, pos: [i32; 3], state: u16) -> bool {
    let Some(current) = player
        .source
        .as_ref()
        .and_then(|source| source.block(pos[0], pos[1], pos[2]))
    else {
        return false;
    };
    if current == u32::from(state) {
        return true;
    }
    let location = BlockPos::new(pos[0], pos[1], pos[2]).pack();
    let changed = player.change_block(location, Some(current), u32::from(state));
    if changed {
        player.changed_blocks.push(pos);
    }
    changed
}

fn is_button(state: u16) -> bool {
    name(state).is_some_and(|name| tags::contains("block", "buttons", name))
}

fn is_sign(state: u16) -> bool {
    name(state).is_some_and(|name| {
        tags::contains("block", "standing_signs", name)
            || tags::contains("block", "wall_signs", name)
    })
}

fn is_bed(state: u16) -> bool {
    name(state).is_some_and(|name| tags::contains("block", "beds", name))
}

/// Vanilla `ButtonBlock.getPressDuration`: stone buttons 20 ticks, the rest 30.
fn press_duration(state: u16) -> u32 {
    match name(state).map(short) {
        Some("stone_button" | "polished_blackstone_button") => 20,
        _ => 30,
    }
}

/// Vanilla `DoorBlock`/`TrapDoorBlock`/`FenceGateBlock` open state.
fn toggle_open(player: &mut Player, pos: [i32; 3], state: u16) -> bool {
    let open = property(state, "open") == Some("true");
    let Some(next) = with_props(state, &[("open", if open { "false" } else { "true" })]) else {
        return false;
    };
    // A door's other half mirrors the click; the upper half needs the same block below.
    let (dx, dy, dz) = match property(state, "half") {
        Some("upper") => (0, -1, 0),
        Some("lower") => (0, 1, 0),
        _ => (0, 0, 0),
    };
    let other = [pos[0] + dx, pos[1] + dy, pos[2] + dz];
    let mirrors = dy != 0
        && state_at(player, other)
            .and_then(name)
            .zip(name(state))
            .is_some_and(|(left, right)| left == right);
    let changed = change(player, pos, next);
    if changed
        && mirrors
        && let Some(other_state) = state_at(player, other)
        && let Some(other_next) = with_props(
            other_state,
            &[("open", if open { "false" } else { "true" })],
        )
    {
        change(player, other, other_next);
    }
    true
}

fn toggle_powered(player: &mut Player, pos: [i32; 3], state: u16) -> bool {
    let powered = property(state, "powered") == Some("true");
    if let Some(next) = with_props(
        state,
        &[("powered", if powered { "false" } else { "true" })],
    ) {
        change(player, pos, next);
    }
    true
}

/// Vanilla `BedBlock.useWithoutItem` and `BedBlock.explodeBed`.
fn use_bed(player: &mut Player, pos: [i32; 3], state: u16) -> bool {
    if !is_bed(state) {
        return false;
    }
    let facing = property(state, "facing").unwrap_or("north");
    // The foot's facing points at the head (vanilla `BedBlock.getConnectedDirection`).
    let (dx, dz) = match facing {
        "north" => (0, -1),
        "south" => (0, 1),
        "west" => (-1, 0),
        _ => (1, 0),
    };
    let head = if property(state, "part") == Some("head") {
        pos
    } else {
        [pos[0] + dx, pos[1], pos[2] + dz]
    };
    let foot = if head == pos {
        [pos[0] - dx, pos[1], pos[2] - dz]
    } else {
        pos
    };
    if !player.dimension.bed_works() {
        // Vanilla `BedBlock.explodeBed`: both halves vanish, then a power-5 fire explosion.
        let air = blocks::default_state("air").unwrap_or(0);
        change(player, head, air);
        change(player, foot, air);
        player.explosions.push((
            [
                f64::from(head[0]) + 0.5,
                f64::from(head[1]) + 0.5,
                f64::from(head[2]) + 0.5,
            ],
            5.0,
            true,
        ));
        return true;
    }
    if property(state, "occupied") == Some("true") {
        return true;
    }
    for part in [head, foot] {
        if let Some(part_state) = state_at(player, part)
            && let Some(next) = with_props(part_state, &[("occupied", "true")])
        {
            change(player, part, next);
        }
    }
    player.sleeping = Some(foot);
    crate::plugins::facts::set_sleeping(player.uuid, true);
    player.spawn = Some(foot);
    true
}

/// Vanilla `SignBlock.useWithoutItem`: open the sign editor for an empty hand or in creative.
fn use_sign(player: &mut Player, pos: [i32; 3], state: u16) -> bool {
    if !is_sign(state) || (player.gamemode != 1 && !player.inventory.selected_item().is_empty()) {
        return false;
    }
    player.editing_sign = Some((pos, true));
    let _ = player.outbound.send(packet(&OpenSignEditor {
        location: BlockPos::new(pos[0], pos[1], pos[2]).pack(),
        is_front_text: true,
    }));
    true
}

/// `Player#openSign`: opens the editor for the sign at `pos` on the given side. `false` when
/// the block there is not a sign.
pub fn open_sign_editor(player: &mut Player, pos: [i32; 3], front: bool) -> bool {
    match state_at(player, pos) {
        Some(state) if is_sign(state) => {}
        _ => return false,
    }
    player.editing_sign = Some((pos, front));
    let _ = player.outbound.send(packet(&OpenSignEditor {
        location: BlockPos::new(pos[0], pos[1], pos[2]).pack(),
        is_front_text: front,
    }));
    true
}

fn set_held(player: &mut Player, item: &str) {
    if let Some(stack) = ItemStack::from_name(item) {
        let slot = player.inventory.selected_slot();
        player.inventory.set(slot, stack);
    }
}

/// Vanilla `BottleItem` and the water `CauldronInteraction`: a glass bottle fills from a water
/// cauldron, which drops one level and empties at the last one.
fn fill_bottle(player: &mut Player, pos: [i32; 3], state: u16) -> bool {
    if player.inventory.selected_item().name() != Some("minecraft:glass_bottle") {
        return false;
    }
    if name(state) != Some("minecraft:water_cauldron") {
        return false;
    }
    let next = match property(state, "level").unwrap_or("1") {
        "1" => blocks::default_state("cauldron"),
        "2" => blocks::state_id("water_cauldron", &[("level", "1")]),
        _ => blocks::state_id("water_cauldron", &[("level", "2")]),
    };
    let Some(next) = next else {
        return false;
    };
    if !change(player, pos, next) {
        return false;
    }
    let mut bottle = ItemStack::from_name("minecraft:potion").expect("vanilla potion");
    bottle.components.insert(
        ferrite_item::component_ids::POTION_CONTENTS,
        ferrite_item::Component::PotionContents(ferrite_item::PotionContents {
            potion: Some("minecraft:water".into()),
            ..Default::default()
        }),
    );
    let slot = player.inventory.selected_slot();
    player.inventory.set(slot, bottle);
    true
}

/// Uses up one of the held item outside creative mode.
fn consume_held(player: &mut Player) {
    if player.gamemode == 1 {
        return;
    }
    let slot = player.inventory.selected_slot();
    player.inventory.remove(slot, 1);
}

/// Wears the held tool by one point outside creative mode; it breaks at its last point.
fn wear_held(player: &mut Player) {
    if player.gamemode == 1 {
        return;
    }
    let slot = player.inventory.selected_slot();
    let stack = player.inventory.selected_item().clone();
    let Some(max) = ferrite_item::item_defaults(stack.item).and_then(|d| d.max_damage) else {
        return;
    };
    let worn = stack.damage() + 1;
    if worn >= max {
        player.inventory.remove(slot, 1);
    } else if let Some(stack) = super::region::worn_stack(stack, worn) {
        player.inventory.set(slot, stack);
    }
}

/// Gives the player an item, dropping it into the inventory or replacing a held stack of one.
fn give(player: &mut Player, item: &str) {
    let Some(mut stack) = ItemStack::from_name(item) else {
        return;
    };
    player.inventory.add(&mut stack, player.gamemode == 1);
}

/// A copy of `state` as the block `to`, keeping the properties both blocks have.
fn as_block(state: u16, to: &str) -> Option<u16> {
    let (_, props) = blocks::state(state)?;
    blocks::state_id(to, props)
}

/// Vanilla `AxeItem` and `HoneycombItem`: honeycomb waxes copper, an axe scrapes the wax and the
/// oxidation off and strips logs. Returns whether anything changed.
fn wax_or_scrape(player: &mut Player, pos: [i32; 3], state: u16, block: &str) -> bool {
    let held = player.inventory.selected_item().name();
    let short_name = short(block);
    let axe = held.is_some_and(|item| item.ends_with("_axe"));
    let next = if held == Some("minecraft:honeycomb") {
        if short_name.starts_with("waxed_") {
            return false;
        }
        as_block(state, &format!("waxed_{short_name}"))
    } else if axe {
        if let Some(plain) = short_name.strip_prefix("waxed_") {
            as_block(state, plain)
        } else if let Some(previous) = scraped(short_name) {
            as_block(state, &previous)
        } else if (short_name.ends_with("_log")
            || short_name.ends_with("_wood")
            || short_name.ends_with("_stem")
            || short_name.ends_with("_hyphae")
            || short_name == "bamboo_block")
            && !short_name.starts_with("stripped_")
        {
            as_block(state, &format!("stripped_{short_name}"))
        } else {
            None
        }
    } else {
        None
    };
    let Some(next) = next else { return false };
    if next == state || !change(player, pos, next) {
        return false;
    }
    if axe {
        wear_held(player);
    } else {
        consume_held(player);
    }
    true
}

/// The block one oxidation stage below `short_name` (`weathered_cut_copper` -> `exposed_cut_copper`).
fn scraped(short_name: &str) -> Option<String> {
    let (stage, rest) = ["oxidized_", "weathered_", "exposed_"]
        .into_iter()
        .find_map(|p| short_name.strip_prefix(p).map(|rest| (p, rest)))?;
    let previous = match stage {
        "oxidized_" => format!("weathered_{rest}"),
        "weathered_" => format!("exposed_{rest}"),
        _ if rest == "copper" => "copper_block".to_owned(),
        _ => rest.to_owned(),
    };
    blocks::default_state(&previous).map(|_| previous)
}

/// Vanilla `SignApplicator` for glow ink: the front text starts to glow.
fn glow_sign(player: &mut Player, pos: [i32; 3], state: u16) -> bool {
    if !is_sign(state) || player.inventory.selected_item().name() != Some("minecraft:glow_ink_sac")
    {
        return false;
    }
    let Some(source) = player.source.clone() else {
        return false;
    };
    let Some(type_id) =
        ferrite_data::registries::id("minecraft:block_entity_type", "minecraft:sign")
            .and_then(|id| u32::try_from(id).ok())
    else {
        return false;
    };
    let result = source.update_block_entity(pos[0], pos[1], pos[2], |entity| {
        let entity = entity.get_or_insert_with(|| {
            let mut data = ferrite_nbt::Compound::new();
            data.insert(
                "id".into(),
                ferrite_nbt::Tag::String("minecraft:sign".into()),
            );
            data.insert("x".into(), ferrite_nbt::Tag::Int(pos[0]));
            data.insert("y".into(), ferrite_nbt::Tag::Int(pos[1]));
            data.insert("z".into(), ferrite_nbt::Tag::Int(pos[2]));
            ferrite_world::BlockEntity { type_id, data }
        });
        let mut text = match entity.data.get("front_text") {
            Some(ferrite_nbt::Tag::Compound(text)) => text.clone(),
            _ => {
                let mut text = ferrite_nbt::Compound::new();
                text.insert(
                    "messages".into(),
                    ferrite_nbt::Tag::List(ferrite_nbt::List::String(vec![String::new(); 4])),
                );
                text.insert("color".into(), ferrite_nbt::Tag::String("black".into()));
                text
            }
        };
        if matches!(
            text.get("has_glowing_text"),
            Some(ferrite_nbt::Tag::Byte(1))
        ) {
            return (false, false);
        }
        text.insert("has_glowing_text".into(), ferrite_nbt::Tag::Byte(1));
        entity
            .data
            .insert("front_text".into(), ferrite_nbt::Tag::Compound(text));
        (true, true)
    });
    match result {
        Ok(true) => {
            consume_held(player);
            player.sync_block_entity(pos);
            true
        }
        _ => false,
    }
}

/// Vanilla `RespawnAnchorBlock.useItemOn`: glowstone adds a charge, up to four.
fn charge_anchor(player: &mut Player, pos: [i32; 3], state: u16, block: &str) -> bool {
    if block != "minecraft:respawn_anchor"
        || player.inventory.selected_item().name() != Some("minecraft:glowstone")
    {
        return false;
    }
    let charges: u8 = property(state, "charges")
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    if charges >= 4 {
        return false;
    }
    let Some(next) = with_props(state, &[("charges", &(charges + 1).to_string())]) else {
        return false;
    };
    if !change(player, pos, next) {
        return false;
    }
    consume_held(player);
    true
}

/// Vanilla `CompassItem.useOn`: a compass used on a lodestone is bound to it.
fn bind_compass(player: &mut Player, pos: [i32; 3], block: &str) -> bool {
    let held = player.inventory.selected_item().clone();
    if block != "minecraft:lodestone" || held.name() != Some("minecraft:compass") {
        return false;
    }
    let tracker = ferrite_item::Component::LodestoneTracker(ferrite_item::LodestoneTracker {
        target: Some((player.dimension.key().to_owned(), pos)),
        tracked: true,
    });
    let Some(id) = tracker.type_id() else {
        return false;
    };
    let mut bound = held.copy_with_count(1);
    bound.components.insert(id, tracker);
    if held.count == 1 && player.gamemode != 1 {
        let slot = player.inventory.selected_slot();
        player.inventory.set(slot, bound);
    } else {
        consume_held(player);
        player.inventory.add(&mut bound, player.gamemode == 1);
    }
    true
}

/// Vanilla `JukeboxBlock`: a music disc goes in and plays; an empty hand takes it out again.
fn use_jukebox(player: &mut Player, pos: [i32; 3], state: u16, block: &str) -> bool {
    if block != "minecraft:jukebox" {
        return false;
    }
    let Some(source) = player.source.clone() else {
        return false;
    };
    if property(state, "has_record") == Some("true") {
        let Some(disc) = super::jukebox::take(&source, pos) else {
            return false;
        };
        let Some(next) = with_props(state, &[("has_record", "false")]) else {
            return false;
        };
        if !change(player, pos, next) {
            return false;
        }
        player.menus.spilled.push((pos, disc));
        broadcast_level_event(player, pos, 1011, 0);
        return true;
    }
    let held = player.inventory.selected_item().clone();
    let Some(song) = held
        .name()
        .and_then(|name| name.strip_prefix("minecraft:music_disc_"))
        .and_then(ferrite_item::wire::registry_ids::jukebox_song_id)
    else {
        return false;
    };
    let Some(next) = with_props(state, &[("has_record", "true")]) else {
        return false;
    };
    if !super::jukebox::insert(&source, pos, &held.copy_with_count(1)) || !change(player, pos, next)
    {
        return false;
    }
    consume_held(player);
    broadcast_level_event(player, pos, 1010, song);
    true
}

/// Vanilla `VaultBlock.useItemOn`: the matching trial key is used up and the vault starts
/// unlocking its rolled rewards once per player. Only a started unlock consumes the key; an
/// inactive, busy or already-rewarded vault leaves the held stack untouched.
fn use_vault(player: &mut Player, pos: [i32; 3], state: u16, block: &str) -> bool {
    if block != "minecraft:vault" {
        return false;
    }
    let Some(source) = player.source.clone() else {
        return false;
    };
    let Some(held) = player.inventory.selected_item().name() else {
        return false;
    };
    let ominous = property(state, "ominous") == Some("true");
    let outcome = super::vault::try_unlock(&source, pos, player.uuid, Some(held), ominous);
    if !super::vault::unlock_consumes_key(outcome) {
        return false;
    }
    consume_held(player);
    // The region tick owns the later transitions; this makes the unlock visible immediately.
    if let Some(next) = with_props(state, &[("vault_state", "unlocking")]) {
        change(player, pos, next);
    }
    true
}

/// Sends a world event (sounds, particles) to everyone tracking the chunk.
fn broadcast_level_event(player: &Player, pos: [i32; 3], event: i32, data: i32) {
    let update = packet(&ferrite_protocol::packets::play::LevelEvent {
        event,
        location: BlockPos::new(pos[0], pos[1], pos[2]).pack(),
        data,
        global: false,
    });
    super::region::broadcast_tracked_chunk(
        &player.tracking.lock().unwrap(),
        player.dimension,
        (pos[0].div_euclid(16), pos[2].div_euclid(16)),
        &update,
    );
}

/// Whether a lit campfire sits up to five blocks below (`CampfireBlock.isSmokeyPos`).
fn smokey(player: &Player, pos: [i32; 3]) -> bool {
    (1..=5).any(|d| {
        state_at(player, [pos[0], pos[1] - d, pos[2]]).is_some_and(|state| {
            tags::contains("block", "campfires", name(state).unwrap_or(""))
                && property(state, "lit") == Some("true")
        })
    })
}

/// Vanilla `BeehiveBlock.useItemOn`: a full hive gives a honey bottle for a glass bottle or
/// three honeycombs for shears. There are no bees to anger.
fn harvest_honey(player: &mut Player, pos: [i32; 3], state: u16, block: &str) -> bool {
    if !tags::contains("block", "beehives", block) || property(state, "honey_level") != Some("5") {
        return false;
    }
    let reward = match player.inventory.selected_item().name() {
        Some("minecraft:glass_bottle") => ("minecraft:honey_bottle", 1),
        Some("minecraft:shears") => ("minecraft:honeycomb", 3),
        _ => return false,
    };
    let Some(next) = with_props(state, &[("honey_level", "0")]) else {
        return false;
    };
    if !change(player, pos, next) {
        return false;
    }
    if reward.0 == "minecraft:honey_bottle" {
        consume_held(player);
    } else {
        wear_held(player);
    }
    for _ in 0..reward.1 {
        give(player, reward.0);
    }
    true
}

/// Remembers a successful use of the held item on a block for the `item_used_on_block`
/// advancements. `item` is what was held before the use.
pub fn record_use(player: &Player, location: i64, item: Option<&'static str>) {
    let pos = BlockPos::unpack(location);
    let pos = [pos.x, pos.y, pos.z];
    let Some((block, props)) = state_at(player, pos).and_then(blocks::state) else {
        return;
    };
    let biome = player
        .source
        .as_ref()
        .and_then(|source| source.biome_name(pos[0], pos[1], pos[2]));
    super::recipe_unlock::USED_ON_BLOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(super::recipe_unlock::UsedRecord {
            player: player.uuid,
            block,
            props,
            item,
            biome,
            smokey: smokey(player, pos),
        });
}

/// Vanilla `BucketItem.useOn` and the `CauldronInteraction` water/lava maps.
fn use_bucket(player: &mut Player, pos: [i32; 3], state: u16) -> bool {
    let held = player.inventory.selected_item().clone();
    let Some(held_name) = held.name() else {
        return false;
    };
    let Some(target_name) = name(state) else {
        return false;
    };
    let target_name = short(target_name);
    match (held_name, target_name) {
        ("minecraft:water_bucket", "cauldron") => {
            if let Some(next) = blocks::state_id("water_cauldron", &[("level", "3")])
                && change(player, pos, next)
            {
                set_held(player, "minecraft:bucket");
            }
            true
        }
        ("minecraft:lava_bucket", "cauldron") => {
            if let Some(next) = blocks::default_state("lava_cauldron")
                && change(player, pos, next)
            {
                set_held(player, "minecraft:bucket");
            }
            true
        }
        ("minecraft:water_bucket", "water_cauldron") => {
            if let Some(next) = blocks::state_id("water_cauldron", &[("level", "3")])
                && change(player, pos, next)
            {
                set_held(player, "minecraft:bucket");
            }
            true
        }
        ("minecraft:bucket", "water_cauldron") => {
            if property(state, "level") == Some("3") {
                if let Some(next) = blocks::default_state("cauldron") {
                    change(player, pos, next);
                }
                set_held(player, "minecraft:water_bucket");
            }
            true
        }
        ("minecraft:bucket", "lava_cauldron") => {
            if let Some(next) = blocks::default_state("cauldron") {
                change(player, pos, next);
            }
            set_held(player, "minecraft:lava_bucket");
            true
        }
        ("minecraft:bucket", "water") => {
            if property(state, "level") == Some("0") {
                if let Some(air) = blocks::default_state("air") {
                    change(player, pos, air);
                }
                set_held(player, "minecraft:water_bucket");
            }
            true
        }
        ("minecraft:bucket", "lava") => {
            if property(state, "level") == Some("0") {
                if let Some(air) = blocks::default_state("air") {
                    change(player, pos, air);
                }
                set_held(player, "minecraft:lava_bucket");
            }
            true
        }
        _ => false,
    }
}

/// Vanilla `useWithoutItem` / `useItemOn` for the blocks this module owns.
pub fn use_block(player: &mut Player, location: i64) -> bool {
    let pos = BlockPos::unpack(location);
    let pos = [pos.x, pos.y, pos.z];
    let Some(state) = state_at(player, pos) else {
        return false;
    };
    let Some(name) = name(state) else {
        return false;
    };
    if is_button(state) {
        if property(state, "powered") == Some("true") {
            return true;
        }
        if let Some(next) = with_props(state, &[("powered", "true")])
            && change(player, pos, next)
        {
            player.button_press.push((pos, press_duration(state)));
        }
        return true;
    }
    if name == "minecraft:lever" {
        return toggle_powered(player, pos, state);
    }
    // Doors and trapdoors: wooden ones only (iron variants need redstone power).
    let short_name = short(name);
    if tags::contains("block", "wooden_doors", name)
        || short_name.ends_with("_door") && short_name != "iron_door"
    {
        return toggle_open(player, pos, state);
    }
    if tags::contains("block", "wooden_trapdoors", name)
        || short_name.ends_with("_trapdoor") && short_name != "iron_trapdoor"
    {
        return toggle_open(player, pos, state);
    }
    if short(name).ends_with("_fence_gate") {
        return toggle_open(player, pos, state);
    }
    if is_bed(state) {
        return use_bed(player, pos, state);
    }
    if glow_sign(player, pos, state) {
        return true;
    }
    if is_sign(state) {
        return use_sign(player, pos, state);
    }
    if wax_or_scrape(player, pos, state, name)
        || charge_anchor(player, pos, state, name)
        || bind_compass(player, pos, name)
        || use_jukebox(player, pos, state, name)
        || use_vault(player, pos, state, name)
        || harvest_honey(player, pos, state, name)
    {
        return true;
    }
    if fill_bottle(player, pos, state) {
        return true;
    }
    if use_bucket(player, pos, state) {
        return true;
    }
    false
}

/// Vanilla `ServerGamePacketListenerImpl.handleSignUpdate`: writes `front_text`/`back_text`
/// into the sign block entity.
pub fn edit_sign(player: &mut Player, update: &SignUpdate) {
    let pos = BlockPos::unpack(update.location);
    let pos = [pos.x, pos.y, pos.z];
    if player
        .editing_sign
        .is_none_or(|(expected, front)| expected != pos || front != update.is_front_text)
    {
        return;
    }
    let Some(state) = state_at(player, pos) else {
        return;
    };
    if !is_sign(state) {
        return;
    }
    if (f64::from(pos[0]) + 0.5 - player.pos[0]).abs() > 8.0
        || (f64::from(pos[1]) + 0.5 - player.pos[1]).abs() > 8.0
        || (f64::from(pos[2]) + 0.5 - player.pos[2]).abs() > 8.0
    {
        return;
    }
    let Some(source) = player.source.clone() else {
        return;
    };
    let Some(type_id) =
        ferrite_data::registries::id("minecraft:block_entity_type", "minecraft:sign")
            .and_then(|id| u32::try_from(id).ok())
    else {
        return;
    };
    let messages = update.lines.to_vec();
    let side = if update.is_front_text {
        "front_text"
    } else {
        "back_text"
    };
    let mut text = ferrite_nbt::Compound::new();
    text.insert(
        "messages".into(),
        ferrite_nbt::Tag::List(ferrite_nbt::List::String(messages)),
    );
    text.insert("color".into(), ferrite_nbt::Tag::String("black".into()));
    text.insert("has_glowing_text".into(), ferrite_nbt::Tag::Byte(0));
    let result = source.update_block_entity(pos[0], pos[1], pos[2], |entity| {
        let entity = entity.get_or_insert_with(|| {
            let mut data = ferrite_nbt::Compound::new();
            data.insert(
                "id".into(),
                ferrite_nbt::Tag::String("minecraft:sign".into()),
            );
            data.insert("x".into(), ferrite_nbt::Tag::Int(pos[0]));
            data.insert("y".into(), ferrite_nbt::Tag::Int(pos[1]));
            data.insert("z".into(), ferrite_nbt::Tag::Int(pos[2]));
            ferrite_world::BlockEntity { type_id, data }
        });
        entity
            .data
            .insert(side.into(), ferrite_nbt::Tag::Compound(text));
        (true, ())
    });
    if let Err(error) = result {
        tracing::warn!(%error, "could not store sign text");
    }
    player.editing_sign = None;
}

/// Vanilla `ServerPlayer.stopSleeping`: the occupied bed halves turn off.
pub fn wake(player: &mut Player) {
    let Some(pos) = player.sleeping.take() else {
        return;
    };
    crate::plugins::facts::set_sleeping(player.uuid, false);
    let Some(state) = state_at(player, pos) else {
        return;
    };
    if let Some(next) = with_props(state, &[("occupied", "false")]) {
        change(player, pos, next);
    }
    if let Some(facing) = property(state, "facing") {
        let (dx, dz) = match facing {
            "north" => (0, -1),
            "south" => (0, 1),
            "west" => (-1, 0),
            _ => (1, 0),
        };
        let other = [pos[0] + dx, pos[1], pos[2] + dz];
        if let Some(state) = state_at(player, other)
            && let Some(next) = with_props(state, &[("occupied", "false")])
        {
            change(player, other, next);
        }
    }
}

/// Vanilla `ButtonBlock`'s scheduled release. Buttons in a region that splits follow their
/// block; a release whose block is no longer a powered button is dropped.
#[derive(Debug, Default)]
pub struct ButtonTicks {
    clock: u32,
    due: Vec<([i32; 3], u32)>,
    scheduled: HashSet<[i32; 3]>,
}

impl ButtonTicks {
    pub fn schedule(&mut self, pos: [i32; 3], delay: u32) {
        if self.scheduled.insert(pos) {
            self.due.push((pos, self.clock.wrapping_add(delay.max(1))));
        }
    }

    pub fn split(&mut self, keep: impl Fn(i32, i32) -> bool) -> Self {
        let mut out = Self {
            clock: self.clock,
            due: Vec::new(),
            scheduled: HashSet::new(),
        };
        let mut moved = Vec::new();
        self.due.retain(|&(pos, due)| {
            if keep(pos[0], pos[2]) {
                moved.push((pos, due));
                false
            } else {
                true
            }
        });
        for (pos, due) in moved {
            self.scheduled.remove(&pos);
            out.scheduled.insert(pos);
            out.due.push((pos, due));
        }
        out
    }

    pub fn merge(&mut self, other: Self) {
        for (pos, due) in other.due {
            if self.scheduled.insert(pos) {
                self.due.push((pos, due));
            }
        }
    }

    pub fn tick(
        &mut self,
        world: &mut impl super::neighbors::NeighborWorld,
        changed: &mut Vec<[i32; 3]>,
    ) {
        self.clock = self.clock.wrapping_add(1);
        let mut index = 0;
        while index < self.due.len() {
            if self.due[index].1 > self.clock {
                index += 1;
                continue;
            }
            let (pos, _) = self.due.swap_remove(index);
            self.scheduled.remove(&pos);
            if !world.owns(pos[0], pos[2]) {
                continue;
            }
            let Some(state) = world.block(pos[0], pos[1], pos[2]) else {
                continue;
            };
            if !is_button(state) || property(state, "powered") != Some("true") {
                continue;
            }
            if let Some(next) = with_props(state, &[("powered", "false")]) {
                world.set_block(pos[0], pos[1], pos[2], next);
                changed.push(pos);
            }
        }
    }
}

#[cfg(test)]
/// A sign's `front_text.messages`, for the bot test that reloads the block entity.
#[cfg(test)]
pub fn sign_messages(entity: &ferrite_world::BlockEntity, front: bool) -> Option<Vec<String>> {
    let side = if front { "front_text" } else { "back_text" };
    let ferrite_nbt::Tag::Compound(text) = entity.data.get(side)? else {
        return None;
    };
    let ferrite_nbt::Tag::List(ferrite_nbt::List::String(lines)) = text.get("messages")? else {
        return None;
    };
    Some(lines.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copper_scrapes_one_stage_and_waxes_keeping_its_state() {
        assert_eq!(
            scraped("weathered_cut_copper").as_deref(),
            Some("exposed_cut_copper")
        );
        assert_eq!(
            scraped("oxidized_copper_door").as_deref(),
            Some("weathered_copper_door")
        );
        assert_eq!(scraped("exposed_copper").as_deref(), Some("copper_block"));
        assert_eq!(scraped("copper_block"), None);
        assert_eq!(scraped("stone"), None);
        let lit = blocks::state_id("exposed_copper_bulb", &[("lit", "true")]).unwrap();
        let waxed = as_block(lit, "waxed_exposed_copper_bulb").unwrap();
        assert_eq!(name(waxed), Some("minecraft:waxed_exposed_copper_bulb"));
        assert_eq!(property(waxed, "lit"), Some("true"));
        assert!(as_block(lit, "waxed_stone").is_none());
    }

    #[test]
    fn props_round_trip_and_preserve_the_other_values() {
        let door = blocks::state_id(
            "oak_door",
            &[("half", "upper"), ("facing", "east"), ("open", "false")],
        )
        .unwrap();
        let opened = with_props(door, &[("open", "true")]).unwrap();
        assert_eq!(property(opened, "open"), Some("true"));
        assert_eq!(property(opened, "half"), Some("upper"));
        assert_eq!(property(opened, "facing"), Some("east"));
        assert!(is_button(blocks::default_state("oak_button").unwrap()));
        assert!(!is_button(blocks::default_state("oak_planks").unwrap()));
    }
}
