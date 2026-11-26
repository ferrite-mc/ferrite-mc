use super::{
    PluginEventPolicy,
    chunk::{ChunkSource, FlatGenerator},
    commands::{XpOperation, XpUnit},
    fx::{FxMap, FxSet},
    survival::{
        DamageKind, EAT_TICKS, Eating, FoodTick, MAX_FOOD, MAX_HEALTH, Survival, SurvivalRules,
    },
    valid_move,
};
use crate::game::plugin_types::InteractAction;
use ferrite_entity::{
    BlockGetter, EntityRecord, EntityStore, Flags, Rng, ai, arrow, bobber, chunk_of, combat,
    effect::{ActiveEffects, Effect, EffectEvent, MobEffectInstance},
    experience, item,
    mob::{self, MobKind},
    potion::PotionContents,
    shoulder::{ShoulderParrot, ShoulderSide},
    spawn::{self, SpawnEnv},
    tracker::{self, Viewer, ViewerKey},
};
use ferrite_net::game::{Outbound, OutboundSender, ServerboundPlay, packet};
use ferrite_profiler::Timings;
use ferrite_protocol::{Encode, Packet, VarInt, packets::play::*};
use ferrite_region::{ChunkPos, RegionData, SectionPos};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

pub(super) fn abilities_for_mode(mode: i32) -> PlayerAbilities {
    match mode {
        1 => PlayerAbilities {
            flags: 0x01 | 0x04 | 0x08,
            flying_speed: 0.05,
            walking_speed: 0.1,
        },
        3 => PlayerAbilities {
            flags: 0x01 | 0x02 | 0x04,
            flying_speed: 0.05,
            walking_speed: 0.1,
        },
        _ => PlayerAbilities {
            flags: 0,
            flying_speed: 0.05,
            walking_speed: 0.1,
        },
    }
}

#[cfg(test)]
type DelayedBlockUpdate = (i64, (i32, i32), [i32; 3], u32);

#[cfg(test)]
fn queue_delayed_piston_update(
    updates: &mut Vec<DelayedBlockUpdate>,
    due_tick: i64,
    pos: [i32; 3],
    state: u32,
) {
    updates.retain(|(_, _, queued_pos, _)| *queued_pos != pos);
    updates.push((
        due_tick,
        (pos[0].div_euclid(16), pos[2].div_euclid(16)),
        pos,
        state,
    ));
}

fn delayed_piston_update_matches_current(state: u32, current: Option<u16>) -> bool {
    current.is_some_and(|current| u32::from(current) == state)
}

fn piston_entity_collision_box(cell: [i32; 3], extending: bool) -> ferrite_math::AABB {
    let lift = if extending { 0.0 } else { 0.05 };
    ferrite_math::AABB::new(
        f64::from(cell[0]),
        f64::from(cell[1]),
        f64::from(cell[2]),
        f64::from(cell[0] + 1),
        f64::from(cell[1] + 1) + lift,
        f64::from(cell[2] + 1),
    )
}

/// Build the overlay packets for one moving block. The block update and block entity share the
/// destination position; the entity is optional only if the data tables are unavailable.
fn moving_piston_packets(
    block: &super::piston::MovingBlock,
) -> Option<(BlockUpdate, Option<BlockEntityData>)> {
    let state = block.moving_state()?;
    let location = ferrite_protocol::types::BlockPos::new(
        block.destination[0],
        block.destination[1],
        block.destination[2],
    )
    .pack();
    let update = BlockUpdate {
        location,
        block_state: VarInt(state as i32),
    };
    let entity = block.entity_nbt().and_then(|nbt| {
        let type_id =
            ferrite_data::registries::id("minecraft:block_entity_type", "minecraft:piston")?;
        Some(BlockEntityData {
            location,
            block_type: VarInt(type_id as i32),
            nbt,
        })
    });
    Some((update, entity))
}

/// Pick the live overlays a player needs after a chunk batch. A player who starts tracking a
/// destination chunk during the two-tick animation missed the original broadcast.
fn moving_piston_replay(
    moving: &[super::piston::MovingBlock],
    sent: &[(i32, i32)],
) -> Vec<(BlockUpdate, Option<BlockEntityData>)> {
    moving
        .iter()
        .filter(|block| {
            sent.contains(&(
                block.destination[0].div_euclid(16),
                block.destination[2].div_euclid(16),
            ))
        })
        .filter_map(moving_piston_packets)
        .collect()
}

/// How `/item` writes stacks into a player's slots (see `slot_ops::distribute`).
#[derive(Debug, Clone)]
pub enum SlotFill {
    /// `replace`/`modify`: slot `i` takes `stacks[i]`; slots past the end are left alone.
    Set(Vec<ferrite_item::ItemStack>),
    /// `override`: like `Set`, but slots past the end of the list are cleared.
    Override(Vec<ferrite_item::ItemStack>),
    /// `fill`: the stacks are cycled over every slot.
    Cycle(Vec<ferrite_item::ItemStack>),
}

pub enum Command {
    /// `Player#breakBlock`: break the block under the player's own rules and answer the
    /// waiting plugin call.
    PluginBreakBlock {
        position: [i32; 3],
    },
    /// A plugin put the player to bed; the verdict answers the waiting plugin call.
    PluginSleep {
        position: [i32; 3],
        force: bool,
    },
    Input(ServerboundPlay),
    Leave,
    SetMode(i32),
    /// `Player.Spigot#respawn`: respawn the player if they are dead.
    Respawn,
    /// A client-only experience update a plugin asked for.
    PluginVisualExperience {
        progress: f32,
        level: Option<i32>,
    },
    PluginGameEvent {
        event: u8,
        value: f32,
    },
    /// A plugin's `Player#openInventory` for a chest-style inventory.
    PluginOpenInventory {
        title: String,
        rows: u8,
        shulker: bool,
        items: Vec<Option<crate::game::plugin_types::ItemSnapshot>>,
    },
    /// A plugin's change to one of this player's shoulders.
    PluginShoulder {
        left: bool,
        op: crate::game::plugin_types::ShoulderOp,
    },
    /// A packet a plugin sends to this player.
    PluginPacket(crate::game::plugin_types::PluginPacket),
    /// A plugin's direct change to a survival number.
    PluginSurvival {
        stat: crate::game::plugin_types::SurvivalStat,
        value: f32,
    },
    /// A plugin's item cooldown for one group.
    PluginCooldown {
        group: String,
        ticks: i32,
    },
    /// A plugin's glowing, invisibility, fire, silence, gravity or freeze change.
    PluginEntityData {
        flags: Option<u8>,
        silent: Option<bool>,
        no_gravity: Option<bool>,
        freeze_ticks: Option<i32>,
    },
    /// A plugin's `Player#setVelocity` or `knockback`.
    PluginVelocity {
        velocity: [f64; 3],
    },
    /// A plugin's `Player#saveData`.
    PluginSaveData,
    /// A plugin's `Player#loadData`: the saved inventory, game mode and (in the same world) position.
    PluginLoadData,
    /// A plugin's bobber cast.
    PluginCastBobber {
        entity: Option<Uuid>,
    },
    /// A plugin's `Player#fireworkBoost`: a rocket attached to the gliding player.
    PluginFireworkBoost {
        flight: u8,
        explosions: u8,
        entity: Option<Uuid>,
    },
    /// A plugin's `Player#openWorkbench` and the other block-less crafting stations.
    PluginOpenStation {
        station: String,
    },
    /// A plugin's `Player#closeInventory`.
    PluginCloseInventory,
    /// Generic damage from a plugin.
    PluginDamage {
        amount: f32,
    },
    /// Plugin-chosen abilities; the game mode still decides instant build and creative flight.
    PluginAbilities {
        allow_flight: bool,
        flying: bool,
        invulnerable: bool,
        fly_speed: f32,
        walk_speed: f32,
    },
    SetViewDistance(i32),
    /// An animation of this player's own entity, shown to everyone tracking its chunk.
    Animate {
        action: u8,
    },
    /// The attribute command changed the player's base max health.
    SetMaxHealth(f32),
    Teleport {
        pos: [f64; 3],
        yaw: Option<f32>,
        pitch: Option<f32>,
    },
    /// An ender pearl landed in another region and the world loop routed it back to its owner.
    EnderPearlTeleport {
        dimension: super::Dimension,
        pos: [f64; 3],
    },
    /// A melee hit generated by a mob owned by a different region.
    MobMelee {
        dimension: super::Dimension,
        damage: f32,
        from: [f64; 3],
        attacker: Uuid,
        knockback_xz: f64,
        launch_y: f64,
        poison_ticks: i32,
    },
    /// Move to another dimension with the `Respawn` + position sequence.
    ChangeDimension {
        dimension: super::Dimension,
        pos: [f64; 3],
        yaw: f32,
        pitch: f32,
    },
    /// Travel to a (possibly runtime) world. The sender resolves the target source, so the
    /// owning region never has to look anything up in shared world state.
    ChangeWorld {
        world: super::WorldId,
        dimension: super::Dimension,
        source: Arc<super::anvil::AnvilChunkSource>,
        pos: [f64; 3],
        yaw: f32,
        pitch: f32,
    },
    World(super::commands_world::WorldCommand),
    Clear {
        target: Uuid,
        target_name: String,
        item: Option<u16>,
        max_count: Option<i32>,
        feedback: Option<OutboundSender>,
    },
    Give {
        target: Uuid,
        target_name: String,
        item: u16,
        count: i32,
        item_name: String,
        components: Vec<(u16, ferrite_nbt::Tag)>,
        feedback: Option<OutboundSender>,
    },
    Experience {
        target: Uuid,
        target_name: String,
        operation: XpOperation,
        amount: i32,
        unit: XpUnit,
        feedback: Option<OutboundSender>,
    },
    /// A plugin's `PlayerInventory#setItem`, applied on the owning region thread.
    SetInventorySlot {
        slot: u8,
        item: Option<crate::game::plugin_types::ItemSnapshot>,
    },
    /// A plugin sends the colours and visible cursors of a map.
    MapData {
        id: i32,
        scale: u8,
        locked: bool,
        colors: Vec<u8>,
        decorations: Vec<crate::game::plugin_types::MapDecoration>,
    },
    /// A plugin shows a written book to the player.
    OpenBook {
        title: String,
        author: String,
        pages: Vec<String>,
    },
    /// A plugin asked for a chunk to be sent again.
    RefreshChunk {
        x: i32,
        z: i32,
    },
    /// A plugin started (0), completed (1) or cleared (2) the use of an item.
    ItemUse {
        action: u8,
        hand: u8,
    },
    /// A plugin wrote item metadata onto the stack in an inventory slot.
    ItemMeta {
        slot: u8,
        name: Option<String>,
        lore: Vec<String>,
        enchants: Vec<(String, i32)>,
        unbreakable: bool,
        model: Option<i32>,
        repair: i32,
        flags: Vec<String>,
        custom_data: Option<ferrite_nbt::Compound>,
        book: Option<crate::game::plugin_types::BookFields>,
        extra: Option<ferrite_nbt::Compound>,
    },
    /// A plugin pointed the filled map in a slot at a saved map.
    ItemMapId {
        slot: u8,
        id: i32,
    },
    /// A plugin wore the stack in an inventory slot down to `damage`.
    ItemDamage {
        slot: u8,
        damage: i32,
    },
    /// A plugin started a riptide spin (`Player#startRiptideAttack`).
    RiptideAttack {
        ticks: u8,
        damage: f32,
    },
    /// `/item` on the player's own inventory: writes the stacks into the slots (`-1` is the
    /// selected hotbar slot).
    ItemSlots {
        slots: Vec<i16>,
        fill: SlotFill,
    },
    /// `/item ... from entity` and `/loot`: sends back the stacks of the slots (`-1` is the
    /// selected hotbar slot).
    ReadSlots {
        slots: Vec<i16>,
        reply: std::sync::mpsc::Sender<Vec<ferrite_item::ItemStack>>,
    },
    /// Publishes the names of the held items to [`super::recipe_unlock::HELD`].
    ScanInventory,
    /// A plugin's `Player#setHealth`, applied on the owning region thread.
    SetHealth {
        health: f32,
    },
    /// A plugin's `Player#setFoodLevel`, applied on the owning region thread.
    SetFood {
        food: u8,
    },
    /// A plugin's experience setters, applied on the owning region thread.
    SetExperience {
        level: Option<i32>,
        progress: Option<f32>,
        total: Option<i32>,
    },
    /// `/kill`: `damageSources().genericKill()` applied on the owning region thread.
    Kill,
    /// `/damage`: hurt the player on the owning region thread.
    Damage(DamageKind, f32),
    /// `/damage` with a damage source: like `Damage`, but a fatal hit announces `death_message`
    /// (built from the damage type and the attacker) instead of the cause's own text.
    DamageFrom {
        kind: DamageKind,
        amount: f32,
        exhaustion: f32,
        death_message: String,
    },
    /// A Paper `EntityScheduler` task for this player, run on the region that owns them.
    PluginEntityTask(crate::game::plugin_types::EntityTask),
    /// `/effect give`: apply a mob effect on the owning region thread.
    ApplyEffect {
        effect: i32,
        amplifier: i32,
        duration: i32,
        /// The world loop already sent `update_mob_effect` in command order.
        notify: bool,
    },
    /// `/effect clear`: remove one effect, or every effect when `effect` is `None`.
    RemoveEffect {
        effect: Option<i32>,
        /// The world loop already sent `remove_mob_effect` in command order.
        notify: bool,
    },
    Enchant {
        target: Uuid,
        target_name: String,
        enchantment: String,
        level: i32,
        feedback: Option<OutboundSender>,
    },
}
pub type Mailbox = Arc<Mutex<VecDeque<Command>>>;
pub type ChunkObservers = HashMap<Uuid, (super::WorldId, HashSet<(i32, i32)>, OutboundSender)>;
pub type Tracking = Arc<Mutex<ChunkObservers>>;

/// How many world ticks a leash holder stays usable after its last sighting. Moving an entity
/// between regions removes it from the shared snapshots for a tick or two, so the lead waits
/// out the hand-off instead of snapping.
const LEASH_HOLDER_GRACE: i64 = 20;

/// Where a lead's holder was last seen, which dimension that was, and the world tick it was
/// seen at.
#[derive(Debug, Clone, Copy)]
struct LeashHolder {
    pos: ferrite_math::Vec3,
    id: i32,
    dimension: super::Dimension,
    /// Login session for a player holder; `None` for an entity holder.
    session: Option<u64>,
    seen: i64,
}

/// The result of resolving one lead holder for this region.
enum LeashHolderLookup {
    /// The holder is in this dimension; `id` is known when a snapshot supplies it.
    Here {
        pos: ferrite_math::Vec3,
        id: Option<i32>,
        session: Option<u64>,
    },
    /// The holder is known to be in another dimension, which is never a local target.
    Elsewhere,
    /// No current owner reports the holder; the last sighting may cover a region hand-off.
    Gone,
}

/// Leash holders any region has seen. Folia keeps entity stores region-local, so a region
/// ticking a leashed mob resolves the holder here instead of reading another region's store.
static LEASH_HOLDERS: Mutex<BTreeMap<Uuid, LeashHolder>> = Mutex::new(BTreeMap::new());

/// Serializes tests that use the process-global leash table with a region tick, which prunes
/// entries against its own world clock. A concurrently running test world would otherwise
/// expire another test's last sighting.
#[cfg(test)]
static LEASH_TEST_TURN: Mutex<()> = Mutex::new(());

#[cfg(test)]
thread_local! {
    static LEASH_TURN_HELD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A reentrant turnstile: a leash test holds it across several lookups, and the mob tick it
/// calls takes it again on the same thread.
#[cfg(test)]
struct LeashTestTurn(Option<std::sync::MutexGuard<'static, ()>>);

#[cfg(test)]
impl Drop for LeashTestTurn {
    fn drop(&mut self) {
        if self.0.is_some() {
            LEASH_TURN_HELD.with(|held| held.set(false));
        }
    }
}

#[cfg(test)]
fn leash_test_turn() -> LeashTestTurn {
    if LEASH_TURN_HELD.with(|held| held.get()) {
        return LeashTestTurn(None);
    }
    let guard = LEASH_TEST_TURN
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    LEASH_TURN_HELD.with(|held| held.set(true));
    LeashTestTurn(Some(guard))
}

/// The stack left in the hand after a dye was used on a sheep: one fewer in survival, the same
/// stack in creative (`Player.interactOn` restores a creative player's stack).
fn stack_after_dye(stack: &ferrite_item::ItemStack, gamemode: i32) -> ferrite_item::ItemStack {
    if gamemode == 1 {
        stack.clone()
    } else {
        stack.copy_with_count(stack.count - 1)
    }
}

/// Vanilla `ItemUtils.createFilledResult`: a container is replaced by its filled result, or
/// decremented while the filled stack is given separately when the hand stack is larger.
/// Creative keeps the original stack and receives the filled stack as an extra.
fn filled_container_result(
    stack: &ferrite_item::ItemStack,
    gamemode: i32,
    filled: &ferrite_item::ItemStack,
) -> (ferrite_item::ItemStack, Option<ferrite_item::ItemStack>) {
    if gamemode == 1 {
        (stack.clone(), Some(filled.clone()))
    } else if stack.count <= 1 {
        (filled.clone(), None)
    } else {
        (stack.copy_with_count(stack.count - 1), Some(filled.clone()))
    }
}

/// Bowls are filled with mushroom stew (or a suspicious stew carrying the mooshroom's effects).
fn mooshroom_bowl_result(
    stack: &ferrite_item::ItemStack,
    gamemode: i32,
    stew: &ferrite_item::ItemStack,
) -> (ferrite_item::ItemStack, Option<ferrite_item::ItemStack>) {
    filled_container_result(stack, gamemode, stew)
}

pub(super) fn worn_stack(
    mut stack: ferrite_item::ItemStack,
    damage: i32,
) -> Option<ferrite_item::ItemStack> {
    let max = ferrite_item::item_defaults(stack.item).and_then(|d| d.max_damage)?;
    if stack.count <= 0 || max <= 0 {
        return None;
    }
    let component = ferrite_item::Component::Damage(damage.clamp(0, max - 1));
    let id = component.type_id()?;
    stack.components.insert(id, component);
    Some(stack)
}

/// A written book with plain-text pages, as the client reads it from a hand.
pub(super) fn written_book(
    title: &str,
    author: &str,
    pages: &[String],
) -> Option<ferrite_item::ItemStack> {
    use ferrite_protocol::{Encode, types::PlainText};
    let mut stack = ferrite_item::ItemStack::from_name("written_book")?;
    let mut raw = Vec::new();
    title.to_owned().encode(&mut raw);
    0u8.encode(&mut raw);
    author.to_owned().encode(&mut raw);
    VarInt(0).encode(&mut raw);
    VarInt(pages.len() as i32).encode(&mut raw);
    for page in pages {
        PlainText(page.clone()).encode(&mut raw);
        0u8.encode(&mut raw);
    }
    1u8.encode(&mut raw);
    stack.components.insert(
        ferrite_item::component_ids::WRITTEN_BOOK_CONTENT,
        ferrite_item::Component::Raw(raw),
    );
    Some(stack)
}

/// Points the filled map `stack` at saved map `id`; `None` for any other item.
fn with_map_id(mut stack: ferrite_item::ItemStack, id: i32) -> Option<ferrite_item::ItemStack> {
    if stack.name() != Some("minecraft:filled_map") {
        return None;
    }
    ferrite_item::set_map_id(&mut stack, id);
    Some(stack)
}

/// The metadata a plugin writes onto a stack (name and lore are text JSON).
pub(super) struct ItemMetaFields {
    pub name: Option<String>,
    pub lore: Vec<String>,
    pub enchants: Vec<(String, i32)>,
    pub unbreakable: bool,
    pub model: Option<i32>,
    pub repair: i32,
    pub flags: Vec<String>,
    pub custom_data: Option<ferrite_nbt::Compound>,
    pub book: Option<crate::game::plugin_types::BookFields>,
    pub extra: Option<ferrite_nbt::Compound>,
}

/// Maps Bukkit item flags to the components hidden by `minecraft:tooltip_display`.
fn tooltip_display(flags: &[String]) -> Option<ferrite_item::TooltipDisplay> {
    let mut display = ferrite_item::TooltipDisplay::default();
    for flag in flags {
        let id = match flag.as_str() {
            "HIDE_ENCHANTS" => ferrite_item::component_ids::ENCHANTMENTS,
            "HIDE_ATTRIBUTES" => ferrite_item::component_ids::ATTRIBUTE_MODIFIERS,
            "HIDE_UNBREAKABLE" => ferrite_item::component_ids::UNBREAKABLE,
            "HIDE_DESTROYS" => ferrite_item::component_ids::CAN_BREAK,
            "HIDE_PLACED_ON" => ferrite_item::component_ids::CAN_PLACE_ON,
            "HIDE_DYE" => ferrite_item::component_ids::DYED_COLOR,
            "HIDE_ARMOR_TRIM" => ferrite_item::component_ids::TRIM,
            "HIDE_STORED_ENCHANTS" => ferrite_item::component_ids::STORED_ENCHANTMENTS,
            "HIDE_ADDITIONAL_TOOLTIP" => {
                display.hide_tooltip = true;
                continue;
            }
            _ => return None,
        };
        display.hidden_components.push(id);
    }
    display.hidden_components.sort_unstable();
    display.hidden_components.dedup();
    (!flags.is_empty()).then_some(display)
}

/// [`with_meta`] for the metadata of a tile snapshot item (`Player#sendBlockUpdate`).
#[allow(dead_code)]
pub(super) fn with_meta_spec(
    stack: ferrite_item::ItemStack,
    meta: &crate::game::plugin_types::ItemMetaSpec,
) -> ferrite_item::ItemStack {
    with_meta(
        stack,
        &ItemMetaFields {
            name: meta.name.clone(),
            lore: meta.lore.clone(),
            enchants: meta.enchants.clone(),
            unbreakable: meta.unbreakable,
            model: meta.model,
            repair: meta.repair,
            flags: meta.flags.clone(),
            custom_data: meta.custom_data.clone(),
            book: meta.book.clone(),
            extra: meta.extra.clone(),
        },
    )
}

/// Replaces the name, lore, enchantments, unbreakable flag, model, repair cost, tooltip flags
/// and persistent data of `stack`; text that does not parse is left out.
pub(super) fn with_meta(
    mut stack: ferrite_item::ItemStack,
    meta: &ItemMetaFields,
) -> ferrite_item::ItemStack {
    use ferrite_item::{Component, component_ids as ids};
    let parse = |json: &String| ferrite_text::Component::from_json(json).ok();
    for id in [
        ids::CUSTOM_NAME,
        ids::LORE,
        ids::ENCHANTMENTS,
        ids::UNBREAKABLE,
        ids::CUSTOM_MODEL_DATA,
        ids::REPAIR_COST,
        ids::TOOLTIP_DISPLAY,
        ids::CUSTOM_DATA,
        ids::WRITABLE_BOOK_CONTENT,
        ids::WRITTEN_BOOK_CONTENT,
        ids::ITEM_NAME,
        ids::ENCHANTMENT_GLINT_OVERRIDE,
        ids::MAX_STACK_SIZE,
        ids::MAX_DAMAGE,
        ids::RARITY,
        ids::ITEM_MODEL,
        ids::TOOLTIP_STYLE,
        ids::USE_REMAINDER,
        ids::ATTRIBUTE_MODIFIERS,
        ids::POTION_CONTENTS,
        ids::BANNER_PATTERNS,
        ids::FIREWORKS,
        ids::PROFILE,
        ids::NOTE_BLOCK_SOUND,
    ] {
        stack.components.remove(id);
    }
    let item = stack.name().map(str::to_owned);
    let mut put = |component: Component| {
        if let Some(id) = component.type_id() {
            stack.components.insert(id, component);
        }
    };
    if let Some(name) = meta.name.as_ref().and_then(parse) {
        put(Component::CustomName(name));
    }
    if !meta.lore.is_empty() {
        put(Component::Lore(
            meta.lore.iter().filter_map(parse).collect(),
        ));
    }
    if !meta.enchants.is_empty() {
        let mut enchantments = ferrite_item::Enchantments::new();
        enchantments.levels.extend(meta.enchants.iter().cloned());
        put(Component::Enchantments(enchantments));
    }
    if meta.unbreakable {
        put(Component::Unbreakable);
    }
    if let Some(model) = meta.model {
        put(Component::CustomModelData(ferrite_item::CustomModelData {
            floats: vec![model as f32],
            ..Default::default()
        }));
    }
    if meta.repair > 0 {
        put(Component::RepairCost(meta.repair));
    }
    if let Some(display) = tooltip_display(&meta.flags) {
        put(Component::TooltipDisplay(display));
    }
    if let Some(data) = &meta.custom_data
        && !data.is_empty()
    {
        put(Component::CustomData(data.clone()));
    }
    if let Some(book) = &meta.book
        && let Some(content) = book_content(item.as_deref(), book)
    {
        put(content);
    }
    if let Some(extra) = &meta.extra {
        for (name, tag) in extra {
            let name = name.strip_prefix("minecraft:").unwrap_or(name.as_str());
            if let Some(id) = ferrite_item::component_id(name) {
                stack
                    .components
                    .insert(id, ferrite_item::Component::from_nbt(id, tag));
            }
        }
    }
    stack
}

/// The book component for a `BookMeta`, when `item` is the matching book item.
fn book_content(
    item: Option<&str>,
    book: &crate::game::plugin_types::BookFields,
) -> Option<ferrite_item::Component> {
    use ferrite_item::{Component, Filterable, WritableBookContent, WrittenBookContent};
    let plain = |raw: String| Filterable {
        raw,
        filtered: None,
    };
    match (item, book.written) {
        (Some("minecraft:written_book"), true) => {
            Some(Component::WrittenBookContent(WrittenBookContent {
                title: plain(book.title.clone().unwrap_or_default()),
                author: book.author.clone().unwrap_or_default(),
                generation: book.generation,
                pages: book
                    .pages
                    .iter()
                    .map(|page| {
                        ferrite_text::Component::from_json(page)
                            .ok()
                            .map(|text| Filterable {
                                raw: text,
                                filtered: None,
                            })
                    })
                    .collect::<Option<_>>()?,
                resolved: false,
            }))
        }
        (Some("minecraft:writable_book"), false) => {
            Some(Component::WritableBookContent(WritableBookContent {
                pages: book.pages.iter().cloned().map(plain).collect(),
            }))
        }
        _ => None,
    }
}

pub(super) fn broadcast_tracked_chunk(
    players: &ChunkObservers,
    world: impl Into<super::WorldId>,
    chunk: (i32, i32),
    update: &Outbound,
) {
    let world = world.into();
    for (player_world, loaded, outbound) in players.values() {
        if *player_world == world && loaded.contains(&chunk) {
            let _ = outbound.send(update.clone());
        }
    }
}

fn block_sound_center(at: [i32; 3]) -> ferrite_math::Vec3 {
    ferrite_math::Vec3::new(
        f64::from(at[0]) + 0.5,
        f64::from(at[1]) + 0.5,
        f64::from(at[2]) + 0.5,
    )
}

/// Vanilla `LevelEvent.PARTICLES_DESTROY_BLOCK` for a turtle egg the region is removing.
fn turtle_egg_break_particles(
    tracking: &Tracking,
    world: impl Into<super::WorldId>,
    at: [i32; 3],
    state: u16,
) {
    let update = packet(&LevelEvent {
        event: 2001,
        location: ferrite_protocol::types::BlockPos::new(at[0], at[1], at[2]).pack(),
        data: i32::from(state),
        global: false,
    });
    let chunk = (at[0].div_euclid(16), at[2].div_euclid(16));
    broadcast_tracked_chunk(&tracking.lock().unwrap(), world, chunk, &update);
}
pub enum Signal {
    /// A `/clone` between dimensions whose source half has run (see `CrossClone`).
    CrossClone(Box<super::commands_world::CrossClone>),
    /// The `move` half of a cross-dimension clone, after the destination was written.
    CrossCloneClear(Box<super::commands_world::CrossClone>),
    Transfer {
        to: ChunkPos,
        player: Box<Player>,
    },
    Hold {
        world: super::WorldId,
        chunk: ChunkPos,
        add: bool,
    },
    /// Several chunk holders changed together, usually from one streamed chunk packet batch.
    HoldBatch {
        world: super::WorldId,
        chunks: Vec<ChunkPos>,
        add: bool,
    },
    /// A pearl landed outside its owner's region; the world loop forwards this to the owner.
    EnderPearlTeleport {
        dimension: super::Dimension,
        player: Uuid,
        pos: [f64; 3],
    },
    /// Forward a cross-region melee hit to the player's owning region.
    MobMelee {
        dimension: super::Dimension,
        player: Uuid,
        damage: f32,
        from: [f64; 3],
        attacker: Uuid,
        knockback_xz: f64,
        launch_y: f64,
        poison_ticks: i32,
    },
    /// A shield in another region blocked a mob's melee hit: the mob's owner knocks it back.
    MobKnockback {
        world_id: super::WorldId,
        attacker: Uuid,
        /// Where the attacker stood when it swung, to find the region that owns it.
        at: [f64; 3],
        /// The blocking player's position, which the attacker is pushed away from.
        source: [f64; 3],
    },
    /// A player opened or broke a `#minecraft:guarded_by_piglins` block near another region.
    PiglinGuardAlert {
        world_id: super::WorldId,
        player: Uuid,
        pos: [f64; 3],
        require_los: bool,
        chunks: Vec<ChunkPos>,
    },
    FluidChange {
        dimension: super::Dimension,
        x: i32,
        y: i32,
        z: i32,
    },
    /// A fluid tick in another region placed a block in this region's chunk.
    FluidPlace {
        dimension: super::Dimension,
        x: i32,
        y: i32,
        z: i32,
        /// Destination state observed when the handoff was queued.
        expected: u32,
        state: u32,
    },
    /// A block changed next to a region boundary: the owner of `target` re-validates its own
    /// neighbours of `origin` (support rules, gravity), because the region that changed
    /// `origin` may not write them.
    NeighborUpdate {
        dimension: super::Dimension,
        origin: [i32; 3],
        target: [i32; 3],
    },
    /// Redstone blocks on the other side of a region boundary need a re-evaluation.
    RedstoneChange {
        dimension: super::Dimension,
        /// `(block to re-evaluate, block that changed)`.
        positions: Vec<super::redstone::Wake>,
    },
    FinishRelease {
        world: super::WorldId,
        chunk: ChunkPos,
        flush: bool,
    },
    FinishReleaseBatch {
        world: super::WorldId,
        chunks: Vec<ChunkPos>,
        flush: bool,
    },
    /// Synchronizes with the coordinator until `chunks` are held (`expect_held = true`) or
    /// released (`expect_held = false`) on their owning regions.
    SyncHolds {
        world: super::WorldId,
        chunks: Vec<ChunkPos>,
        expect_held: bool,
        complete: tokio::sync::oneshot::Sender<()>,
    },
    /// The player left its old region for another dimension; the world loop inserts it into
    /// the target dimension's region scheduler.
    ChangeDimension {
        player: Box<Player>,
    },
    Update {
        uuid: Uuid,
        pos: [f64; 3],
        yaw: f32,
        pitch: f32,
        ground: bool,
    },
    Leave {
        world: super::WorldId,
        uuid: Uuid,
        chunk: ChunkPos,
        held: Vec<ChunkPos>,
    },
    /// The entities a chunk held when its last holder released it, for the world loop to write
    /// to the chunk's sidecar before the chunk is unloaded.
    SaveEntities {
        world: super::WorldId,
        chunk: ChunkPos,
        entities: Vec<EntityRecord>,
    },
    SaveCachedEntities {
        world: super::WorldId,
        chunk: ChunkPos,
        entities: Arc<Vec<EntityRecord>>,
    },
    /// An entity walked into another region section; the main loop hands it to the region
    /// owning `to` through that region's task queue (Folia: only the owner touches it).
    EntityTransfer {
        world_id: super::WorldId,
        to: ChunkPos,
        entity: Box<EntityRecord>,
    },
    /// A player used a spawn egg; the target region owns creation of the new entity.
    Summon {
        world_id: super::WorldId,
        chunk: ChunkPos,
        kind: MobKind,
        pos: [f64; 3],
    },
    /// A stage-1 sapling triggered growth and needs the worldgen feature host off the region thread.
    GrowSapling {
        world: super::WorldId,
        dimension: super::Dimension,
        origin: [i32; 3],
        saplings: Vec<([i32; 3], u32)>,
        feature: &'static str,
        context: super::generator::TreeContext,
    },
    /// The worldgen feature host finished placing a tree; route back to the origin sapling's region.
    ApplyTree {
        world: super::WorldId,
        origin: [i32; 3],
        saplings: Vec<([i32; 3], u32)>,
        placed: Result<super::generator::Placed, String>,
    },
    /// Tree blocks belonging to a chunk owned by another region after split or border growth.
    TreePlaceChunk {
        world: super::WorldId,
        chunk: ChunkPos,
        blocks: Vec<([i32; 3], u32, u16)>,
        entities: Vec<([i32; 3], ferrite_nbt::Compound)>,
    },
}
/// A queued explosion: center, power, fire, block damage, player credit.
type PendingExplosion = ([f64; 3], f32, bool, bool, Option<Uuid>);

/// What a region tick needs from the world loop.
#[derive(Clone)]
pub struct TickContext {
    pub signals: UnboundedSender<Signal>,
    /// Every online player's sent chunks and connection; reaches players that left the region.
    pub tracking: Tracking,
    /// Online player positions, shared as snapshots across region boundaries.
    pub positions: Arc<Mutex<HashMap<Uuid, [f64; 3]>>>,
    /// World blocks, for entity physics.
    pub source: Option<Arc<super::anvil::AnvilChunkSource>>,
    /// The dimension this world runs, for bed and fire rules.
    pub dimension: super::Dimension,
    /// Identity of the independently ticking world.
    pub world_id: super::WorldId,
    /// Current weather, for fire and cauldron rules.
    #[allow(dead_code)] // the live weather is read from `state`; kept for the startup setting
    pub raining: bool,
    /// Live game rules (natural regeneration, keepInventory, death messages, ...).
    pub rules: Arc<Mutex<ferrite_command::gamerule::GameRules>>,
    /// World spawn, where a player without a bed respawns (`ServerPlayer.respawn`).
    pub spawn: [f64; 3],
    /// Command-visible entity snapshot this region publishes at the end of each tick.
    pub command_entities: Arc<Mutex<super::entity_index::EntityIndex>>,
    /// Server command state, used by command blocks (storage, scores, `/function`).
    pub state: Arc<Mutex<super::commands::GameState>>,
    /// Shared per-dimension wandering-trader roll; region-local copies would spawn duplicates.
    pub trader_spawner: Arc<Mutex<super::wandering_trader::SpawnerState>>,
    pub profiler: Option<Arc<super::profile::Profiler>>,
    /// The server's live command tree, for command blocks; `None` in tests that do not need it.
    pub tree: Option<Arc<SharedCommandTree>>,
    /// The plugin bridge as it is attached right now, so events fire from regions without
    /// players; `None` in tests that do not need it.
    pub hooks: Option<Arc<SharedPluginHooks>>,
}

/// Picks the bridge for an event about a non-player entity: the live shared handle first, else
/// the first player's own handle. Exactly one bridge is returned, so the event fires once.
pub(super) fn resolve_damage_hooks(
    shared: Option<&SharedPluginHooks>,
    mut players: impl Iterator<
        Item = (
            Arc<dyn crate::game::plugin_types::ServerHooks>,
            PluginEventPolicy,
        ),
    >,
) -> Option<(
    Arc<dyn crate::game::plugin_types::ServerHooks>,
    PluginEventPolicy,
)> {
    shared
        .and_then(SharedPluginHooks::load)
        .or_else(|| players.next())
}

/// The plugin hooks and the failure policy that goes with them. The bridge attaches after the
/// world loop starts, so regions read the current value when an event fires instead of
/// capturing it at startup.
#[derive(Default)]
pub struct SharedPluginHooks {
    current: Mutex<
        Option<(
            Arc<dyn crate::game::plugin_types::ServerHooks>,
            PluginEventPolicy,
        )>,
    >,
}

impl SharedPluginHooks {
    /// Replaces the bridge every later event sees.
    pub fn publish(
        &self,
        hooks: Option<Arc<dyn crate::game::plugin_types::ServerHooks>>,
        policy: PluginEventPolicy,
    ) {
        *self
            .current
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = hooks.map(|hooks| (hooks, policy));
    }

    pub fn load(
        &self,
    ) -> Option<(
        Arc<dyn crate::game::plugin_types::ServerHooks>,
        PluginEventPolicy,
    )> {
        self.current
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

/// The command tree as the world loop currently sees it. The tree can be rebuilt after the loop
/// starts, so region-driven commands must read the current tree per
/// command instead of the one captured at startup. The lock is held only to clone an `Arc`, and
/// only when a command actually runs, never per tick.
pub struct SharedCommandTree {
    current: Mutex<Arc<ferrite_command::CommandDispatcher<super::commands::Source>>>,
}

impl SharedCommandTree {
    pub fn new(tree: Arc<ferrite_command::CommandDispatcher<super::commands::Source>>) -> Self {
        Self {
            current: Mutex::new(tree),
        }
    }

    /// Replaces the tree every later command lookup sees.
    #[cfg(test)]
    pub fn publish(&self, tree: Arc<ferrite_command::CommandDispatcher<super::commands::Source>>) {
        *self
            .current
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = tree;
    }

    pub fn load(&self) -> Arc<ferrite_command::CommandDispatcher<super::commands::Source>> {
        Arc::clone(
            &self
                .current
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        )
    }
}

fn lock_game_state<'a>(
    state: &'a Arc<Mutex<super::commands::GameState>>,
    profiler: Option<&super::profile::Profiler>,
) -> std::sync::MutexGuard<'a, super::commands::GameState> {
    let started = std::time::Instant::now();
    let guard = state.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(profiler) = profiler {
        profiler.record_state_lock_wait(started.elapsed());
    }
    guard
}

/// Runs a command block's command on the region thread. World edits and block-entity data are
/// applied immediately (that is what command blocks exist for); actions that belong to the
/// world loop (give, teleport, chat, ...) are dropped, so command block output never reaches
/// players, as in vanilla.
struct BlockCommands<'a> {
    source: &'a Arc<super::anvil::AnvilChunkSource>,
    tracking: &'a Tracking,
    state: &'a Arc<Mutex<super::commands::GameState>>,
    dimension: super::dimension::Dimension,
    tree: &'a SharedCommandTree,
}

impl super::command_block::Runner for BlockCommands<'_> {
    fn run(&mut self, pos: [i32; 3], command: &str) -> Result<String, String> {
        let origin = super::commands_world::Origin {
            pos: [
                f64::from(pos[0]) + 0.5,
                f64::from(pos[1]) + 0.5,
                f64::from(pos[2]) + 0.5,
            ],
            yaw: 0.0,
            pitch: 0.0,
        };
        let tree = self.tree.load();
        let (actions, mut received) = tokio::sync::mpsc::unbounded_channel();
        let source = super::commands::Source {
            permission: 2,
            origin: origin.pos,
            yaw: 0.0,
            pitch: 0.0,
            uuid: None,
            name: None,
            anchor: ferrite_command::EntityAnchor::Feet,
            dimension: self.dimension,
            world: Some(Arc::clone(self.source)),
            admin: None,
            state: Arc::clone(self.state),
            actions,
            tree: Some(Arc::clone(&tree)),
            function_depth: 0,
        };
        tree.execute(command.trim_start_matches('/'), source)
            .map_err(|error| error.to_string())?;
        let mut output = String::new();
        let mut success = true;
        while let Ok(action) = received.try_recv() {
            match action {
                super::commands::Action::World(command) => {
                    match super::commands_world::apply_world_result(
                        &command,
                        &origin,
                        &**self.source,
                        self.tracking,
                    ) {
                        Ok(outcome) => output = outcome.message,
                        Err(error) => {
                            success = false;
                            output = error;
                        }
                    }
                }
                super::commands::Action::Data(command) => {
                    match super::commands_data::apply_block(Some(&**self.source), &command) {
                        Ok(feedback) => output = feedback.text,
                        Err(error) => {
                            success = false;
                            output = error;
                        }
                    }
                }
                super::commands::Action::Feedback(feedback)
                | super::commands::Action::Broadcast(feedback) => output = feedback.text,
                super::commands::Action::PlayerFeedback { feedback, .. } => {
                    output = feedback.text;
                }
                // The world loop owns everything else (`/give`, `/tp`, `/say`, ...). The
                // command itself parsed and ran, so it counts as a success like vanilla does
                // when its target has no effect.
                _ => {}
            }
        }
        if success { Ok(output) } else { Err(output) }
    }
}
/// Block access for entity physics, memoized per chunk for one tick.
pub(super) struct TickBlocks<'a> {
    source: Option<&'a super::anvil::AnvilChunkSource>,
    seen: std::cell::RefCell<super::fluid::ChunkReadCache>,
}
/// Physics reads through the tick's memoized block getter; placements write through the
/// region's fluid world, so tracking players get the `block_update`.
struct FallingAdapter<'a> {
    read: &'a TickBlocks<'a>,
    world: super::fluid::RegionFluidWorld<'a>,
}

impl BlockGetter for FallingAdapter<'_> {
    fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
        self.read.block_state(x, y, z)
    }
}

impl ferrite_entity::falling::FallingWorld for FallingAdapter<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u16) {
        ferrite_fluid::FluidWorld::set_block(&mut self.world, x, y, z, u32::from(state));
    }
}

impl BlockGetter for TickBlocks<'_> {
    fn brightness(&self, x: i32, y: i32, z: i32) -> Option<u8> {
        self.source?;
        Some(SpawnEnv::light(self, x, y, z))
    }

    fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
        let source = self.source?;
        self.seen
            .borrow_mut()
            .block(source, x, y, z)
            .and_then(|s| u16::try_from(s).ok())
    }
}
impl super::wandering_trader::SpawnWorld for TickBlocks<'_> {
    fn light(&self, x: i32, y: i32, z: i32) -> u8 {
        SpawnEnv::light(self, x, y, z)
    }

    fn biome_name(&self, x: i32, y: i32, z: i32) -> Option<String> {
        self.source?.biome_name(x, y, z)
    }

    fn is_chunk_loaded(&self, x: i32, z: i32) -> bool {
        let Some(source) = self.source else {
            return false;
        };
        self.seen
            .borrow_mut()
            .chunk_arc(source, x.div_euclid(16), z.div_euclid(16))
            .is_some()
    }
}
impl SpawnEnv for TickBlocks<'_> {
    // shortcut: the biome is read at y = 64 for the whole column, so cave biomes do not count.
    fn creature_table(&self, x: i32, z: i32) -> Option<&[ferrite_entity::spawn::SpawnEntry]> {
        super::biome_spawns::creature(&self.source?.biome_name(x, 64, z)?)
    }

    fn monster_table(&self, x: i32, z: i32) -> Option<&[ferrite_entity::spawn::SpawnEntry]> {
        super::biome_spawns::monster(&self.source?.biome_name(x, 64, z)?)
    }

    fn light(&self, x: i32, y: i32, z: i32) -> u8 {
        if let Some(source) = self.source
            && let Some(level) = source.brightness(x, y, z)
        {
            return level;
        }
        // Without stored light (a superflat test world) only open sky counts as daylight.
        for scan in y..=y.saturating_add(64) {
            match self.block_state(x, scan, z) {
                Some(state)
                    if usize::from(state) < ferrite_data::blocks::STATE_COUNT
                        && ferrite_data::block_props::get(state).is_air => {}
                _ => return 0,
            }
        }
        15
    }
}
/// How many queued chunks to request ahead of the batch being sent.
const PREFETCH_CHUNKS: usize = 1200;

pub struct Player {
    pub uuid: Uuid,
    /// Login session; a reconnect gets a new one, so trackers treat it as a new viewer.
    pub session: u64,
    /// Network entity id.
    pub entity: i32,
    /// Blocks this player broke in survival this tick, with their previous state.
    pub broken: Vec<(ferrite_protocol::types::BlockPos, u32)>,
    /// Blocks this player placed or broke this tick, for neighbor updates.
    pub changed_blocks: Vec<[i32; 3]>,
    /// TNT this player primed this tick, with the fuse length; spawned as `PrimedTnt`.
    pub primed_tnt: Vec<([i32; 3], i32)>,
    /// Rockets a plugin attached to this player this tick: `(flight, explosions)`.
    pub firework_launches: Vec<(u8, u8, Option<Uuid>)>,
    /// Eyes of ender this player threw this tick (spawn positions).
    pub eye_throws: Vec<[f64; 3]>,
    /// A plugin asked for a fishing bobber (`launchProjectile(FishHook)`).
    pub bobber_cast: Option<Option<Uuid>>,
    /// Vanilla `Player.enchantmentSeed` (`XpSeed`): fixes the enchanting table's offers until the player enchants.
    pub enchant_seed: i32,
    /// Cells where this player placed end crystals this tick.
    pub crystals: Vec<[i32; 3]>,
    /// Fire this player lit this tick; the region schedules `FireBlock` ticks for it.
    pub ignited: Vec<[i32; 3]>,
    /// Carved pumpkins or jack o' lanterns this player placed this tick. The region matches the
    /// golem patterns after the player update, while it owns the structure's chunks.
    pub placed_pumpkins: Vec<[i32; 3]>,
    /// The dimension this player is in; beds explode outside the overworld.
    pub dimension: super::Dimension,
    /// Identity of the world that owns this player.
    pub world_id: super::WorldId,
    /// The bed this player is sleeping in, if any.
    pub sleeping: Option<[i32; 3]>,
    /// The respawn point a bed set, if any.
    pub spawn: Option<[i32; 3]>,
    /// Bed explosions to run this tick: center, power, whether fire is left behind.
    pub explosions: Vec<([f64; 3], f32, bool)>,
    /// Buttons pressed this tick, with their press duration.
    pub button_press: Vec<([i32; 3], u32)>,
    /// The sign this player may edit.
    pub editing_sign: Option<([i32; 3], bool)>,
    pub name: String,
    /// Permission level at join; command block editing needs 2 (gamemasters).
    pub permission: u8,
    /// Plugin event hooks; `None` when the server runs without plugins.
    pub plugins: Option<Arc<dyn crate::game::plugin_types::ServerHooks>>,
    /// What a failed plugin event means for the action it guards.
    pub plugin_event_errors: PluginEventPolicy,
    pub positions: Arc<Mutex<HashMap<Uuid, [f64; 3]>>>,
    pub tracking: Tracking,
    pub source: Option<Arc<super::anvil::AnvilChunkSource>>,
    /// Directory this player's state is saved to on leave; `None` keeps nothing (test worlds).
    pub playerdata: Option<std::path::PathBuf>,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub ground: bool,
    /// Vanilla `LivingEntity` fall-flying state, used by fireworks and riptide.
    pub gliding: bool,
    pub chunk: ChunkPos,
    pub mailbox: Mailbox,
    pub outbound: OutboundSender,
    pub signals: UnboundedSender<Signal>,
    pub biome: i32,
    pub gamemode: i32,
    pub experience_level: i32,
    pub experience_progress: f32,
    pub experience_total: i32,
    pub inventory: ferrite_inventory::PlayerInventory,
    pub menus: super::container::Containers,
    pub breaking: Option<(i64, u32, u32)>,
    pub view: i32,
    pub pending_teleport: Option<i32>,
    pub next_teleport: i32,
    pub loaded: FxSet<(i32, i32)>,
    pub queue: VecDeque<(i32, i32)>,
    /// How many queue-front chunks were already handed to the chunk source, so a tick does not
    /// take the shared cache lock again for each of them.
    pub prefetched: usize,
    /// Ticks the queue head has been loading; the prefetch is repeated every 20th.
    pub prefetch_wait: u8,
    pub awaiting_batch: bool,
    pub rate: f32,
    /// Vanilla `ServerPlayer` survival state: health, absorption, food, air, fire, fall.
    pub survival: Survival,
    /// `LivingEntity.activeEffects`: the region owns the authoritative effect store.
    pub effects: ActiveEffects,
    /// Dimension type id from `CommonPlayerSpawnInfo`, echoed in `respawn`.
    pub dimension_type: i32,
    pub seed: i64,
    /// `CommonPlayerSpawnInfo.isFlat`, echoed in `respawn`.
    pub flat: bool,
    /// Set by `client_command` action 0 (`perform respawn`); the region performs it.
    pub respawn_requested: bool,
    /// Network ids of entities this player attacked this tick.
    pub attacks: Vec<i32>,
    /// `(entity id, hand)` of the entities this player right-clicked this tick.
    pub uses: Vec<(i32, i32)>,
    /// Network ids of entities this player middle-clicked (`PickItemFromEntity`) this tick.
    pub pick_entities: Vec<i32>,
    /// Whether this player is currently flying (`PlayerAbilities`).
    pub flying: bool,
    /// Ticks since the last swing, vanilla `attackStrengthTicker`.
    pub attack_ticks: i32,
    /// Whether a shield is raised (vanilla `LivingEntity.isBlocking`).
    pub blocking: bool,
    /// Ticks the shield stays disabled after an axe hit.
    pub shield_disabled: i32,
    /// Ticks a drawn bow has been held.
    pub bow_charge: i32,
    /// Whether the player is drawing a bow.
    pub using_bow: bool,
    /// The drawn item is a trident (thrown on release instead of an arrow).
    pub trident_draw: bool,
    /// Where the client moved the horse it rides since the region last looked (x, y, z, yaw).
    pub vehicle_moves: Vec<[f64; 4]>,
    /// The rider pressed the inventory key on a donkey or mule.
    pub open_animal_chest: bool,
    /// Whether the player holds sneak (input bit 5).
    pub sneaking: bool,
    /// Ticks left of a riptide spin (`LivingEntity.autoSpinAttackTicks`).
    pub spin_ticks: u8,
    /// What the running spin hurts each mob it touches for (`autoSpinAttackDmg`); 0 for a plain
    /// trident riptide, which only carries the player.
    pub spin_damage: f32,
    /// Mobs the running spin already hurt, so one spin hits each of them once.
    pub spin_hit: Vec<Uuid>,
    /// The bow was released this tick; the region fires the arrow.
    pub bow_released: bool,
    /// Pending piglin guard-area anger checks queued by chest opens or block breaks (`require_los`).
    pub piglin_alerts: Vec<bool>,
    /// Hives whose honey this player harvested this tick, and whether shears were used. The
    /// owning region releases the bees unless a lit campfire is below (`BeehiveBlock.useItemOn`, `releaseBeesAndResetHoneyLevel`).
    pub hive_harvests: Vec<([i32; 3], Uuid, bool)>,
    /// Hives this player broke this tick, with the block state and the occupants taken before the
    /// item drop was rolled, so the silk-touch item keeps its `minecraft:bees` component.
    pub broken_hives: Vec<([i32; 3], u16, Uuid, super::beehive::HiveData)>,
    /// The dimension registry, used to swap the chunk source on a dimension change.
    pub dimensions: Arc<super::DimensionRegistry>,
    /// Ticks spent standing in a nether portal.
    pub portal_ticks: u32,
    /// Ticks before a portal may move this player again.
    pub portal_cooldown: u32,
    /// A portal trip waiting for its destination chunk to load.
    pub portal_pending: Option<(super::Dimension, [f64; 3])>,
    /// The end gateway block this player just teleported through, for the region to start the
    /// purple beam.
    pub gateway_used: Option<[i32; 3]>,
    /// Saved maps already sent to this player: id and the identity of the data sent.
    pub maps_sent: HashMap<i32, usize>,
    /// Left the End once already, so the credits are not shown again (vanilla `seenCredits`).
    /// shortcut: kept in memory only, not saved with the player data.
    pub seen_credits: bool,
    /// The credits are playing; the client answers with a respawn request when they end.
    pub watching_credits: bool,
    /// Whether chunk-stream holder changes are batched into `Signal::HoldBatch` (default `true`).
    pub batch_chunk_holders: Arc<AtomicBool>,
}
/// A final block update held back until the client has finished its moving-piston animation.
#[derive(Debug)]
pub struct DelayedUpdate {
    pub pos: [i32; 3],
    pub due: i64,
    pub chunk: (i32, i32),
    /// Final state the delayed packet represents; discard it if a later action changed the block.
    pub expected_state: u32,
    pub packet: Outbound,
}
#[derive(Default)]
pub struct Region {
    /// Explosions queued by projectiles during the tick: center, power, whether fire is left
    /// behind and whether blocks break.
    pub queued_explosions: Vec<([f64; 3], f32, bool, bool)>,
    /// Final block updates held back until the client finished the push animation. Keyed by
    /// position through [`Region::queue_delayed_update`] so a reversal replaces stale updates.
    pub delayed_updates: Vec<DelayedUpdate>,
    /// Client-side moving-piston blocks that have not reached the two-tick completion yet.
    pub moving_pistons: super::piston::Animations,
    /// The stack of every trident in flight, by entity uuid, so it can drop as it was thrown.
    pub thrown_tridents: HashMap<Uuid, (Uuid, ferrite_item::ItemStack)>,
    /// The bottle each splash potion in flight was thrown from, keyed by projectile uuid.
    pub thrown_potions: HashMap<Uuid, ferrite_item::ItemStack>,
    /// Clouds left by landed lingering potions and dragon breath. Each has an `area_effect_cloud`
    /// entity in `entities` (same uuid) that clients see; the cloud state itself is not saved.
    pub potion_clouds: Vec<ferrite_entity::potion::PotionCloud>,
    /// Arm swings `/swing` asked for on non-player entities: entity, `Animate` action.
    pending_swings: Vec<(Uuid, u8)>,
    /// Mobs `/damage` hurt this tick, waiting for their hurt animation.
    pending_hurts: Vec<Uuid>,
    /// Runtime clocks of the end gateways this region holds: age and purple-beam cooldown.
    gateway_beams: HashMap<[i32; 3], super::end_fight::GatewayBeam>,
    /// Effect changes on mobs this tick, waiting to be sent to the players tracking them.
    pending_mob_effects: Vec<(Uuid, MobEffectNotice)>,
    /// The dragon boss bar each player was last shown: bar id and progress.
    dragon_bars: HashMap<Uuid, (Uuid, f32)>,
    /// Village raids whose village this region holds (`RaidManager`).
    pub raids: super::raid::RaidManager,
    /// Trial spawners whose block this region holds.
    pub trial_spawners: super::trial_spawner::TrialSpawnerManager,
    pub players: HashMap<Uuid, Player>,
    pub entities: EntityStore,
    pub(crate) generator: FlatGenerator,
    fluid_ticks: ferrite_fluid::FluidTicks,
    /// Origins of block changes made by neighbouring regions since the last tick; they feed the
    /// gravity scheduler (the neighbour update itself is queued in `updates`).
    remote_changes: Vec<[i32; 3]>,
    random_ticks: super::random_tick::RandomTicks,
    updates: super::neighbors::Updates,
    gravity: super::gravity::Gravity,
    fire: super::fire::FireTicks,
    buttons: super::blocks::ButtonTicks,
    vaults: super::vault::Vaults,
    dispensers: super::dispenser::Dispensers,
    hoppers: super::hopper::Hoppers,
    redstone: super::redstone::Redstone,
    command_blocks: super::command_block::CommandBlocks,
    /// Monotonic per-region tick counter, the `LastExecution` clock command blocks use.
    tick_clock: i64,
    held_chunks: HashMap<ChunkPos, usize>,
    /// Chunks held for the first time this tick; their borders get the wake-up pass vanilla runs
    /// when a chunk becomes readable, so fluid and support rules that stopped at the chunk edge
    /// resume instead of staying frozen.
    pending_chunk_loads: Vec<ChunkPos>,
    /// Tick monitor slot, assigned by the game loop on this region's first tick.
    pub profile_slot: Option<u32>,
    /// Scope tree attached by the game loop; `None` until the first tick.
    pub timings: Option<Arc<Timings>>,
    rng: Rng,
    /// Ticks this region has run, for the beacon's 80-tick effect refresh.
    beacon_clock: u32,
    /// UUIDs this region last published into `TickContext::command_entities`.
    published_entities: HashSet<Uuid>,
    /// Entity UUIDs this region last published into the command index.
    published_facts: HashSet<Uuid>,
    /// Hives destroyed by anything but a player, with the occupants taken when the block entity
    /// vanished. The bee tick releases them, where cross-region hand-off has a `TickContext`.
    pending_hive_breaks: Vec<([i32; 3], u16, super::beehive::HiveData)>,
    /// Reusable per-tick scratch buffers so normal ticks perform zero auxiliary allocations.
    scratch_ids: Vec<Uuid>,
    scratch_vault_players: Vec<super::vault::VaultPlayer>,
    scratch_alive: Vec<ferrite_math::Vec3>,
    scratch_entity_ids: FxMap<Uuid, i32>,
    /// Reused key list for the per-tick mob-effect pass; keeps that hot path allocation-free.
    scratch_effect_keys: Vec<ferrite_entity::EntityKey>,
    scratch_entity_keys: Vec<ferrite_entity::EntityKey>,
    scratch_targets: Vec<ai::Target>,
    scratch_mob_players: HashMap<Uuid, ferrite_math::Vec3>,
    scratch_mob_held: HashMap<Uuid, [Option<&'static str>; 2]>,
    scratch_mob_falling: Vec<(ferrite_entity::EntityKey, f32)>,
    ai_scratch: ai::AiScratch,
}
impl RegionData for Region {
    fn merge(&mut self, mut other: Self) {
        other
            .fluid_ticks
            .transfer_to(&mut self.fluid_ticks, |_| true);
        for (chunk, count) in other.held_chunks {
            *self.held_chunks.entry(chunk).or_default() += count;
        }
        self.pending_chunk_loads.extend(other.pending_chunk_loads);
        self.gravity.merge(other.gravity);
        self.fire.merge(other.fire);
        self.buttons.merge(other.buttons);
        self.vaults.merge(other.vaults);
        self.dispensers.merge(other.dispensers);
        self.hoppers.merge(other.hoppers);
        self.redstone.merge(other.redstone);
        self.command_blocks.merge(other.command_blocks);
        self.tick_clock = self.tick_clock.max(other.tick_clock);
        for update in other.delayed_updates {
            self.queue_delayed_update(update);
        }
        self.moving_pistons.merge(other.moving_pistons);
        self.players.extend(other.players);
        self.entities.merge(other.entities);
        self.thrown_potions.extend(other.thrown_potions);
        self.potion_clouds.extend(other.potion_clouds);
        self.gateway_beams.extend(other.gateway_beams);
        self.raids.merge(other.raids);
        self.trial_spawners.merge(other.trial_spawners);
        self.published_entities.extend(other.published_entities);
        self.published_facts.extend(other.published_facts);
        self.random_ticks.merge(other.random_ticks);
        self.pending_hive_breaks.extend(other.pending_hive_breaks);
        self.generator.merge(other.generator);
    }
    fn split(&mut self, sections: &HashSet<SectionPos>) -> Self {
        let ids: Vec<_> = self
            .players
            .iter()
            .filter(|(_, p)| sections.contains(&SectionPos(p.chunk.0 >> 4, p.chunk.1 >> 4)))
            .map(|(id, _)| *id)
            .collect();
        let in_sections = |pos: [i32; 3]| {
            sections.contains(&SectionPos(
                pos[0].div_euclid(16) >> 4,
                pos[2].div_euclid(16) >> 4,
            ))
        };
        let (delayed_updates, staying) = std::mem::take(&mut self.delayed_updates)
            .into_iter()
            .partition(|update| in_sections(update.pos));
        self.delayed_updates = staying;
        let (hive_breaks, staying_hive_breaks) = std::mem::take(&mut self.pending_hive_breaks)
            .into_iter()
            .partition(|(pos, _, _)| in_sections(*pos));
        self.pending_hive_breaks = staying_hive_breaks;
        let moving_pistons = self.moving_pistons.split(in_sections);
        let (potion_clouds, staying_clouds) = std::mem::take(&mut self.potion_clouds)
            .into_iter()
            .partition(|cloud| {
                in_sections([
                    cloud.center.x.floor() as i32,
                    0,
                    cloud.center.z.floor() as i32,
                ])
            });
        self.potion_clouds = staying_clouds;
        let moving_beams: Vec<_> = self
            .gateway_beams
            .keys()
            .copied()
            .filter(|at| in_sections(*at))
            .collect();
        let gateway_beams = moving_beams
            .into_iter()
            .filter_map(|at| self.gateway_beams.remove_entry(&at))
            .collect();
        let random_ticks = self.random_ticks.split(in_sections);
        let mut result = Self {
            queued_explosions: Vec::new(),
            potion_clouds,
            gateway_beams,
            delayed_updates,
            moving_pistons,
            thrown_tridents: HashMap::new(),
            thrown_potions: HashMap::new(),
            pending_swings: Vec::new(),
            pending_hurts: Vec::new(),
            pending_mob_effects: Vec::new(),
            dragon_bars: HashMap::new(),
            raids: self.raids.split(in_sections),
            trial_spawners: self.trial_spawners.split(in_sections),
            players: HashMap::new(),
            rng: Rng::default(),
            beacon_clock: 0,
            entities: self
                .entities
                .split(|(x, z)| sections.contains(&SectionPos(x >> 4, z >> 4))),
            generator: self.generator.split(sections),
            fluid_ticks: ferrite_fluid::FluidTicks::default(),
            remote_changes: Vec::new(),
            random_ticks,
            updates: super::neighbors::Updates::default(),
            gravity: self.gravity.split(|x, z| {
                sections.contains(&SectionPos(x.div_euclid(16) >> 4, z.div_euclid(16) >> 4))
            }),
            fire: self.fire.split(|x, z| {
                sections.contains(&SectionPos(x.div_euclid(16) >> 4, z.div_euclid(16) >> 4))
            }),
            buttons: self.buttons.split(|x, z| {
                sections.contains(&SectionPos(x.div_euclid(16) >> 4, z.div_euclid(16) >> 4))
            }),
            vaults: self.vaults.split(|x, z| {
                sections.contains(&SectionPos(x.div_euclid(16) >> 4, z.div_euclid(16) >> 4))
            }),
            dispensers: self.dispensers.split(|x, z| {
                sections.contains(&SectionPos(x.div_euclid(16) >> 4, z.div_euclid(16) >> 4))
            }),
            hoppers: self.hoppers.split(|x, z| {
                sections.contains(&SectionPos(x.div_euclid(16) >> 4, z.div_euclid(16) >> 4))
            }),
            redstone: self.redstone.split(|x, z| {
                sections.contains(&SectionPos(x.div_euclid(16) >> 4, z.div_euclid(16) >> 4))
            }),
            command_blocks: self.command_blocks.split(|x, z| {
                sections.contains(&SectionPos(x.div_euclid(16) >> 4, z.div_euclid(16) >> 4))
            }),
            tick_clock: self.tick_clock,
            held_chunks: HashMap::new(),
            pending_chunk_loads: Vec::new(),
            profile_slot: None,
            timings: self.timings.clone(),
            published_entities: HashSet::new(),
            published_facts: HashSet::new(),
            pending_hive_breaks: hive_breaks,
            scratch_ids: Vec::new(),
            scratch_vault_players: Vec::new(),
            scratch_alive: Vec::new(),
            scratch_entity_ids: FxMap::default(),
            scratch_effect_keys: Vec::new(),
            scratch_entity_keys: Vec::new(),
            scratch_targets: Vec::new(),
            scratch_mob_players: HashMap::new(),
            scratch_mob_held: HashMap::new(),
            scratch_mob_falling: Vec::new(),
            ai_scratch: ai::AiScratch::default(),
        };
        // Facts follow the entities into the new region. Otherwise the old region's next tick
        // would remove the new region's entries from the shared registry.
        for key in result.entities.keys() {
            if let Some(data) = result.entities.data(*key) {
                if self.published_facts.remove(&data.uuid) {
                    result.published_facts.insert(data.uuid);
                }
                if self.published_entities.remove(&data.uuid) {
                    result.published_entities.insert(data.uuid);
                }
                if let Some(bottle) = self.thrown_potions.remove(&data.uuid) {
                    result.thrown_potions.insert(data.uuid, bottle);
                }
            }
        }
        let moving: Vec<_> = self
            .held_chunks
            .keys()
            .copied()
            .filter(|c| sections.contains(&SectionPos(c.0 >> 4, c.1 >> 4)))
            .collect();
        for c in moving {
            result
                .held_chunks
                .insert(c, self.held_chunks.remove(&c).unwrap());
        }
        let (moved_pending, staying_pending): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.pending_chunk_loads)
                .into_iter()
                .partition(|c| sections.contains(&SectionPos(c.0 >> 4, c.1 >> 4)));
        self.pending_chunk_loads = staying_pending;
        result.pending_chunk_loads = moved_pending;
        self.fluid_ticks
            .transfer_to(&mut result.fluid_ticks, |(x, _, z)| {
                sections.contains(&SectionPos(
                    (x.div_euclid(16)) >> 4,
                    (z.div_euclid(16)) >> 4,
                ))
            });
        for id in ids {
            if let Some(p) = self.players.remove(&id) {
                result.players.insert(id, p);
            }
        }
        result
    }
}
type ChunkList = Vec<(i32, i32)>;

fn spiral_offsets(radius: i32) -> &'static [(i32, i32)] {
    static TABLES: [OnceLock<Vec<(i32, i32)>>; 33] = [const { OnceLock::new() }; 33];
    let r = radius.clamp(0, 32) as usize;
    TABLES[r].get_or_init(|| {
        let rad = r as i32;
        let side = (2 * rad + 1) as usize;
        let mut offsets = Vec::with_capacity(side * side);
        for dx in -rad..=rad {
            for dz in -rad..=rad {
                offsets.push((dx, dz));
            }
        }
        offsets.sort_unstable_by_key(|&(dx, dz)| (dx * dx + dz * dz, dx, dz));
        offsets
    })
}

fn chunk_window<S: std::hash::BuildHasher>(
    center: (i32, i32),
    radius: i32,
    loaded: &HashSet<(i32, i32), S>,
) -> (ChunkList, ChunkList) {
    if loaded.is_empty() && (0..=32).contains(&radius) {
        let offsets = spiral_offsets(radius);
        let mut new = Vec::with_capacity(offsets.len());
        for &(dx, dz) in offsets {
            new.push((center.0 + dx, center.1 + dz));
        }
        return (Vec::new(), new);
    }
    if (0..=31).contains(&radius) {
        let mut forget = Vec::new();
        let mut present = [0u64; 64];
        for &(x, z) in loaded {
            let dx = x - center.0;
            let dz = z - center.1;
            if !super::chunk_level::in_view(dx, dz, radius) {
                forget.push((x, z));
            } else {
                present[(dz + radius) as usize] |= 1u64 << (dx + radius);
            }
        }
        let offsets = spiral_offsets(radius);
        let mut new = Vec::with_capacity(offsets.len());
        for &(dx, dz) in offsets {
            if (present[(dz + radius) as usize] & (1u64 << (dx + radius))) == 0 {
                new.push((center.0 + dx, center.1 + dz));
            }
        }
        return (forget, new);
    }
    let forget = loaded
        .iter()
        .copied()
        .filter(|(x, z)| !super::chunk_level::in_view(x - center.0, z - center.1, radius))
        .collect();
    let mut new = Vec::new();
    for x in center.0 - radius..=center.0 + radius {
        for z in center.1 - radius..=center.1 + radius {
            if !loaded.contains(&(x, z)) {
                new.push((x, z));
            }
        }
    }
    // Equal distances have no meaningful order, so the allocation-free unstable sort is used.
    new.sort_unstable_by_key(|(x, z)| {
        let dx = x - center.0;
        let dz = z - center.1;
        (dx * dx + dz * dz, *x, *z)
    });
    (forget, new)
}
pub(super) fn send<P: ferrite_protocol::Packet>(p: &Player, msg: &P) {
    let _ = p.outbound.send(packet(msg));
}
/// Namespaced block id of a global state id, for Bukkit event materials.
fn material_name(state: u32) -> &'static str {
    u16::try_from(state)
        .ok()
        .and_then(ferrite_data::blocks::state)
        .map_or("minecraft:air", |(name, _)| name)
}
/// World key sent in deferred block-event payloads: the vanilla dimension key, or the UUID of a
/// runtime world (the same identity `register_world` publishes to plugins).
fn plugin_world_name(world_id: super::WorldId) -> String {
    match world_id {
        super::WorldId::Vanilla(dimension) => dimension.key().to_owned(),
        super::WorldId::Runtime(id) => id.to_string(),
    }
}
/// Runs an event hook. A missing bridge allows the action; a failed call is
/// decided by `plugin-event-errors` (Bukkit-compatible default: allow).
fn event_allows(
    plugins: Option<&dyn crate::game::plugin_types::ServerHooks>,
    policy: PluginEventPolicy,
    event: impl FnOnce(
        &dyn crate::game::plugin_types::ServerHooks,
    ) -> Result<bool, crate::game::plugin_types::Error>,
) -> bool {
    let Some(plugins) = plugins else {
        return true;
    };
    match event(plugins) {
        Ok(allowed) => allowed,
        Err(error) => {
            let allow = policy == PluginEventPolicy::Allow;
            tracing::warn!(%error, allow, "plugin event failed");
            allow
        }
    }
}
/// Applies a Bukkit `EntityDamageEvent` result. `None` cancels the hit; a failed event follows
/// `plugin-event-errors` (Allow keeps the original damage, Deny cancels it).
pub(super) fn plugin_damage(
    plugins: Option<&dyn crate::game::plugin_types::ServerHooks>,
    policy: PluginEventPolicy,
    amount: f32,
    event: impl FnOnce(
        &dyn crate::game::plugin_types::ServerHooks,
    ) -> Result<f64, crate::game::plugin_types::Error>,
) -> Option<f32> {
    let Some(plugins) = plugins else {
        return Some(amount);
    };
    match event(plugins) {
        Ok(damage) if damage.is_finite() && (0.0..=f64::from(f32::MAX)).contains(&damage) => {
            (damage > 0.0).then_some(damage as f32)
        }
        Ok(damage) => {
            let allow = policy == PluginEventPolicy::Allow;
            tracing::warn!(damage, allow, "plugin damage event returned invalid damage");
            allow.then_some(amount)
        }
        Err(error) => {
            let allow = policy == PluginEventPolicy::Allow;
            tracing::warn!(%error, allow, "plugin damage event failed");
            allow.then_some(amount)
        }
    }
}
/// Bukkit's damage cause for the host's internal damage kind.
pub(super) fn damage_cause(kind: DamageKind) -> &'static str {
    match kind {
        DamageKind::Fall => "FALL",
        DamageKind::Drown => "DROWNING",
        DamageKind::Lava => "LAVA",
        DamageKind::Fire => "FIRE",
        DamageKind::Void => "VOID",
        DamageKind::Starve => "STARVATION",
        DamageKind::Magic => "MAGIC",
        DamageKind::Wither => "WITHER",
        DamageKind::Explosion => "ENTITY_EXPLOSION",
        DamageKind::Mob => "ENTITY_ATTACK",
        DamageKind::Arrow | DamageKind::Fireball => "PROJECTILE",
        DamageKind::GenericKill => "KILL",
        DamageKind::Generic => "CUSTOM",
    }
}
/// The mob a spawn egg places (`SpawnEggItem.getType`), e.g. `minecraft:cow_spawn_egg`.
fn spawn_egg_kind(item: Option<&str>) -> Option<MobKind> {
    let name = item?;
    MobKind::from_name(name.strip_suffix("_spawn_egg")?)
}

/// Vanilla's total experience required to reach `level`.
fn total_for_level(level: i32) -> i64 {
    let level = i64::from(level.max(0));
    if level <= 16 {
        level * level + 6 * level
    } else if level <= 31 {
        (2.5 * level as f64 * level as f64 - 40.5 * level as f64 + 360.0) as i64
    } else {
        (4.5 * level as f64 * level as f64 - 162.5 * level as f64 + 2220.0) as i64
    }
}

/// Level and bar progress for a total amount of experience.
fn level_and_progress(total: i32) -> (i32, f32) {
    let total = i64::from(total.max(0));
    let mut level = 0;
    while level < 21_863 && total >= total_for_level(level + 1) {
        level += 1;
    }
    let base = total_for_level(level);
    let next = total_for_level(level + 1);
    let progress = if next > base {
        ((total - base) as f32 / (next - base) as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (level, progress)
}

/// The authoritative merchant state an open trade menu is synchronized against. The entity
/// store belongs to the region; a player's menu is only a snapshot of its offers.
struct TraderSync<'a> {
    entities: &'a mut EntityStore,
}

impl TraderSync<'_> {
    /// Refreshes the player's open trade menu from the villager, so the next click validates
    /// against newer uses/demand and the player's current reputation instead of a stale copy.
    ///
    /// Returns whether a click may be handled. A trader menu whose entity is no longer in this
    /// region is closed instead: its snapshot must not keep selling after a region hand-off.
    fn refresh(&self, player: &mut Player) -> bool {
        let Some((trader, _)) = player.open_trader() else {
            return true;
        };
        let Some(key) = trader_key(self.entities, trader) else {
            player.close_open_menu();
            return false;
        };
        let Some(data) = self.entities.data(key) else {
            player.close_open_menu();
            return false;
        };
        let Some(villager) = &data.villager else {
            player.close_open_menu();
            return false;
        };
        let offers = if data
            .mob
            .as_ref()
            .is_some_and(|mob| mob.kind == MobKind::Villager)
        {
            villager.offers_for_player(player.uuid)
        } else {
            villager.offers.clone()
        };
        player.set_trader_offers(offers);
        true
    }

    /// Merges the sales the player made since the last refresh into the merchant and applies
    /// the villager XP those sales earned.
    fn commit(&mut self, player: &mut Player) {
        let Some((trader, trades)) = player.open_trader() else {
            return;
        };
        if trader_key(self.entities, trader).is_none() {
            player.close_open_menu();
            return;
        }
        let xp = player.take_trader_xp();
        let sold = merge_trader_trades(self.entities, player.uuid, trader, &trades);
        push_trades(player.uuid, player.pos[1], sold);
        if sold > 0 || xp > 0 {
            add_trader_xp(self.entities, trader, xp);
            self.refresh(player);
        }
    }
}

/// `AbstractVillager.notifyTrade` -> `CriteriaTriggers.TRADE`, once per sale merged.
fn push_trades(player: Uuid, y: f64, sold: i32) {
    for _ in 0..sold {
        super::recipe_unlock::push_plain(
            player,
            super::recipe_unlock::PlainEvent {
                trigger: "minecraft:villager_trade",
                player_y: y,
                ..Default::default()
            },
        );
    }
}

/// The entity holding `trader`'s uuid.
fn trader_key(entities: &EntityStore, trader: Uuid) -> Option<ferrite_entity::EntityKey> {
    entities
        .keys()
        .iter()
        .copied()
        .find(|&key| entities.data(key).is_some_and(|data| data.uuid == trader))
}

/// Merges one player's trade snapshot into the merchant. Only new uses are applied, so a stale
/// second screen cannot rewind another player's sale or go past `max_uses`; each applied sale
/// credits trading gossip to the player whose menu made it.
fn merge_trader_trades(
    entities: &mut EntityStore,
    player: Uuid,
    trader: Uuid,
    trades: &[ferrite_inventory::Trade],
) -> i32 {
    let Some(key) = trader_key(entities, trader) else {
        return 0;
    };
    let Some(data) = entities.data_mut(key) else {
        return 0;
    };
    let village = data
        .mob
        .as_ref()
        .is_some_and(|mob| mob.kind == MobKind::Villager);
    let Some(villager) = data.villager.as_mut() else {
        return 0;
    };
    let sold = villager.merge_trades(trades);
    if village {
        for _ in 0..sold {
            villager.record_trade(player);
        }
    }
    sold
}

/// Adds trade XP to a villager after its menu sales were merged. Wandering traders do not
/// level up, so only `MobKind::Villager` records receive the XP.
fn add_trader_xp(entities: &mut EntityStore, trader: Uuid, xp: i32) {
    if xp <= 0 {
        return;
    }
    let Some(key) = trader_key(entities, trader) else {
        return;
    };
    if entities.data(key).is_some_and(|data| {
        data.mob
            .as_ref()
            .is_some_and(|mob| mob.kind == MobKind::Villager)
    }) {
        ferrite_entity::villager::add_trade_xp(entities, key, xp);
    }
}

impl Player {
    /// `Player#loadData`: reads the data file over the live player. The inventory, selected slot
    /// and game mode always apply; the position only when the file was saved in this world, so a
    /// player is never moved across worlds without the usual transfer. Nothing happens when
    /// there is no file. Returns whether the position changed.
    fn load_player_data(&mut self) -> bool {
        let Some(saved) = self
            .playerdata
            .as_deref()
            .and_then(|dir| super::playerdata::load_from(dir, &self.uuid))
        else {
            return false;
        };
        self.close_container();
        self.inventory = saved.inventory();
        self.init_inventory_menu();
        send(
            self,
            &SetHeldSlot {
                slot: VarInt(self.inventory.selected_slot() as i32),
            },
        );
        self.gamemode = saved.gamemode;
        send(
            self,
            &GameEvent {
                event: 3,
                value: saved.gamemode as f32,
            },
        );
        if let Some(vitals) = &saved.vitals {
            self.apply_vitals(vitals);
        }
        let same_world = saved.world
            == match self.world_id {
                super::WorldId::Runtime(id) => Some(id),
                super::WorldId::Vanilla(_) => None,
            }
            && saved.dimension() == self.dimension;
        let movable = saved
            .pos
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 30_000_000.0);
        if same_world && movable {
            self.pos = saved.pos;
            self.yaw = saved.yaw;
            self.pitch = saved.pitch;
            self.teleport();
            return true;
        }
        false
    }

    /// Health, hunger and experience as the data file keeps them.
    pub fn vitals(&self) -> super::playerdata::Vitals {
        super::playerdata::Vitals {
            health: self.survival.health,
            absorption: self.survival.absorption,
            food: self.survival.food.food,
            saturation: self.survival.food.saturation,
            exhaustion: self.survival.food.exhaustion,
            xp_level: self.experience_level,
            xp_progress: self.experience_progress,
            xp_total: self.experience_total,
        }
    }

    /// Puts saved health, hunger and experience back and tells the client. A dead or corrupt
    /// save is ignored (the player is alive now and stays alive); a dead player is not changed.
    pub fn apply_vitals(&mut self, vitals: &super::playerdata::Vitals) {
        if self.survival.dead {
            return;
        }
        let Some(vitals) = vitals.sanitized(self.survival.max_health) else {
            return;
        };
        self.survival.health = vitals.health;
        self.survival.absorption = vitals.absorption;
        self.survival.food.food = vitals.food;
        self.survival.food.saturation = vitals.saturation;
        self.survival.food.exhaustion = vitals.exhaustion;
        self.experience_level = vitals.xp_level;
        self.experience_progress = vitals.xp_progress;
        self.experience_total = vitals.xp_total;
        send(
            self,
            &SetHealth {
                health: self.survival.health,
                food: VarInt(self.survival.food.food),
                saturation: self.survival.food.saturation,
            },
        );
        send(
            self,
            &SetExperience {
                progress: self.experience_progress,
                level: VarInt(self.experience_level),
                total: VarInt(self.experience_total),
            },
        );
    }

    /// Records the client's on-ground flag; landing ends fall-flying.
    fn set_ground(&mut self, ground: bool) {
        self.ground = ground;
        if ground {
            self.gliding = false;
        }
    }

    /// Sends the riptide spin flag to this player and the other viewers in its dimension.
    fn broadcast_spin(&self, spin: bool) {
        send(self, &spin_metadata(self.entity, spin));
        broadcast_player_metadata(&self.tracking, self.world_id, self.uuid, self.entity, spin);
    }

    /// Clears a riptide spin before the synced player state loses it.
    fn stop_spin(&mut self) {
        if end_player_spin(
            &mut self.spin_ticks,
            &self.tracking,
            self.dimension,
            self.uuid,
            self.entity,
        ) {
            send(self, &spin_metadata(self.entity, false));
        }
    }

    pub fn refresh_chunks(&mut self) {
        send(
            self,
            &CenterChunk {
                x: VarInt(self.chunk.0),
                z: VarInt(self.chunk.1),
            },
        );
        let (forget, new) = chunk_window((self.chunk.0, self.chunk.1), self.view, &self.loaded);
        if !forget.is_empty() {
            let mut released = Vec::with_capacity(forget.len());
            {
                let mut tracking = self.tracking.lock().unwrap();
                let entry = tracking.get_mut(&self.uuid);
                let mut tracked_loaded = entry.map(|(_, loaded, _)| loaded);
                for (x, z) in forget {
                    send(self, &ForgetChunk { z, x });
                    self.loaded.remove(&(x, z));
                    if let Some(plugins) = self.plugins.clone() {
                        let _ = plugins.chunk_unloaded(
                            crate::game::plugin_types::default_world_id(),
                            x,
                            z,
                        );
                    }
                    if let Some(loaded) = tracked_loaded.as_deref_mut() {
                        loaded.remove(&(x, z));
                    }
                    released.push(ChunkPos(x, z));
                }
            }
            if self.batch_chunk_holders.load(Ordering::Relaxed) {
                let _ = self.signals.send(Signal::HoldBatch {
                    world: self.world_id,
                    chunks: released,
                    add: false,
                });
            } else {
                for chunk in released {
                    let _ = self.signals.send(Signal::Hold {
                        world: self.world_id,
                        chunk,
                        add: false,
                    });
                }
            }
        }
        self.queue = new.into();
        self.prefetched = 0;
    }
    fn teleport(&mut self) {
        let id = self.next_teleport;
        self.next_teleport += 1;
        self.pending_teleport = Some(id);
        send(
            self,
            &PositionSync {
                teleport_id: VarInt(id),
                x: self.pos[0],
                y: self.pos[1],
                z: self.pos[2],
                velocity_x: 0.0,
                velocity_y: 0.0,
                velocity_z: 0.0,
                yaw: self.yaw,
                pitch: self.pitch,
                flags: 0,
            },
        );
    }
    fn land_ender_pearl(&mut self, pos: [f64; 3]) -> bool {
        if !pos
            .iter()
            .all(|value| value.is_finite() && value.abs() <= 30_000_000.0)
        {
            return false;
        }
        self.pos = pos;
        self.teleport();
        if !matches!(self.gamemode, 1 | 3) {
            self.hurt(DamageKind::Fall, 5.0);
        }
        true
    }
    pub(crate) fn chunks_tick(
        &mut self,
        generator: &mut FlatGenerator,
        moving_pistons: &[super::piston::MovingBlock],
    ) {
        if self.awaiting_batch || self.queue.is_empty() {
            return;
        }
        // Start loading the whole head of the queue at once; `get` hands back chunks that are ready
        // and spawns background loads for the rest. Without this, chunks became ready one per tick.
        let old_prefetched = self.prefetched;
        let scan_limit = self.queue.len().min(PREFETCH_CHUNKS);
        let mut any_newly_ready = false;
        if let Some(source) = self.source.clone() {
            if old_prefetched < scan_limit {
                let slice = &self.queue.make_contiguous()[old_prefetched..scan_limit];
                any_newly_ready = source.prefetch_batch(slice, self.biome);
            }
            self.prefetched = scan_limit;
        }
        let count = (self.rate.clamp(1.0, 64.0) as usize).min(self.queue.len());
        let check_limit = if any_newly_ready || old_prefetched > 0 || self.source.is_none() {
            scan_limit
        } else {
            0
        };
        let mut chunks = Vec::with_capacity(count);
        let mut idx = 0usize;
        let mut sent_indices: Vec<usize> = Vec::with_capacity(count);
        while chunks.len() < count && idx < check_limit {
            let (x, z) = self.queue[idx];
            let outbound = if let Some(source) = &self.source {
                match source.get_frame(x, z, self.biome) {
                    None => {
                        if self.loaded.is_empty() && (x, z) == (self.chunk.0, self.chunk.1) {
                            break;
                        }
                        idx += 1;
                        continue;
                    }
                    // The cache shares one pre-compressed `SharedFrame` across all recipients,
                    // avoiding both a region-thread `Vec` copy and redundant socket compression.
                    Some(Ok(frame)) => Outbound::SharedFrame(frame),
                    Some(Err(error)) => {
                        tracing::error!(x, z, %error, "failed to load chunk");
                        let _ = self
                            .outbound
                            .send(Outbound::Disconnect("Failed to load world chunk".into()));
                        self.mailbox.lock().unwrap().push_back(Command::Leave);
                        return;
                    }
                }
            } else {
                let chunk = generator.get(x, z, self.biome);
                let mut packet = Vec::new();
                ferrite_protocol::encode_packet(&chunk, &mut packet);
                Outbound::Packet(packet.into())
            };
            sent_indices.push(idx);
            chunks.push((x, z, outbound));
            idx += 1;
        }
        if !sent_indices.is_empty() {
            // Single O(queue.len()) compaction instead of repeated O(queue.len()) shifts.
            let mut drop_pos = 0usize;
            let mut read_idx = 0usize;
            let mut removed_prefetched = 0usize;
            let prefetched = self.prefetched;
            self.queue.retain(|_| {
                let keep = if sent_indices.get(drop_pos).copied() == Some(read_idx) {
                    drop_pos += 1;
                    if read_idx < prefetched {
                        removed_prefetched += 1;
                    }
                    false
                } else {
                    true
                };
                read_idx += 1;
                keep
            });
            self.prefetched = self.prefetched.saturating_sub(removed_prefetched);
        }
        if chunks.is_empty() {
            if self.source.is_some() {
                // The source may have refused deeper chunks while busy; ask again, but
                // not every tick: each ask takes the shared cache lock.
                self.prefetch_wait += 1;
                if self.prefetch_wait >= 20 {
                    self.prefetch_wait = 0;
                    self.prefetched = 0;
                }
            }
            return;
        }
        self.prefetch_wait = 0;
        static BATCH_START: OnceLock<Outbound> = OnceLock::new();
        let _ = self
            .outbound
            .send(BATCH_START.get_or_init(|| packet(&ChunkBatchStart)).clone());
        let count = chunks.len();
        let mut sent = Vec::with_capacity(count);
        let mut held = Vec::with_capacity(count);
        for (x, z, outbound) in chunks {
            let _ = self.outbound.send(outbound);
            self.loaded.insert((x, z));
            if let Some(plugins) = self.plugins.clone() {
                let _ =
                    plugins.chunk_loaded(crate::game::plugin_types::default_world_id(), x, z, true);
            }
            sent.push((x, z));
            held.push(ChunkPos(x, z));
        }
        if !held.is_empty() {
            if self.batch_chunk_holders.load(Ordering::Relaxed) {
                let _ = self.signals.send(Signal::HoldBatch {
                    world: self.world_id,
                    chunks: held,
                    add: true,
                });
            } else {
                for chunk in held {
                    let _ = self.signals.send(Signal::Hold {
                        world: self.world_id,
                        chunk,
                        add: true,
                    });
                }
            }
        }
        // Held only for this insert: loading and encoding above must not serialize the regions.
        if let Some((_, loaded, _)) = self.tracking.lock().unwrap().get_mut(&self.uuid) {
            loaded.extend(sent.iter().copied());
        }
        send(
            self,
            &ChunkBatchFinished {
                size: VarInt(count as i32),
            },
        );
        // A player who starts tracking a chunk during the animation missed the original
        // broadcast. Replay the still-live overlay after the batch; the scheduled final update
        // will reach it through the normal tracked-chunk broadcast.
        for (update, entity) in moving_piston_replay(moving_pistons, &sent) {
            send(self, &update);
            if let Some(entity) = entity {
                send(self, &entity);
            }
        }
        self.awaiting_batch = true;
    }
    fn interaction_ready(&self, location: i64) -> bool {
        let pos = ferrite_protocol::types::BlockPos::unpack(location);
        if (f64::from(pos.x) - self.pos[0]).abs() > 7.0
            || (f64::from(pos.y) - self.pos[1]).abs() > 7.0
            || (f64::from(pos.z) - self.pos[2]).abs() > 7.0
        {
            return true;
        }
        let Some(source) = &self.source else {
            return true;
        };
        let mut ready = true;
        let min_x = (self.pos[0] - 6.0).floor() as i32;
        let max_x = (self.pos[0] + 6.0).floor() as i32;
        let min_z = (self.pos[2] - 6.0).floor() as i32;
        let max_z = (self.pos[2] + 6.0).floor() as i32;
        for x in min_x.div_euclid(16)..=max_x.div_euclid(16) {
            for z in min_z.div_euclid(16)..=max_z.div_euclid(16) {
                match source.get(x, z, self.biome) {
                    None => ready = false,
                    Some(Err(error)) => {
                        tracing::error!(x,z,%error,"failed to load interaction chunk");
                        return true;
                    }
                    Some(Ok(_)) => {}
                }
            }
        }
        ready
    }
    fn hit(&self, target: ferrite_protocol::types::BlockPos) -> Option<ferrite_math::BlockHit> {
        use ferrite_math::{AABB, Vec3, look_from_yaw_pitch, raycast_shapes};
        use std::borrow::Cow;
        const FULL_BOX: [AABB; 1] = [AABB::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0)];
        let source = self.source.as_ref()?;
        let eye = Vec3::new(self.pos[0], self.pos[1] + 1.62, self.pos[2]);
        let look = look_from_yaw_pitch(self.yaw, self.pitch);
        let distance = if self.gamemode == 1 { 5.0 } else { 4.5 };
        // Vanilla gives levers and buttons an outline shape; the generated block-state report
        // only models collision, so hand-activated components would be unselectable. Give them
        // a full-cell outline, but still let a nearer collision block occlude the ray.
        let outline = source
            .block(target.x, target.y, target.z)
            .and_then(|v| u16::try_from(v).ok())
            .is_some_and(|state| super::redstone::use_block(state).is_some());
        raycast_shapes(eye, look, distance, |p| -> Cow<'static, [AABB]> {
            if outline && (p.x, p.y, p.z) == (target.x, target.y, target.z) {
                return Cow::Borrowed(&FULL_BOX);
            }
            match source
                .block(p.x, p.y, p.z)
                .and_then(|v| u16::try_from(v).ok())
                .filter(|v| usize::from(*v) < ferrite_data::blocks::STATE_COUNT)
            {
                Some(0) => Cow::Borrowed(&[]),
                Some(state) => {
                    let props = ferrite_data::block_props::get(state);
                    if props.is_air {
                        return Cow::Borrowed(&[]);
                    }
                    let shapes = ferrite_data::block_props::collision_shape(state);
                    // Vanilla `ClipContext.Fluid.SOURCE_ONLY`: fluids and interaction blocks
                    // without a collision shape (doors, signs) are still clickable.
                    if shapes.is_empty()
                        || (shapes.len() == 1 && shapes[0] == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0])
                    {
                        Cow::Borrowed(&FULL_BOX)
                    } else {
                        Cow::Owned(
                            shapes
                                .iter()
                                .map(|b| AABB::new(b[0], b[1], b[2], b[3], b[4], b[5]))
                                .collect(),
                        )
                    }
                }
                None => Cow::Borrowed(&FULL_BOX),
            }
        })
        .filter(|hit| (hit.pos.x, hit.pos.y, hit.pos.z) == (target.x, target.y, target.z))
    }
    fn send_block_snapshot(&self, location: i64) {
        let pos = ferrite_protocol::types::BlockPos::unpack(location);
        if let Some(state) = self
            .source
            .as_ref()
            .and_then(|s| s.block(pos.x, pos.y, pos.z))
        {
            send(
                self,
                &BlockUpdate {
                    location,
                    block_state: VarInt(state as i32),
                },
            );
        }
    }
    /// Sends one command block's NBT to every player that has its chunk loaded (vanilla
    /// `ClientboundBlockEntityDataPacket` when a command block opens or is edited).
    pub(super) fn sync_block_entity(&self, pos: [i32; 3]) {
        let Some(source) = &self.source else {
            return;
        };
        let Some(entity) = source.block_entity(pos[0], pos[1], pos[2]) else {
            return;
        };
        let chunk = (pos[0].div_euclid(16), pos[2].div_euclid(16));
        let ferrite_world::BlockEntity { type_id, mut data } = entity;
        data.shift_remove("x");
        data.shift_remove("y");
        data.shift_remove("z");
        let update = packet(&BlockEntityData {
            location: ferrite_protocol::types::BlockPos::new(pos[0], pos[1], pos[2]).pack(),
            block_type: VarInt(i32::try_from(type_id).unwrap_or(0)),
            nbt: data,
        });
        broadcast_tracked_chunk(
            &self.tracking.lock().unwrap(),
            self.dimension,
            chunk,
            &update,
        );
    }

    /// `ServerboundSetCommandBlockPacket`: an operator's editor writes the block entity
    /// (vanilla `CommandBlockEntity` setters), including a mode change of the block itself.
    fn edit_command_block(&mut self, packet: &SetCommandBlock) {
        if self.permission < 2 || self.gamemode == 3 {
            return;
        }
        let Some(source) = &self.source else {
            return;
        };
        let pos = ferrite_protocol::types::BlockPos::unpack(packet.location);
        let pos = [pos.x, pos.y, pos.z];
        if super::command_block::edit(source, pos, packet) {
            self.sync_block_entity(pos);
        }
    }

    fn edit_jigsaw_block(&mut self, packet: &ServerboundSetJigsawBlock) {
        if self.permission < 2 || self.gamemode == 3 || !self.infinite_materials() {
            return;
        }
        let Some(source) = &self.source else {
            return;
        };
        let pos = [packet.pos.x, packet.pos.y, packet.pos.z];
        let mut be = ferrite_nbt::Compound::new();
        be.insert(
            "id".to_owned(),
            ferrite_nbt::Tag::String("minecraft:jigsaw".to_owned()),
        );
        be.insert("x".to_owned(), ferrite_nbt::Tag::Int(pos[0]));
        be.insert("y".to_owned(), ferrite_nbt::Tag::Int(pos[1]));
        be.insert("z".to_owned(), ferrite_nbt::Tag::Int(pos[2]));
        be.insert(
            "name".to_owned(),
            ferrite_nbt::Tag::String(packet.name.clone()),
        );
        be.insert(
            "target".to_owned(),
            ferrite_nbt::Tag::String(packet.target.clone()),
        );
        be.insert(
            "pool".to_owned(),
            ferrite_nbt::Tag::String(packet.pool.clone()),
        );
        be.insert(
            "final_state".to_owned(),
            ferrite_nbt::Tag::String(packet.final_state.clone()),
        );
        be.insert(
            "joint".to_owned(),
            ferrite_nbt::Tag::String(packet.joint.clone()),
        );
        be.insert(
            "selection_priority".to_owned(),
            ferrite_nbt::Tag::Int(packet.selection_priority.0),
        );
        be.insert(
            "placement_priority".to_owned(),
            ferrite_nbt::Tag::Int(packet.placement_priority.0),
        );
        super::commands_world::WorldBlocks::set_block_entity(source.as_ref(), pos, &be);
        self.sync_block_entity(pos);
    }

    pub(crate) fn give_picked_item(&mut self, stack: ferrite_item::ItemStack) {
        if stack.is_empty() || !self.infinite_materials() {
            return;
        }
        // Check if matching item already sits in hotbar (`0..9`) -> select it cleanly.
        for slot in 0..9usize {
            let existing = self.inventory.get(slot);
            if !existing.is_empty() && existing.item == stack.item {
                self.inventory.selected = slot;
                send(
                    self,
                    &SetHeldSlot {
                        slot: VarInt(slot as i32),
                    },
                );
                return;
            }
        }
        let target_slot = if self.inventory.get(self.inventory.selected).is_empty() {
            self.inventory.selected
        } else if let Some(empty) = (0..9usize).find(|&s| self.inventory.get(s).is_empty()) {
            empty
        } else {
            self.inventory.selected
        };
        self.inventory.selected = target_slot;
        self.inventory.set(target_slot, stack.clone());
        let menu_slot = 36 + target_slot;
        self.menus.inventory.set_remote_slot(menu_slot, &stack);
        let out = self.menus.inventory.broadcast_changes(&self.inventory);
        super::container::emit(self, out);
        send(
            self,
            &SetHeldSlot {
                slot: VarInt(target_slot as i32),
            },
        );
    }

    fn pick_item_from_block(&mut self, packet: &ServerboundPickItemFromBlock) {
        if !self.infinite_materials() {
            return;
        }
        let pos = [packet.pos.x, packet.pos.y, packet.pos.z];
        let Some(state) = self
            .source
            .as_ref()
            .and_then(|s| s.block(pos[0], pos[1], pos[2]))
        else {
            return;
        };
        let Ok(state_u16) = u16::try_from(state) else {
            return;
        };
        let Some((block_name, _)) = ferrite_data::blocks::state(state_u16) else {
            return;
        };
        let item_name = match block_name {
            "minecraft:water" => "minecraft:water_bucket",
            "minecraft:lava" => "minecraft:lava_bucket",
            "minecraft:tall_seagrass" => "minecraft:seagrass",
            "minecraft:piston_head" => "minecraft:piston",
            "minecraft:carrots" => "minecraft:carrot",
            "minecraft:potatoes" => "minecraft:potato",
            "minecraft:beetroots" => "minecraft:beetroot_seeds",
            "minecraft:wheat" => "minecraft:wheat_seeds",
            other => other,
        };
        let Some(item_id) = ferrite_item::item_id(item_name) else {
            return;
        };
        self.give_picked_item(ferrite_item::ItemStack::with_count(item_id, 1));
    }

    fn handle_edit_book(&mut self, packet: &ServerboundEditBook) {
        let slot = usize::try_from(packet.slot.0).ok();
        let Some(inv_slot) = slot.filter(|&s| s < 9 || s == ferrite_inventory::OFFHAND) else {
            return;
        };
        let cur = self.inventory.get(inv_slot).clone();
        if ferrite_item::item_name(cur.item) != Some("minecraft:writable_book") {
            return;
        }
        let pages: Vec<ferrite_item::Filterable<String>> = packet
            .pages
            .iter()
            .take(100)
            .map(|p| ferrite_item::Filterable {
                raw: p.chars().take(1024).collect(),
                filtered: None,
            })
            .collect();
        let next = if let Some(title) = &packet.title {
            let Some(written_id) = ferrite_item::item_id("minecraft:written_book") else {
                return;
            };
            let mut stack = ferrite_item::ItemStack::with_count(written_id, 1);
            let title_clean: String = title.chars().take(32).collect();
            stack.components.insert(
                ferrite_item::component_ids::WRITTEN_BOOK_CONTENT,
                ferrite_item::Component::WrittenBookContent(ferrite_item::WrittenBookContent {
                    title: ferrite_item::Filterable {
                        raw: title_clean,
                        filtered: None,
                    },
                    author: self.name.clone(),
                    generation: 0,
                    pages: pages
                        .into_iter()
                        .map(|p| ferrite_item::Filterable {
                            raw: ferrite_text::Component::text(p.raw),
                            filtered: None,
                        })
                        .collect(),
                    resolved: true,
                }),
            );
            stack
        } else {
            let mut stack = cur;
            stack.components.insert(
                ferrite_item::component_ids::WRITABLE_BOOK_CONTENT,
                ferrite_item::Component::WritableBookContent(ferrite_item::WritableBookContent {
                    pages,
                }),
            );
            stack
        };
        self.inventory.set(inv_slot, next);
        let out = self.menus.inventory.broadcast_changes(&self.inventory);
        super::container::emit(self, out);
    }

    /// Applies a block change and tells every player tracking the chunk. `expected` keeps the
    /// compare-and-set used by player interaction; commands pass `None`.
    pub(crate) fn change_block(&self, location: i64, expected: Option<u32>, state: u32) -> bool {
        let pos = ferrite_protocol::types::BlockPos::unpack(location);
        let tracking = self.tracking.lock().unwrap();
        let before = self
            .source
            .as_ref()
            .and_then(|source| source.block(pos.x, pos.y, pos.z))
            .and_then(|old| u16::try_from(old).ok());
        if self.source.as_ref().is_some_and(|source| {
            source
                .compare_set_block(pos.x, pos.y, pos.z, expected, state)
                .is_ok()
        }) {
            let chunk = (pos.x.div_euclid(16), pos.z.div_euclid(16));
            // A removed piston head takes its extended base with it unless the operator allows
            // headless pistons (vanilla PistonHeadBlock; Paper allow-headless-pistons).
            if let (Some(source), Some(before), Ok(after)) =
                (&self.source, before, u16::try_from(state))
                && let Some(base) = super::piston::remove_orphaned_base(
                    source,
                    [pos.x, pos.y, pos.z],
                    before,
                    after,
                    super::piston::allow_headless_pistons(),
                )
            {
                let base_pos = ferrite_protocol::types::BlockPos {
                    x: base[0],
                    y: base[1],
                    z: base[2],
                };
                let cleared = packet(&BlockUpdate {
                    location: base_pos.pack(),
                    block_state: VarInt(0),
                });
                broadcast_tracked_chunk(&tracking, self.world_id, chunk, &cleared);
            }
            // One encoding, shared by every observer.
            let update = packet(&BlockUpdate {
                location,
                block_state: VarInt(state as i32),
            });
            broadcast_tracked_chunk(&tracking, self.world_id, chunk, &update);
            let _ = self.signals.send(Signal::FluidChange {
                dimension: self.dimension,
                x: pos.x,
                y: pos.y,
                z: pos.z,
            });
            true
        } else {
            false
        }
    }
    /// Breaks a block (to air); an inventory block drops its contents (vanilla
    /// `Containers.dropContentsOnDestroy`).
    fn break_block(&mut self, location: i64, state: u32) -> bool {
        if !self.change_block(location, Some(state), 0) {
            return false;
        }
        let pos = ferrite_protocol::types::BlockPos::unpack(location);
        let pos = [pos.x, pos.y, pos.z];
        self.changed_blocks.push(pos);
        if let Some(source) = &self.source {
            let name = u16::try_from(state)
                .ok()
                .and_then(ferrite_data::blocks::state)
                .map(|(name, _)| name);
            if name.is_some_and(super::beehive::is_hive) {
                // Vanilla `BeehiveBlock.playerDestroy` drops the item before it empties the
                // hive, so a silk-touch item keeps the bees. The occupants are released by the
                // region tick, where a bee can cross an ownership boundary safely.
                let (components, data) = super::beehive::take(source, pos);
                self.menus.broken_entities.insert(pos, components);
                self.broken_hives
                    .push((pos, u16::try_from(state).unwrap_or(0), self.uuid, data));
            } else if name.is_some_and(super::block_container::is_shulker_box) {
                // A shulker box keeps its contents in the item it drops (vanilla
                // `ShulkerBoxBlock`), so they are not spilled here.
                let components = super::shulker::take_components(source, pos);
                self.menus.broken_entities.insert(pos, components);
            } else {
                for stack in super::block_container::take_all_contents(source, pos, state) {
                    self.menus.spilled.push((pos, stack));
                }
            }
            if name == Some("minecraft:beacon") {
                let _ = source.update_block_entity(pos[0], pos[1], pos[2], |entity| {
                    (entity.take().is_some(), ())
                });
            }
            if !super::targeting::is_untargetable_mode(self.gamemode)
                && name
                    .is_some_and(|n| ferrite_data::tags::contains("block", "guarded_by_piglins", n))
            {
                self.piglin_alerts.push(false);
            }
        }
        true
    }
    /// Vanilla `ShulkerBoxBlock.playerWillDestroy`: a creative player who breaks a shulker box
    /// with contents still drops the box, carrying them (other blocks drop nothing in creative).
    fn drop_creative_shulker(&mut self, location: i64, state: u32) {
        let at = ferrite_protocol::types::BlockPos::unpack(location);
        let at = [at.x, at.y, at.z];
        let Some(components) = self.menus.broken_entities.remove(&at) else {
            return;
        };
        if !super::shulker::has_contents(&components) {
            return;
        }
        let item = u16::try_from(state)
            .ok()
            .and_then(ferrite_data::blocks::state)
            .and_then(|(name, _)| super::shulker::item_with(name, &components));
        if let Some(item) = item {
            self.menus.spilled.push((at, item));
        }
    }
    /// `Player#breakBlock`: breaks the block as this player would, and says whether it broke.
    /// A refusal (air, an unbreakable block for a non-creative player, a mode that cannot
    /// break, a game-master block without permission, a cancelled event) leaves it standing.
    fn plugin_break_block(&mut self, position: [i32; 3]) -> bool {
        let Some(state) = self
            .source
            .as_ref()
            .and_then(|s| s.block(position[0], position[1], position[2]))
        else {
            return false;
        };
        if !super::interaction::may_break(self.gamemode, self.permission, state) {
            return false;
        }
        let clicked = crate::game::plugin_types::BlockPosition {
            x: position[0],
            y: position[1],
            z: position[2],
        };
        let world = plugin_world_name(self.world_id);
        if !self.plugin_allows(|plugins| {
            plugins.block_broken(
                self.uuid,
                &self.name,
                crate::game::plugin_types::BlockBreak {
                    world: &world,
                    block: clicked,
                    material: material_name(state),
                },
            )
        }) {
            return false;
        }
        let at = ferrite_protocol::types::BlockPos::new(position[0], position[1], position[2]);
        if !self.break_block(at.pack(), state) {
            return false;
        }
        self.broken.push((at, state));
        if self.gamemode == 0 {
            self.survival.food.add_exhaustion(0.005);
        }
        true
    }
    fn place(&mut self, p: &UseItemOn) {
        if !(self.gamemode == 0 || self.gamemode == 1)
            || !(0..=1).contains(&p.hand.0)
            || !(0..=5).contains(&p.face.0)
            || ![p.cursor_x, p.cursor_y, p.cursor_z]
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            || p.world_border_hit
        {
            return;
        }
        let clicked = ferrite_protocol::types::BlockPos::unpack(p.location);
        if !self
            .hit(clicked)
            .is_some_and(|hit| hit.face == ferrite_math::Direction::ALL[p.face.0 as usize])
        {
            return;
        }
        let Some(source) = &self.source else { return };
        let Some(clicked_state) = source
            .block(clicked.x, clicked.y, clicked.z)
            .and_then(|s| u16::try_from(s).ok())
        else {
            return;
        };
        // Vanilla `CampfireBlock.useItemOn` -> `CampfireBlockEntity.placeFood`: food goes on the
        // first empty slot, whatever the campfire's lit state.
        if ferrite_data::blocks::state(clicked_state).is_some_and(|(name, _)| {
            matches!(name, "minecraft:campfire" | "minecraft:soul_campfire")
        }) {
            let slot = self.hand_slot(p.hand.0);
            let held = self.inventory.get(slot).clone();
            if !held.is_empty()
                && let Some(recipe) = super::campfire_recipe(&held)
                && super::campfire::place_food(
                    source.as_ref(),
                    [clicked.x, clicked.y, clicked.z],
                    &held,
                    recipe.cook_time,
                )
            {
                if self.gamemode != 1 {
                    self.inventory
                        .set(slot, held.copy_with_count(held.count - 1));
                }
                self.sync_block_entity([clicked.x, clicked.y, clicked.z]);
                return;
            }
        }
        // Vanilla `LeverBlock.use` / `ButtonBlock.use`: a right-click toggles the component
        // instead of placing the held item.
        if let Some(next) = super::redstone::use_block(clicked_state) {
            if self.change_block(p.location, Some(u32::from(clicked_state)), u32::from(next)) {
                self.changed_blocks.push([clicked.x, clicked.y, clicked.z]);
            }
            return;
        }
        // Vanilla `CommandBlock.useWithoutItem`: an operator's right-click opens the editor,
        // which needs the block entity on the client. Nobody may place against the block.
        if ferrite_data::blocks::state(clicked_state)
            .is_some_and(|(name, _)| super::command_block::Mode::of_block(name).is_some())
        {
            if self.permission >= 2 {
                self.sync_block_entity([clicked.x, clicked.y, clicked.z]);
            }
            return;
        }
        // Vanilla `EnderEyeItem.useOn`: an eye fills an empty end portal frame, and a ring of
        // twelve filled frames lights the portal.
        let held = self.inventory.get(self.hand_slot(p.hand.0)).clone();
        if held.name() == Some("minecraft:ender_eye")
            && let Some(filled) = super::end_frame::with_eye(clicked_state)
        {
            if self.change_block(
                p.location,
                Some(u32::from(clicked_state)),
                u32::from(filled),
            ) {
                self.changed_blocks.push([clicked.x, clicked.y, clicked.z]);
                if self.gamemode == 0 || self.gamemode == 2 {
                    let slot = self.hand_slot(p.hand.0);
                    self.inventory
                        .set(slot, held.copy_with_count(held.count - 1));
                }
                let read = |x, y, z| {
                    source
                        .block(x, y, z)
                        .and_then(|state| u16::try_from(state).ok())
                };
                if let Some(center) =
                    super::end_frame::complete_ring(&read, [clicked.x, clicked.y, clicked.z])
                    && let Some(blocks) = super::end_frame::portal_blocks(center)
                {
                    for (at, state) in blocks {
                        let location = ferrite_protocol::types::BlockPos::new(at[0], at[1], at[2]);
                        if self.change_block(location.pack(), None, u32::from(state)) {
                            self.changed_blocks.push(at);
                        }
                    }
                }
            }
            return;
        }
        // Vanilla `EndCrystalItem.useOn`: a crystal goes on obsidian or bedrock with room above.
        if held.name() == Some("minecraft:end_crystal")
            && ferrite_data::blocks::state(clicked_state)
                .is_some_and(|(name, _)| matches!(name, "minecraft:obsidian" | "minecraft:bedrock"))
        {
            let free = |dy| {
                source
                    .block(clicked.x, clicked.y + dy, clicked.z)
                    .and_then(|s| u16::try_from(s).ok())
                    .is_some_and(|s| {
                        usize::from(s) < ferrite_data::blocks::STATE_COUNT
                            && ferrite_data::block_props::get(s).is_air
                    })
            };
            if free(1) && free(2) {
                self.crystals.push([clicked.x, clicked.y + 1, clicked.z]);
                if self.gamemode != 1 {
                    let slot = self.hand_slot(p.hand.0);
                    self.inventory
                        .set(slot, held.copy_with_count(held.count - 1));
                }
            }
            return;
        }
        // Vanilla `SpawnEggItem.useOn`: spawn the egg's mob on the clicked face and, in
        // survival, use the egg up. A replaceable block is spawned inside, like any placement.
        if let Some(kind) = spawn_egg_kind(held.name()) {
            let face = ferrite_math::Direction::ALL[p.face.0 as usize];
            let (dx, dy, dz) = if ferrite_data::block_props::get(clicked_state).replaceable {
                (0, 0, 0)
            } else {
                face.step()
            };
            let pos = [
                f64::from(clicked.x + dx) + 0.5,
                f64::from(clicked.y + dy),
                f64::from(clicked.z + dz) + 0.5,
            ];
            let chunk = ChunkPos((pos[0].floor() as i32) >> 4, (pos[2].floor() as i32) >> 4);
            if self
                .signals
                .send(Signal::Summon {
                    world_id: self.world_id,
                    chunk,
                    kind,
                    pos,
                })
                .is_ok()
                && self.gamemode == 0
            {
                let slot = self.hand_slot(p.hand.0);
                self.inventory
                    .set(slot, held.copy_with_count(held.count - 1));
            }
            return;
        }
        // Vanilla `FlintAndSteelItem.useOn` / `FireChargeItem.useOn` on TNT: prime it.
        if matches!(
            held.name(),
            Some("minecraft:flint_and_steel" | "minecraft:fire_charge")
        ) && ferrite_data::blocks::state(clicked_state)
            .is_some_and(|(name, _)| name == "minecraft:tnt")
        {
            if self.change_block(p.location, Some(u32::from(clicked_state)), 0) {
                self.changed_blocks.push([clicked.x, clicked.y, clicked.z]);
                self.primed_tnt
                    .push(([clicked.x, clicked.y, clicked.z], ferrite_entity::tnt::FUSE));
                if self.gamemode == 0 && held.name() == Some("minecraft:fire_charge") {
                    let slot = self.hand_slot(p.hand.0);
                    self.inventory
                        .set(slot, held.copy_with_count(held.count - 1));
                }
            }
            return;
        }
        // Vanilla `FlintAndSteelItem.useOn` / `FireChargeItem.useOn`: light the block face.
        if matches!(
            held.name(),
            Some("minecraft:flint_and_steel" | "minecraft:fire_charge")
        ) {
            let face = ferrite_math::Direction::ALL[p.face.0 as usize];
            let (dx, dy, dz) = if ferrite_data::block_props::get(clicked_state).replaceable {
                (0, 0, 0)
            } else {
                face.step()
            };
            let target = ferrite_protocol::types::BlockPos::new(
                clicked.x + dx,
                clicked.y + dy,
                clicked.z + dz,
            );
            let Some(old) = source
                .block(target.x, target.y, target.z)
                .and_then(|s| u16::try_from(s).ok())
            else {
                return;
            };
            // Vanilla `FlintAndSteelItem.useOn` lights an obsidian frame before fire.
            if let Some(blocks) =
                super::portal::frame_blocks(source.as_ref(), [target.x, target.y, target.z])
            {
                for (at, state) in blocks {
                    let location = ferrite_protocol::types::BlockPos::new(at[0], at[1], at[2]);
                    if self.change_block(location.pack(), None, u32::from(state)) {
                        self.changed_blocks.push(at);
                    }
                }
                return;
            }
            // Vanilla `BaseFireBlock.getState`: soul fire over soul sand or soul soil.
            if let Some(state) =
                super::fire::placement_state(source.as_ref(), [target.x, target.y, target.z])
                && self.change_block(target.pack(), Some(u32::from(old)), u32::from(state))
            {
                self.changed_blocks.push([target.x, target.y, target.z]);
                self.ignited.push([target.x, target.y, target.z]);
                if self.gamemode == 0 && held.name() == Some("minecraft:fire_charge") {
                    let slot = self.hand_slot(p.hand.0);
                    self.inventory
                        .set(slot, held.copy_with_count(held.count - 1));
                }
            }
            return;
        }
        let face = ferrite_math::Direction::ALL[p.face.0 as usize];
        let (dx, dy, dz) = if ferrite_data::block_props::get(clicked_state).replaceable {
            (0, 0, 0)
        } else {
            face.step()
        };
        let target =
            ferrite_protocol::types::BlockPos::new(clicked.x + dx, clicked.y + dy, clicked.z + dz);
        let Some(old) = source
            .block(target.x, target.y, target.z)
            .and_then(|s| u16::try_from(s).ok())
        else {
            return;
        };
        if !ferrite_data::block_props::get(old).replaceable {
            return;
        }
        let slot = self.hand_slot(p.hand.0);
        let item = self.inventory.get(slot).clone();
        if item.is_empty() {
            return;
        }
        let item_id = i32::from(item.item);
        // Vanilla `BucketItem.place` / `SolidBucketItem.useOn`: fluid and powder snow buckets
        // place their block.
        let bucket_name = match item.name() {
            Some("minecraft:water_bucket") => Some("water"),
            Some("minecraft:lava_bucket") => Some("lava"),
            Some("minecraft:powder_snow_bucket") => Some("powder_snow"),
            _ => None,
        };
        let state = if let Some(fluid) = bucket_name {
            if fluid == "water"
                && super::blocks::can_waterlog(old)
                && let Some(wet) = super::blocks::with_props(old, &[("waterlogged", "true")])
            {
                wet
            } else {
                ferrite_data::blocks::default_state(fluid).expect("vanilla bucket block")
            }
        } else {
            let Some(state) =
                super::interaction::placement_state(item_id, p.face.0, p.cursor_y, self.yaw, old)
            else {
                return;
            };
            state
        };
        // Vanilla `BlockItem.place` -> `BlockPlaceContext.canPlace` -> `canSurvive`.
        if !super::neighbors::can_survive(state, [target.x, target.y, target.z], source.as_ref()) {
            self.send_block_snapshot(target.pack());
            return;
        }
        if !self.plugin_allows(|plugins| {
            plugins.block_placed(
                self.uuid,
                &self.name,
                crate::game::plugin_types::BlockPosition {
                    x: target.x,
                    y: target.y,
                    z: target.z,
                },
                u8::try_from(p.face.0.clamp(0, 5)).unwrap_or(0),
                material_name(u32::from(state)),
                material_name(u32::from(old)),
            )
        }) {
            self.send_block_snapshot(target.pack());
            return;
        }
        let positions = self.positions.lock().unwrap();
        let offset = ferrite_math::Vec3::new(target.x as f64, target.y as f64, target.z as f64);
        if positions.iter().any(|(uuid, pos)| {
            let pos = if *uuid == self.uuid { &self.pos } else { pos };
            let player_box = ferrite_math::AABB::new(
                pos[0] - 0.3,
                pos[1],
                pos[2] - 0.3,
                pos[0] + 0.3,
                pos[1] + 1.8,
                pos[2] + 0.3,
            );
            ferrite_data::block_props::collision_shape(state)
                .iter()
                .any(|b| {
                    player_box.intersects(
                        ferrite_math::AABB::new(b[0], b[1], b[2], b[3], b[4], b[5]).move_by(offset),
                    )
                })
        }) {
            return;
        }
        let placed = self.change_block(target.pack(), Some(u32::from(old)), u32::from(state));
        drop(positions);
        if placed {
            self.changed_blocks.push([target.x, target.y, target.z]);
            if ferrite_data::blocks::state(state).is_some_and(|(name, _)| {
                matches!(
                    name,
                    "minecraft:carved_pumpkin"
                        | "minecraft:jack_o_lantern"
                        | "minecraft:wither_skeleton_skull"
                        | "minecraft:wither_skeleton_wall_skull"
                )
            }) {
                self.placed_pumpkins.push([target.x, target.y, target.z]);
            }
            if let Some((name, props)) = ferrite_data::blocks::state(state) {
                super::recipe_unlock::PLACED
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(super::recipe_unlock::BlockRecord {
                        player: self.uuid,
                        block: name,
                        props,
                    });
            }
            // Block entities are created with the block, so the menu, /data and the chunk save
            // all find the state they expect.
            match ferrite_data::blocks::state(state).map(|(name, _)| name) {
                Some("minecraft:brewing_stand") => {
                    if let Some(source) = self.source.clone() {
                        super::brewing::ensure(source.as_ref(), [target.x, target.y, target.z]);
                    }
                }
                Some("minecraft:beacon") => {
                    if let Some(source) = self.source.clone() {
                        super::beacon::ensure(source.as_ref(), [target.x, target.y, target.z]);
                    }
                }
                Some("minecraft:campfire" | "minecraft:soul_campfire") => {
                    if let Some(source) = self.source.clone() {
                        super::campfire::ensure(source.as_ref(), [target.x, target.y, target.z]);
                    }
                }
                Some(block) if super::beehive::is_hive(block) => {
                    if let Some(source) = self.source.clone() {
                        super::beehive::ensure(source.as_ref(), [target.x, target.y, target.z]);
                    }
                }
                Some(block) if super::block_container::is_shulker_box(block) => {
                    // The item's contents, name and lock go into the new block entity; a
                    // nested shulker box in it is handed back instead of being placed.
                    if let Some(source) = self.source.clone() {
                        let at = [target.x, target.y, target.z];
                        for stack in super::shulker::apply_item(source.as_ref(), at, &item) {
                            self.menus.spilled.push((at, stack));
                        }
                    }
                }
                _ => {}
            }
        }
        // Survival uses the item up (vanilla `BlockItem.place`: `shrink(1)` unless the player
        // has infinite materials); the client gets the slot with the tick's `broadcastChanges`.
        if placed && self.gamemode == 0 {
            if bucket_name.is_some() {
                self.inventory.set(
                    slot,
                    ferrite_item::ItemStack::from_name("bucket").expect("vanilla bucket"),
                );
            } else {
                self.inventory
                    .set(slot, item.copy_with_count(item.count - 1));
            }
        }
    }
    /// Runs an event hook; a missing bridge or a failed call is decided by the
    /// configured [`PluginEventPolicy`]. Callers cancel the action on `false`,
    /// which resends the original block or teleport state.
    fn plugin_allows(
        &self,
        event: impl FnOnce(
            &dyn crate::game::plugin_types::ServerHooks,
        ) -> Result<bool, crate::game::plugin_types::Error>,
    ) -> bool {
        event_allows(self.plugins.as_deref(), self.plugin_event_errors, event)
    }

    fn feedback(&self, feedback: Option<&OutboundSender>, text: String) {
        match feedback {
            Some(tx) => {
                let _ = tx.send(packet(&SystemChat {
                    content: ferrite_protocol::types::PlainText(text),
                    overlay: false,
                }));
            }
            None => tracing::info!("{text}"),
        }
    }
    /// `/clear`: removes up to `max_count` matching items (all when `None`).
    fn clear_items(
        &mut self,
        item: Option<u16>,
        max_count: Option<i32>,
        target_name: &str,
        feedback: Option<&OutboundSender>,
    ) {
        let matches = |stack: &ferrite_item::ItemStack| {
            !stack.is_empty() && item.is_none_or(|id| stack.item == id)
        };
        if max_count == Some(0) {
            let found: i32 = (0..ferrite_inventory::SIZE)
                .map(|slot| self.inventory.get(slot))
                .filter(|stack| matches(stack))
                .map(|stack| stack.count)
                .sum();
            let text = if found == 0 {
                format!("No items were found on player {target_name}")
            } else {
                format!("Found {found} matching item(s) on player {target_name}")
            };
            self.feedback(feedback, text);
            return;
        }
        let limit = max_count.unwrap_or(i32::MAX);
        let mut removed = 0;
        for slot in 0..ferrite_inventory::SIZE {
            if removed >= limit {
                break;
            }
            if !matches(self.inventory.get(slot)) {
                continue;
            }
            let take = self.inventory.get(slot).count.min(limit - removed);
            removed += self.inventory.remove(slot, take).count;
        }
        if removed == 0 {
            self.feedback(
                feedback,
                format!("No items were found on player {target_name}"),
            );
            return;
        }
        self.sync_containers();
        self.feedback(
            feedback,
            format!("Removed {removed} items from {target_name}"),
        );
    }
    /// `-1` is the selected hotbar slot; other values must be valid inventory indexes.
    fn resolve_slot(&self, slot: i16) -> Option<usize> {
        if slot == -1 {
            return Some(self.inventory.selected_slot());
        }
        usize::try_from(slot)
            .ok()
            .filter(|slot| *slot < ferrite_inventory::SIZE)
    }

    /// Stores `stack` in `slot` and sends it to the client.
    fn put_slot(&mut self, slot: usize, stack: ferrite_item::ItemStack) {
        self.inventory.set(slot, stack.clone());
        send(
            self,
            &SetPlayerInventory {
                slot: VarInt(slot as i32),
                item: stack,
            },
        );
    }

    fn fill_slots(&mut self, slots: &[i16], fill: SlotFill) {
        let slots: Vec<usize> = slots.iter().filter_map(|s| self.resolve_slot(*s)).collect();
        let (mode, stacks) = match fill {
            SlotFill::Set(stacks) => (super::commands::SlotMode::Replace, stacks),
            SlotFill::Override(stacks) => (super::commands::SlotMode::Override, stacks),
            SlotFill::Cycle(stacks) => (super::commands::SlotMode::Fill, stacks),
        };
        let assigned = super::slot_ops::distribute(mode, &stacks, slots.len());
        for (slot, stack) in slots.iter().zip(assigned) {
            if let Some(stack) = stack {
                self.put_slot(*slot, stack);
            }
        }
        self.sync_containers();
    }

    fn set_inventory_slot(
        &mut self,
        slot: u8,
        item: Option<crate::game::plugin_types::ItemSnapshot>,
    ) {
        let slot = usize::from(slot);
        if slot >= ferrite_inventory::SIZE {
            return;
        }
        let stack = match item {
            None => ferrite_item::ItemStack::empty(),
            Some(item) => {
                let Some(id) = ferrite_item::item_id(&item.material) else {
                    tracing::warn!(material = %item.material, "plugin set an unknown item");
                    return;
                };
                ferrite_item::ItemStack::with_count(id, i32::from(item.amount))
            }
        };
        self.inventory.set(slot, stack.clone());
        send(
            self,
            &SetPlayerInventory {
                slot: VarInt(slot as i32),
                item: stack,
            },
        );
        self.sync_containers();
    }

    /// Writes plugin item metadata onto the stack in a slot; an empty slot is ignored.
    fn set_item_meta(&mut self, slot: u8, meta: &ItemMetaFields) {
        let slot = usize::from(slot);
        if slot >= ferrite_inventory::SIZE || self.inventory.get(slot).is_empty() {
            return;
        }
        let stack = with_meta(self.inventory.get(slot).clone(), meta);
        self.inventory.set(slot, stack.clone());
        send(
            self,
            &SetPlayerInventory {
                slot: VarInt(slot as i32),
                item: stack,
            },
        );
        self.sync_containers();
    }

    /// Points the filled map in `slot` at saved map `id`; any other stack is left alone.
    fn set_item_map_id(&mut self, slot: u8, id: i32) {
        let slot = usize::from(slot);
        if slot >= ferrite_inventory::SIZE {
            return;
        }
        let Some(stack) = with_map_id(self.inventory.get(slot).clone(), id) else {
            return;
        };
        self.inventory.set(slot, stack.clone());
        send(
            self,
            &SetPlayerInventory {
                slot: VarInt(slot as i32),
                item: stack,
            },
        );
        self.sync_containers();
    }

    /// Sets the wear of the stack in a slot; an empty slot or an item that cannot wear is ignored.
    fn set_item_damage(&mut self, slot: u8, damage: i32) {
        let slot = usize::from(slot);
        if slot >= ferrite_inventory::SIZE {
            return;
        }
        let Some(stack) = worn_stack(self.inventory.get(slot).clone(), damage) else {
            return;
        };
        self.inventory.set(slot, stack.clone());
        send(
            self,
            &SetPlayerInventory {
                slot: VarInt(slot as i32),
                item: stack,
            },
        );
        self.sync_containers();
    }

    /// `/give`: places the stack back in the inventory, dropping what does not fit.
    fn give_items(
        &mut self,
        item: u16,
        count: i32,
        components: &[(u16, ferrite_nbt::Tag)],
        item_name: &str,
        target_name: &str,
        feedback: Option<&OutboundSender>,
    ) {
        let mut stack = ferrite_item::ItemStack::with_count(item, count);
        for (id, tag) in components {
            stack
                .components
                .insert(*id, ferrite_item::Component::from_nbt(*id, tag));
        }
        let leftover = self.inventory.place_back(stack);
        let given = count - leftover.as_ref().map_or(0, |stack| stack.count);
        if let Some(leftover) = leftover {
            self.menus.thrown.push(leftover);
        }
        if given > 0 {
            self.sync_containers();
        }
        self.feedback(
            feedback,
            format!("Gave {given} {item_name} to {target_name}"),
        );
    }
    /// `/enchant`: adds or replaces the enchantment on the main-hand item.
    fn enchant_item(
        &mut self,
        enchantment: &str,
        level: i32,
        target_name: &str,
        feedback: Option<&OutboundSender>,
    ) {
        let slot = self.inventory.selected_slot();
        let mut stack = self.inventory.get(slot).clone();
        if stack.is_empty() {
            self.feedback(
                feedback,
                super::feedback::render("commands.enchant.failed.itemless", &[target_name]),
            );
            return;
        }
        let mut enchantments = match stack
            .components
            .get(ferrite_item::component_ids::ENCHANTMENTS)
        {
            Some(ferrite_item::Component::Enchantments(enchantments)) => enchantments.clone(),
            _ => ferrite_item::Enchantments::new(),
        };
        enchantments.set(enchantment.to_owned(), level);
        stack.components.insert(
            ferrite_item::component_ids::ENCHANTMENTS,
            ferrite_item::Component::Enchantments(enchantments),
        );
        self.inventory.set(slot, stack);
        self.sync_containers();
        let path = enchantment
            .strip_prefix("minecraft:")
            .unwrap_or(enchantment);
        let name = path
            .split('_')
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        self.feedback(
            feedback,
            super::feedback::render("commands.enchant.success.single", &[&name, target_name]),
        );
    }
    fn set_experience_total(&mut self, total: i64) {
        let total = total.clamp(0, i64::from(i32::MAX)) as i32;
        let (level, progress) = level_and_progress(total);
        self.experience_total = total;
        self.experience_level = level;
        self.experience_progress = progress;
    }
    /// Adds experience points, as taking a furnace result or a grindstone output does (vanilla
    /// `Player.giveExperiencePoints`). Fractional points are rounded down, like vanilla's
    /// `Mth.floor` of the accumulated recipe experience.
    pub fn add_experience(&mut self, points: f32) {
        let points = points.floor();
        if !points.is_finite() || points <= 0.0 {
            return;
        }
        self.set_experience_total(i64::from(self.experience_total) + points as i64);
        send(
            self,
            &SetExperience {
                progress: self.experience_progress,
                level: VarInt(self.experience_level),
                total: VarInt(self.experience_total),
            },
        );
    }

    /// Spends experience levels, as taking an anvil result does (vanilla
    /// `Player.giveExperienceLevels(-cost)`).
    pub fn spend_levels(&mut self, levels: i32) {
        if levels <= 0 {
            return;
        }
        self.set_experience_level(self.experience_level.saturating_sub(levels));
        send(
            self,
            &SetExperience {
                progress: self.experience_progress,
                level: VarInt(self.experience_level),
                total: VarInt(self.experience_total),
            },
        );
    }

    fn set_experience_level(&mut self, level: i32) {
        let level = level.clamp(0, 21_863);
        self.experience_level = level;
        self.experience_progress = 0.0;
        self.experience_total = total_for_level(level).min(i64::from(i32::MAX)) as i32;
    }
    /// `/xp`: add, set or query experience points/levels.
    fn apply_experience(
        &mut self,
        operation: XpOperation,
        amount: i32,
        unit: XpUnit,
        target_name: &str,
        feedback: Option<&OutboundSender>,
    ) {
        if operation == XpOperation::Query {
            let text = match unit {
                XpUnit::Points => {
                    format!("{target_name} has {} experience", self.experience_total)
                }
                XpUnit::Levels => format!(
                    "{target_name} has {} experience levels",
                    self.experience_level
                ),
            };
            self.feedback(feedback, text);
            return;
        }
        let old_level = self.experience_level;
        let old_total = self.experience_total;
        match (operation, unit) {
            (XpOperation::Add, XpUnit::Points) => {
                self.set_experience_total(i64::from(self.experience_total) + i64::from(amount));
            }
            (XpOperation::Add, XpUnit::Levels) => {
                self.set_experience_level(self.experience_level.saturating_add(amount));
            }
            (XpOperation::Set, XpUnit::Points) => self.set_experience_total(i64::from(amount)),
            (XpOperation::Set, XpUnit::Levels) => self.set_experience_level(amount),
            (XpOperation::Query, _) => unreachable!(),
        }
        if let Some(plugins) = self.plugins.clone() {
            if self.experience_level != old_level
                && let Ok(level) = plugins.player_level_changed(
                    self.uuid,
                    &self.name,
                    old_level,
                    self.experience_level,
                )
            {
                let level = level.clamp(0, 21_863);
                if level != self.experience_level {
                    self.set_experience_level(level);
                }
            }
            let gained = self.experience_total - old_total;
            if gained != 0
                && let Ok(amount) = plugins.player_exp_changed(self.uuid, &self.name, gained)
                && amount != gained
            {
                self.set_experience_total(
                    i64::from(self.experience_total) + i64::from(amount - gained),
                );
            }
        }
        send(
            self,
            &SetExperience {
                progress: self.experience_progress,
                level: VarInt(self.experience_level),
                total: VarInt(self.experience_total),
            },
        );
        let text = match operation {
            XpOperation::Add if amount < 0 => match unit {
                XpUnit::Points => format!(
                    "Took {} experience from {target_name}",
                    amount.unsigned_abs()
                ),
                XpUnit::Levels => {
                    format!("Took {} levels from {target_name}", amount.unsigned_abs())
                }
            },
            XpOperation::Add => match unit {
                XpUnit::Points => format!("Gave {amount} experience to {target_name}"),
                XpUnit::Levels => format!("Gave {amount} levels to {target_name}"),
            },
            XpOperation::Set => match unit {
                XpUnit::Points => {
                    format!("Set {target_name}'s experience to {amount}")
                }
                XpUnit::Levels => {
                    format!("Set {target_name}'s experience level to {amount}")
                }
            },
            XpOperation::Query => unreachable!(),
        };
        self.feedback(feedback, text);
    }
    /// Applies damage through the vanilla `LivingEntity.hurt` rules, with the game-mode
    /// immunity checks of `Player.isInvulnerableTo`.
    pub(crate) fn hurt(&mut self, kind: DamageKind, amount: f32) -> bool {
        self.hurt_with_exhaustion(kind, amount, 0.1)
    }

    pub(crate) fn hurt_with_exhaustion(
        &mut self,
        kind: DamageKind,
        amount: f32,
        exhaustion: f32,
    ) -> bool {
        if self.gamemode == 3 || (self.gamemode == 1 && kind.ignores_creative()) {
            return false;
        }
        // `FireResistanceMobEffect`: immune to fire and lava, and the fire goes out.
        if self.effects.fire_resistance() && kind.is_fire() {
            self.survival.fire_ticks = 0;
            return false;
        }
        // `LivingEntity.getDamageAfterArmorAbsorb` includes the resistance attribute unless the
        // damage type is in `#minecraft:bypasses_resistance`.
        let reduction = if kind.bypasses_resistance() {
            0.0
        } else {
            self.effects.attributes().damage_reduction as f32
        };
        let amount = (amount * (1.0 - reduction)).max(0.0);
        if amount <= 0.0 {
            return false;
        }
        let Some(amount) = plugin_damage(
            self.plugins.as_deref(),
            self.plugin_event_errors,
            amount,
            |plugins| {
                plugins.entity_damaged(self.uuid, &self.name, damage_cause(kind), f64::from(amount))
            },
        ) else {
            return false;
        };
        if !self.survival.damage(kind, amount) {
            return false;
        }
        // `Player.hurt` -> `causeFoodExhaustion(source.getFoodExhaustion())`.
        self.survival.food.add_exhaustion(exhaustion);
        self.breaking = None;
        self.survival.eating = None;
        // `Player.checkTotemDeathProtection`: the totem revives before `ServerPlayer.die`.
        if self.survival.dead && self.try_totem() {
            self.sync_health();
            return true;
        }
        // Vanilla `ServerPlayer.die` sends the combat-kill packet before the zero-health update,
        // so fatal damage is synced by the region's death processing instead.
        send(
            self,
            &HurtAnimation {
                id: VarInt(self.entity),
                yaw: self.yaw,
            },
        );
        let hurt_sound = if self.survival.dead {
            "minecraft:entity.player.death"
        } else if kind.is_fire() {
            "minecraft:entity.player.hurt_on_fire"
        } else if matches!(kind, DamageKind::Drown) {
            "minecraft:entity.player.hurt_drown"
        } else {
            "minecraft:entity.player.hurt"
        };
        if let Some(sound) = ferrite_data::registries::id("minecraft:sound_event", hurt_sound) {
            send(
                self,
                &SoundEffect {
                    sound: super::commands_extras::sound_holder(sound),
                    source: VarInt(7), // SoundSource.PLAYERS
                    x: (self.pos[0] * 8.0) as i32,
                    y: (self.pos[1] * 8.0) as i32,
                    z: (self.pos[2] * 8.0) as i32,
                    volume: 1.0,
                    pitch: 1.0,
                    seed: 0,
                },
            );
        }
        if !self.survival.dead {
            self.sync_health();
        }
        true
    }

    /// Applies one effect instance and, when it is not instantaneous, sends
    /// `ClientboundUpdateMobEffectPacket` (unless a stronger instance kept the slot).
    pub(super) fn apply_effect(&mut self, instance: MobEffectInstance) {
        let kind = instance.kind();
        let instant = kind.is_instant();
        self.apply_effect_quiet(instance);
        if !instant
            && self
                .effects
                .get(kind)
                .is_some_and(|current| *current == instance)
        {
            send(
                self,
                &UpdateMobEffect {
                    entity_id: VarInt(self.entity),
                    effect: VarInt(instance.effect),
                    amplifier: VarInt(instance.amplifier),
                    duration: VarInt(instance.duration),
                    flags: instance.flags(),
                },
            );
        }
    }

    /// Removes one effect and tells the client (`LivingEntity.removeEffect`).
    pub(super) fn remove_effect(&mut self, kind: Effect) {
        if self.effects.get(kind).is_none() {
            return;
        }
        let events = self.effects.remove(kind);
        send(
            self,
            &RemoveMobEffect {
                entity_id: VarInt(self.entity),
                effect: VarInt(kind.id()),
            },
        );
        self.apply_effect_events(events);
        self.refresh_attributes();
    }

    /// Stores the effect without a packet (the world loop already announced it).
    fn apply_effect_quiet(&mut self, instance: MobEffectInstance) {
        let events = self.effects.add(instance);
        self.apply_effect_events(events);
        self.refresh_attributes();
    }

    /// An instant effect at `scale` of its strength (splash potions and lingering clouds).
    fn apply_instant_scaled(&mut self, instance: MobEffectInstance, scale: f64) {
        let events = ActiveEffects::instant_events(instance, scale);
        self.apply_effect_events(events);
        self.refresh_attributes();
    }

    /// `LivingEntity.tickEffects`: ticks the store and sends removals for expired effects.
    fn tick_effects(&mut self) {
        let ticks = self.effects.tick();
        for expired in &ticks.expired {
            send(
                self,
                &RemoveMobEffect {
                    entity_id: VarInt(self.entity),
                    effect: VarInt(expired.effect),
                },
            );
        }
        self.apply_effect_events(ticks.events);
        self.refresh_attributes();
    }

    /// Milk and respawn: remove every effect, telling the client about each one.
    fn clear_effects(&mut self) {
        let (removed, events) = self.effects.clear();
        for instance in removed {
            send(
                self,
                &RemoveMobEffect {
                    entity_id: VarInt(self.entity),
                    effect: VarInt(instance.effect),
                },
            );
        }
        self.apply_effect_events(events);
        self.refresh_attributes();
    }

    /// Applies the health, food and absorption side of the ticked effects.
    fn apply_effect_events(&mut self, events: Vec<EffectEvent>) {
        for event in events {
            match event {
                EffectEvent::Heal(amount) => {
                    self.survival.heal(amount);
                }
                EffectEvent::Damage {
                    amount,
                    can_kill,
                    wither,
                } => {
                    // `PoisonMobEffect` cannot take the last health point.
                    let allowed = if can_kill {
                        amount
                    } else {
                        (self.survival.health - 1.0).max(0.0).min(amount)
                    };
                    if allowed > 0.0 {
                        self.hurt(
                            if wither {
                                DamageKind::Wither
                            } else {
                                DamageKind::Magic
                            },
                            allowed,
                        );
                    }
                }
                EffectEvent::Exhaustion(amount) => self.survival.food.add_exhaustion(amount),
                EffectEvent::Food => {
                    self.survival.food.food = (self.survival.food.food + 1).min(MAX_FOOD);
                    self.survival.food.saturation =
                        (self.survival.food.saturation + 1.0).min(self.survival.food.food as f32);
                }
                EffectEvent::Absorb(amount) => self.survival.absorption += amount,
                EffectEvent::RemoveAbsorb(amount) => {
                    self.survival.absorption = (self.survival.absorption - amount).max(0.0);
                }
            }
        }
    }

    /// Feeds `Attributes.MAX_HEALTH` (health boost) into the survival state.
    fn refresh_attributes(&mut self) {
        let bonus = self.effects.attributes().max_health_bonus;
        self.survival
            .set_max_health(self.survival.base_max_health + bonus);
        // The client sizes the health bar from its own `max_health` attribute, so tell it.
        if self.survival.max_health.to_bits() != self.survival.sent_max_health.to_bits()
            && let Some(attribute) =
                ferrite_data::registries::id("minecraft:attribute", "minecraft:max_health")
        {
            self.survival.sent_max_health = self.survival.max_health;
            send(
                self,
                &ferrite_protocol::packets::play::UpdateAttributes {
                    entity_id: VarInt(self.entity),
                    attributes: vec![ferrite_protocol::packets::play::AttributeSnapshot {
                        attribute: VarInt(attribute),
                        base: f64::from(self.survival.max_health),
                        modifiers: Vec::new(),
                    }],
                },
            );
        }
    }

    /// `Player.checkTotemDeathProtection`: consume a totem and revive with the vanilla buffs.
    fn try_totem(&mut self) -> bool {
        let Some(slot) = (0..ferrite_inventory::SIZE).find(|&slot| {
            matches!(
                self.inventory.get(slot).name(),
                Some("minecraft:totem_of_undying")
            )
        }) else {
            return false;
        };
        if !matches!(self.gamemode, 1 | 3) {
            self.inventory.remove(slot, 1);
        }
        super::recipe_unlock::push_plain(
            self.uuid,
            super::recipe_unlock::PlainEvent {
                trigger: "minecraft:used_totem",
                item: Some("minecraft:totem_of_undying".into()),
                ..Default::default()
            },
        );
        self.survival.health = 1.0;
        self.survival.dead = false;
        self.survival.death_cause = None;
        self.survival.death_message = None;
        self.survival.death_processed = false;
        self.survival.absorption = 4.0;
        self.apply_effect(MobEffectInstance::new(Effect::Regeneration.id(), 900, 1));
        self.apply_effect(MobEffectInstance::new(Effect::Absorption.id(), 100, 1));
        self.apply_effect(MobEffectInstance::new(Effect::FireResistance.id(), 800, 0));
        true
    }

    /// `ClientboundSetHealthPacket`, sent only when the client's bar is stale.
    pub(crate) fn sync_health(&mut self) {
        if !self.survival.needs_sync() {
            return;
        }
        send(
            self,
            &SetHealth {
                health: self.survival.health,
                food: VarInt(self.survival.food.food),
                saturation: self.survival.food.saturation,
            },
        );
        self.survival.mark_synced();
    }

    /// One `Player.tick` of survival mechanics: environment hazards, food/regeneration and
    /// eating progress. Called once per region tick, after the player's input was applied.
    pub(crate) fn survival_tick(&mut self, blocks: &TickBlocks, rules: &SurvivalRules) {
        if self.survival.dead {
            // A dead player's zero health is synced by the death processing, after the combat
            // kill packet (`ServerPlayer.die` order).
            if self.survival.death_processed {
                self.sync_health();
            }
            return;
        }
        let feet = [
            self.pos[0].floor() as i32,
            self.pos[1].floor() as i32,
            self.pos[2].floor() as i32,
        ];
        let eye_y = (self.pos[1] + 1.62).floor() as i32;
        let feet_state = blocks.block_state(feet[0], feet[1], feet[2]).unwrap_or(0);
        let eye_state = blocks.block_state(feet[0], eye_y, feet[2]).unwrap_or(0);
        let feet_fluid = ferrite_fluid::fluid_state(u32::from(feet_state)).kind;
        let eye_fluid = ferrite_fluid::fluid_state(u32::from(eye_state)).kind;
        let fire =
            super::survival::is_fire_block(feet_state) || super::survival::is_fire_block(eye_state);
        let fire_resistance = self.effects.fire_resistance();
        let water_breathing = self.effects.water_breathing();
        let slow_falling = self.effects.slow_falling();
        if fire_resistance {
            self.survival.fire_ticks = 0;
        }
        if water_breathing {
            self.survival.air = super::survival::MAX_AIR;
        }
        // The nether and the end start at y 0, so their void is 64 blocks below that.
        self.survival.void_y = f64::from(self.dimension.min_y()) - 64.0;
        self.survival.void_damage = Some(4.0);
        let damage = self.survival.environment_tick(
            self.pos,
            self.ground,
            eye_fluid == ferrite_fluid::FluidKind::Water,
            feet_fluid == ferrite_fluid::FluidKind::Lava,
            fire,
            self.gamemode,
            self.survival.sprinting,
        );
        for (kind, amount) in damage.into_iter().flatten() {
            if (fire_resistance && matches!(kind, DamageKind::Lava | DamageKind::Fire))
                || (water_breathing && kind == DamageKind::Drown)
                || (slow_falling && kind == DamageKind::Fall)
            {
                continue;
            }
            self.hurt(kind, amount);
        }
        if !self.survival.dead {
            match self.survival.food_tick(
                rules.natural_regeneration,
                self.gamemode,
                rules.hard_difficulty,
            ) {
                FoodTick::Heal(amount) => {
                    self.survival.heal(amount);
                }
                FoodTick::Starve => {
                    self.hurt(DamageKind::Starve, 1.0);
                }
                FoodTick::None => {}
            }
            self.tick_effects();
            self.tick_eating();
        }
        if !self.survival.dead {
            self.sync_health();
        }
    }

    /// `ServerboundUseItemPacket` for food: vanilla `Item.use` starts `useItem` when the player
    /// can eat the stack. The item is consumed by [`Player::tick_eating`] after
    /// `minecraft:consumable`'s 1.6 s.
    fn use_item(&mut self, p: &ferrite_protocol::packets::play::ServerboundUseItem) {
        send(
            self,
            &AcknowledgeBlockChange {
                sequence: p.sequence,
            },
        );
        self.start_use(p.hand.0);
    }

    /// `Player#openBook`: the client reads the book from the main hand, so the stack is swapped
    /// in for the packet and the real one restored right after (the way Paper does it).
    fn open_book(&mut self, title: &str, author: &str, pages: &[String]) {
        let Some(book) = written_book(title, author, pages) else {
            return;
        };
        let slot = self.hand_slot(0) as i32;
        send(
            self,
            &SetPlayerInventory {
                slot: VarInt(slot),
                item: book,
            },
        );
        send(
            self,
            &ferrite_protocol::packets::play::OpenBook { hand: VarInt(0) },
        );
        let original = self.inventory.get(self.hand_slot(0)).clone();
        send(
            self,
            &SetPlayerInventory {
                slot: VarInt(slot),
                item: original,
            },
        );
    }

    /// Vanilla `LivingEntity.startUsingItem` for the stack in `hand` (0 main, 1 off).
    fn start_use(&mut self, hand: i32) {
        if self.survival.dead || !(0..=1).contains(&hand) {
            return;
        }
        let slot = self.hand_slot(hand);
        let stack = self.inventory.get(slot).clone();
        // Milk bucket: `MilkBucketItem.use` starts the 32-tick drink duration;
        // `finishUsingItem` clears effects and leaves a bucket via [`Player::tick_eating`].
        if matches!(stack.name(), Some("minecraft:milk_bucket")) {
            if self.survival.eating.is_none() {
                self.survival.eating = Some(Eating {
                    slot,
                    remaining: EAT_TICKS,
                });
            }
            return;
        }
        if self.use_potion(slot) {
            return;
        }
        // Snowballs, eggs and ender pearls leave the hand at once (vanilla `Item.use`).
        if let Some(kind) = stack.name().and_then(arrow::Kind::thrown_from_name) {
            if !matches!(self.gamemode, 1 | 3) {
                self.inventory.remove(slot, 1);
                send(
                    self,
                    &SetPlayerInventory {
                        slot: VarInt(slot as i32),
                        item: self.inventory.get(slot).clone(),
                    },
                );
            }
            // A potion keeps its contents so the burst can apply them where it lands.
            let contents = kind.is_potion().then(|| stack.copy_with_count(1));
            self.menus.launched.push((kind, contents));
            return;
        }
        // An eye of ender used in the air flies toward the nearest stronghold (vanilla `EnderEyeItem.use`).
        if stack.name() == Some("minecraft:ender_eye") {
            if !matches!(self.gamemode, 1 | 3) {
                self.inventory.remove(slot, 1);
                send(
                    self,
                    &SetPlayerInventory {
                        slot: VarInt(slot as i32),
                        item: self.inventory.get(slot).clone(),
                    },
                );
            }
            self.eye_throws
                .push([self.pos[0], self.pos[1] + 1.62 - 0.1, self.pos[2]]);
            return;
        }
        // A fishing rod casts a bobber, or reels in the one already out; the region does it.
        if matches!(stack.name(), Some("minecraft:fishing_rod")) {
            self.menus.fishing.push(slot);
            return;
        }
        // Vanilla `LivingEntity.startUsingItem`: a shield raises, a bow starts drawing.
        match stack.name() {
            Some("minecraft:shield") if self.shield_disabled == 0 => {
                self.blocking = true;
                return;
            }
            Some("minecraft:bow") => {
                self.using_bow = true;
                self.trident_draw = false;
                self.bow_charge = 0;
                return;
            }
            Some("minecraft:trident") => {
                self.using_bow = true;
                self.trident_draw = true;
                self.bow_charge = 0;
                return;
            }
            _ => {}
        }
        let Some(ferrite_item::Component::Food(food)) =
            stack.effective_component(ferrite_item::component_ids::FOOD)
        else {
            return;
        };
        if self.survival.food.food >= MAX_FOOD && !food.can_always_eat {
            return;
        }
        self.survival.eating = Some(Eating {
            slot,
            remaining: EAT_TICKS,
        });
    }

    /// `PotionItem.use`: potions have `minecraft:consumable`, not `minecraft:food`, so they
    /// start the same use duration and finish through [`Player::tick_eating`].
    fn use_potion(&mut self, slot: usize) -> bool {
        let stack = self.inventory.get(slot);
        if stack
            .effective_component(ferrite_item::component_ids::POTION_CONTENTS)
            .is_none()
        {
            return false;
        }
        if self.survival.eating.is_none() {
            self.survival.eating = Some(Eating {
                slot,
                remaining: EAT_TICKS,
            });
        }
        true
    }

    /// `LivingEntity.updateUsingItem`/`ItemStack.finishUsingItem`: count the use ticks down,
    /// then apply food, potion, or milk bucket completion and shrink the stack in survival.
    fn tick_eating(&mut self) {
        let Some(mut eating) = self.survival.eating else {
            return;
        };
        if self.survival.dead
            || (eating.slot != self.inventory.selected_slot()
                && eating.slot != ferrite_inventory::OFFHAND)
        {
            self.survival.eating = None;
            return;
        }
        let stack = self.inventory.get(eating.slot).clone();
        let milk = stack.name() == Some("minecraft:milk_bucket");
        let food = match stack.effective_component(ferrite_item::component_ids::FOOD) {
            Some(ferrite_item::Component::Food(food)) => Some(food),
            _ => None,
        };
        let potion = match stack.effective_component(ferrite_item::component_ids::POTION_CONTENTS) {
            Some(ferrite_item::Component::PotionContents(contents)) => {
                Some(ferrite_entity::potion::effects_from_component(&contents))
            }
            Some(ferrite_item::Component::Opaque(tag)) => {
                PotionContents::from_tag(&tag).map(|contents| contents.all_effects())
            }
            _ => None,
        };
        if !milk && food.is_none() && potion.is_none() {
            self.survival.eating = None;
            return;
        }
        eating.remaining -= 1;
        if eating.remaining > 0 {
            self.survival.eating = Some(eating);
            return;
        }
        if let Some(name) = stack.name() {
            super::recipe_unlock::CONSUMED
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((self.uuid, name));
        }
        if milk {
            self.clear_effects();
        } else if let Some(food) = food {
            self.survival.food.eat(food.nutrition, food.saturation);
        } else if let Some(effects) = potion {
            // `PotionContents.applyToLivingEntity`: instant effects act immediately, the rest
            // enter the normal effect store and tick down.
            for instance in effects {
                self.apply_effect(instance);
            }
        }
        // `Player.hasInfiniteMaterials`: creative does not use the item up.
        if !matches!(self.gamemode, 1 | 3) {
            let remainder = stack.effective_component(ferrite_item::component_ids::USE_REMAINDER);
            self.inventory.remove(eating.slot, 1);
            if let Some(ferrite_item::Component::UseRemainder(item)) = remainder {
                if self.inventory.get(eating.slot).is_empty() {
                    self.inventory.set(eating.slot, item.as_ref().clone());
                } else {
                    if let Some(leftover) = self.inventory.place_back(item.as_ref().clone()) {
                        self.menus.thrown.push(leftover);
                    }
                    self.sync_containers();
                }
            }
        }
        send(
            self,
            &SetPlayerInventory {
                slot: VarInt(eating.slot as i32),
                item: self.inventory.get(eating.slot).clone(),
            },
        );
        self.survival.eating = None;
        self.sync_health();
    }

    /// Vanilla `LivingEntity.isDamageSourceBlocked`: a raised shield facing the attacker
    /// stops the hit. The caller applies the shield's knockback and cooldown.
    pub(crate) fn blocks_damage_from(&self, source: [f64; 3]) -> bool {
        if !self.blocking || self.shield_disabled > 0 {
            return false;
        }
        combat::shield_blocks(
            self.yaw,
            ferrite_math::Vec3::new(self.pos[0], self.pos[1], self.pos[2]),
            ferrite_math::Vec3::new(source[0], source[1], source[2]),
        )
    }

    /// `LivingEntity.releaseUsingItem`: a drawn bow fires and a raised shield drops.
    fn release_use(&mut self) {
        if self.using_bow {
            self.bow_released = true;
            self.using_bow = false;
        }
        self.blocking = false;
        self.survival.eating = None;
    }

    /// Vanilla `Entity.handleNetherPortal`/`ServerPlayer.changeDimension`: after four seconds
    /// in a portal block, find the paired dimension's portal (searching 128 blocks, creating
    /// a frame when none exists) and travel there.
    fn tick_portal(&mut self, gateway_blocked: bool) {
        self.portal_cooldown = self.portal_cooldown.saturating_sub(1);
        if self.survival.dead {
            self.portal_ticks = 0;
            return;
        }
        let Some(source) = self.source.clone() else {
            return;
        };
        let feet = [
            self.pos[0].floor() as i32,
            self.pos[1].floor() as i32,
            self.pos[2].floor() as i32,
        ];
        // The block at the feet decides where the portal leads: a nether portal pairs the
        // overworld with the nether, an end portal the overworld with the end.
        let inside = source
            .block(feet[0], feet[1], feet[2])
            .and_then(|state| u16::try_from(state).ok());
        let target = inside.and_then(|state| {
            if super::portal::is_end_portal(state) {
                match self.dimension {
                    super::Dimension::Overworld => Some(super::Dimension::End),
                    super::Dimension::End => Some(super::Dimension::Overworld),
                    super::Dimension::Nether => None,
                }
            } else if super::portal::is_end_gateway(state)
                && self.dimension == super::Dimension::End
            {
                Some(super::Dimension::End)
            } else if super::portal::is_portal(state) && self.dimension != super::Dimension::End {
                super::portal::counterpart(self.dimension)
            } else {
                None
            }
        });
        let in_portal = target.is_some();
        let target = target.unwrap_or(self.dimension);
        if !in_portal {
            self.portal_ticks = 0;
            self.portal_pending = None;
            return;
        }
        if self.portal_cooldown > 0 || self.watching_credits {
            return;
        }
        // The first way home from the End plays the credits (`WIN_GAME`) before anything moves.
        if self.dimension == super::Dimension::End
            && target == super::Dimension::Overworld
            && !self.seen_credits
        {
            self.seen_credits = true;
            self.watching_credits = true;
            send(
                self,
                &GameEvent {
                    event: 4,
                    value: 1.0,
                },
            );
            return;
        }
        let gateway = inside.is_some_and(super::portal::is_end_gateway);
        // Vanilla `TheEndGatewayBlockEntity.teleportEntity` refuses while the creation beam is
        // still playing or the purple beam is cooling down.
        if gateway && gateway_blocked {
            return;
        }
        if self.portal_pending.is_none() {
            self.portal_ticks += 1;
            // A gateway works at once; the portals make you wait.
            if !gateway && self.portal_ticks < super::portal::WAIT_TICKS {
                return;
            }
            self.portal_ticks = 0;
            let scaled = match (self.dimension, target) {
                (super::Dimension::Overworld, super::Dimension::End) => super::portal::END_ARRIVAL,
                (super::Dimension::End, super::Dimension::Overworld) => {
                    // Leaving the end lands on the bed (respawn point), else at the world spawn.
                    let spawn = self
                        .spawn
                        .unwrap_or_else(|| self.dimensions.spawn(super::Dimension::Overworld));
                    [
                        f64::from(spawn[0]) + 0.5,
                        f64::from(spawn[1]),
                        f64::from(spawn[2]) + 0.5,
                    ]
                }
                (super::Dimension::End, super::Dimension::End) => {
                    let [x, z] = super::portal::gateway_exit(self.pos[0], self.pos[2]);
                    [x, 0.0, z]
                }
                _ => super::portal::scale(self.dimension, target, self.pos),
            };
            self.portal_pending = Some((target, scaled));
        }
        let Some((target, scaled)) = self.portal_pending else {
            return;
        };
        let target_source = self.dimensions.source(target).clone();
        target_source.prefetch(
            (scaled[0].floor() as i32).div_euclid(16),
            (scaled[2].floor() as i32).div_euclid(16),
            self.biome,
        );
        let destination = match (self.dimension, target) {
            (super::Dimension::Overworld, super::Dimension::End) => {
                // The platform spans two chunks; both must be loaded before it is built.
                for (x, z) in super::portal::END_PLATFORM_CHUNKS {
                    target_source.prefetch(x, z, self.biome);
                }
                super::portal::create_end_platform(target_source.as_ref())
            }
            (super::Dimension::End, super::Dimension::Overworld) => Some(scaled),
            (super::Dimension::End, super::Dimension::End) => {
                // Vanilla picks a spot on the nearest outer island rather than the exact point
                // 1024 blocks out. Give the 3x3 chunks around the ideal point time to load,
                // then search them for ground; an all-void neighbourhood cancels the trip.
                let (x, z) = (scaled[0].floor() as i32, scaled[2].floor() as i32);
                let (chunk_x, chunk_z) = (x.div_euclid(16), z.div_euclid(16));
                for dx in -1..=1 {
                    for dz in -1..=1 {
                        target_source.prefetch(chunk_x + dx, chunk_z + dz, self.biome);
                    }
                }
                let landing = super::end_fight::gateway_landing([x, z], |x, y, z| {
                    target_source.block(x, y, z)
                });
                if landing.is_none() {
                    let loaded = (-1..=1).all(|dx| {
                        (-1..=1).all(|dz| {
                            target_source
                                .with_loaded_chunk(chunk_x + dx, chunk_z + dz, |_| ())
                                .is_some()
                        })
                    });
                    if loaded {
                        self.portal_pending = None;
                        self.portal_cooldown = super::portal::COOLDOWN_TICKS;
                        return;
                    }
                }
                landing
            }
            _ => super::portal::find_portal(
                target_source.as_ref(),
                scaled,
                super::portal::SEARCH_RADIUS,
            )
            .or_else(|| super::portal::create_portal(target_source.as_ref(), scaled)),
        };
        let Some(destination) = destination else {
            // The destination chunk is still loading; retry on the next tick.
            return;
        };
        self.portal_pending = None;
        self.portal_ticks = 0;
        if gateway {
            self.gateway_used = Some(feet);
            self.pos = destination;
            self.teleport();
        } else {
            self.change_dimension(target, destination, self.yaw, self.pitch);
        }
        self.portal_cooldown = super::portal::COOLDOWN_TICKS;
        let sound =
            ferrite_data::registries::id("minecraft:sound_event", "minecraft:block.portal.travel")
                .unwrap_or(0);
        send(
            self,
            &SoundEffect {
                sound: super::commands_extras::sound_holder(sound),
                source: VarInt(0),
                x: (destination[0] * 8.0) as i32,
                y: (destination[1] * 8.0) as i32,
                z: (destination[2] * 8.0) as i32,
                volume: 0.5,
                pitch: 1.0,
                seed: 0,
            },
        );
    }

    /// Vanilla `ServerPlayer.respawn`: a fresh survival state at the world spawn. The client
    /// discards every chunk on `respawn`, so the loaded set is released and re-queued.
    pub(crate) fn respawn(&mut self, spawn: [f64; 3]) {
        self.stop_spin();
        let spawn = match self.plugins.clone() {
            Some(plugins) => plugins
                .player_respawned(self.uuid, &self.name, spawn, 0.0, 0.0)
                .unwrap_or(spawn),
            None => spawn,
        };
        let old_loaded = std::mem::take(&mut self.loaded);
        for (x, z) in old_loaded {
            let _ = self.signals.send(Signal::Hold {
                world: self.world_id,
                chunk: ChunkPos(x, z),
                add: false,
            });
        }
        if let Some((_, loaded, _)) = self.tracking.lock().unwrap().get_mut(&self.uuid) {
            loaded.clear();
        }
        self.clear_effects();
        self.survival = Survival::new();
        self.survival.last_pos = spawn;
        self.respawn_requested = false;
        self.breaking = None;
        self.portal_pending = None;
        self.portal_ticks = 0;
        self.pos = spawn;
        self.yaw = 0.0;
        self.pitch = 0.0;
        self.ground = true;
        // Keep the old chunk here so the tick loop notices `next != chunk`, refreshes the
        // window and hands the player to the region owning the spawn chunk (Folia ownership).
        let spawn_chunk = ChunkPos(
            (spawn[0].floor() as i32).div_euclid(16),
            (spawn[2].floor() as i32).div_euclid(16),
        );
        send(
            self,
            &Respawn {
                dimension_type: VarInt(self.dimension.dimension_type_id()),
                dimension: self.dimension.key().into(),
                seed: self.seed,
                game_mode: VarInt(self.gamemode),
                previous_game_mode: VarInt(0),
                debug: false,
                flat: self.flat,
                death_location: None,
                portal_cooldown: VarInt(self.portal_cooldown.min(i32::MAX as u32) as i32),
                sea_level: VarInt(self.dimension.sea_level()),
                data_to_keep: 3,
            },
        );
        send(
            self,
            &SetHealth {
                health: MAX_HEALTH,
                food: VarInt(MAX_FOOD),
                saturation: 5.0,
            },
        );
        self.survival.mark_synced();
        send(
            self,
            &SetExperience {
                progress: self.experience_progress,
                level: VarInt(self.experience_level),
                total: VarInt(self.experience_total),
            },
        );
        self.init_inventory_menu();
        let id = self.next_teleport;
        self.next_teleport += 1;
        self.pending_teleport = Some(id);
        send(
            self,
            &PositionSync {
                teleport_id: VarInt(id),
                x: spawn[0],
                y: spawn[1],
                z: spawn[2],
                velocity_x: 0.0,
                velocity_y: 0.0,
                velocity_z: 0.0,
                yaw: 0.0,
                pitch: 0.0,
                flags: 0,
            },
        );
        send(
            self,
            &CenterChunk {
                x: VarInt(spawn_chunk.0),
                z: VarInt(spawn_chunk.1),
            },
        );
        let (_, new) = chunk_window((spawn_chunk.0, spawn_chunk.1), self.view, &self.loaded);
        self.queue = new.into();
        self.prefetched = 0;
        self.awaiting_batch = false;
        self.positions.lock().unwrap().insert(self.uuid, spawn);
        let _ = self.signals.send(Signal::Update {
            uuid: self.uuid,
            pos: spawn,
            yaw: 0.0,
            pitch: 0.0,
            ground: true,
        });
    }

    /// Vanilla dimension travel: a `respawn`-style client reset followed by the target
    /// position. The region hands the player to the target dimension after `update` sees
    /// that `self.dimension` no longer matches its own.
    pub(crate) fn change_dimension(
        &mut self,
        dimension: super::Dimension,
        pos: [f64; 3],
        yaw: f32,
        pitch: f32,
    ) {
        let source = self.dimensions.source(dimension).clone();
        self.change_world(
            super::WorldId::Vanilla(dimension),
            dimension,
            source,
            pos,
            yaw,
            pitch,
        );
    }

    /// Moves the player to `world`, releasing every chunk it held in the old world and
    /// re-pointing tracking before the new world's chunks are requested.
    pub(crate) fn change_world(
        &mut self,
        world: super::WorldId,
        dimension: super::Dimension,
        source: Arc<super::anvil::AnvilChunkSource>,
        pos: [f64; 3],
        yaw: f32,
        pitch: f32,
    ) {
        self.stop_spin();
        if matches!(
            self.menus.open_at,
            Some(super::container::OpenAt::Trader(_))
        ) {
            self.close_container();
        }
        let old = self.world_id;
        let old_dimension = self.dimension;
        let old_loaded = std::mem::take(&mut self.loaded);
        for (x, z) in old_loaded {
            let _ = self.signals.send(Signal::Hold {
                world: old,
                chunk: ChunkPos(x, z),
                add: false,
            });
        }
        if let Some((tracked_world, loaded, _)) = self.tracking.lock().unwrap().get_mut(&self.uuid)
        {
            loaded.clear();
            *tracked_world = world;
        }
        self.dimension = dimension;
        self.world_id = world;
        self.source = Some(source);
        self.dimension_type = dimension.dimension_type_id();
        self.pos = pos;
        self.yaw = yaw;
        self.pitch = pitch;
        self.ground = true;
        self.chunk = ChunkPos(
            (pos[0].floor() as i32).div_euclid(16),
            (pos[2].floor() as i32).div_euclid(16),
        );
        self.loaded.clear();
        self.queue.clear();
        self.prefetched = 0;
        self.awaiting_batch = false;
        self.portal_ticks = 0;
        self.portal_cooldown = 200;
        self.survival.last_pos = pos;
        if old_dimension == dimension {
            // The client keeps its level when the dimension key is unchanged, so chunks of the
            // old world would linger: bounce through another dimension first, as Paper does.
            let other = if dimension == super::Dimension::Overworld {
                super::Dimension::Nether
            } else {
                super::Dimension::Overworld
            };
            send(
                self,
                &Respawn {
                    dimension_type: VarInt(other.dimension_type_id()),
                    dimension: other.key().into(),
                    seed: self.seed,
                    game_mode: VarInt(self.gamemode),
                    previous_game_mode: VarInt(0),
                    debug: false,
                    flat: self.flat,
                    death_location: None,
                    portal_cooldown: VarInt(0),
                    sea_level: VarInt(other.sea_level()),
                    data_to_keep: 0,
                },
            );
        }
        send(
            self,
            &Respawn {
                dimension_type: VarInt(dimension.dimension_type_id()),
                dimension: dimension.key().into(),
                seed: self.seed,
                game_mode: VarInt(self.gamemode),
                previous_game_mode: VarInt(0),
                debug: false,
                flat: self.flat,
                death_location: None,
                portal_cooldown: VarInt(200),
                sea_level: VarInt(dimension.sea_level()),
                data_to_keep: 3,
            },
        );
        send(
            self,
            &SetHealth {
                health: MAX_HEALTH,
                food: VarInt(MAX_FOOD),
                saturation: 5.0,
            },
        );
        send(
            self,
            &SetExperience {
                progress: self.experience_progress,
                level: VarInt(self.experience_level),
                total: VarInt(self.experience_total),
            },
        );
        self.init_inventory_menu();
        let id = self.next_teleport;
        self.next_teleport += 1;
        self.pending_teleport = Some(id);
        send(
            self,
            &PositionSync {
                teleport_id: VarInt(id),
                x: pos[0],
                y: pos[1],
                z: pos[2],
                velocity_x: 0.0,
                velocity_y: 0.0,
                velocity_z: 0.0,
                yaw,
                pitch,
                flags: 0,
            },
        );
        self.refresh_chunks();
        self.positions.lock().unwrap().insert(self.uuid, pos);
        let _ = self.signals.send(Signal::Update {
            uuid: self.uuid,
            pos,
            yaw,
            pitch,
            ground: true,
        });
    }

    fn update(&mut self, trader: &mut TraderSync<'_>) -> bool {
        if let Some((_, _, elapsed)) = &mut self.breaking {
            *elapsed = elapsed.saturating_add(1);
        }
        self.attack_ticks = self.attack_ticks.saturating_add(1).min(40);
        self.shield_disabled = self.shield_disabled.saturating_sub(1);
        if self.using_bow && !self.survival.dead {
            self.bow_charge = self.bow_charge.saturating_add(1).min(20);
        }
        let mut commands: VecDeque<_> = self.mailbox.lock().unwrap().drain(..).collect();
        let start = self.pos;
        let mut changed = false;
        while let Some(command) = commands.pop_front() {
            let location = match &command {
                Command::Input(ServerboundPlay::PlayerAction(p))
                    if p.status.0 == 0 || p.status.0 == 2 =>
                {
                    Some(p.location)
                }
                Command::Input(ServerboundPlay::UseItemOn(p)) => Some(p.location),
                _ => None,
            };
            if location.is_some_and(|location| !self.interaction_ready(location)) {
                let mut mailbox = self.mailbox.lock().unwrap();
                commands.push_front(command);
                commands.append(&mut mailbox);
                *mailbox = commands;
                break;
            }
            let msg = match command {
                Command::Leave => {
                    trader.commit(self);
                    self.save_on_exit();
                    // After the save, which records the vehicle the player was riding.
                    super::ride::forget(self.uuid);
                    return false;
                }
                Command::SetMode(mode) => {
                    self.gamemode = mode;
                    if mode != 1 && mode != 3 {
                        self.flying = false;
                    }
                    send(
                        self,
                        &GameEvent {
                            event: 3,
                            value: mode as f32,
                        },
                    );
                    send(self, &abilities_for_mode(mode));
                    continue;
                }
                Command::SetViewDistance(vd) => {
                    self.view = vd;
                    self.refresh_chunks();
                    continue;
                }
                Command::Respawn => {
                    if self.survival.dead {
                        self.respawn_requested = true;
                    }
                    continue;
                }
                Command::PluginVisualExperience { progress, level } => {
                    send(
                        self,
                        &SetExperience {
                            progress,
                            level: VarInt(level.unwrap_or(self.experience_level)),
                            total: VarInt(self.experience_total),
                        },
                    );
                    continue;
                }
                Command::PluginGameEvent { event, value } => {
                    send(self, &GameEvent { event, value });
                    continue;
                }
                Command::PluginOpenInventory {
                    title,
                    rows,
                    shulker,
                    items,
                } => {
                    self.open_plugin_inventory(&title, rows, shulker, &items);
                    continue;
                }
                Command::PluginShoulder { left, op } => {
                    let side = if left {
                        ShoulderSide::Left
                    } else {
                        ShoulderSide::Right
                    };
                    self.menus.shoulders.push((side, op));
                    continue;
                }
                Command::PluginPacket(packet) => {
                    self.send_plugin_packet(packet);
                    continue;
                }
                Command::PluginSurvival { stat, value } => {
                    use crate::game::plugin_types::SurvivalStat;
                    match stat {
                        SurvivalStat::Saturation => {
                            let food = self.survival.food.food as f32;
                            self.survival.food.saturation = value.min(food);
                        }
                        SurvivalStat::Exhaustion => self.survival.food.exhaustion = value,
                        SurvivalStat::Absorption => self.survival.absorption = value,
                        SurvivalStat::FallDistance => {
                            self.survival.fall_distance = f64::from(value)
                        }
                        SurvivalStat::Air => self.survival.air = value as i32,
                        SurvivalStat::MaxHealth => {
                            self.survival.base_max_health = value;
                            self.refresh_attributes();
                        }
                    }
                    continue;
                }
                Command::PluginCooldown { group, ticks } => {
                    send(
                        self,
                        &ItemCooldown {
                            group,
                            ticks: VarInt(ticks),
                        },
                    );
                    continue;
                }
                Command::PluginEntityData {
                    flags,
                    silent,
                    no_gravity,
                    freeze_ticks,
                } => {
                    let mut values = Vec::new();
                    if let Some(flags) = flags {
                        values.push(MetaEntry {
                            index: 0,
                            value: MetaValue::Byte(flags as i8),
                        });
                    }
                    if let Some(ticks) = freeze_ticks {
                        values.push(MetaEntry {
                            index: 7,
                            value: MetaValue::Int(ticks),
                        });
                    }
                    if let Some(silent) = silent {
                        values.push(MetaEntry {
                            index: 4,
                            value: MetaValue::Boolean(silent),
                        });
                    }
                    if let Some(no_gravity) = no_gravity {
                        values.push(MetaEntry {
                            index: 5,
                            value: MetaValue::Boolean(no_gravity),
                        });
                    }
                    if !values.is_empty() {
                        send(
                            self,
                            &SetEntityData {
                                id: VarInt(self.entity),
                                values,
                            },
                        );
                    }
                    continue;
                }
                Command::PluginVelocity { velocity } => {
                    send(
                        self,
                        &SetEntityMotion {
                            id: VarInt(self.entity),
                            velocity: ferrite_protocol::types::LpVec3(velocity),
                        },
                    );
                    continue;
                }
                Command::PluginCastBobber { entity } => {
                    self.bobber_cast = Some(entity);
                    continue;
                }
                Command::PluginSaveData => {
                    self.save_player_data();
                    continue;
                }
                Command::PluginLoadData => {
                    changed |= self.load_player_data();
                    continue;
                }
                Command::PluginFireworkBoost {
                    flight,
                    explosions,
                    entity,
                } => {
                    self.firework_launches.push((flight, explosions, entity));
                    continue;
                }
                Command::PluginOpenStation { station } => {
                    self.open_plugin_station(&station);
                    continue;
                }
                Command::PluginCloseInventory => {
                    self.close_plugin_inventory();
                    continue;
                }
                Command::PluginDamage { amount } => {
                    self.hurt(DamageKind::Generic, amount);
                    continue;
                }
                Command::PluginAbilities {
                    allow_flight,
                    flying,
                    invulnerable,
                    fly_speed,
                    walk_speed,
                } => {
                    let creative = self.gamemode == 1;
                    let spectator = self.gamemode == 3;
                    let may_fly = allow_flight || creative || spectator;
                    let flags = u8::from(invulnerable || creative || spectator)
                        | (u8::from(flying && may_fly) << 1)
                        | (u8::from(may_fly) << 2)
                        | (u8::from(creative) << 3);
                    send(
                        self,
                        &PlayerAbilities {
                            flags,
                            flying_speed: fly_speed / 2.0,
                            walking_speed: walk_speed / 2.0,
                        },
                    );
                    continue;
                }
                Command::SetHealth { health } => {
                    self.survival.health = health.clamp(0.0, self.survival.max_health);
                    send(
                        self,
                        &SetHealth {
                            health: self.survival.health,
                            food: VarInt(self.survival.food.food),
                            saturation: self.survival.food.saturation,
                        },
                    );
                    continue;
                }
                Command::SetFood { food } => {
                    self.survival.food.food = i32::from(food.min(20));
                    self.survival.food.saturation = self
                        .survival
                        .food
                        .saturation
                        .min(self.survival.food.food as f32);
                    send(
                        self,
                        &SetHealth {
                            health: self.survival.health,
                            food: VarInt(self.survival.food.food),
                            saturation: self.survival.food.saturation,
                        },
                    );
                    continue;
                }
                Command::SetExperience {
                    level,
                    progress,
                    total,
                } => {
                    if let Some(level) = level {
                        self.experience_level = level.max(0);
                    }
                    if let Some(progress) = progress {
                        self.experience_progress = progress.clamp(0.0, 1.0);
                    }
                    if let Some(total) = total {
                        self.experience_total = total.max(0);
                    }
                    send(
                        self,
                        &SetExperience {
                            progress: self.experience_progress,
                            level: VarInt(self.experience_level),
                            total: VarInt(self.experience_total),
                        },
                    );
                    continue;
                }
                Command::Animate { action } => {
                    let update = ferrite_net::game::packet(&Animate {
                        id: VarInt(self.entity),
                        action,
                    });
                    let tracking = self.tracking.lock().unwrap();
                    broadcast_tracked_chunk(
                        &tracking,
                        self.world_id,
                        (self.chunk.0, self.chunk.1),
                        &update,
                    );
                    continue;
                }
                Command::SetMaxHealth(value) => {
                    self.survival.base_max_health = value;
                    self.refresh_attributes();
                    continue;
                }
                Command::Teleport { pos, yaw, pitch } => {
                    if pos.iter().all(|v| v.is_finite() && v.abs() <= 30_000_000.0) {
                        self.pos = pos;
                        if let Some(yaw) = yaw {
                            self.yaw = yaw;
                        }
                        if let Some(pitch) = pitch {
                            self.pitch = pitch;
                        }
                        self.teleport();
                        changed = true;
                    }
                    continue;
                }
                Command::EnderPearlTeleport { dimension, pos } => {
                    if dimension == self.dimension {
                        changed |= self.land_ender_pearl(pos);
                    }
                    continue;
                }
                Command::MobMelee {
                    dimension,
                    damage,
                    from,
                    attacker,
                    knockback_xz,
                    launch_y,
                    poison_ticks,
                } => {
                    if dimension == self.dimension {
                        if self.blocks_damage_from(from) {
                            self.shield_disabled = 5;
                            // The attacker belongs to another region: ask its owner to recoil it.
                            let _ = self.signals.send(Signal::MobKnockback {
                                world_id: self.world_id,
                                attacker,
                                at: from,
                                source: self.pos,
                            });
                        } else if self.hurt(DamageKind::Mob, damage) {
                            let dx = self.pos[0] - from[0];
                            let dz = self.pos[2] - from[2];
                            let length = (dx * dx + dz * dz).sqrt().max(1.0e-4);
                            let y = if self.ground { 0.4 } else { 0.0 } + launch_y;
                            send(
                                self,
                                &SetEntityMotion {
                                    id: VarInt(self.entity),
                                    velocity: ferrite_protocol::types::LpVec3([
                                        dx / length * knockback_xz,
                                        y,
                                        dz / length * knockback_xz,
                                    ]),
                                },
                            );
                            if poison_ticks > 0 {
                                self.apply_effect(MobEffectInstance::new(
                                    ferrite_entity::effect::Effect::Poison.id(),
                                    poison_ticks,
                                    0,
                                ));
                            }
                        }
                    }
                    continue;
                }
                Command::ChangeDimension {
                    dimension,
                    pos,
                    yaw,
                    pitch,
                } => {
                    if dimension != self.dimension
                        && pos.iter().all(|v| v.is_finite() && v.abs() <= 30_000_000.0)
                    {
                        self.change_dimension(dimension, pos, yaw, pitch);
                        changed = true;
                    }
                    continue;
                }
                Command::ChangeWorld {
                    world,
                    dimension,
                    source,
                    pos,
                    yaw,
                    pitch,
                } => {
                    if pos.iter().all(|v| v.is_finite() && v.abs() <= 30_000_000.0) {
                        if world == self.world_id {
                            self.pos = pos;
                            self.yaw = yaw;
                            self.pitch = pitch;
                            self.teleport();
                        } else {
                            self.change_world(world, dimension, source, pos, yaw, pitch);
                        }
                        changed = true;
                    }
                    continue;
                }
                Command::World(command) => {
                    super::commands_world::run(self, command);
                    continue;
                }
                Command::Clear {
                    target,
                    target_name,
                    item,
                    max_count,
                    feedback,
                } => {
                    if target == self.uuid {
                        self.clear_items(item, max_count, &target_name, feedback.as_ref());
                    }
                    continue;
                }
                Command::Give {
                    target,
                    target_name,
                    item,
                    count,
                    item_name,
                    components,
                    feedback,
                } => {
                    if target == self.uuid {
                        self.give_items(
                            item,
                            count,
                            &components,
                            &item_name,
                            &target_name,
                            feedback.as_ref(),
                        );
                    }
                    continue;
                }
                Command::Experience {
                    target,
                    target_name,
                    operation,
                    amount,
                    unit,
                    feedback,
                } => {
                    if target == self.uuid {
                        self.apply_experience(
                            operation,
                            amount,
                            unit,
                            &target_name,
                            feedback.as_ref(),
                        );
                    }
                    continue;
                }
                Command::PluginSleep { position, force } => {
                    super::blocks::plugin_sleep(self, position, force);
                    continue;
                }
                Command::PluginBreakBlock { position } => {
                    self.plugin_break_block(position);
                    continue;
                }
                Command::SetInventorySlot { slot, item } => {
                    self.set_inventory_slot(slot, item);
                    continue;
                }
                Command::MapData {
                    id,
                    scale,
                    locked,
                    colors,
                    decorations,
                } => {
                    send(
                        self,
                        &ferrite_protocol::packets::play::MapItemData {
                            map_id: VarInt(id),
                            scale,
                            locked,
                            decorations: Some(
                                decorations
                                    .into_iter()
                                    .map(|decoration| {
                                        ferrite_protocol::packets::play::MapDecoration {
                                            kind: VarInt(decoration.kind),
                                            x: decoration.x,
                                            y: decoration.y,
                                            direction: decoration.direction as i8,
                                            caption: decoration.caption.map(
                                                |caption| match caption {
                                                    crate::game::plugin_types::MapCaption::Plain(text) => {
                                                        ferrite_protocol::types::NetworkText::Plain(
                                                            text,
                                                        )
                                                    }
                                                    crate::game::plugin_types::MapCaption::Rich(tag) => {
                                                        ferrite_protocol::types::NetworkText::Rich(
                                                            tag,
                                                        )
                                                    }
                                                },
                                            ),
                                        }
                                    })
                                    .collect(),
                            ),
                            width: 128,
                            height: 128,
                            x: 0,
                            z: 0,
                            colors,
                        },
                    );
                    continue;
                }
                Command::OpenBook {
                    title,
                    author,
                    pages,
                } => {
                    self.open_book(&title, &author, &pages);
                    continue;
                }
                Command::RefreshChunk { x, z } => {
                    if self.dimension == crate::game::Dimension::Overworld
                        && self.loaded.contains(&(x, z))
                        && !self.queue.contains(&(x, z))
                    {
                        self.queue.push_front((x, z));
                        self.prefetched = 0;
                    }
                    continue;
                }
                Command::ItemUse { action, hand } => {
                    match action {
                        0 => self.start_use(i32::from(hand)),
                        1 => {
                            if let Some(eating) = &mut self.survival.eating {
                                eating.remaining = 1;
                            } else if self.using_bow {
                                self.release_use();
                            }
                        }
                        _ => self.release_use(),
                    }
                    continue;
                }
                Command::ItemMeta {
                    slot,
                    name,
                    lore,
                    enchants,
                    unbreakable,
                    model,
                    repair,
                    flags,
                    custom_data,
                    book,
                    extra,
                } => {
                    self.set_item_meta(
                        slot,
                        &ItemMetaFields {
                            name,
                            lore,
                            enchants,
                            unbreakable,
                            model,
                            repair,
                            flags,
                            custom_data,
                            book,
                            extra,
                        },
                    );
                    continue;
                }
                Command::ItemMapId { slot, id } => {
                    self.set_item_map_id(slot, id);
                    continue;
                }
                Command::ItemDamage { slot, damage } => {
                    self.set_item_damage(slot, damage);
                    continue;
                }
                Command::RiptideAttack { ticks, damage } => {
                    self.spin_ticks = ticks;
                    self.spin_damage = damage;
                    self.spin_hit.clear();
                    self.broadcast_spin(true);
                    continue;
                }
                Command::ItemSlots { slots, fill } => {
                    self.fill_slots(&slots, fill);
                    continue;
                }
                Command::ScanInventory => {
                    let held = (0..46)
                        .filter_map(|slot| self.resolve_slot(slot))
                        .filter_map(|slot| {
                            let stack = self.inventory.get(slot);
                            (!stack.is_empty())
                                .then(|| ferrite_item::item_name(stack.item).map(str::to_owned))
                                .flatten()
                        })
                        .collect();
                    {
                        let head = self.inventory.get(ferrite_inventory::HEAD);
                        let hidden = super::waypoints::hides(
                            (!head.is_empty())
                                .then(|| ferrite_item::item_name(head.item))
                                .flatten(),
                            self.sneaking,
                            self.effects
                                .has(ferrite_entity::effect::Effect::Invisibility),
                        );
                        let mut set = super::waypoints::HEAD_HIDDEN
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        if hidden {
                            set.insert(self.uuid);
                        } else {
                            set.remove(&self.uuid);
                        }
                    }
                    {
                        // `LevitationTrigger`: how far the player has risen since the effect began.
                        let mut starts = super::recipe_unlock::LEVITATION_START
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        if self.effects.has(ferrite_entity::effect::Effect::Levitation) {
                            let start = *starts.entry(self.uuid).or_insert(self.pos);
                            super::recipe_unlock::push_plain(
                                self.uuid,
                                super::recipe_unlock::PlainEvent {
                                    trigger: "minecraft:levitation",
                                    delta: [
                                        self.pos[0] - start[0],
                                        self.pos[1] - start[1],
                                        self.pos[2] - start[2],
                                    ],
                                    ..Default::default()
                                },
                            );
                        } else {
                            starts.remove(&self.uuid);
                        }
                    }
                    super::recipe_unlock::EFFECTS
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(
                            self.uuid,
                            self.effects
                                .iter()
                                .map(|instance| super::recipe_unlock::EffectFact {
                                    name: format!(
                                        "minecraft:{}",
                                        ferrite_entity::effect::Effect::from_id(instance.effect)
                                            .name()
                                    ),
                                    amplifier: instance.amplifier,
                                    duration: instance.duration,
                                })
                                .collect(),
                        );
                    super::recipe_unlock::HELD
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(self.uuid, held);
                    let mainhand = self.resolve_slot(-1).and_then(|slot| {
                        super::commands_loot::HeldTool::from_stack(self.inventory.get(slot))
                    });
                    let offhand = self
                        .resolve_slot(ferrite_inventory::OFFHAND as i16)
                        .and_then(|slot| {
                            super::commands_loot::HeldTool::from_stack(self.inventory.get(slot))
                        });
                    super::commands_loot::HELD_TOOLS
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(self.uuid, [mainhand, offhand]);
                    if let Some(biome) = self.source.as_ref().and_then(|source| {
                        source.biome_name(
                            self.pos[0].floor() as i32,
                            self.pos[1].floor() as i32,
                            self.pos[2].floor() as i32,
                        )
                    }) {
                        super::recipe_unlock::BIOMES
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push((self.uuid, biome));
                    }
                    if let Some((name, props)) = self.source.as_ref().and_then(|source| {
                        let (x, y, z) = (
                            self.pos[0].floor() as i32,
                            self.pos[1].floor() as i32,
                            self.pos[2].floor() as i32,
                        );
                        source
                            .block(x, y, z)
                            .and_then(|state| u16::try_from(state).ok())
                            .and_then(ferrite_data::blocks::state)
                    }) {
                        let changed = {
                            let mut last = super::recipe_unlock::LAST_BLOCK
                                .lock()
                                .unwrap_or_else(|e| e.into_inner());
                            last.insert(self.uuid, (name, props)) != Some((name, props))
                        };
                        if changed {
                            super::recipe_unlock::BLOCKS
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .push(super::recipe_unlock::BlockRecord {
                                    player: self.uuid,
                                    block: name,
                                    props,
                                });
                        }
                    }
                    continue;
                }
                Command::ReadSlots { slots, reply } => {
                    let stacks = slots
                        .iter()
                        .filter_map(|slot| self.resolve_slot(*slot))
                        .map(|slot| self.inventory.get(slot).clone())
                        .collect();
                    let _ = reply.send(stacks);
                    continue;
                }
                Command::PluginEntityTask(task) => {
                    if let Some(plugins) = &self.plugins
                        && let Err(error) = plugins.run_entity_task(task)
                    {
                        tracing::warn!(%error, "plugin entity task failed");
                    }
                    continue;
                }
                Command::Damage(kind, amount) => {
                    self.hurt(kind, amount);
                    continue;
                }
                Command::DamageFrom {
                    kind,
                    amount,
                    exhaustion,
                    death_message,
                } => {
                    self.survival.death_message = Some(death_message);
                    self.hurt_with_exhaustion(kind, amount, exhaustion);
                    if !self.survival.dead {
                        self.survival.death_message = None;
                    }
                    continue;
                }
                Command::Kill => {
                    // Vanilla `/kill` uses `damageSources().genericKill()`: it bypasses
                    // invulnerability and creative mode, but absorption still absorbs first.
                    self.hurt(DamageKind::GenericKill, f32::MAX);
                    continue;
                }
                Command::ApplyEffect {
                    effect,
                    amplifier,
                    duration,
                    notify,
                } => {
                    let instance = MobEffectInstance::new(effect, duration, amplifier);
                    if notify {
                        self.apply_effect(instance);
                    } else {
                        self.apply_effect_quiet(instance);
                    }
                    continue;
                }
                Command::RemoveEffect { effect, notify } => {
                    match effect {
                        Some(effect) => {
                            let kind = Effect::from_id(effect);
                            if self.effects.has(kind) {
                                let events = self.effects.remove(kind);
                                if notify {
                                    send(
                                        self,
                                        &RemoveMobEffect {
                                            entity_id: VarInt(self.entity),
                                            effect: VarInt(effect),
                                        },
                                    );
                                }
                                self.apply_effect_events(events);
                                self.refresh_attributes();
                            }
                        }
                        None => {
                            if notify {
                                self.clear_effects();
                            } else {
                                let (_, events) = self.effects.clear();
                                self.apply_effect_events(events);
                                self.refresh_attributes();
                            }
                        }
                    }
                    continue;
                }
                Command::Enchant {
                    target,
                    target_name,
                    enchantment,
                    level,
                    feedback,
                } => {
                    if target == self.uuid {
                        self.enchant_item(&enchantment, level, &target_name, feedback.as_ref());
                    }
                    continue;
                }
                Command::Input(msg) => msg,
            };
            match msg {
                ServerboundPlay::PlayerAction(p) if matches!(p.status.0, 3..=6) => {
                    match p.status.0 {
                        3 => self.drop_selected(true),
                        4 => self.drop_selected(false),
                        5 => self.release_use(),
                        _ => self.swap_hands(),
                    }
                }
                ServerboundPlay::Attack(p) => self.attacks.push(p.entity_id.0),
                // A right-click on an entity ("use"), not an attack: left-clicks arrive as `Attack`
                // above. Feeding animals is the only use so far (taming, saddles come later).
                ServerboundPlay::Interact(p) => self.uses.push((p.entity_id.0, p.hand.0)),
                ServerboundPlay::ContainerClick(p) => {
                    // Another viewer may have bought this offer since our menu was opened, so
                    // validate the click against the current uses and this player's price. A
                    // trader menu whose entity left this region is closed without handling the
                    // click, so a region hand-off cannot leave a live stale snapshot behind.
                    if trader.refresh(self) {
                        self.handle_container_click(p);
                        trader.commit(self);
                    }
                }
                ServerboundPlay::ContainerButtonClick(p) => self.handle_container_button(p),
                ServerboundPlay::SetBeacon(p) => self.handle_set_beacon(p),
                ServerboundPlay::PlaceRecipe(p) => self.handle_place_recipe(p),
                ServerboundPlay::SelectTrade(p) => self.handle_select_trade(p),
                ServerboundPlay::RenameItem(p) => self.rename_item(p.name),
                ServerboundPlay::ContainerClose(_) => {
                    trader.commit(self);
                    self.close_container();
                }
                ServerboundPlay::PlayerAction(p) => {
                    let pos = ferrite_protocol::types::BlockPos::unpack(p.location);
                    let world = plugin_world_name(self.world_id);
                    match p.status.0 {
                        0 if self.hit(pos).is_some() => {
                            let clicked = crate::game::plugin_types::BlockPosition {
                                x: pos.x,
                                y: pos.y,
                                z: pos.z,
                            };
                            let state = self
                                .source
                                .as_ref()
                                .and_then(|s| s.block(pos.x, pos.y, pos.z));
                            let allowed = self.plugin_allows(|plugins| {
                                plugins.player_interacted(
                                    self.uuid,
                                    &self.name,
                                    InteractAction::LeftClickBlock,
                                    crate::game::plugin_types::BlockInteract {
                                        world: &world,
                                        block: clicked,
                                        face: u8::try_from(p.face.clamp(0, 5)).unwrap_or(0),
                                        material: material_name(state.unwrap_or(0)),
                                    },
                                )
                            });
                            if allowed && let Some(state) = state {
                                if self.gamemode == 1 {
                                    if self.plugin_allows(|plugins| {
                                        plugins.block_broken(
                                            self.uuid,
                                            &self.name,
                                            crate::game::plugin_types::BlockBreak {
                                                world: &world,
                                                block: clicked,
                                                material: material_name(state),
                                            },
                                        )
                                    }) && self.break_block(p.location, state)
                                    {
                                        self.drop_creative_shulker(p.location, state);
                                    }
                                } else if self.gamemode == 0 {
                                    // Zero hardness breaks on the first hit (vanilla: destroy
                                    // progress >= 1 on START), so no finish action follows.
                                    let instant = u16::try_from(state).is_ok_and(|s| {
                                        usize::from(s) < ferrite_data::blocks::STATE_COUNT
                                            && ferrite_data::block_props::get(s).hardness == 0.0
                                    });
                                    if instant {
                                        if self.plugin_allows(|plugins| {
                                            plugins.block_broken(
                                                self.uuid,
                                                &self.name,
                                                crate::game::plugin_types::BlockBreak {
                                                    world: &world,
                                                    block: clicked,
                                                    material: material_name(state),
                                                },
                                            )
                                        }) && self.break_block(p.location, state)
                                        {
                                            self.broken.push((pos, state));
                                            // `ServerGamePacketListenerImpl`: breaking a block
                                            // costs 0.005 exhaustion in survival.
                                            self.survival.food.add_exhaustion(0.005);
                                        }
                                    } else {
                                        self.breaking = Some((p.location, state, 0));
                                    }
                                }
                            }
                        }
                        1 => self.breaking = None,
                        2 => {
                            if let Some((location, state, elapsed)) = self.breaking.take()
                                && location == p.location
                                && self.hit(pos).is_some()
                                && u16::try_from(state)
                                    .ok()
                                    .and_then(super::interaction::break_ticks)
                                    .is_some_and(|ticks| elapsed >= ticks)
                                && self.plugin_allows(|plugins| {
                                    plugins.block_broken(
                                        self.uuid,
                                        &self.name,
                                        crate::game::plugin_types::BlockBreak {
                                            world: &world,
                                            block: crate::game::plugin_types::BlockPosition {
                                                x: pos.x,
                                                y: pos.y,
                                                z: pos.z,
                                            },
                                            material: material_name(state),
                                        },
                                    )
                                })
                                && self.break_block(location, state)
                            {
                                self.broken.push((pos, state));
                                // `ServerGamePacketListenerImpl`: 0.005 exhaustion per broken block.
                                self.survival.food.add_exhaustion(0.005);
                            }
                        }
                        _ => {}
                    }
                    self.send_block_snapshot(p.location);
                    send(
                        self,
                        &AcknowledgeBlockChange {
                            sequence: p.sequence,
                        },
                    );
                }
                ServerboundPlay::UseItemOn(p) => {
                    let clicked = ferrite_protocol::types::BlockPos::unpack(p.location);
                    let world = plugin_world_name(self.world_id);
                    let state = self
                        .source
                        .as_ref()
                        .and_then(|s| s.block(clicked.x, clicked.y, clicked.z));
                    let allowed = self.plugin_allows(|plugins| {
                        plugins.player_interacted(
                            self.uuid,
                            &self.name,
                            InteractAction::RightClickBlock,
                            crate::game::plugin_types::BlockInteract {
                                world: &world,
                                block: crate::game::plugin_types::BlockPosition {
                                    x: clicked.x,
                                    y: clicked.y,
                                    z: clicked.z,
                                },
                                face: u8::try_from(p.face.0.clamp(0, 5)).unwrap_or(0),
                                material: material_name(state.unwrap_or(0)),
                            },
                        )
                    });
                    if allowed {
                        let hit = self.hit(clicked);
                        let used = (0..=1).contains(&p.hand.0)
                            && hit.is_some()
                            && self.use_block(p.location, p.hand.0);
                        if !used {
                            self.place(&p);
                        }
                    } else {
                        self.send_block_snapshot(p.location);
                    }
                    self.send_block_snapshot(p.location);
                    if let Some(face) = usize::try_from(p.face.0)
                        .ok()
                        .and_then(|i| ferrite_math::Direction::ALL.get(i))
                    {
                        let pos = ferrite_protocol::types::BlockPos::unpack(p.location);
                        let (dx, dy, dz) = face.step();
                        self.send_block_snapshot(
                            ferrite_protocol::types::BlockPos::new(
                                pos.x + dx,
                                pos.y + dy,
                                pos.z + dz,
                            )
                            .pack(),
                        );
                    }
                    send(
                        self,
                        &AcknowledgeBlockChange {
                            sequence: p.sequence,
                        },
                    );
                }
                ServerboundPlay::CookieResponse(_) => {}
                ServerboundPlay::CustomPayload(p) => {
                    if let Some(plugins) = &self.plugins {
                        let _ = plugins
                            .plugin_message_received(self.uuid, &self.name, &p.channel, &p.data.0);
                    }
                }
                ServerboundPlay::ResourcePackResponse(p) => {
                    if let Some(plugins) = &self.plugins {
                        let _ =
                            plugins.resource_pack_status(self.uuid, &self.name, p.id, p.action.0);
                    }
                }
                ServerboundPlay::PlayerInput(p) => {
                    // Input bit 5 is sneak: it gets a rider off its vehicle.
                    self.sneaking = p.flags & 0x20 != 0;
                    if p.flags & 0x20 != 0 {
                        super::ride::request_dismount(self.uuid);
                    }
                }
                ServerboundPlay::MoveVehicle(p) => {
                    let to = [p.x, p.y, p.z];
                    if self.pending_teleport.is_none()
                        && valid_move(start, to)
                        && p.yaw.is_finite()
                        && p.pitch.is_finite()
                        && super::ride::vehicle_of(self.uuid).is_some()
                    {
                        self.pos = to;
                        self.yaw = p.yaw;
                        self.pitch = p.pitch;
                        self.vehicle_moves.push([p.x, p.y, p.z, f64::from(p.yaw)]);
                        changed = true;
                    }
                }
                ServerboundPlay::PlayerCommand(p) => {
                    // 26.3 `ServerboundPlayerCommandPacket.Action` ids: 0 STOP_SLEEPING,
                    // 1 START_SPRINTING, 2 STOP_SPRINTING, 3/4 riding jump, 5 OPEN_INVENTORY.
                    if p.action.0 == 0 {
                        super::blocks::wake(self);
                    }
                    if p.action.0 == 5 {
                        self.open_animal_chest = true;
                    }
                    // `ServerboundPlayerCommandPacket.Action.START_SPRINTING/STOP_SPRINTING`.
                    if p.entity_id.0 == self.entity {
                        match p.action.0 {
                            1 => {
                                self.survival.sprinting = true;
                            }
                            2 => {
                                self.survival.sprinting = false;
                            }
                            // START_FALL_FLYING; vanilla refuses it on the ground.
                            6 if !self.ground => {
                                self.gliding = true;
                            }
                            _ => {}
                        }
                    }
                }
                ServerboundPlay::SignUpdate(p) => super::blocks::edit_sign(self, &p),
                ServerboundPlay::SetCommandBlock(p) => self.edit_command_block(&p),
                ServerboundPlay::SetJigsawBlock(p) => self.edit_jigsaw_block(&p),
                ServerboundPlay::EditBook(p) => self.handle_edit_book(&p),
                ServerboundPlay::PickItemFromBlock(p) => self.pick_item_from_block(&p),
                ServerboundPlay::PickItemFromEntity(p) => {
                    if self.infinite_materials() {
                        self.pick_entities.push(p.id.0);
                    }
                }
                ServerboundPlay::PlayerAbilities(p) => {
                    if self.gamemode == 1 || self.gamemode == 3 {
                        self.flying = p.flags & 0x02 != 0;
                        if !self.flying {
                            self.gliding = false;
                        }
                    } else {
                        self.flying = false;
                        send(self, &abilities_for_mode(self.gamemode));
                    }
                }
                ServerboundPlay::BundleItemSelected(_) => {}
                ServerboundPlay::ClientInfo(_) | ServerboundPlay::ChangeGameMode(_) => {}
                ServerboundPlay::Punch | ServerboundPlay::ClientTickEnd => {}
                ServerboundPlay::SetHeldItem(p) => {
                    // Vanilla `handleSetCarriedItem` ignores slots outside the hotbar.
                    if (0..=8).contains(&p.slot) {
                        self.using_bow = false;
                        self.bow_charge = 0;
                        self.blocking = false;
                        if p.slot as usize != self.inventory.selected
                            && self
                                .survival
                                .eating
                                .is_some_and(|eating| eating.slot != ferrite_inventory::OFFHAND)
                        {
                            self.survival.eating = None;
                        }
                        self.inventory.selected = p.slot as usize;
                    }
                }
                ServerboundPlay::CreativeSlot(p) => self.handle_creative_slot(p),
                ServerboundPlay::Confirm(p) => {
                    if self.pending_teleport == Some(p.id.0) {
                        self.pending_teleport = None;
                    }
                }
                ServerboundPlay::ChunkBatchReceived(rate) => {
                    if rate.is_finite() {
                        self.rate = rate.clamp(1.0, 64.0);
                        self.awaiting_batch = false;
                    }
                }
                ServerboundPlay::Position(p) => {
                    if self.pending_teleport.is_none() {
                        let to = [p.x, p.y, p.z];
                        if valid_move(start, to) {
                            let movement = crate::game::plugin_types::PlayerMove {
                                from: self.pos,
                                to,
                                from_yaw: self.yaw,
                                from_pitch: self.pitch,
                                to_yaw: self.yaw,
                                to_pitch: self.pitch,
                            };
                            if to != self.pos
                                && !self.plugin_allows(|plugins| {
                                    plugins.player_moved(self.uuid, &self.name, &movement)
                                })
                            {
                                self.teleport();
                                continue;
                            }
                            self.pos = to;
                            self.set_ground(p.flags & 1 != 0);
                            changed = true;
                        } else {
                            self.teleport();
                        }
                    }
                }
                ServerboundPlay::PositionRotation(p) => {
                    if self.pending_teleport.is_none() {
                        let to = [p.x, p.y, p.z];
                        if valid_move(start, to) && p.yaw.is_finite() && p.pitch.is_finite() {
                            let movement = crate::game::plugin_types::PlayerMove {
                                from: self.pos,
                                to,
                                from_yaw: self.yaw,
                                from_pitch: self.pitch,
                                to_yaw: p.yaw,
                                to_pitch: p.pitch,
                            };
                            if to != self.pos
                                && !self.plugin_allows(|plugins| {
                                    plugins.player_moved(self.uuid, &self.name, &movement)
                                })
                            {
                                self.teleport();
                                continue;
                            }
                            self.pos = to;
                            self.yaw = p.yaw;
                            self.pitch = p.pitch;
                            self.set_ground(p.flags & 1 != 0);
                            changed = true;
                        } else {
                            self.teleport();
                        }
                    }
                }
                ServerboundPlay::Rotation(p) => {
                    if p.yaw.is_finite() && p.pitch.is_finite() {
                        self.yaw = p.yaw;
                        self.pitch = p.pitch;
                        self.set_ground(p.flags & 1 != 0);
                        changed = true;
                    }
                }
                ServerboundPlay::Flags(p) => {
                    self.set_ground(p.flags & 1 != 0);
                }
                ServerboundPlay::UseItem(p) => self.use_item(&p),
                // `ServerboundClientCommandPacket.Action.PERFORM_RESPAWN`.
                ServerboundPlay::ClientCommand(p) if p.action.0 == 0 && self.survival.dead => {
                    self.respawn_requested = true;
                }
                // The credits ended: the portal carries on, with no wait left.
                ServerboundPlay::ClientCommand(p) if p.action.0 == 0 && self.watching_credits => {
                    self.watching_credits = false;
                    self.portal_ticks = super::portal::WAIT_TICKS;
                }
                _ => {}
            }
        }
        if changed {
            self.positions.lock().unwrap().insert(self.uuid, self.pos);
            let _ = self.signals.send(Signal::Update {
                uuid: self.uuid,
                pos: self.pos,
                yaw: self.yaw,
                pitch: self.pitch,
                ground: self.ground,
            });
        }
        true
    }
}
/// Sends a tracker packet to a player: through its connection while it is in this region,
/// otherwise through the world-wide table (it left the region since it started tracking).
/// A change to a mob's active effects that its trackers have not been told about yet.
#[derive(Debug, Clone, Copy, PartialEq)]
enum MobEffectNotice {
    Added(ferrite_entity::effect::MobEffectInstance),
    Removed(i32),
}

fn deliver(players: &HashMap<Uuid, Player>, ctx: &TickContext, viewer: ViewerKey, bytes: &[u8]) {
    let packet: Arc<[u8]> = bytes.to_vec().into();
    deliver_arc(players, ctx, viewer, &packet);
}

fn deliver_arc(
    players: &HashMap<Uuid, Player>,
    ctx: &TickContext,
    (uuid, session): ViewerKey,
    bytes: &Arc<[u8]>,
) {
    let packet = Outbound::Packet(Arc::clone(bytes));
    match players.get(&uuid) {
        Some(p) if p.session == session => {
            let _ = p.outbound.send(packet);
        }
        Some(_) => {}
        None => {
            if let Some((_, _, outbound)) = ctx.tracking.lock().unwrap().get(&uuid) {
                let _ = outbound.send(packet);
            }
        }
    }
}

/// The mobs in `bbox` a riptide spin has not hurt yet; they are added to `hit` so one spin hurts
/// each of them once.
fn spin_victims(
    entities: &ferrite_entity::EntityStore,
    bbox: ferrite_math::AABB,
    hit: &mut Vec<Uuid>,
) -> Vec<(ferrite_entity::EntityKey, Uuid)> {
    let mut victims = Vec::new();
    for key in entities.query_vec(bbox) {
        if !mob::is_mob(entities, key) {
            continue;
        }
        let Some(target) = entities.data(key).map(|data| data.uuid) else {
            continue;
        };
        if !hit.contains(&target) {
            hit.push(target);
            victims.push((key, target));
        }
    }
    victims
}

/// Sends player metadata to other online players in the same dimension. Player entities are not
/// in the region entity tracker, so state changes such as riptide spin need this world-wide path.
fn should_broadcast_player_metadata(
    source: Uuid,
    viewer: Uuid,
    source_world: super::WorldId,
    viewer_world: super::WorldId,
) -> bool {
    source != viewer && source_world == viewer_world
}

/// The wire form of `set_entity_data` for one shoulder: packet id, entity id, metadata index,
/// the optional-varint serializer id (19), the presence flag, the variant and the terminator.
fn shoulder_metadata_bytes(entity: i32, side: ShoulderSide, variant: Option<i32>) -> Vec<u8> {
    let mut bytes = Vec::new();
    VarInt(SetEntityData::ID).encode(&mut bytes);
    VarInt(entity).encode(&mut bytes);
    side.metadata_index().encode(&mut bytes);
    VarInt(19).encode(&mut bytes);
    variant.is_some().encode(&mut bytes);
    if let Some(variant) = variant {
        VarInt(variant).encode(&mut bytes);
    }
    bytes.push(0xFF);
    bytes
}

fn broadcast_shoulder_bytes(
    tracking: &Tracking,
    dimension: impl Into<super::WorldId>,
    source: Uuid,
    bytes: &[u8],
) {
    let dimension = dimension.into();
    let packet = Outbound::Packet(bytes.to_vec().into());
    for (uuid, (viewer_dimension, _, outbound)) in tracking.lock().unwrap().iter() {
        if should_broadcast_player_metadata(source, *uuid, dimension, *viewer_dimension) {
            let _ = outbound.send(packet.clone());
        }
    }
}

fn spin_metadata(entity: i32, spin: bool) -> SetEntityData {
    SetEntityData {
        id: VarInt(entity),
        values: vec![MetaEntry {
            index: 8,
            value: MetaValue::Byte(if spin { 4 } else { 0 }),
        }],
    }
}

fn broadcast_player_metadata(
    tracking: &Tracking,
    dimension: impl Into<super::WorldId>,
    source: Uuid,
    entity: i32,
    spin: bool,
) {
    let dimension = dimension.into();
    let bytes = packet(&spin_metadata(entity, spin));
    for (uuid, (viewer_dimension, _, outbound)) in tracking.lock().unwrap().iter() {
        if should_broadcast_player_metadata(source, *uuid, dimension, *viewer_dimension) {
            let _ = outbound.send(bytes.clone());
        }
    }
}

/// Ends a spin immediately for an interruption, respawn or dimension change. Returns `false`
/// when the player was not spinning, so the clear packet is sent only once.
fn end_player_spin(
    spin_ticks: &mut u8,
    tracking: &Tracking,
    dimension: impl Into<super::WorldId>,
    source: Uuid,
    entity: i32,
) -> bool {
    if *spin_ticks == 0 {
        return false;
    }
    *spin_ticks = 0;
    broadcast_player_metadata(tracking, dimension, source, entity, false);
    true
}

fn trader_spawning_enabled(rules: &ferrite_command::gamerule::GameRules) -> bool {
    rules.get_bool("spawn_wandering_traders").unwrap_or(true)
}

fn trader_spawning_enabled_in_dimension(
    rules: &ferrite_command::gamerule::GameRules,
    dimension: super::Dimension,
) -> bool {
    dimension == super::Dimension::Overworld && trader_spawning_enabled(rules)
}

#[cfg(test)]
pub(super) fn apply_trade_sync(
    entities: &mut EntityStore,
    key: ferrite_entity::EntityKey,
    player: Uuid,
    trades: &[(i32, i32)],
) {
    let Some(data) = entities.data_mut(key) else {
        return;
    };
    let village = data
        .mob
        .as_ref()
        .is_some_and(|mob| mob.kind == ferrite_entity::mob::MobKind::Villager);
    let Some(villager) = data.villager.as_mut() else {
        return;
    };
    if villager.offers.len() != trades.len() {
        return;
    }
    let sold: i32 = villager
        .offers
        .iter()
        .zip(trades)
        .map(|(offer, (uses, _))| (*uses - offer.uses).max(0))
        .sum();
    for (offer, (uses, demand)) in villager.offers.iter_mut().zip(trades) {
        // Independent open menus hold snapshots. An older one must not roll back a newer
        // sale and make that same sale look new when the newer snapshot arrives again.
        offer.uses = offer.uses.max(*uses);
        offer.demand = offer.demand.max(*demand);
    }
    if village {
        for _ in 0..sold {
            villager.record_trade(player);
        }
    }
}

/// The fixed `(x, z)` of a chunk face: the coordinate that is set is the edge, the other walks.
type Face = (Option<u8>, Option<u8>);

/// What the seam wake pass needs to know about one block state.
#[derive(Clone, Copy, Default)]
struct SeamInfo {
    interesting: bool,
    fluid: bool,
    gravity: bool,
    wall: bool,
}

impl Region {
    pub fn hold_chunk(&mut self, chunk: ChunkPos) {
        let first = !self.held_chunks.contains_key(&chunk);
        *self.held_chunks.entry(chunk).or_default() += 1;
        if first && !self.pending_chunk_loads.contains(&chunk) {
            self.pending_chunk_loads.push(chunk);
        }
    }
    /// Drops one viewer's hold; the chunk leaves the region when the last one goes.
    pub fn release_chunk(&mut self, chunk: ChunkPos) {
        if let Some(count) = self.held_chunks.get_mut(&chunk) {
            *count -= 1;
            if *count == 0 {
                self.held_chunks.remove(&chunk);
                self.pending_chunk_loads.retain(|&pending| pending != chunk);
            }
        }
    }
    /// Every held chunk with its hold count, so a stopped region's chunks can be re-held.
    pub fn held_chunk_counts(&self) -> Vec<(ChunkPos, usize)> {
        self.held_chunks.iter().map(|(&c, &n)| (c, n)).collect()
    }
    pub fn has_chunk_hold(&self, chunk: ChunkPos) -> bool {
        self.held_chunks.contains_key(&chunk)
    }

    /// The wake-up pass for chunks that were held since the last tick. A fluid that reached an
    /// unloaded chunk edge stopped there (`FluidWorld::owns` and the unloaded read-as-stone rule);
    /// once both sides are loaded it is scheduled again, and a support rule that could only see
    /// the unloaded side as sturdy is re-validated. Only border cells that actually hold fluid,
    /// a gravity block or a block whose `canSurvive` now fails are queued, so a full chunk load
    /// does not enqueue an update for every block on the seam.
    /// Runs from `Region::tick`, one chunk per tick. The seam faces are read straight from the
    /// chunks' paletted containers and only the sections whose palettes hold something that can
    /// need a wake are visited (`docs/world-interaction-audit.md` has the measured cost).
    pub fn wake_loaded_chunks(
        &mut self,
        source: &super::anvil::AnvilChunkSource,
        signals: UnboundedSender<Signal>,
    ) {
        let mut pending = std::mem::take(&mut self.pending_chunk_loads);
        if pending.is_empty() {
            return;
        }
        // A join loads many chunks at once. Keep the work per tick bounded; the rest stays queued
        // and gets the same wake-up a few ticks later.
        const CHUNKS_PER_TICK: usize = 1;
        if pending.len() > CHUNKS_PER_TICK {
            self.pending_chunk_loads = pending.split_off(CHUNKS_PER_TICK);
        }
        // What the seam pass needs to know about a state, looked up once per distinct state:
        // the block-name based queries are far dearer than the palette reads around them.
        let info_cache: std::cell::RefCell<Vec<Option<SeamInfo>>> =
            std::cell::RefCell::new(vec![None; 1 << 16]);
        let info = |state: u32| -> SeamInfo {
            if let Some(Some(known)) = info_cache.borrow().get(state as usize) {
                return *known;
            }
            let answer = u16::try_from(state)
                .ok()
                .map_or(SeamInfo::default(), |small| {
                    let name = ferrite_data::blocks::state(small).map(|(name, _)| name);
                    let props = ferrite_data::block_props::get(small);
                    let fluid = props.fluid.is_some();
                    let gravity = super::gravity::is_gravity(small);
                    // Cave and void air are separate states and are not full blocks; without this
                    // check every underground cave section would look interesting.
                    let air = matches!(
                        name,
                        Some("minecraft:air" | "minecraft:cave_air" | "minecraft:void_air")
                    );
                    SeamInfo {
                        interesting: !air && (fluid || !props.can_occlude || gravity),
                        fluid,
                        gravity,
                        // The support rules that read a horizontal neighbour
                        // (`neighbors::wall_support`: wall torches, wall signs, wall buttons)
                        // belong to states with a `facing`; every other rule reads only the
                        // block's own column, which a seam cannot change.
                        wall: ferrite_data::blocks::state(small).is_some_and(|(_, props)| {
                            props.iter().any(|(name, _)| *name == "facing")
                        }),
                    }
                });
            if let Some(slot) = info_cache.borrow_mut().get_mut(state as usize) {
                *slot = Some(answer);
            }
            answer
        };
        let interesting = |state: u32| info(state).interesting;
        for chunk in pending {
            if !self.held_chunks.contains_key(&chunk) {
                continue;
            }
            // Which sections of a chunk hold a fluid, a non-full block or a gravity block,
            // read from the section palettes alone (one lock per chunk).
            let mask = |at: ChunkPos| {
                source.with_loaded_chunk(at.0, at.1, |chunk| {
                    let bits: Vec<bool> = chunk
                        .sections()
                        .iter()
                        .map(|section| {
                            section
                                .blocks()
                                .palette()
                                .iter()
                                .any(|&state| interesting(state))
                        })
                        .collect();
                    (chunk.min_y(), bits)
                })
            };
            let Some((min_y, inside_mask)) = mask(chunk) else {
                continue;
            };
            if inside_mask.is_empty() {
                continue;
            }
            let (x0, z0) = (chunk.0 * 16, chunk.1 * 16);
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let outside_chunk = ChunkPos(chunk.0 + dx, chunk.1 + dz);
                // Still unloaded: nothing has become readable on that side.
                let Some((_, outside_mask)) = mask(outside_chunk) else {
                    continue;
                };
                // The sections where either side could need a wake.
                let sections: Vec<usize> = (0..inside_mask.len())
                    .filter(|&index| {
                        inside_mask[index] || outside_mask.get(index).copied().unwrap_or(false)
                    })
                    .collect();
                if sections.is_empty() {
                    continue;
                }
                // The two faces of the seam, read straight from the chunks' paletted containers
                // (one lock each) instead of one `source.block` call per cell. `face[t][y]`
                // walks along the seam, then up the section.
                let (inside_face, outside_face): (Face, Face) = match (dx, dz) {
                    (1, 0) => ((Some(15), None), (Some(0), None)),
                    (-1, 0) => ((Some(0), None), (Some(15), None)),
                    (0, 1) => ((None, Some(15)), (None, Some(0))),
                    _ => ((None, Some(0)), (None, Some(15))),
                };
                let read = |at: ChunkPos, fixed: (Option<u8>, Option<u8>)| {
                    source.with_loaded_chunk(at.0, at.1, |chunk| {
                        let mut cells: Vec<u32> = Vec::with_capacity(sections.len() * 256);
                        for &index in &sections {
                            let base = min_y + index as i32 * 16;
                            for t in 0..16u8 {
                                let (x, z) = (fixed.0.unwrap_or(t), fixed.1.unwrap_or(t));
                                for y in base..base + 16 {
                                    cells.push(chunk.block(x, y, z).unwrap_or(u32::MAX));
                                }
                            }
                        }
                        cells
                    })
                };
                let (Some(inside_cells), Some(outside_cells)) =
                    (read(chunk, inside_face), read(outside_chunk, outside_face))
                else {
                    continue;
                };
                let mut cell = 0;
                for &index in &sections {
                    let base = min_y + index as i32 * 16;
                    for t in 0..16i32 {
                        let (x, z) = if dx != 0 {
                            (if dx > 0 { x0 + 15 } else { x0 }, z0 + t)
                        } else {
                            (x0 + t, if dz > 0 { z0 + 15 } else { z0 })
                        };
                        let (ox, oz) = (x + dx, z + dz);
                        for y in base..base + 16 {
                            let (inside_state, outside_state) =
                                (inside_cells[cell], outside_cells[cell]);
                            cell += 1;
                            if inside_state == u32::MAX || outside_state == u32::MAX {
                                continue;
                            }
                            // A pair of full blocks (or air) cannot need a fluid, support or
                            // gravity wake; this keeps the pass on the surface.
                            if !interesting(inside_state) && !interesting(outside_state) {
                                continue;
                            }
                            self.wake_seam_cell(
                                source,
                                &signals,
                                outside_chunk,
                                ([x, y, z], inside_state),
                                ([ox, y, oz], outside_state),
                                (info(inside_state), info(outside_state)),
                            );
                        }
                    }
                }
            }
        }
    }

    /// The wake-up for one pair of cells that touch across a chunk seam: `inside` is in a
    /// chunk this region just took, `outside` is across the seam in `outside_chunk`.
    fn wake_seam_cell(
        &mut self,
        source: &super::anvil::AnvilChunkSource,
        signals: &UnboundedSender<Signal>,
        outside_chunk: ChunkPos,
        inside: ([i32; 3], u32),
        outside: ([i32; 3], u32),
        (inside_info, outside_info): (SeamInfo, SeamInfo),
    ) {
        let dimension = source.dimension();
        let ([x, y, z], inside_state) = inside;
        let ([ox, _, oz], outside_state) = outside;
        // Only fluid that could not move because the other side was unloaded needs a wake. An
        // ocean border with the same fluid on both sides was generated settled and does not
        // (that case alone is what made the join-time scan schedule 130k ticks).
        let same_fluid = inside_info.fluid
            && outside_info.fluid
            && ferrite_fluid::is_liquid_block(inside_state)
            && ferrite_fluid::is_liquid_block(outside_state)
            && ferrite_fluid::fluid_state(inside_state).kind
                == ferrite_fluid::fluid_state(outside_state).kind;
        if !same_fluid
            && inside_info.fluid
            && ferrite_fluid::is_liquid_block(inside_state)
            && ferrite_fluid::can_hold_fluid(
                outside_state,
                ferrite_fluid::fluid_state(inside_state).kind,
            )
        {
            self.schedule_fluid((x, y, z));
        }
        if !same_fluid
            && outside_info.fluid
            && ferrite_fluid::is_liquid_block(outside_state)
            && ferrite_fluid::can_hold_fluid(
                inside_state,
                ferrite_fluid::fluid_state(outside_state).kind,
            )
        {
            if self.held_chunks.contains_key(&outside_chunk) {
                self.schedule_fluid((ox, y, oz));
            } else {
                let _ = signals.send(Signal::FluidChange {
                    dimension,
                    x: ox,
                    y,
                    z: oz,
                });
            }
        }
        let inside_u16 = u16::try_from(inside_state).ok();
        let outside_u16 = u16::try_from(outside_state).ok();
        if inside_info.wall
            && inside_u16
                .is_some_and(|state| !super::neighbors::can_survive(state, [x, y, z], source))
        {
            // The block is ours; the unreadable side is now readable and failed its support
            // test. Re-validate it without forwarding the origin.
            self.updates.remote_changed([ox, y, oz]);
        }
        if outside_info.wall
            && outside_u16
                .is_some_and(|state| !super::neighbors::can_survive(state, [ox, y, oz], source))
        {
            if self.held_chunks.contains_key(&outside_chunk) {
                self.updates.block_changed([x, y, z]);
            } else {
                let _ = signals.send(Signal::NeighborUpdate {
                    dimension,
                    origin: [x, y, z],
                    target: [ox, y, oz],
                });
            }
        }
        if inside_info.gravity || outside_info.gravity {
            self.schedule_gravity(source, &[[x, y, z], [ox, y, oz]]);
        }
    }
    /// Knocks the entity with `uuid` away from `source`, for a hit another region's shield
    /// blocked. Does nothing when the entity has since left this region.
    pub fn knock_back_by_uuid(&mut self, uuid: Uuid, source: ferrite_math::Vec3) -> bool {
        match trader_key(&self.entities, uuid) {
            Some(key) => {
                mob::knockback(&mut self.entities, key, source);
                true
            }
            None => false,
        }
    }
    /// Removes and returns every entity standing in `chunk`, so the world loop can persist the
    /// chunk's entities when it unloads. Entities in chunks that stay loaded are untouched.
    pub fn take_chunk_entities(&mut self, chunk: ChunkPos) -> Vec<EntityRecord> {
        let keys: Vec<_> = self
            .entities
            .keys()
            .iter()
            .copied()
            .filter(|&key| {
                self.entities
                    .pos(key)
                    .is_some_and(|pos| chunk_of(pos) == (chunk.0, chunk.1))
            })
            .collect();
        let records: Vec<_> = keys
            .into_iter()
            .filter_map(|key| self.entities.remove(key))
            .collect();
        for record in &records {
            self.published_facts.remove(&record.data.uuid);
        }
        records
    }
    /// Copies every entity currently in a held chunk for periodic persistence without unloading
    /// it or changing its live in-memory state.
    pub fn snapshot_chunk_entities(&self, chunk: ChunkPos) -> Vec<EntityRecord> {
        self.entities
            .keys()
            .iter()
            .copied()
            .filter_map(|key| {
                let pos = self.entities.pos(key)?;
                if ferrite_entity::chunk_of(pos) != (chunk.0, chunk.1) {
                    return None;
                }
                Some(EntityRecord {
                    pos,
                    vel: self.entities.vel(key)?,
                    flags: self.entities.flags(key)?,
                    data: self.entities.data(key)?.clone(),
                })
            })
            .collect()
    }
    /// Puts entities loaded from a chunk sidecar back into this region.
    pub fn add_entities(&mut self, records: Vec<EntityRecord>) {
        for record in records {
            self.entities.insert(record);
        }
    }
    /// Removes every entity from the region, for the shutdown save.
    pub fn drain_entities(&mut self) -> Vec<EntityRecord> {
        let keys: Vec<_> = self.entities.keys().to_vec();
        let records: Vec<_> = keys
            .into_iter()
            .filter_map(|key| self.entities.remove(key))
            .collect();
        records
    }
    /// A block next to this region changed in another region (`Signal::NeighborUpdate`). Only
    /// this region's own neighbours of `origin` are re-validated on the next tick.
    pub fn remote_block_changed(&mut self, origin: [i32; 3]) {
        self.updates.remote_changed(origin);
        self.remote_changes.push(origin);
    }
    pub fn schedule_fluid(&mut self, pos: (i32, i32, i32)) {
        let chunk = ChunkPos(pos.0.div_euclid(16), pos.2.div_euclid(16));
        if self.held_chunks.contains_key(&chunk) {
            self.fluid_ticks.schedule(pos, 0);
        }
    }
    pub fn tick_random(
        &mut self,
        source: &super::anvil::AnvilChunkSource,
        tracking: &Tracking,
        signals: UnboundedSender<Signal>,
        speed: u32,
        spread_vines: bool,
        turtle_hatch_chance: f32,
    ) {
        let turtle_eggs = self.random_ticks.tick(
            source,
            &self.held_chunks,
            tracking,
            signals,
            super::random_tick::RandomTickOptions {
                speed,
                spread_vines,
                turtle_hatch_chance,
            },
            &mut self.fire,
        );
        for event in turtle_eggs {
            use super::random_tick::TurtleEggEvent;
            match event {
                TurtleEggEvent::Crack(at) => {
                    let pitch = 0.9 + self.rng.next_f32() * 0.2;
                    self.play_sound_near(
                        block_sound_center(at),
                        ferrite_entity::turtle::EGG_CRACK_SOUND,
                        0.5,
                        pitch,
                    );
                }
                TurtleEggEvent::Hatch { at, state, eggs } => {
                    let chunk = ChunkPos(at[0].div_euclid(16), at[2].div_euclid(16));
                    if !self.held_chunks.contains_key(&chunk) {
                        continue;
                    }
                    let pitch = 0.9 + self.rng.next_f32() * 0.2;
                    self.play_sound_near(
                        block_sound_center(at),
                        ferrite_entity::turtle::EGG_HATCH_SOUND,
                        0.7,
                        pitch,
                    );
                    turtle_egg_break_particles(tracking, source.world_id(), at, state);
                    for record in ferrite_entity::turtle::hatch_babies(at, eggs, &mut self.rng) {
                        self.entities.insert(record);
                    }
                }
            }
        }
    }

    pub fn clear_pending_tree(&mut self, origin: [i32; 3]) {
        self.random_ticks.pending_trees.remove(&origin);
    }

    /// Applies a tree placed by the worldgen feature host under region ownership: local chunks
    /// are written directly and foreign chunks are handed to their owning region via `TreePlaceChunk`.
    pub fn apply_tree(
        &mut self,
        source: &super::anvil::AnvilChunkSource,
        tracking: &Tracking,
        signals: &UnboundedSender<Signal>,
        origin: [i32; 3],
        saplings: &[([i32; 3], u32)],
        placed: Result<super::generator::Placed, String>,
    ) {
        self.random_ticks.pending_trees.remove(&origin);
        let origin_chunk = ChunkPos(origin[0].div_euclid(16), origin[2].div_euclid(16));
        if !self.held_chunks.contains_key(&origin_chunk) {
            return;
        }
        let validated = match placed {
            Ok(placed) => super::random_tick::validate_tree_placement(source, saplings, placed),
            Err(_) => None,
        };
        let Some(validated) = validated else {
            // Vanilla `TreeGrower.growTree` removes all four saplings before placing a
            // 2x2/mega feature and never restores them; a failed single sapling stays at stage 1.
            if saplings.len() == 4 {
                self.clear_tree_saplings(source, tracking, signals, saplings);
            }
            return;
        };
        let mut local_blocks = Vec::new();
        let mut foreign_blocks: HashMap<ChunkPos, Vec<([i32; 3], u32, u16)>> = HashMap::new();
        for entry in validated.blocks {
            let chunk = ChunkPos(entry.0[0].div_euclid(16), entry.0[2].div_euclid(16));
            if self.held_chunks.contains_key(&chunk) {
                local_blocks.push(entry);
            } else {
                foreign_blocks.entry(chunk).or_default().push(entry);
            }
        }
        let mut local_entities = Vec::new();
        let mut foreign_entities: HashMap<ChunkPos, Vec<([i32; 3], ferrite_nbt::Compound)>> =
            HashMap::new();
        for entity in validated.entities {
            let chunk = ChunkPos(entity.0[0].div_euclid(16), entity.0[2].div_euclid(16));
            if self.held_chunks.contains_key(&chunk) {
                local_entities.push(entity);
            } else {
                foreign_entities.entry(chunk).or_default().push(entity);
            }
        }
        if !local_blocks.is_empty() {
            self.apply_tree_chunk_inner(source, tracking, signals, &local_blocks, &local_entities);
        }
        for (chunk, blocks) in foreign_blocks {
            let entities = foreign_entities.remove(&chunk).unwrap_or_default();
            let _ = signals.send(Signal::TreePlaceChunk {
                world: source.world_id(),
                chunk,
                blocks,
                entities,
            });
        }
    }

    /// Removes the four saplings of a failed 2x2/mega attempt, matching `TreeGrower.growTree`.
    fn clear_tree_saplings(
        &mut self,
        source: &super::anvil::AnvilChunkSource,
        tracking: &Tracking,
        signals: &UnboundedSender<Signal>,
        saplings: &[([i32; 3], u32)],
    ) {
        let mut local_blocks = Vec::new();
        let mut foreign_blocks: HashMap<ChunkPos, Vec<([i32; 3], u32, u16)>> = HashMap::new();
        for &(pos, expected) in saplings {
            if source.block(pos[0], pos[1], pos[2]) != Some(expected) {
                continue;
            }
            let Ok(empty) = u16::try_from(super::random_tick::sapling_empty_replacement(expected))
            else {
                continue;
            };
            let chunk = ChunkPos(pos[0].div_euclid(16), pos[2].div_euclid(16));
            let entry = (pos, expected, empty);
            if self.held_chunks.contains_key(&chunk) {
                local_blocks.push(entry);
            } else {
                foreign_blocks.entry(chunk).or_default().push(entry);
            }
        }
        if !local_blocks.is_empty() {
            self.apply_tree_chunk_inner(source, tracking, signals, &local_blocks, &[]);
        }
        for (chunk, blocks) in foreign_blocks {
            let _ = signals.send(Signal::TreePlaceChunk {
                world: source.world_id(),
                chunk,
                blocks,
                entities: Vec::new(),
            });
        }
    }

    /// Applies handed-off tree blocks on the region that owns `chunk`.
    pub fn apply_tree_chunk(
        &mut self,
        source: &super::anvil::AnvilChunkSource,
        tracking: &Tracking,
        signals: &UnboundedSender<Signal>,
        chunk: ChunkPos,
        blocks: &[([i32; 3], u32, u16)],
        entities: &[([i32; 3], ferrite_nbt::Compound)],
    ) {
        if !self.held_chunks.contains_key(&chunk) {
            return;
        }
        let matching: Vec<_> = blocks
            .iter()
            .copied()
            .filter(|&(pos, expected, _)| {
                ChunkPos(pos[0].div_euclid(16), pos[2].div_euclid(16)) == chunk
                    && source.block(pos[0], pos[1], pos[2]) == Some(expected)
            })
            .collect();
        if !matching.is_empty() {
            self.apply_tree_chunk_inner(source, tracking, signals, &matching, entities);
        }
    }

    fn apply_tree_chunk_inner(
        &mut self,
        source: &super::anvil::AnvilChunkSource,
        tracking: &Tracking,
        signals: &UnboundedSender<Signal>,
        blocks: &[([i32; 3], u32, u16)],
        entities: &[([i32; 3], ferrite_nbt::Compound)],
    ) {
        let batch: Vec<([i32; 3], u32, u32)> = blocks
            .iter()
            .map(|&(pos, expected, state)| (pos, expected, u32::from(state)))
            .collect();
        let mut applied = Vec::with_capacity(blocks.len());
        if source.compare_set_blocks(&batch).is_ok() {
            for &(pos, _, state) in blocks {
                applied.push((pos, state));
            }
        } else {
            for &(pos, expected, state) in blocks {
                if source
                    .compare_set_block(pos[0], pos[1], pos[2], Some(expected), u32::from(state))
                    .is_ok()
                {
                    applied.push((pos, state));
                }
            }
        }
        if applied.is_empty() {
            return;
        }
        let mut changed_set = HashSet::with_capacity(applied.len());
        {
            let observers = tracking.lock().unwrap();
            let world_id = source.world_id();
            for &(pos, state) in &applied {
                changed_set.insert(pos);
                let update = BlockUpdate {
                    location: ferrite_protocol::types::BlockPos::new(pos[0], pos[1], pos[2]).pack(),
                    block_state: VarInt(i32::from(state)),
                };
                broadcast_tracked_chunk(
                    &observers,
                    world_id,
                    (pos[0].div_euclid(16), pos[2].div_euclid(16)),
                    &packet(&update),
                );
            }
        }
        for (pos, data) in entities {
            if changed_set.contains(pos) {
                super::commands_world::WorldBlocks::set_block_entity(source, *pos, data);
            }
        }
        let mut changed: Vec<[i32; 3]> = applied.iter().map(|&(pos, _)| pos).collect();
        for &pos in &changed {
            self.updates.block_changed(pos);
        }
        let mut removed = Vec::new();
        {
            let mut world =
                super::fluid::RegionFluidWorld::new(source, &self.held_chunks, tracking)
                    .with_signals(signals.clone());
            self.updates.apply(&mut world, &mut removed);
        }
        for (pos, removed_state) in &removed {
            self.spawn_block_drops(source, *pos, *removed_state);
            changed.push(*pos);
        }
        for &pos in &changed {
            let _ = signals.send(Signal::FluidChange {
                dimension: source.dimension(),
                x: pos[0],
                y: pos[1],
                z: pos[2],
            });
        }
        self.schedule_gravity(source, &changed);
        let positions = changed
            .iter()
            .flat_map(|&pos| {
                [
                    (0, 0, 0),
                    (1, 0, 0),
                    (-1, 0, 0),
                    (0, 1, 0),
                    (0, -1, 0),
                    (0, 0, 1),
                    (0, 0, -1),
                ]
                .into_iter()
                .map(move |(dx, dy, dz)| ([pos[0] + dx, pos[1] + dy, pos[2] + dz], pos))
            })
            .collect();
        let _ = signals.send(Signal::RedstoneChange {
            dimension: source.dimension(),
            positions,
        });
    }
    pub fn tick_fluids(
        &mut self,
        source: &super::anvil::AnvilChunkSource,
        tracking: &Tracking,
        signals: UnboundedSender<Signal>,
        water_conversion: bool,
        lava_conversion: bool,
    ) {
        let mut world = super::fluid::RegionFluidWorld::with_source_rules(
            source,
            &self.held_chunks,
            tracking,
            water_conversion,
            lava_conversion,
        )
        .with_signals(signals.clone())
        .with_batched_updates();
        let timings = self.timings.clone();
        {
            let _advance = timings.as_ref().map(|t| t.scope("advance"));
            self.fluid_ticks.advance(&mut world);
        }
        let washed = std::mem::take(&mut world.washed);
        // Only blocks water actually destroyed need a shape pass. Queueing a neighbour pass for
        // every water level change would turn an ocean border into an update storm on every
        // flowing-water tick.
        for (pos, _) in &washed {
            self.updates.block_changed(*pos);
        }
        let mut removed = Vec::new();
        {
            let _updates = timings.as_ref().map(|t| t.scope("updates"));
            self.updates.apply(&mut world, &mut removed);
        }
        world.flush_updates();
        drop(world);
        for (pos, state) in &washed {
            self.spawn_block_drops(source, *pos, *state);
        }
        for (pos, state) in &removed {
            self.spawn_block_drops(source, *pos, *state);
            let _ = signals.send(Signal::FluidChange {
                dimension: source.dimension(),
                x: pos[0],
                y: pos[1],
                z: pos[2],
            });
        }
        if !washed.is_empty() || !removed.is_empty() {
            let mut changed: Vec<[i32; 3]> = washed.iter().map(|(pos, _)| *pos).collect();
            changed.extend(removed.iter().map(|(pos, _)| *pos));
            // Redstone beside a destroyed block needs its own wake; the fluid signal only
            // schedules fluid ticks. The world loop routes this to the component's owner.
            let positions = changed
                .iter()
                .flat_map(|&pos| {
                    [
                        (0, 0, 0),
                        (1, 0, 0),
                        (-1, 0, 0),
                        (0, 1, 0),
                        (0, -1, 0),
                        (0, 0, 1),
                        (0, 0, -1),
                    ]
                    .into_iter()
                    .map(move |(dx, dy, dz)| ([pos[0] + dx, pos[1] + dy, pos[2] + dz], pos))
                })
                .collect();
            let _ = signals.send(Signal::RedstoneChange {
                dimension: source.dimension(),
                positions,
            });
            self.schedule_gravity(source, &changed);
        }
    }

    /// Applies a fluid placement handed over by the region that simulated it. The world loop
    /// schedules this on the chunk owner, so only the owner writes the block.
    pub fn apply_fluid_place(
        &mut self,
        source: &super::anvil::AnvilChunkSource,
        tracking: &Tracking,
        signals: UnboundedSender<Signal>,
        (x, y, z): (i32, i32, i32),
        expected: u32,
        state: u32,
    ) {
        let chunk = ChunkPos(x.div_euclid(16), z.div_euclid(16));
        if !self.held_chunks.contains_key(&chunk) {
            return;
        }
        let Some(old) = source.block(x, y, z) else {
            return;
        };
        if old != expected || old == state {
            return;
        }
        if let Err(error) = source.compare_set_block(x, y, z, Some(old), state) {
            tracing::warn!(x, y, z, %error, "handed-off fluid block update failed");
            return;
        }
        let origin = [x, y, z];
        // `WaterFluid.beforeDestroyingBlock`: the owner that applies the handed-off placement
        // also produces the loot of the plant, torch or rail the water replaced.
        if let Ok(old_id) = u16::try_from(old)
            && ferrite_fluid::is_liquid_block(state)
            && ferrite_fluid::fluid_state(state).kind == ferrite_fluid::FluidKind::Water
            && ferrite_fluid::fluid_state(old).kind == ferrite_fluid::FluidKind::Empty
            && ferrite_data::blocks::state(old_id).is_some_and(|(name, _)| !name.ends_with("air"))
        {
            self.spawn_block_drops(source, origin, old_id);
        }
        let update = BlockUpdate {
            location: ferrite_protocol::types::BlockPos::new(x, y, z).pack(),
            block_state: VarInt(state as i32),
        };
        broadcast_tracked_chunk(
            &tracking.lock().unwrap(),
            source.world_id(),
            (chunk.0, chunk.1),
            &packet(&update),
        );
        for (dx, dy, dz) in [
            (0, 0, 0),
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ] {
            let (Some(x), Some(y), Some(z)) =
                (x.checked_add(dx), y.checked_add(dy), z.checked_add(dz))
            else {
                continue;
            };
            self.schedule_fluid((x, y, z));
        }
        // The owner now runs the same neighbour pass a local `setBlock` would, including
        // forwarding the cells on its own border to the regions that own them.
        self.updates.block_changed(origin);
        let mut removed = Vec::new();
        {
            let mut world =
                super::fluid::RegionFluidWorld::new(source, &self.held_chunks, tracking)
                    .with_signals(signals.clone());
            self.updates.apply(&mut world, &mut removed);
        }
        for (pos, removed_state) in &removed {
            self.spawn_block_drops(source, *pos, *removed_state);
            let _ = signals.send(Signal::FluidChange {
                dimension: source.dimension(),
                x: pos[0],
                y: pos[1],
                z: pos[2],
            });
        }
        let mut changed = vec![origin];
        changed.extend(removed.iter().map(|(pos, _)| *pos));
        self.schedule_gravity(source, &changed);
        let positions = changed
            .iter()
            .flat_map(|&pos| {
                [
                    (0, 0, 0),
                    (1, 0, 0),
                    (-1, 0, 0),
                    (0, 1, 0),
                    (0, -1, 0),
                    (0, 0, 1),
                    (0, 0, -1),
                ]
                .into_iter()
                .map(move |(dx, dy, dz)| ([pos[0] + dx, pos[1] + dy, pos[2] + dz], pos))
            })
            .collect();
        let _ = signals.send(Signal::RedstoneChange {
            dimension: source.dimension(),
            positions,
        });
    }

    /// Re-evaluates redstone around a change another region made at the boundary.
    pub fn wake_redstone(
        &mut self,
        source: &super::anvil::AnvilChunkSource,
        tracking: &Tracking,
        pos: [i32; 3],
        origin: [i32; 3],
    ) {
        let mut world = super::fluid::RegionFluidWorld::new(source, &self.held_chunks, tracking);
        self.redstone.foreign_changed(&mut world, pos, origin);
    }
    /// Creates a mob at an absolute position with explicit Hard difficulty gating for zombie doors.
    pub fn summon_with_difficulty(
        &mut self,
        kind: MobKind,
        pos: [f64; 3],
        no_ai: bool,
        hard: bool,
    ) -> bool {
        if !pos
            .iter()
            .all(|value| value.is_finite() && value.abs() <= 30_000_000.0)
        {
            return false;
        }
        let record = mob::new_mob_with_difficulty(
            kind,
            ferrite_math::Vec3::new(pos[0], pos[1], pos[2]),
            &mut self.rng,
            hard,
        );
        let key = self.entities.insert(record);
        if no_ai && let Some(mob) = mob::state_mut(&mut self.entities, key) {
            mob.no_ai = true;
        }
        true
    }

    /// Creates an end crystal at an absolute position (`/summon minecraft:end_crystal`).
    pub fn summon_end_crystal(&mut self, pos: [f64; 3]) {
        if pos
            .iter()
            .all(|value| value.is_finite() && value.abs() <= 30_000_000.0)
        {
            self.entities.insert(ferrite_entity::dragon::new_crystal(
                ferrite_math::Vec3::new(pos[0], pos[1], pos[2]),
            ));
        }
    }

    /// Queues neighbor/redstone/fluid updates for non-strict world-command block edits.
    pub(crate) fn queue_command_updates(&mut self, changes: &[super::commands_world::Change]) {
        for change in changes {
            let pos = ferrite_protocol::types::BlockPos::unpack(change.location);
            self.updates.block_changed([pos.x, pos.y, pos.z]);
        }
    }

    /// Creates any summonable entity with explicit Hard difficulty gating for empty-NBT mob spawns.
    pub fn summon_entity_with_difficulty(
        &mut self,
        entity: &str,
        pos: [f64; 3],
        mut nbt: ferrite_nbt::Compound,
        uuid: Option<Uuid>,
        hard: bool,
    ) -> bool {
        if !pos
            .iter()
            .all(|value| value.is_finite() && value.abs() <= 30_000_000.0)
        {
            return false;
        }
        if nbt.is_empty() && (entity == "minecraft:end_crystal" || entity == "end_crystal") {
            let mut record = ferrite_entity::dragon::new_crystal(ferrite_math::Vec3::new(
                pos[0], pos[1], pos[2],
            ));
            if let Some(uuid) = uuid {
                record.data.uuid = uuid;
            }
            self.entities.insert(record);
            return true;
        }
        if nbt.is_empty()
            && let Some(mut record) = ferrite_entity::vehicle::new_vehicle(
                entity,
                ferrite_math::Vec3::new(pos[0], pos[1], pos[2]),
                0.0,
            )
        {
            if let Some(uuid) = uuid {
                record.data.uuid = uuid;
            }
            self.entities.insert(record);
            return true;
        }
        if nbt.is_empty()
            && let Some(kind) = MobKind::from_name(entity)
        {
            let mut record = mob::new_mob_with_difficulty(
                kind,
                ferrite_math::Vec3::new(pos[0], pos[1], pos[2]),
                &mut self.rng,
                hard,
            );
            if kind == MobKind::Spider
                && let Some(effect) =
                    ferrite_entity::spider::random_spawn_effect(&mut self.rng, hard)
            {
                record
                    .data
                    .effects
                    .add(ferrite_entity::effect::MobEffectInstance::new(
                        effect.id(),
                        -1,
                        0,
                    ));
            }
            if let Some(uuid) = uuid {
                record.data.uuid = uuid;
            }
            self.entities.insert(record);
            return true;
        }
        let id = if entity.contains(':') {
            entity.to_owned()
        } else {
            format!("minecraft:{entity}")
        };
        nbt.insert("id".into(), ferrite_nbt::Tag::String(id));
        nbt.insert(
            "Pos".into(),
            ferrite_nbt::Tag::List(ferrite_nbt::List::Double(pos.to_vec())),
        );
        let Some(mut record) = super::entity_save::entity_from_nbt(&nbt) else {
            return false;
        };
        if let Some(uuid) = uuid {
            record.data.uuid = uuid;
        }
        self.entities.insert(record);
        true
    }
    /// The current position of a lead holder, and its client entity id when this region or a
    /// world-visible snapshot knows it. Another region's entity store is never touched directly.
    /// A holder that is known to be in another dimension is rejected immediately; only a holder
    /// that no owner currently reports may be served from the last sighting.
    fn leash_holder(&self, ctx: &TickContext, holder: Uuid) -> LeashHolderLookup {
        let player = {
            let state = lock_game_state(&ctx.state, ctx.profiler.as_deref());
            state
                .players
                .iter()
                .find(|player| player.uuid == holder)
                .map(|player| (player.dimension, player.pos, player.session))
        };
        // The global player list records a dimension transfer before every region has
        // republished, so it wins over a possibly stale local or snapshot entry.
        if let Some((dimension, _, _)) = player
            && dimension != ctx.dimension
        {
            return LeashHolderLookup::Elsewhere;
        }
        if let Some(local) = self.players.values().find(|player| player.uuid == holder)
            && player.is_none_or(|(_, _, session)| session == local.session)
        {
            let session = player
                .map(|(_, _, session)| session)
                .unwrap_or(local.session);
            return LeashHolderLookup::Here {
                pos: ferrite_math::Vec3::new(local.pos[0], local.pos[1], local.pos[2]),
                id: Some(local.entity),
                session: Some(session),
            };
        }
        if player.is_none() {
            let local = self.entities.keys().iter().copied().find(|&key| {
                self.entities
                    .data(key)
                    .is_some_and(|data| data.uuid == holder)
                    && self
                        .entities
                        .flags(key)
                        .is_none_or(|flags| !flags.has(Flags::REMOVED))
                    && mob::state(&self.entities, key).is_none_or(|state| !state.is_dead())
            });
            if let Some(key) = local {
                let Some(pos) = self.entities.pos(key) else {
                    return LeashHolderLookup::Gone;
                };
                let Some(data) = self.entities.data(key) else {
                    return LeashHolderLookup::Gone;
                };
                return LeashHolderLookup::Here {
                    pos,
                    id: Some(data.id),
                    session: None,
                };
            }
        }
        // A reconnected player keeps the UUID but gets a new login session. Resolve the
        // current session even when its new entity lives in another region; the caller
        // compares it with the last sighting and breaks the stale lead.
        if let Some((_, pos, session)) = player {
            let id = ctx
                .command_entities
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(holder)
                .map(|entity| entity.network_id);
            return LeashHolderLookup::Here {
                pos: ferrite_math::Vec3::new(pos[0], pos[1], pos[2]),
                id,
                session: Some(session),
            };
        }
        let entity = ctx
            .command_entities
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(holder)
            .map(|entity| (entity.dimension, entity.pos, entity.network_id));
        match entity {
            Some((dimension, pos, id)) if dimension == ctx.dimension => LeashHolderLookup::Here {
                pos: ferrite_math::Vec3::new(pos[0], pos[1], pos[2]),
                id: Some(id),
                session: None,
            },
            Some(_) => LeashHolderLookup::Elsewhere,
            None => LeashHolderLookup::Gone,
        }
    }

    /// Resolves the holders of this region's leads to (position, entity id). A holder that is
    /// known to be in another dimension is dropped at once, and one that no region reports
    /// anymore leaves the table after [`LEASH_HOLDER_GRACE`] ticks, so a same-dimension region
    /// hand-off keeps the lead and a real removal breaks it exactly once.
    fn leash_holders(&self, ctx: &TickContext) -> HashMap<Uuid, (ferrite_math::Vec3, i32)> {
        let now = ctx
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .game_time;
        let mut referenced: Vec<(Uuid, i32)> = self
            .entities
            .keys()
            .iter()
            .filter_map(|&key| mob::state(&self.entities, key).and_then(|state| state.leash))
            .collect();
        referenced.sort_unstable();
        referenced.dedup();
        let mut current = Vec::new();
        let mut elsewhere = Vec::new();
        for &(holder, stored) in &referenced {
            match self.leash_holder(ctx, holder) {
                LeashHolderLookup::Here { pos, id, session } => {
                    current.push((holder, pos, id.unwrap_or(stored), session));
                }
                LeashHolderLookup::Elsewhere => elsewhere.push(holder),
                LeashHolderLookup::Gone => {}
            }
        }
        let mut table = LEASH_HOLDERS
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for holder in elsewhere {
            if table
                .get(&holder)
                .is_some_and(|entry| entry.dimension == ctx.dimension)
            {
                table.remove(&holder);
            }
        }
        let mut reconnected = Vec::new();
        for (holder, pos, id, session) in current {
            let old_id = table
                .get(&holder)
                .and_then(|entry| match (entry.session, session) {
                    (Some(old), Some(new)) if old != new => Some(entry.id),
                    _ => None,
                });
            // Keep blocking the old session while a lead still points at its entity id. If
            // only a newly attached lead remains, the old session's cache can be replaced.
            if let Some(old_id) = old_id
                && referenced.iter().any(|&(referenced_holder, stored)| {
                    referenced_holder == holder && stored == old_id
                })
            {
                reconnected.push(holder);
                continue;
            }
            table.insert(
                holder,
                LeashHolder {
                    pos,
                    id,
                    dimension: ctx.dimension,
                    session,
                    seen: now,
                },
            );
        }
        // Keep the previous session's entry alive while the stale lead is still here. Every
        // lookup in this grace window must see the mismatch; removing it would make the next
        // lookup accept the reconnected player and revive the lead. Once the lead breaks, the
        // entry is no longer refreshed and expires normally.
        for &holder in &reconnected {
            if let Some(entry) = table.get_mut(&holder) {
                entry.seen = now;
            }
        }
        let mut holders = HashMap::new();
        for (holder, _) in referenced {
            if reconnected.contains(&holder) {
                continue;
            }
            if let Some(entry) = table.get(&holder)
                && entry.dimension == ctx.dimension
                && now.saturating_sub(entry.seen) <= LEASH_HOLDER_GRACE
            {
                holders.insert(holder, (entry.pos, entry.id));
            }
        }
        table.retain(|_, entry| now.saturating_sub(entry.seen) <= LEASH_HOLDER_GRACE);
        holders
    }

    /// A lead loaded from disk knows only its holder's uuid; give it the client's entity id,
    /// even when the holder is owned by another region.
    fn has_leashes(&self) -> bool {
        self.entities
            .keys()
            .iter()
            .any(|&key| mob::state(&self.entities, key).is_some_and(|state| state.leash.is_some()))
    }

    fn refresh_leash_ids(
        &mut self,
        ctx: &TickContext,
        holders: &HashMap<Uuid, (ferrite_math::Vec3, i32)>,
    ) {
        let mut keys = std::mem::take(&mut self.scratch_entity_keys);
        keys.clear();
        keys.extend_from_slice(self.entities.keys());
        for &key in &keys {
            let Some((holder, id)) = mob::state(&self.entities, key).and_then(|s| s.leash) else {
                continue;
            };
            let Some(&(_, real)) = holders.get(&holder) else {
                continue;
            };
            if real != id {
                if let Some(state) = mob::state_mut(&mut self.entities, key) {
                    state.leash = Some((holder, real));
                }
                self.send_link(ctx, key, real);
            }
        }
        self.scratch_entity_keys = keys;
    }

    /// Ticks the modelled mobs: goals, movement, fall damage, despawn, natural spawning and
    /// death loot.
    #[cfg(test)]
    fn tick_mobs(&mut self, ctx: &TickContext, blocks: &TickBlocks<'_>) {
        self.tick_mobs_with_owner_attacks(ctx, blocks, &[]);
    }

    fn tick_mobs_with_owner_attacks(
        &mut self,
        ctx: &TickContext,
        blocks: &TickBlocks<'_>,
        owner_attacks: &[(Uuid, ferrite_entity::EntityKey)],
    ) {
        // The villager schedule runs before AI so each villager has this tick's activity and
        // destination. `/tick freeze` already guards this call at the region level.
        let (day_time, game_time, hard, peaceful) = {
            let state = lock_game_state(&ctx.state, ctx.profiler.as_deref());
            (
                state.clock_time,
                state.game_time,
                state.difficulty == 3,
                state.difficulty == 0,
            )
        };
        ferrite_entity::villager::tick_all(&mut self.entities, blocks, day_time, game_time);
        // Read immutable player snapshots across regions without taking another region's state.
        // Read the two shared maps separately so no locks are held together.
        // Publish every player's cross-region view in three lock acquisitions instead of three
        // per player. This runs even when this region has no entities: mobs in other regions
        // still need the players' modes, gazes and snapshots.
        let mut untargetable = Vec::with_capacity(self.players.len());
        let mut gazes = Vec::with_capacity(self.players.len());
        let mut snapshots = Vec::with_capacity(self.players.len());
        for p in self.players.values() {
            untargetable.push((p.uuid, p.gamemode));
            // Endermen anywhere read where this player looks, and whether a carved pumpkin
            // hides them.
            gazes.push((
                p.uuid,
                ferrite_entity::enderman::Gaze {
                    look: ferrite_entity::combat::look_vector(p.yaw, p.pitch),
                    pumpkin: p.inventory.get(ferrite_inventory::HEAD).name()
                        == Some("minecraft:carved_pumpkin"),
                },
            ));
            snapshots.push((p.uuid, Self::player_snapshot(p)));
        }
        super::targeting::publish_many(untargetable);
        super::targeting::publish_gazes(gazes);
        super::targeting::publish_snapshots(snapshots);

        let mut players = std::mem::take(&mut self.scratch_mob_players);
        players.clear();
        {
            let tracking = ctx.tracking.lock().unwrap();
            players.extend(
                ctx.positions
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(uuid, _)| {
                        tracking
                            .get(uuid)
                            .is_some_and(|(world, _, _)| *world == ctx.world_id)
                    })
                    .map(|(&uuid, pos)| (uuid, ferrite_math::Vec3::new(pos[0], pos[1], pos[2]))),
            );
        }
        // The owner has the freshest position; test contexts may have no shared snapshot.
        for p in self
            .players
            .values()
            .filter(|p| p.dimension == ctx.dimension)
        {
            players.insert(
                p.uuid,
                ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]),
            );
        }
        let mut held_items = std::mem::take(&mut self.scratch_mob_held);
        held_items.clear();
        held_items.extend(
            self.players
                .values()
                .filter(|p| p.dimension == ctx.dimension)
                .map(|p| {
                    (
                        p.uuid,
                        [
                            p.inventory.get(p.hand_slot(0)).name(),
                            p.inventory.get(p.hand_slot(1)).name(),
                        ],
                    )
                }),
        );
        // The batch above already published every local player, so the global snapshot is
        // complete without a second per-player clone-and-insert pass.
        let player_info = super::targeting::snapshots();
        let bright_outside = is_bright_outside(ctx.dimension, day_time);
        let rules = ai::MobRules {
            untargetable: super::targeting::untargetable(),
            player_info,
            // Vanilla `Difficulty.HARD` is 3, `PEACEFUL` is 0.
            hard,
            peaceful,
            gazes: super::targeting::gazes(),
            held_items,
            raining: ctx.raining,
            // Daylight only exists in the overworld during daytime ticks.
            monsters_burn: bright_outside,
            bright_outside: Some(bright_outside),
            griefing: ctx
                .rules
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_bool("mob_griefing")
                .unwrap_or(true),
        };
        // Capture fall distance before AI resets it on landing; the turtle-egg fall-on rule
        // needs to know that the mob actually fell this tick.
        let mut falling = std::mem::take(&mut self.scratch_mob_falling);
        falling.clear();
        falling.extend(self.entities.keys().iter().filter_map(|&key| {
            let state = mob::state(&self.entities, key)?;
            (state.fall_distance > 0.0).then_some((key, state.fall_distance))
        }));
        let mut targets = std::mem::take(&mut self.scratch_targets);
        targets.clear();
        targets.extend(players.iter().map(|(&uuid, &pos)| ai::Target { uuid, pos }));
        self.scratch_mob_players = players;
        let trading = self.trading_traders();
        self.tick_trader_lifetimes(&trading, ctx);
        let has_leashes = self.has_leashes();
        let holders = if has_leashes {
            // A test world's clock must not prune another test's last sighting.
            #[cfg(test)]
            let _turn = leash_test_turn();
            self.leash_holders(ctx)
        } else {
            HashMap::new()
        };
        if has_leashes {
            self.refresh_leash_ids(ctx, &holders);
        }
        let holder_positions: Vec<(Uuid, ferrite_math::Vec3)> = if has_leashes {
            holders
                .iter()
                .map(|(holder, (pos, _))| (*holder, *pos))
                .collect()
        } else {
            Vec::new()
        };
        let piglin_safe = ctx.dimension == super::Dimension::Nether;
        let mut keys = std::mem::take(&mut self.scratch_entity_keys);
        keys.clear();
        keys.extend_from_slice(self.entities.keys());
        for &key in &keys {
            if let Some(mob_state) = mob::state(&self.entities, key) {
                if mob_state.kind == MobKind::SnowGolem
                    && !mob_state.is_dead()
                    && mob_state.hurt_time == 0
                    && let Some(pos) = self.entities.pos(key)
                {
                    let melts = piglin_safe
                        || ctx.source.as_ref().is_some_and(|source| {
                            let biome = source.biome_name(
                                pos.x.floor() as i32,
                                pos.y.floor() as i32,
                                pos.z.floor() as i32,
                            );
                            ferrite_entity::golem::melts_in_biome(biome.as_deref())
                        });
                    if melts {
                        mob::hurt(&mut self.entities, key, 1.0, None);
                    }
                } else if mob_state.kind == MobKind::Piglin
                    && !mob_state.is_dead()
                    && mob::tick_piglin_zombification(&mut self.entities, key, piglin_safe)
                {
                    mob::convert_zombified_piglin(&mut self.entities, key);
                }
            }
        }
        let events = ai::tick_full_with_scratch(
            &mut self.entities,
            blocks,
            &targets,
            &rules,
            owner_attacks,
            &holder_positions,
            &mut self.rng,
            &mut self.ai_scratch,
        );
        let positions: Vec<_> = targets.iter().map(|target| target.pos).collect();
        self.scratch_targets = targets;
        for event in events {
            match event {
                ai::MobEvent::LeashBroke { key } => {
                    self.send_link(ctx, key, 0);
                    self.drop_lead(key);
                }
                ai::MobEvent::Melee {
                    player,
                    damage,
                    from,
                    attacker,
                } => {
                    if self.players.contains_key(&player) {
                        self.damage_player(player, damage, DamageKind::Mob, from, Some(attacker));
                    } else {
                        let attacker = self
                            .entities
                            .data(attacker)
                            .map_or(Uuid::nil(), |data| data.uuid);
                        let _ = ctx.signals.send(Signal::MobMelee {
                            dimension: ctx.dimension,
                            player,
                            damage,
                            from: [from.x, from.y, from.z],
                            attacker,
                            knockback_xz: 0.4,
                            launch_y: 0.0,
                            poison_ticks: 0,
                        });
                    }
                }
                ai::MobEvent::SpecialMelee {
                    player,
                    damage,
                    from,
                    attacker,
                    knockback_xz,
                    launch_y,
                    poison_ticks,
                } => {
                    if self.players.contains_key(&player) {
                        self.damage_player_with(
                            player,
                            damage,
                            DamageKind::Mob,
                            from,
                            Some(attacker),
                            knockback_xz,
                            launch_y,
                            poison_ticks,
                        );
                    } else {
                        let attacker = self
                            .entities
                            .data(attacker)
                            .map_or(Uuid::nil(), |data| data.uuid);
                        let _ = ctx.signals.send(Signal::MobMelee {
                            dimension: ctx.dimension,
                            player,
                            damage,
                            from: [from.x, from.y, from.z],
                            attacker,
                            knockback_xz,
                            launch_y,
                            poison_ticks,
                        });
                    }
                }
                ai::MobEvent::Snowball {
                    from,
                    velocity,
                    owner,
                } => {
                    self.entities.insert(arrow::new_thrown(
                        arrow::Kind::Snowball,
                        from,
                        velocity,
                        owner,
                    ));
                }
                ai::MobEvent::SnowTrail { at } => self.place_snow_trail(ctx, at),
                ai::MobEvent::Sound {
                    at,
                    sound,
                    volume,
                    pitch,
                } => self.play_sound_near(at, sound, volume, pitch),
                ai::MobEvent::Arrow {
                    from,
                    direction,
                    damage,
                    speed,
                    effect,
                    owner,
                } => {
                    let vel = ferrite_math::Vec3::new(
                        direction.x * speed,
                        direction.y * speed,
                        direction.z * speed,
                    );
                    let mut record = arrow::new_arrow(from, vel, damage, owner, &mut self.rng);
                    if let Some(effect) = effect {
                        record = arrow::with_effect(record, effect);
                    }
                    self.entities.insert(record);
                }
                ai::MobEvent::SmallFireball {
                    from,
                    velocity,
                    owner,
                } => {
                    self.entities.insert(arrow::new_thrown(
                        arrow::Kind::SmallFireball,
                        from,
                        velocity,
                        owner,
                    ));
                }
                ai::MobEvent::WitherSkull {
                    from,
                    velocity,
                    owner,
                    dangerous,
                } => {
                    let mut record =
                        arrow::new_thrown(arrow::Kind::WitherSkull, from, velocity, owner);
                    if dangerous {
                        record.data.metadata.set(
                            ferrite_entity::metadata::wither_skull::DANGEROUS,
                            ferrite_protocol::packets::play::MetaValue::Boolean(true),
                        );
                    }
                    self.entities.insert(record);
                    self.play_sound_near(from, "minecraft:entity.wither.shoot", 2.0, 1.0);
                }
                ai::MobEvent::SplashPotion {
                    from,
                    velocity,
                    owner,
                    potion,
                } => {
                    let stack = potion.stack("minecraft:splash_potion");
                    let record = arrow::new_thrown_potion(
                        arrow::Kind::SplashPotion,
                        from,
                        velocity,
                        owner,
                        stack.clone(),
                    );
                    self.thrown_potions.insert(record.data.uuid, stack);
                    self.entities.insert(record);
                }
                ai::MobEvent::EffectAdded { key, instance } => {
                    if let Some(uuid) = self.entities.data(key).map(|data| data.uuid) {
                        self.pending_mob_effects
                            .push((uuid, MobEffectNotice::Added(instance)));
                    }
                }
                ai::MobEvent::LayEgg { at } => {
                    // Vanilla `Chicken.spawnAtLocation(new ItemStack(Items.EGG))`.
                    if let Some(stack) = ferrite_item::ItemStack::from_name("minecraft:egg") {
                        let record = item::new_item_stack(
                            at,
                            ferrite_math::Vec3::new(0.0, 0.2, 0.0),
                            &stack,
                            &mut self.rng,
                        );
                        self.entities.insert(record);
                    }
                }
                ai::MobEvent::LayTurtleEgg { key, at, eggs } => {
                    if !self.lay_turtle_egg(ctx, at, eggs) {
                        // The target stopped being valid (or this region no longer owns it)
                        // before the block write. Keep the clutch so the turtle can return.
                        if let Some(state) = mob::state_mut(&mut self.entities, key) {
                            state.has_egg = true;
                            state.laying_egg = false;
                            state.lay_egg_counter = 0;
                            state.lay_egg_target = None;
                        }
                        if let Some(data) = self.entities.data_mut(key) {
                            ferrite_entity::mob::sync_state_metadata(data);
                        }
                    }
                }
                ai::MobEvent::Spit { from, direction } => {
                    let vel = ferrite_math::Vec3::new(
                        direction.x * 1.5,
                        direction.y * 1.5,
                        direction.z * 1.5,
                    );
                    self.entities.insert(arrow::new_spit(from, vel));
                }
                ai::MobEvent::DragonFireball { from, direction } => {
                    let vel = ferrite_math::Vec3::new(direction.x, direction.y, direction.z);
                    self.entities.insert(arrow::new_thrown(
                        arrow::Kind::DragonFireball,
                        from,
                        vel * ferrite_entity::dragon::FIREBALL_SPEED,
                        None,
                    ));
                }
                ai::MobEvent::Breath { pos, owner } => {
                    // The event carries the head's aim point; the cloud forms on the ground
                    // below it, found by walking down through air.
                    let pos = ctx.source.as_deref().map_or(pos, |source| {
                        ferrite_math::Vec3::new(
                            pos.x,
                            ferrite_entity::dragon::breath_floor(pos.y, 0, |y| {
                                source
                                    .block(pos.x.floor() as i32, y, pos.z.floor() as i32)
                                    .is_none_or(|state| {
                                        state == 0
                                            || u16::try_from(state)
                                                .ok()
                                                .and_then(ferrite_data::blocks::state)
                                                .is_some_and(|(name, _)| {
                                                    matches!(
                                                        name,
                                                        "minecraft:air"
                                                            | "minecraft:cave_air"
                                                            | "minecraft:void_air"
                                                    )
                                                })
                                    })
                            }),
                            pos.z,
                        )
                    });
                    let cloud = ferrite_entity::dragon::perched_breath_cloud(pos, owner);
                    self.entities.insert(cloud.spawn_record());
                    self.potion_clouds.push(cloud);
                }
                ai::MobEvent::EatBlock { at, grass } => self.eat_block(ctx, at, grass),
                ai::MobEvent::TakeBlock { key, at } => {
                    self.take_enderman_block(ctx, key, at);
                }
                ai::MobEvent::PlaceCarried { key, at, state } => {
                    self.place_enderman_block(ctx, key, at, state);
                }
                ai::MobEvent::Teleport { from, to } => {
                    self.play_enderman_teleport(ctx, from, to);
                }
                ai::MobEvent::BreakDoor {
                    at, broken: true, ..
                } => self.break_door(ctx, at),
                ai::MobEvent::BreakDoor { .. } => {}
                ai::MobEvent::SetDoor { at, open } => self.set_door_open(ctx, at, open),
                ai::MobEvent::Hiss { at } => self.play_sound_near(
                    at,
                    ferrite_entity::creeper::HISS_SOUND,
                    ferrite_entity::creeper::HISS_VOLUME,
                    ferrite_entity::creeper::HISS_PITCH,
                ),
                ai::MobEvent::Status { key, status } => self.broadcast_status(ctx, key, status),
                ai::MobEvent::Bred(bred) => {
                    // Vanilla gives 1-7 experience points to the breeding and counts it for the
                    // player who fed the parent.
                    let amount = 1 + self.rng.next_int(7);
                    for record in experience::award(bred.pos, amount) {
                        self.entities.insert(record);
                    }
                    if let Some(player) = bred.cause {
                        super::recipe_unlock::BRED
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push((player, bred.kind.name()));
                    }
                }
                ai::MobEvent::Explode { center, power } => {
                    self.explode([center.x, center.y, center.z], power, true, ctx);
                    if let Some(source) = ctx.source.as_deref() {
                        let mut broken = Vec::new();
                        self.apply_neighbor_updates(source, ctx, &mut broken);
                        for (pos, state) in broken {
                            self.spawn_block_drops(source, pos, state);
                        }
                    }
                }
                ai::MobEvent::DrownedConvert { key } => {
                    ferrite_entity::mob::convert_submerged_zombie(&mut self.entities, key);
                }
                ai::MobEvent::CreeperPotionCloud {
                    center,
                    effects,
                    owner,
                } => {
                    let cloud = ferrite_entity::potion::PotionCloud::lingering(center, &effects)
                        .with_owner(owner);
                    self.entities.insert(cloud.spawn_record());
                    self.potion_clouds.push(cloud);
                }
                ai::MobEvent::PiglinBarter { key, at, toward } => {
                    self.finish_piglin_barter(ctx, key, at, toward);
                }
                ai::MobEvent::FrogEat {
                    prey,
                    prey_kind,
                    frog_variant,
                    at,
                } => {
                    if let Some(state) = mob::state_mut(&mut self.entities, prey) {
                        state.loot_dropped = true;
                    }
                    let drop_id = match prey_kind {
                        MobKind::MagmaCube => {
                            ferrite_entity::frog::froglight_for_variant(frog_variant)
                        }
                        _ => "minecraft:slime_ball",
                    };
                    if let Some(stack) = ferrite_item::ItemStack::from_name(drop_id) {
                        self.entities.insert(item::new_item_stack(
                            at,
                            ferrite_math::Vec3::ZERO,
                            &stack,
                            &mut self.rng,
                        ));
                    }
                }
                ai::MobEvent::GoatSnapHorn {
                    key,
                    at,
                    is_screaming,
                } => {
                    let snapped = if let Some(state) = mob::state_mut(&mut self.entities, key) {
                        if state.has_left_horn {
                            state.has_left_horn = false;
                            true
                        } else if state.has_right_horn {
                            state.has_right_horn = false;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    if snapped {
                        if let Some(data) = self.entities.data_mut(key) {
                            mob::sync_state_metadata(data);
                        }
                        if let Some(horn) =
                            ferrite_item::ItemStack::from_name("minecraft:goat_horn")
                        {
                            self.entities.insert(item::new_item_stack(
                                at,
                                ferrite_math::Vec3::new(0.0, 0.2, 0.0),
                                &horn,
                                &mut self.rng,
                            ));
                        }
                        let sound = if is_screaming {
                            "minecraft:entity.goat.screaming.horn_break"
                        } else {
                            "minecraft:entity.goat.horn_break"
                        };
                        self.play_sound_near(at, sound, 1.0, 1.0);
                    }
                }
                ai::MobEvent::HarvestSweetBerries { at, pos } => {
                    if let Some(source) = ctx.source.as_deref()
                        && self
                            .held_chunks
                            .contains_key(&ChunkPos(at[0].div_euclid(16), at[2].div_euclid(16)))
                        && let Some(age_one) = ferrite_data::blocks::state_id(
                            "minecraft:sweet_berry_bush",
                            &[("age", "1")],
                        )
                    {
                        self.place_blocks(ctx, &[(at, age_one)]);
                        self.play_sound_near(
                            pos,
                            "minecraft:block.sweet_berry_bush.pick_berries",
                            1.0,
                            1.0,
                        );
                        if let Some(stack) =
                            ferrite_item::ItemStack::from_name("minecraft:sweet_berries")
                        {
                            self.entities.insert(item::new_item_stack(
                                ferrite_math::Vec3::new(
                                    f64::from(at[0]) + 0.5,
                                    f64::from(at[1]) + 0.5,
                                    f64::from(at[2]) + 0.5,
                                ),
                                ferrite_math::Vec3::ZERO,
                                &stack,
                                &mut self.rng,
                            ));
                        }
                        let _ = source;
                    }
                }
                ai::MobEvent::RabbitNibbleCarrots { at, new_age } => {
                    if self
                        .held_chunks
                        .contains_key(&ChunkPos(at[0].div_euclid(16), at[2].div_euclid(16)))
                    {
                        let age_str = new_age.to_string();
                        if let Some(next_state) = ferrite_data::blocks::state_id(
                            "minecraft:carrots",
                            &[("age", &age_str)],
                        ) {
                            self.place_blocks(ctx, &[(at, next_state)]);
                        }
                    }
                }
                ai::MobEvent::WitchHealRaider {
                    witch: _,
                    victim,
                    at,
                } => {
                    if let Some(state) = mob::state_mut(&mut self.entities, victim)
                        && !state.is_dead()
                    {
                        let max = state.kind.max_health();
                        state.health = (state.health + 4.0).min(max);
                    }
                    mob::add_effect(
                        &mut self.entities,
                        victim,
                        ferrite_entity::effect::MobEffectInstance::new(
                            ferrite_entity::effect::Effect::Regeneration.id(),
                            900,
                            0,
                        ),
                    );
                    self.play_sound_near(at, "minecraft:entity.witch.throw", 1.0, 1.0);
                }
            }
        }
        self.trample_turtle_eggs(ctx, blocks, &falling);
        self.scratch_mob_falling = falling;
        let day_time = lock_game_state(&ctx.state, ctx.profiler.as_deref()).clock_time;
        self.tick_trader_drinks(day_time, &trading, ctx);
        let trader_spawning = {
            let rules = ctx.rules.lock().unwrap_or_else(|error| error.into_inner());
            trader_spawning_enabled_in_dimension(&rules, ctx.dimension)
        };
        let (clock_time, bounds) = {
            let state = lock_game_state(&ctx.state, ctx.profiler.as_deref());
            let border = &state.border;
            (
                state.clock_time,
                super::wandering_trader::SpawnBounds {
                    min_y: ctx.dimension.min_y(),
                    max_y: ctx.dimension.min_y() + ctx.dimension.height(),
                    border_center_x: border.center_x,
                    border_center_z: border.center_z,
                    border_diameter: border.diameter,
                },
            )
        };
        self.tick_trader_spawn(
            blocks,
            trader_spawning,
            clock_time,
            bounds,
            &positions,
            &ctx.trader_spawner,
        );
        let spawn_env = RegionSpawnEnv {
            blocks,
            sky_darken: sky_darken(ctx.dimension, clock_time, ctx.raining),
        };
        let spawn_hooks = ctx.hooks.as_deref().and_then(SharedPluginHooks::load);
        for mut record in spawn::natural_spawn_in_chunks(
            &self.entities,
            &spawn_env,
            &positions,
            &mut self.rng,
            1,
            self.held_chunks.len(),
        ) {
            if let Some((hooks, _)) = &spawn_hooks
                && !hooks
                    .mob_spawn_allowed(
                        ctx.dimension.key(),
                        record.pos.x.floor() as i32,
                        record.pos.y.floor() as i32,
                        record.pos.z.floor() as i32,
                    )
                    .unwrap_or(true)
            {
                continue;
            }
            if record
                .data
                .mob
                .as_ref()
                .is_some_and(|s| s.kind == MobKind::Sheep)
                && let Some(source) = ctx.source.as_ref()
            {
                let biome = source.biome_name(
                    record.pos.x.floor() as i32,
                    record.pos.y.floor() as i32,
                    record.pos.z.floor() as i32,
                );
                let climate = ferrite_entity::sheep::climate_for_biome(biome.as_deref());
                if climate != ferrite_entity::sheep::SheepBiomeClimate::Temperate {
                    if let Some(mob) = record.data.mob.as_mut() {
                        mob.wool_color =
                            ferrite_entity::sheep::random_color_for_climate(&mut self.rng, climate);
                    }
                    ferrite_entity::mob::sync_state_metadata(&mut record.data);
                    record.data.metadata.clear_dirty();
                }
            }
            self.entities.insert(record);
        }
        if !peaceful {
            for record in spawn::natural_spawn_monsters_in_chunks(
                &self.entities,
                &spawn_env,
                &positions,
                &mut self.rng,
                1,
                self.held_chunks.len(),
            ) {
                if let Some((hooks, _)) = &spawn_hooks
                    && !hooks
                        .mob_spawn_allowed(
                            ctx.dimension.key(),
                            record.pos.x.floor() as i32,
                            record.pos.y.floor() as i32,
                            record.pos.z.floor() as i32,
                        )
                        .unwrap_or(true)
                {
                    continue;
                }
                let mut record = record;
                if record
                    .data
                    .mob
                    .as_ref()
                    .is_some_and(|s| s.kind == MobKind::Spider)
                    && let Some(effect) =
                        ferrite_entity::spider::random_spawn_effect(&mut self.rng, rules.hard)
                {
                    record
                        .data
                        .effects
                        .add(ferrite_entity::effect::MobEffectInstance::new(
                            effect.id(),
                            -1,
                            0,
                        ));
                }
                // Rides live in a process-wide table that the `ride` tests assert on in full, so a
                // jockey rolled by an unrelated region test must not leak into it; `spider_jockey`
                // has its own test.
                let jockey = if cfg!(test) {
                    None
                } else {
                    self.spider_jockey(&mut record)
                };
                self.entities.insert(record);
                if let Some(rider) = jockey {
                    self.entities.insert(rider);
                }
            }
        }
        // Mobs that died this tick drop their entity loot table once (vanilla
        // `LivingEntity.dropFromLootTable`). The entity itself stays in the store until the
        // tracker has sent `remove_entities` and `sweep_removed` drops the record.
        let dead: Vec<_> = self
            .entities
            .keys()
            .iter()
            .filter_map(|&key| {
                let data = self.entities.data(key)?;
                let mob = data.mob?;
                if !mob.is_dead() || mob.loot_dropped {
                    return None;
                }
                Some((
                    key,
                    mob.kind,
                    self.entities.pos(key)?,
                    mob.age < 0,
                    mob.last_killer,
                    mob.fire > 0,
                    mob.carried,
                    mob.raid_captain,
                ))
            })
            .collect();
        for (key, kind, pos, baby, killer, burning, carried, captain) in dead {
            if let Some(dead_uuid) = self.entities.data(key).map(|d| d.uuid)
                && let Some(vehicle_uuid) = super::ride::vehicle_of(dead_uuid)
            {
                for vkey in self.entities.keys().to_vec() {
                    if self
                        .entities
                        .data(vkey)
                        .is_some_and(|d| d.uuid == vehicle_uuid)
                        && let Some(vstate) = mob::state_mut(&mut self.entities, vkey)
                    {
                        vstate.has_rider = false;
                    }
                }
                super::ride::forget(dead_uuid);
            }
            // A sheep's table picks its wool by colour and sheared state.
            let facts = mob::state(&self.entities, key)
                .filter(|s| s.kind == MobKind::Sheep)
                .map(ferrite_entity::sheep::loot_properties)
                .unwrap_or_default();
            if let Some(killer) = killer {
                super::recipe_unlock::KILLS
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push((killer, kind.name()));
                // `Raider.isCaptain` -> `Raids` Bad Omen for the killer.
                if captain && let Some(player) = self.players.get_mut(&killer) {
                    player.apply_effect(MobEffectInstance::new(
                        Effect::BadOmen.id(),
                        super::raid::BAD_OMEN_DURATION,
                        0,
                    ));
                }
            }
            if mob::state(&self.entities, key).is_some_and(|s| s.leash.is_some()) {
                self.drop_lead(key);
                if let Some(state) = mob::state_mut(&mut self.entities, key) {
                    state.leash = None;
                }
            }
            // A horse drops its saddle and body armour; a donkey, mule or llama drops its
            // chest, what is in it, and its saddle. A baby equine drops no gear at all.
            let cube_size = mob::state(&self.entities, key).map_or(1, |s| s.cube_size);
            if let Some(mob) = mob::state_mut(&mut self.entities, key) {
                mob.loot_dropped = true;
            }
            if matches!(kind, MobKind::Slime | MobKind::MagmaCube) && cube_size > 1 {
                let child_size = cube_size / 2;
                let count = 2 + self.rng.next_int(3);
                for i in 0..count {
                    let offset_x = ((i % 2) as f64 - 0.5) * f64::from(cube_size) * 0.25;
                    let offset_z = ((i / 2) as f64 - 0.5) * f64::from(cube_size) * 0.25;
                    let mut child = mob::new_mob(
                        kind,
                        ferrite_math::Vec3::new(pos.x + offset_x, pos.y + 0.5, pos.z + offset_z),
                        &mut self.rng,
                    );
                    if let Some(state) = child.data.mob.as_mut() {
                        mob::set_cube_size_state(state, child_size);
                    }
                    mob::sync_state_metadata(&mut child.data);
                    self.entities.insert(child);
                }
            }
            let gear = if baby || !kind.is_equine() {
                Vec::new()
            } else {
                match (
                    kind,
                    self.entities.data(key),
                    mob::state(&self.entities, key),
                ) {
                    (MobKind::Horse, _, Some(state)) => {
                        super::equine_chest::horse_gear(state.saddled, state.armor)
                    }
                    (_, Some(data), Some(state)) => super::equine_chest::gear(
                        data.uuid,
                        kind,
                        state.chest,
                        state.saddled,
                        state.armor,
                    ),
                    _ => Vec::new(),
                }
            };
            // Roll non-equine equipment (and standard 6 slots on equines) against per-slot drop
            // chances with the killer's Looting level and Curse of Vanishing suppression,
            // matching vanilla `Mob.dropCustomDeathLoot`.
            let equipment_drops = if baby {
                Vec::new()
            } else {
                let looting = super::drops::killer_looting(killer);
                super::drops::roll_equipment_drops(
                    &self.entities,
                    key,
                    killer,
                    looting,
                    kind.is_equine(),
                    &mut self.rng,
                )
            };
            // Nothing may stay behind to be saved, reopened or dropped a second time.
            if let Some(data) = self.entities.data(key) {
                super::equine_chest::clear(data.uuid);
            }
            let piglin_drops = if kind == MobKind::Piglin {
                ferrite_entity::piglin::take_inventory(&mut self.entities, key)
            } else {
                Vec::new()
            };
            if kind == ferrite_entity::mob::MobKind::EnderDragon {
                self.dragon_died(ctx, pos);
            }
            // A baby animal drops nothing.
            let mut drops = if baby {
                Vec::new()
            } else {
                super::drops::mob_drops_with(kind, &mut self.rng, killer, burning, &facts)
            };
            // `EnderMan.dropCustomDeathLoot` drops the carried block even when the loot table
            // itself is empty.
            if kind == MobKind::Enderman
                && let Some(carried) = carried
                && let Some((name, _)) = ferrite_data::blocks::state(carried)
                && let Some(item) = ferrite_item::item_id(name)
            {
                drops.push(ferrite_protocol::packets::play::ItemStack {
                    count: 1,
                    item_id: i32::from(item),
                });
            }
            let block = [
                pos.x.floor() as i32,
                pos.y.floor() as i32,
                pos.z.floor() as i32,
            ];
            for stack in drops {
                let record = item::pop_resource(block, stack, &mut self.rng);
                self.entities.insert(record);
            }
            for stack in gear
                .into_iter()
                .chain(equipment_drops)
                .chain(piglin_drops)
                .filter(|s| !s.is_empty())
            {
                let record = item::pop_resource_stack(block, &stack, &mut self.rng);
                self.entities.insert(record);
            }
        }
        self.tick_end_fight(ctx);
        ferrite_entity::dragon::heal_from_crystals(&mut self.entities);
        self.update_dragon_bar();
        let ai::MobRules { held_items, .. } = rules;
        self.scratch_mob_held = held_items;
    }
    /// Starts the fight when a player is in the end, and stands the crystals on the spikes.
    fn tick_end_fight(&mut self, ctx: &TickContext) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        if source.dimension() != super::Dimension::End || self.players.is_empty() {
            return;
        }
        {
            let mut state = lock_game_state(&ctx.state, ctx.profiler.as_deref());
            let fight = &mut state.more.end_fight;
            if !fight.loaded {
                fight.loaded = true;
                if let Some(saved) = source.load_fight() {
                    fight.restore(saved);
                }
            }
        }
        self.repair_end_structures(ctx);
        self.run_end_fight(ctx);
        let mut state = lock_game_state(&ctx.state, ctx.profiler.as_deref());
        let fight = &mut state.more.end_fight;
        // Stage transitions are the durable checkpoints; saving the per-tick timer would rewrite
        // the fight file 600 times during one respawn.
        let stage = fight.respawn_stage;
        let stage_changed = fight
            .on_disk
            .as_ref()
            .is_none_or(|saved| saved.respawn_stage != stage);
        // Spike crystals are restored one at a time; checkpoint those too, but not the timer.
        let crystals_changed = fight.on_disk.as_ref().is_some_and(|saved| {
            saved.crystals.len() != fight.crystals.len()
                || saved
                    .crystals
                    .iter()
                    .copied()
                    .ne(fight.crystals.iter().copied())
        });
        let now = fight.saved();
        if fight.started
            && (stage.is_none() || stage_changed || crystals_changed)
            && fight.on_disk.as_ref() != Some(&now)
        {
            if let Err(error) = source.save_fight(&now) {
                tracing::warn!("could not save the dragon fight: {error:#}");
            }
            fight.on_disk = Some(now);
        }
    }

    /// Restores the fixed End structures the saved fight says must exist: the exit portal after
    /// a kill (the dragon can die in another region, or the server can stop between the fight
    /// save and the block writes) and any opened gateway whose chunk this region holds.
    fn repair_end_structures(&mut self, ctx: &TickContext) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let (killed, dragon_alive, respawn_stage, exit_portal_level, gateways) = {
            let state = ctx.state.lock().unwrap();
            let fight = &state.more.end_fight;
            (
                fight.killed,
                fight.dragon_alive,
                fight.respawn_stage,
                fight.exit_portal_level,
                fight.gateways,
            )
        };
        let mut blocks = Vec::new();
        let mut new_gateways = Vec::new();
        let mut gateway_entities = Vec::new();
        let mut beams = Vec::new();
        if self.held_chunks.contains_key(&ChunkPos(0, 0)) {
            let portal = ferrite_data::blocks::default_state("minecraft:end_portal").map(u32::from);
            let present = portal.is_some_and(|portal| {
                exit_portal_level.is_some_and(|level| source.block(1, level, 0) == Some(portal))
            });
            if super::end_fight::needs_exit_portal(killed, dragon_alive, respawn_stage, present) {
                let level = exit_portal_level
                    .or_else(|| super::end_fight::podium_floor(|y| source.block(0, y, 0)));
                if let Some(level) = level {
                    ctx.state.lock().unwrap().more.end_fight.exit_portal_level = Some(level);
                    blocks.extend(super::end_fight::podium_blocks([0, level, 0], true));
                }
            }
        }
        for index in 0..gateways {
            let at = super::end_fight::gateway_position(index);
            if !self
                .held_chunks
                .contains_key(&ChunkPos(at[0].div_euclid(16), at[2].div_euclid(16)))
            {
                continue;
            }
            let present = source
                .block(at[0], at[1], at[2])
                .and_then(|state| u16::try_from(state).ok())
                .is_some_and(super::portal::is_end_gateway);
            if !present {
                blocks.extend(super::end_fight::gateway_blocks(at));
                new_gateways.push(at);
            } else if source.block_entity(at[0], at[1], at[2]).is_none() {
                // The block survived but an older save never wrote its block entity; do not
                // replay the creation beam for a gateway that has been open for a while.
                gateway_entities.push(at);
            } else {
                // Keep the stored `Age` moving so a viewer who arrives later, or the chunk
                // after a reload, does not see the creation beam start again from zero.
                super::end_fight::advance_gateway_age(source, at);
                let stored = source
                    .block_entity(at[0], at[1], at[2])
                    .map_or(0, |entity| super::end_fight::gateway_stored_age(&entity));
                let beam = self
                    .gateway_beams
                    .entry(at)
                    .or_insert_with(|| super::end_fight::GatewayBeam::new(stored));
                if beam.tick() {
                    beams.push(at);
                }
            }
        }
        if !blocks.is_empty() {
            self.place_blocks(ctx, &blocks);
        }
        for at in new_gateways {
            self.set_gateway_age(ctx, at, 0);
        }
        for at in gateway_entities {
            self.set_gateway_age(ctx, at, super::end_fight::GATEWAY_SPAWN_TICKS);
        }
        for at in beams {
            self.send_gateway_beam(ctx, at);
        }
    }

    /// Starts the purple teleport beam of the gateway at `at` for the chunk's viewers: vanilla
    /// `TheEndGatewayBlockEntity.triggerCooldown` sends block event 1.
    fn send_gateway_beam(&self, ctx: &TickContext, at: [i32; 3]) {
        let Some(block) = super::end_fight::gateway_block_id() else {
            return;
        };
        let event = BlockEvent {
            pos: ferrite_protocol::types::BlockPos::new(at[0], at[1], at[2]),
            block: VarInt(block),
            param_a: super::end_fight::GATEWAY_BEAM_EVENT,
            param_b: 0,
        };
        broadcast_tracked_chunk(
            &ctx.tracking.lock().unwrap(),
            ctx.dimension,
            (at[0].div_euclid(16), at[2].div_euclid(16)),
            &packet(&event),
        );
    }

    /// Whether a gateway at `at` would take a player this tick. Vanilla refuses while the
    /// creation beam plays and while the purple beam cools down; a gateway whose chunk has no
    /// runtime clock yet is ready.
    pub fn gateway_accepts_teleport(&self, at: [i32; 3]) -> bool {
        self.gateway_beams
            .get(&at)
            .is_none_or(super::end_fight::GatewayBeam::accepts_teleport)
    }

    /// A player teleported through the gateway at `at`: start its purple beam unless one runs.
    fn gateway_teleported(&mut self, ctx: &TickContext, at: [i32; 3]) {
        let started = self
            .gateway_beams
            .entry(at)
            .or_insert_with(|| {
                let stored = ctx
                    .source
                    .as_deref()
                    .and_then(|source| source.block_entity(at[0], at[1], at[2]))
                    .map_or(super::end_fight::GATEWAY_SPAWN_TICKS, |entity| {
                        super::end_fight::gateway_stored_age(&entity)
                    });
                super::end_fight::GatewayBeam::new(stored)
            })
            .trigger();
        if started {
            self.send_gateway_beam(ctx, at);
        }
    }

    fn guarded_spikes(&self, source: &super::anvil::AnvilChunkSource) -> Vec<u8> {
        let tops: Vec<_> = (0..super::end_fight::SPIKE_COUNT)
            .map(|index| {
                let [x, z] = super::end_fight::spike_column(index);
                if !self
                    .held_chunks
                    .contains_key(&ChunkPos(x.div_euclid(16), z.div_euclid(16)))
                {
                    return None;
                }
                super::end_fight::podium_floor(|y| source.block(x, y, z))
            })
            .collect();
        let strict = super::end_fight::guarded_spike_indices(&tops);
        if strict.is_empty() {
            super::end_fight::guarded_spike_indices_lenient(&tops)
        } else {
            strict
        }
    }

    fn run_end_fight(&mut self, ctx: &TickContext) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let mut state = lock_game_state(&ctx.state, ctx.profiler.as_deref());
        let fight = &mut state.more.end_fight;
        if fight.guarded.is_empty() {
            fight.guarded = self.guarded_spikes(source).into_iter().collect();
        }
        // The region that holds the chunk at the origin starts the fight.
        if !fight.started && self.held_chunks.contains_key(&ChunkPos(0, 0)) {
            let Some(floor) = super::end_fight::podium_floor(|y| source.block(0, y, 0)) else {
                return;
            };
            fight.started = true;
            fight.exit_portal_level = Some(floor);
            let portal = ferrite_data::blocks::default_state("minecraft:end_portal");
            // A portal on the podium means an earlier dragon already died.
            if portal.is_some_and(|p| source.block(1, floor, 0) == Some(u32::from(p))) {
                fight.killed = true;
                return;
            }
            fight.dragon_alive = true;
            drop(state);
            let (spot, _) = ferrite_entity::dragon::position_at(0);
            let dragon = mob::new_mob(mob::MobKind::EnderDragon, spot, &mut self.rng);
            self.entities.insert(dragon);
            self.place_blocks(ctx, &super::end_fight::podium_blocks([0, floor, 0], false));
            return;
        }
        if !fight.started {
            return;
        }
        // A staged respawn keeps ticking after the exit portal was taken down. Only the region
        // holding the origin owns the sequence; other End regions have no crystals in the four
        // cells and must not mistake that for an aborted sequence.
        if fight.respawn_stage.is_some() && self.held_chunks.contains_key(&ChunkPos(0, 0)) {
            let Some(level) = fight.exit_portal_level else {
                fight.abort_respawn();
                return;
            };
            let keys: Vec<_> = super::end_fight::respawn_crystal_cells(level)
                .iter()
                .filter_map(|&cell| self.crystal_at(cell))
                .collect();
            if keys.is_empty() {
                // Vanilla `abortRespawnSequence`: no tracked crystal survived, so put the exit
                // portal back and leave the spike crystals to the normal placement pass.
                fight.abort_respawn();
                drop(state);
                self.place_blocks(ctx, &super::end_fight::podium_blocks([0, level, 0], true));
                return;
            }
            let actions = fight.tick_respawn();
            drop(state);
            self.apply_respawn_actions(ctx, &keys, actions);
            return;
        }

        // Four end crystals around the exit portal bring the dragon back (`tryRespawn`).
        if fight.killed && !fight.dragon_alive && self.held_chunks.contains_key(&ChunkPos(0, 0)) {
            let portal = ferrite_data::blocks::default_state("minecraft:end_portal").map(u32::from);
            let level = (0..=255).find(|&y| portal.is_some() && source.block(1, y, 0) == portal);
            if let Some(level) = level {
                let cells = super::end_fight::respawn_crystal_cells(level);
                let found =
                    super::end_fight::require_respawn_crystals(cells, |cell| self.crystal_at(cell));
                if let Some(keys) = found {
                    fight.begin_respawn(level);
                    // Consume START now so the beams appear and the portal comes down this tick.
                    let actions = fight.tick_respawn();
                    drop(state);
                    // Vanilla rebuilds the spike crystals one by one during the sequence.
                    let keep: std::collections::BTreeSet<_> = keys.iter().copied().collect();
                    for key in self.entities.keys().to_vec() {
                        if !keep.contains(&key)
                            && ferrite_entity::dragon::is_crystal(&self.entities, key)
                            && let Some(flags) = self.entities.flags_mut(key)
                        {
                            flags.set(ferrite_entity::Flags::REMOVED, true);
                        }
                    }
                    self.place_blocks(ctx, &super::end_fight::podium_blocks([0, level, 0], false));
                    self.apply_respawn_actions(ctx, &keys, actions);
                    return;
                }
            }
        }
        let mut placed = Vec::new();
        let mut cages = Vec::new();
        for index in 0..super::end_fight::SPIKE_COUNT {
            if fight.crystals.contains(&index) {
                continue;
            }
            let [x, z] = super::end_fight::spike_column(index);
            if !self
                .held_chunks
                .contains_key(&ChunkPos(x.div_euclid(16), z.div_euclid(16)))
            {
                continue;
            }
            let Some(top) = super::end_fight::podium_floor(|y| source.block(x, y, z)) else {
                continue;
            };
            fight.crystals.insert(index);
            let bedrock = ferrite_data::blocks::default_state("minecraft:bedrock");
            if bedrock.is_some_and(|b| source.block(x, top, z) == Some(u32::from(b))) {
                placed.push(ferrite_entity::dragon::new_crystal(
                    ferrite_math::Vec3::new(
                        f64::from(x) + 0.5,
                        f64::from(top) + 1.0,
                        f64::from(z) + 0.5,
                    ),
                ));
                if fight.guarded.contains(&index) {
                    cages.extend(super::end_fight::spike_cage_blocks([x, top, z]));
                }
            }
        }
        drop(state);
        if !cages.is_empty() {
            self.place_blocks(ctx, &cages);
        }
        for record in placed {
            self.entities.insert(record);
        }
    }
    /// One end crystal whose box is inside the one-block respawn cell, if any.
    fn crystal_at(&self, [x, y, z]: [i32; 3]) -> Option<ferrite_entity::EntityKey> {
        let cell = ferrite_math::AABB::new(
            f64::from(x),
            f64::from(y),
            f64::from(z),
            f64::from(x + 1),
            f64::from(y + 1),
            f64::from(z + 1),
        );
        self.entities
            .query_vec(cell)
            .into_iter()
            .find(|&key| ferrite_entity::dragon::is_crystal(&self.entities, key))
    }

    /// Applies the actions a staged respawn tick produced: the four crystals' beams, the fixed
    /// summon roar, one restored spike crystal every 40 ticks, and the single final spawn.
    fn apply_respawn_actions(
        &mut self,
        ctx: &TickContext,
        keys: &[ferrite_entity::EntityKey],
        actions: Vec<super::end_fight::RespawnAction>,
    ) {
        use super::end_fight::{RESPAWN_BEAM_TARGET, RespawnAction};
        for action in actions {
            match action {
                RespawnAction::SetBeams(target) => {
                    for &key in keys {
                        if let Some(data) = self.entities.data_mut(key) {
                            super::end_fight::set_crystal_beam(data, target);
                        }
                    }
                }
                RespawnAction::Roar => {
                    let event = LevelEvent {
                        event: super::end_fight::DRAGON_SUMMON_ROAR_EVENT,
                        location: ferrite_protocol::types::BlockPos::new(
                            RESPAWN_BEAM_TARGET[0],
                            RESPAWN_BEAM_TARGET[1],
                            RESPAWN_BEAM_TARGET[2],
                        )
                        .pack(),
                        data: 0,
                        global: false,
                    };
                    for player in self.players.values() {
                        send(player, &event);
                    }
                }
                RespawnAction::AimAtSpike { index } => {
                    let [x, z] = super::end_fight::spike_column(index);
                    let Some(top) = ctx.source.as_deref().and_then(|source| {
                        super::end_fight::podium_floor(|y| source.block(x, y, z))
                    }) else {
                        continue;
                    };
                    for &key in keys {
                        if let Some(data) = self.entities.data_mut(key) {
                            super::end_fight::set_crystal_beam(data, Some([x, top + 1, z]));
                        }
                    }
                }
                RespawnAction::RestoreSpike { index } => {
                    let [x, z] = super::end_fight::spike_column(index);
                    if !self
                        .held_chunks
                        .contains_key(&ChunkPos(x.div_euclid(16), z.div_euclid(16)))
                    {
                        continue;
                    }
                    let Some(top) = ctx.source.as_deref().and_then(|source| {
                        super::end_fight::podium_floor(|y| source.block(x, y, z))
                    }) else {
                        continue;
                    };
                    let guarded = ctx
                        .state
                        .lock()
                        .unwrap()
                        .more
                        .end_fight
                        .guarded
                        .contains(&index);
                    // A respawn resumed after a restart may already have this spike's crystal.
                    if self.crystal_at([x, top + 1, z]).is_none() {
                        self.entities.insert(ferrite_entity::dragon::new_crystal(
                            ferrite_math::Vec3::new(
                                f64::from(x) + 0.5,
                                f64::from(top) + 1.0,
                                f64::from(z) + 0.5,
                            ),
                        ));
                    }
                    ctx.state
                        .lock()
                        .unwrap()
                        .more
                        .end_fight
                        .crystals
                        .insert(index);
                    if guarded {
                        self.place_blocks(ctx, &super::end_fight::spike_cage_blocks([x, top, z]));
                    }
                }
                RespawnAction::SpawnDragon => {
                    let explosions = ctx
                        .state
                        .lock()
                        .unwrap()
                        .more
                        .end_fight
                        .exit_portal_level
                        .map(|level| {
                            super::end_fight::respawn_crystal_explosions(
                                super::end_fight::respawn_crystal_cells(level),
                            )
                        })
                        .unwrap_or_default();
                    for &key in keys {
                        if let Some(data) = self.entities.data_mut(key) {
                            super::end_fight::set_crystal_beam(data, None);
                        }
                        if let Some(flags) = self.entities.flags_mut(key) {
                            flags.set(ferrite_entity::Flags::REMOVED, true);
                        }
                    }
                    // Vanilla SUMMONING_DRAGON: each crystal explodes with power 6 and no block
                    // damage before the dragon appears. The crystals are already removed so the
                    // blast cannot chain into them again.
                    for at in explosions {
                        self.explode_by(at, 6.0, false, None, ctx);
                    }
                    let (spot, _) = ferrite_entity::dragon::position_at(0);
                    let dragon = mob::new_mob(mob::MobKind::EnderDragon, spot, &mut self.rng);
                    self.entities.insert(dragon);
                    for &player in self.players.keys() {
                        super::recipe_unlock::push_plain(
                            player,
                            super::recipe_unlock::PlainEvent {
                                trigger: "minecraft:summoned_entity",
                                entity: Some("minecraft:ender_dragon".to_owned()),
                                ..Default::default()
                            },
                        );
                    }
                    lock_game_state(&ctx.state, ctx.profiler.as_deref())
                        .more
                        .end_fight
                        .finish_respawn();
                }
            }
        }
    }

    /// Vanilla `TurtleEggBlock.stepOn` / `fallOn` for mobs: a random roll crushes one egg.
    /// Turtles are exempt (the bat kind is not modelled), and non-player living mobs obey
    /// `mobGriefing`.
    fn trample_turtle_eggs(
        &mut self,
        ctx: &TickContext,
        blocks: &TickBlocks<'_>,
        falling: &[(ferrite_entity::EntityKey, f32)],
    ) {
        let griefing = ctx
            .rules
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get_bool("mob_griefing")
            .unwrap_or(true);
        if !griefing {
            return;
        }
        let mut changes: Vec<([i32; 3], u16, u16)> = Vec::new();
        for &key in self.entities.keys() {
            let Some(state) = mob::state(&self.entities, key).copied() else {
                continue;
            };
            if state.health <= 0.0 || state.kind == MobKind::Turtle {
                continue;
            }
            let Some(flags) = self.entities.flags(key) else {
                continue;
            };
            if !flags.has(Flags::ON_GROUND) {
                continue;
            }
            let Some(pos) = self.entities.pos(key) else {
                continue;
            };
            let Some(vel) = self.entities.vel(key) else {
                continue;
            };
            let fell = falling.iter().any(|(other, _)| *other == key) && state.fall_distance == 0.0;
            let moving = vel.x * vel.x + vel.z * vel.z > 1e-6;
            if !fell && !moving {
                continue;
            }
            if !ferrite_entity::turtle::mob_can_trample(state.kind, fell, true) {
                continue;
            }
            // Turtle eggs have a low collision shape, so the feet can be either in the egg
            // cell or just above a full-cube treatment of it.
            let mut egg = None;
            for y in [pos.y.floor() as i32, (pos.y - 0.1).floor() as i32] {
                let at = [pos.x.floor() as i32, y, pos.z.floor() as i32];
                let Some(block) = blocks.block_state(at[0], at[1], at[2]) else {
                    continue;
                };
                let Some(props) = ferrite_entity::turtle::egg_props(block) else {
                    continue;
                };
                egg = Some((at, block, props));
                break;
            }
            let Some((at, block, (eggs, hatch))) = egg else {
                continue;
            };
            if changes.iter().any(|(other, _, _)| *other == at) {
                continue;
            }
            let chance = if fell {
                ferrite_entity::turtle::FALL_TRAMPLE_CHANCE
            } else {
                ferrite_entity::turtle::TRAMPLE_CHANCE
            };
            if self.rng.next_int(chance) != 0 {
                continue;
            }
            let next = ferrite_entity::turtle::without_one_egg(eggs, hatch).unwrap_or(0);
            changes.push((at, next, block));
        }
        if !changes.is_empty() {
            let writes: Vec<_> = changes.iter().map(|(at, next, _)| (*at, *next)).collect();
            self.place_blocks(ctx, &writes);
            for (at, _, block) in changes {
                let pitch = 0.9 + self.rng.next_f32() * 0.2;
                self.play_sound_near(
                    block_sound_center(at),
                    ferrite_entity::turtle::EGG_BREAK_SOUND,
                    0.7,
                    pitch,
                );
                turtle_egg_break_particles(&ctx.tracking, ctx.world_id, at, block);
            }
        }
    }

    /// Vanilla `TurtleEggBlock.stepOn` / `fallOn` for a player. A sneaking step is careful and
    /// skips the step-on roll, but not the fall-on roll, matching `isSteppingCarefully`.
    fn trample_turtle_eggs_for_player(&mut self, p: &mut Player, ctx: &TickContext) {
        if p.gamemode == 3 {
            return;
        }
        let Some(source) = p.source.clone() else {
            return;
        };
        let x = p.pos[0].floor() as i32;
        let z = p.pos[2].floor() as i32;
        let mut egg = None;
        for y in [p.pos[1].floor() as i32, (p.pos[1] - 0.1).floor() as i32] {
            let Some(block) = source.block(x, y, z).and_then(|id| u16::try_from(id).ok()) else {
                continue;
            };
            let Some(props) = ferrite_entity::turtle::egg_props(block) else {
                continue;
            };
            egg = Some(([x, y, z], block, props));
            break;
        }
        let Some((feet, block, (eggs, hatch))) = egg else {
            return;
        };
        let moved = (p.pos[0] - p.survival.last_pos[0]).abs() > 1e-3
            || (p.pos[2] - p.survival.last_pos[2]).abs() > 1e-3;
        let fell = p.ground && p.survival.fall_distance > 0.0;
        if !moved && !fell {
            return;
        }
        if !fell && p.sneaking {
            return;
        }
        let chance = if fell {
            ferrite_entity::turtle::FALL_TRAMPLE_CHANCE
        } else {
            ferrite_entity::turtle::TRAMPLE_CHANCE
        };
        if self.rng.next_int(chance) != 0 {
            return;
        }
        // `TurtleEggBlock.decreaseEggs`: the last egg removes the block (`0`) instead of
        // leaving a zero-egg state.
        let next = ferrite_entity::turtle::without_one_egg(eggs, hatch).unwrap_or(0);
        let location = ferrite_protocol::types::BlockPos::new(feet[0], feet[1], feet[2]).pack();
        if p.change_block(location, Some(u32::from(block)), u32::from(next)) {
            p.changed_blocks.push(feet);
            let pitch = 0.9 + self.rng.next_f32() * 0.2;
            self.play_sound_near(
                block_sound_center(feet),
                ferrite_entity::turtle::EGG_BREAK_SOUND,
                0.7,
                pitch,
            );
            turtle_egg_break_particles(&ctx.tracking, ctx.world_id, feet, block);
        }
    }

    /// `Turtle$TurtleLayEggGoal`: places a turtle egg cluster on a sand block if this region
    /// owns the block and the world has not changed under the turtle.
    fn lay_turtle_egg(&mut self, ctx: &TickContext, at: [i32; 3], eggs: u8) -> bool {
        let Some(source) = ctx.source.as_deref() else {
            return false;
        };
        let chunk = ChunkPos(at[0].div_euclid(16), at[2].div_euclid(16));
        if !self.held_chunks.contains_key(&chunk) {
            return false;
        }
        let Some(below) = source
            .block(at[0], at[1] - 1, at[2])
            .and_then(|id| u16::try_from(id).ok())
        else {
            return false;
        };
        let Some(here) = source
            .block(at[0], at[1], at[2])
            .and_then(|id| u16::try_from(id).ok())
        else {
            return false;
        };
        if !ferrite_entity::turtle::is_sand(below) || !ferrite_entity::turtle::is_air(here) {
            return false;
        }
        let Some(state) = ferrite_entity::turtle::egg_state(eggs, 0) else {
            return false;
        };
        self.place_blocks(ctx, &[(at, state)]);
        true
    }

    /// Writes blocks into the chunks this region holds and tells the viewers.
    fn place_blocks(&mut self, ctx: &TickContext, blocks: &[([i32; 3], u16)]) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        {
            let mut world =
                super::fluid::RegionFluidWorld::new(source, &self.held_chunks, &ctx.tracking);
            for (at, state) in blocks {
                let chunk = ChunkPos(at[0].div_euclid(16), at[2].div_euclid(16));
                if self.held_chunks.contains_key(&chunk) {
                    ferrite_fluid::FluidWorld::set_block(
                        &mut world,
                        at[0],
                        at[1],
                        at[2],
                        u32::from(*state),
                    );
                }
            }
        }
        for (at, _) in blocks {
            self.updates.block_changed(*at);
        }
    }
    /// Writes a gateway's block entity and sends it to the chunk's viewers. Vanilla has no block
    /// event for the gateway beam: `TheEndGatewayRenderer` reads `Age` from the block entity, and
    /// the client animates the magenta creation beam itself while `Age < 200`.
    fn set_gateway_age(&mut self, ctx: &TickContext, at: [i32; 3], age: i64) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        if !self
            .held_chunks
            .contains_key(&ChunkPos(at[0].div_euclid(16), at[2].div_euclid(16)))
        {
            return;
        }
        let Some(entity) = super::end_fight::gateway_block_entity(age) else {
            return;
        };
        let written = source.update_block_entity(at[0], at[1], at[2], |slot| {
            let same = slot.as_ref().is_some_and(|current| {
                current.type_id == entity.type_id && current.data == entity.data
            });
            if same {
                return (false, false);
            }
            *slot = Some(entity.clone());
            (true, true)
        });
        match written {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                tracing::warn!(?at, %error, "could not write the end gateway block entity");
                return;
            }
        }
        let update = packet(&BlockEntityData {
            location: ferrite_protocol::types::BlockPos::new(at[0], at[1], at[2]).pack(),
            block_type: VarInt(i32::try_from(entity.type_id).unwrap_or(0)),
            nbt: super::end_fight::gateway_update_tag(&entity),
        });
        broadcast_tracked_chunk(
            &ctx.tracking.lock().unwrap(),
            ctx.dimension,
            (at[0].div_euclid(16), at[2].div_euclid(16)),
            &update,
        );
    }
    /// Shows the living dragon's health to every player of the dimension as a boss bar, keeps
    /// it current and takes it away when the dragon is gone (vanilla `EnderDragon.bossEvent`).
    fn update_dragon_bar(&mut self) {
        let dragon = self.entities.keys().iter().find_map(|&key| {
            let data = self.entities.data(key)?;
            let mob = data.mob?;
            (mob.kind == ferrite_entity::mob::MobKind::EnderDragon && !mob.is_dead()).then(|| {
                (
                    data.uuid,
                    (mob.health / mob.kind.max_health()).clamp(0.0, 1.0),
                )
            })
        });
        self.dragon_bars
            .retain(|player, _| self.players.contains_key(player));
        for (uuid, p) in &self.players {
            let shown = self.dragon_bars.get(uuid).copied();
            match (dragon, shown) {
                (Some((bar, progress)), None) => {
                    send(
                        p,
                        &BossBar {
                            uuid: bar,
                            action: BossBarAction::Add {
                                name: ferrite_protocol::types::PlainText("Ender Dragon".into()),
                                progress,
                                color: VarInt(0),
                                overlay: 0,
                                // Darkens the sky, plays the boss music and thickens the fog.
                                flags: 0b111,
                            },
                        },
                    );
                    self.dragon_bars.insert(*uuid, (bar, progress));
                }
                (Some((bar, progress)), Some((shown_bar, shown_progress))) => {
                    if shown_bar != bar {
                        send(
                            p,
                            &BossBar {
                                uuid: shown_bar,
                                action: BossBarAction::Remove,
                            },
                        );
                        self.dragon_bars.remove(uuid);
                    } else if (shown_progress - progress).abs() > f32::EPSILON {
                        send(
                            p,
                            &BossBar {
                                uuid: bar,
                                action: BossBarAction::UpdateProgress(progress),
                            },
                        );
                        self.dragon_bars.insert(*uuid, (bar, progress));
                    }
                }
                (None, Some((bar, _))) => {
                    send(
                        p,
                        &BossBar {
                            uuid: bar,
                            action: BossBarAction::Remove,
                        },
                    );
                    self.dragon_bars.remove(uuid);
                }
                (None, None) => {}
            }
        }
    }
    /// The dragon's death: the exit portal rises on the podium under the world origin with the
    /// dragon egg on top, and the dragon's experience rains where it fell.
    fn dragon_died(&mut self, ctx: &TickContext, pos: ferrite_math::Vec3) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        // The podium stands on the end island's surface at the origin.
        let floor = (0..=255)
            .rev()
            .find(|&y| source.block(0, y, 0).is_some_and(|state| state != 0))
            .unwrap_or(63);
        let origin = [0, floor, 0];
        let first;
        let gateway;
        {
            let mut state = lock_game_state(&ctx.state, ctx.profiler.as_deref());
            let fight = &mut state.more.end_fight;
            first = !std::mem::replace(&mut fight.killed, true);
            fight.dragon_alive = false;
            fight.exit_portal_level = Some(floor);
            fight.abort_respawn();
            // The next gateway of the ring opens; a chunk that is not loaded here keeps the old
            // air and `repair_end_structures` writes the gateway once it is loaded.
            gateway = fight.open_next_gateway();
        }
        self.place_blocks(ctx, &super::end_fight::dragon_death_blocks(origin, first));
        if let Some(at) = gateway {
            self.place_blocks(ctx, &super::end_fight::gateway_blocks(at));
            self.set_gateway_age(ctx, at, 0);
        }
        for record in experience::award(pos, super::end_fight::dragon_experience(first) as i32) {
            self.entities.insert(record);
        }
    }
    /// Resolves this tick's `serverbound_attack` swings: reach, weapon damage and attack
    /// strength, critical hits, sweeping, knockback and the hurt animation. A drawn bow that
    /// was released this tick fires its arrow first.
    /// Sends an entity event (vanilla `broadcastEntityEvent`) to the players tracking `key`.
    /// Tells the players tracking `key` which entity holds its lead (0 for none).
    fn send_link(&self, ctx: &TickContext, key: ferrite_entity::EntityKey, holder: i32) {
        let Some(id) = self.entities.data(key).map(|data| data.id) else {
            return;
        };
        let players = &self.players;
        tracker::broadcast(
            &self.entities,
            key,
            &ferrite_protocol::packets::play::SetEntityLink {
                attached: id,
                holder,
            },
            &mut |viewer, bytes| deliver(players, ctx, viewer, bytes),
        );
    }

    /// A snapped lead falls where the animal stands.
    fn drop_lead(&mut self, key: ferrite_entity::EntityKey) {
        let (Some(pos), Some(item_id)) = (
            self.entities.pos(key),
            ferrite_item::item_id("minecraft:lead"),
        ) else {
            return;
        };
        let stack = ferrite_protocol::packets::play::ItemStack {
            count: 1,
            item_id: i32::from(item_id),
        };
        let block = [
            pos.x.floor() as i32,
            pos.y.floor() as i32,
            pos.z.floor() as i32,
        ];
        let record = item::pop_resource(block, stack, &mut self.rng);
        self.entities.insert(record);
    }

    /// Right-click on an employed villager or a wandering trader opens its trading screen.
    fn use_trader(&mut self, player: Uuid, key: ferrite_entity::EntityKey) -> bool {
        use ferrite_entity::mob::MobKind;
        let Some(state) = ferrite_entity::mob::state(&self.entities, key).copied() else {
            return false;
        };
        if !matches!(state.kind, MobKind::Villager | MobKind::WanderingTrader) || state.age < 0 {
            return false;
        }
        let Some(data) = self.entities.data(key) else {
            return false;
        };
        let (uuid, villager) = (data.uuid, data.villager.clone());
        let Some(villager) = villager.filter(|v| !v.offers.is_empty()) else {
            return false;
        };
        let village = state.kind == MobKind::Villager;
        let offers = if village {
            villager.offers_for_player(player)
        } else {
            villager.offers
        };
        if let Some(p) = self.players.get_mut(&player) {
            p.open_merchant_offers(
                state.kind.display_name(),
                offers,
                if village {
                    i32::from(villager.level)
                } else {
                    1
                },
                villager.xp,
                village,
                village,
                Some(super::container::OpenAt::Trader(uuid)),
            );
        }
        true
    }

    /// Applies sales left in open trade screens after the player loop. A click normally commits
    /// its own sale immediately; this is the once-per-tick safety net. Merging only forward
    /// deltas keeps a stale second screen from rewinding another viewer's uses or demand.
    fn sync_trades(&mut self) {
        let open: Vec<_> = self
            .players
            .iter_mut()
            .filter_map(|(player, p)| {
                let (trader, trades) = p.open_trader()?;
                let xp = p.take_trader_xp();
                Some((*player, trader, trades, xp))
            })
            .collect();
        for (player, trader, trades, xp) in open {
            let sold = merge_trader_trades(&mut self.entities, player, trader, &trades);
            if let Some(at) = self.players.get(&player) {
                push_trades(player, at.pos[1], sold);
            }
            add_trader_xp(&mut self.entities, trader, xp);
        }
    }

    /// The daily wandering trader roll (`WanderingTraderSpawner`): near a random player, with two
    /// trader llamas on leads, gone after 48000 ticks.
    fn tick_trader_spawn(
        &mut self,
        blocks: &TickBlocks<'_>,
        enabled: bool,
        clock_time: i64,
        bounds: super::wandering_trader::SpawnBounds,
        anchors: &[ferrite_math::Vec3],
        spawner: &Arc<Mutex<super::wandering_trader::SpawnerState>>,
    ) {
        use super::wandering_trader;
        use ferrite_entity::mob::{MobKind, new_mob};
        if anchors.is_empty() {
            return;
        }
        let should_spawn = spawner
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .roll(enabled, clock_time, &mut self.rng);
        if !should_spawn {
            return;
        }
        let anchor = anchors[self.rng.next_int(anchors.len() as i32) as usize];
        let anchor = [anchor.x, anchor.y, anchor.z];
        let Some(pos) = wandering_trader::find_spawn(blocks, anchor, bounds, &mut self.rng) else {
            return;
        };
        spawner
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .spawn_succeeded(clock_time);
        let at = ferrite_math::Vec3::new(
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]),
            f64::from(pos[2]) + 0.5,
        );
        let mut trader = new_mob(MobKind::WanderingTrader, at, &mut self.rng);
        if let Some(state) = trader.data.mob.as_mut() {
            state.lifetime = wandering_trader::DESPAWN_DELAY;
        }
        let (trader_uuid, trader_id) = (trader.data.uuid, trader.data.id);
        self.entities.insert(trader);
        for _ in 0..2 {
            let Some(llama_pos) = wandering_trader::find_llama_spawn(
                blocks,
                [at.x, at.y, at.z],
                bounds,
                &mut self.rng,
            ) else {
                continue;
            };
            let llama_at = ferrite_math::Vec3::new(
                f64::from(llama_pos[0]) + 0.5,
                f64::from(llama_pos[1]),
                f64::from(llama_pos[2]) + 0.5,
            );
            let mut llama = new_mob(MobKind::TraderLlama, llama_at, &mut self.rng);
            if let Some(state) = llama.data.mob.as_mut() {
                state.lifetime = wandering_trader::DESPAWN_DELAY;
                state.leash = Some((trader_uuid, trader_id));
            }
            self.entities.insert(llama);
        }
    }

    /// The traders whose trade screen is open for one of this region's players.
    fn trading_traders(&self) -> HashSet<Uuid> {
        self.players
            .values()
            .filter_map(|p| p.open_trader().map(|(trader, _)| trader))
            .collect()
    }

    /// Advances the 48000-tick clock for wandering traders and their llama companions. Vanilla
    /// pauses the trader clock while a player has its trade screen open; the llamas share the
    /// same clock so they do not vanish from under an active trade. An expired entity disappears
    /// without loot, and any leash or caravan link aimed at it is cleared in the same tick so no
    /// orphaned link reaches the next AI pass.
    fn tick_trader_lifetimes(&mut self, trading: &HashSet<Uuid>, ctx: &TickContext) {
        use super::wandering_trader;
        use ferrite_entity::mob::MobKind;

        let keys: Vec<_> = self
            .entities
            .keys()
            .iter()
            .copied()
            .filter(|key| {
                self.entities.flags(*key).is_some_and(|flags| {
                    !flags.has(Flags::REMOVED)
                        && self.entities.data(*key).is_some_and(|data| {
                            data.mob.as_ref().is_some_and(|mob| {
                                matches!(mob.kind, MobKind::WanderingTrader | MobKind::TraderLlama)
                            })
                        })
                })
            })
            .collect();

        let mut expired = Vec::new();
        for key in keys {
            let Some(uuid) = self.entities.data(key).map(|data| data.uuid) else {
                continue;
            };
            let Some(data) = self.entities.data_mut(key) else {
                continue;
            };
            let Some(mob) = data.mob.as_mut() else {
                continue;
            };
            let paused = match mob.kind {
                MobKind::WanderingTrader => trading.contains(&uuid),
                MobKind::TraderLlama => mob
                    .leash
                    .is_some_and(|(holder, _)| trading.contains(&holder)),
                _ => false,
            };
            if wandering_trader::tick_despawn_delay(&mut mob.lifetime, paused) {
                mob.health = 0.0;
                mob.loot_dropped = true;
                expired.push(key);
            }
        }
        if expired.is_empty() {
            return;
        }

        let expired_keys: HashSet<_> = expired.iter().copied().collect();
        let expired_uuids: HashSet<Uuid> = expired
            .iter()
            .filter_map(|key| self.entities.data(*key).map(|data| data.uuid))
            .collect();
        for key in &expired {
            if let Some(flags) = self.entities.flags_mut(*key) {
                flags.set(Flags::REMOVED, true);
            }
        }

        // Clear every stored link that points at an entity removed by this pass. The generic
        // leash code would break a lead one tick later, after a lead item had a chance to drop;
        // an expiry should leave no item and no stale caravan head behind.
        let linked: Vec<_> = self
            .entities
            .keys()
            .iter()
            .copied()
            .filter(|key| {
                !expired_keys.contains(key)
                    && self.entities.data(*key).is_some_and(|data| {
                        data.mob.as_ref().is_some_and(|mob| {
                            mob.leash
                                .is_some_and(|(holder, _)| expired_uuids.contains(&holder))
                                || mob
                                    .caravan
                                    .is_some_and(|head| expired_uuids.contains(&head))
                        })
                    })
            })
            .collect();
        for key in linked {
            let detached = self
                .entities
                .data_mut(key)
                .and_then(|data| data.mob.as_mut())
                .map(|mob| {
                    let leash = mob
                        .leash
                        .is_some_and(|(holder, _)| expired_uuids.contains(&holder));
                    if leash {
                        mob.leash = None;
                    }
                    if mob
                        .caravan
                        .is_some_and(|head| expired_uuids.contains(&head))
                    {
                        mob.caravan = None;
                    }
                    leash
                })
                .unwrap_or(false);
            if detached {
                self.send_link(ctx, key, 0);
            }
        }

        // A trader that expires outside the trade screen has no menu, but an entity removed by a
        // concurrent path must not leave a client menu pointing at a gone entity.
        for player in self.players.values_mut() {
            if player
                .open_trader()
                .is_some_and(|(trader, _)| expired_uuids.contains(&trader))
            {
                player.close_open_menu();
            }
        }
    }

    /// Night makes a trader drink invisibility; daylight makes it drink milk. The transition
    /// changes only its effects, never its villager offers or trade uses.
    fn tick_trader_drinks(&mut self, day_time: i64, trading: &HashSet<Uuid>, ctx: &TickContext) {
        use super::wandering_trader;
        let keys: Vec<_> = self
            .entities
            .keys()
            .iter()
            .copied()
            .filter(|key| {
                self.entities.flags(*key).is_some_and(|flags| {
                    !flags.has(Flags::REMOVED)
                        && self.entities.data(*key).is_some_and(|data| {
                            data.mob
                                .as_ref()
                                .is_some_and(|mob| mob.kind == MobKind::WanderingTrader)
                        })
                })
            })
            .collect();
        for key in keys {
            let Some(data) = self.entities.data_mut(key) else {
                continue;
            };
            let id = data.id;
            let ticks = data.effects.tick();
            let invisible = data.effects.has(Effect::Invisibility);
            let trading = trading.contains(&data.uuid);
            let applied = wandering_trader::drink_for(day_time, invisible, trading)
                .map(|drink| wandering_trader::apply_drink(data, drink))
                .unwrap_or_default();
            let removed: Vec<_> = ticks.expired.into_iter().chain(applied.removed).collect();
            let players = &self.players;
            for instance in &removed {
                tracker::broadcast(
                    &self.entities,
                    key,
                    &RemoveMobEffect {
                        entity_id: VarInt(id),
                        effect: VarInt(instance.effect),
                    },
                    &mut |viewer, bytes| deliver(players, ctx, viewer, bytes),
                );
            }
            if let Some(instance) = applied.added {
                tracker::broadcast(
                    &self.entities,
                    key,
                    &UpdateMobEffect {
                        entity_id: VarInt(id),
                        effect: VarInt(instance.effect),
                        amplifier: VarInt(instance.amplifier),
                        duration: VarInt(instance.duration),
                        flags: instance.flags(),
                    },
                    &mut |viewer, bytes| deliver(players, ctx, viewer, bytes),
                );
            }
        }
    }

    /// Right-click with a lead on a leashable animal ties it to the player; the holder's right
    /// click takes it off again and gets the lead back. `true` when the click was used.
    fn use_lead(
        &mut self,
        ctx: &TickContext,
        uuid: Uuid,
        key: ferrite_entity::EntityKey,
        held: &str,
        slot: usize,
        gamemode: i32,
    ) -> bool {
        let Some(state) = ferrite_entity::mob::state(&self.entities, key).copied() else {
            return false;
        };
        if state.leash.is_some_and(|(holder, _)| holder == uuid) {
            if let Some(state) = ferrite_entity::mob::state_mut(&mut self.entities, key) {
                state.leash = None;
            }
            self.send_link(ctx, key, 0);
            if gamemode != 1 {
                let lead = ferrite_item::ItemStack::from_name("minecraft:lead");
                if let (Some(lead), Some(p)) = (lead, self.players.get_mut(&uuid)) {
                    if let Some(rest) = p.inventory.place_back(lead) {
                        p.menus.thrown.push(rest);
                    }
                    p.sync_containers();
                }
            }
            return true;
        }
        if held != "minecraft:lead"
            || state.leash.is_some()
            || !(state.kind.is_animal() || state.kind.is_equine())
        {
            return false;
        }
        let Some(holder_id) = self.players.get(&uuid).map(|p| p.entity) else {
            return false;
        };
        if let Some(state) = ferrite_entity::mob::state_mut(&mut self.entities, key) {
            state.leash = Some((uuid, holder_id));
        }
        self.send_link(ctx, key, holder_id);
        if gamemode != 1
            && let Some(p) = self.players.get_mut(&uuid)
        {
            p.inventory.remove(slot, 1);
            send(
                p,
                &SetPlayerInventory {
                    slot: VarInt(slot as i32),
                    item: p.inventory.get(slot).clone(),
                },
            );
        }
        true
    }

    /// Ticks firework rockets: free ones climb, attached ones push their gliding player along
    /// their look direction, and a rocket whose life ends bursts (status 17) and hurts what is near.
    fn tick_fireworks(&mut self, ctx: &TickContext) {
        // Rockets plugins attached this tick.
        /// Owner, position, flight, star count and the id the plugin handle already carries.
        type Launch = (Uuid, [f64; 3], u8, u8, Option<Uuid>);
        let launches: Vec<Launch> = self
            .players
            .values_mut()
            .flat_map(|p| {
                let (uuid, pos) = (p.uuid, p.pos);
                p.firework_launches
                    .drain(..)
                    .map(move |(flight, explosions, entity)| {
                        (uuid, pos, flight, explosions, entity)
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        for (uuid, pos, flight, explosions, entity) in launches {
            let mut record = ferrite_entity::firework::new_firework(
                ferrite_math::Vec3::new(pos[0], pos[1], pos[2]),
                flight,
                explosions,
                Some(uuid),
                &mut self.rng,
            );
            if let Some(entity) = entity {
                record.data.uuid = entity;
            }
            self.entities.insert(record);
        }
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let blocks = TickBlocks {
            source: Some(source),
            seen: Default::default(),
        };
        let tick = ferrite_entity::firework::tick_fireworks(&mut self.entities, &blocks);
        for (key, uuid) in tick.attached {
            let Some(p) = self.players.get(&uuid) else {
                if let Some(f) = self.entities.flags_mut(key) {
                    f.set(ferrite_entity::Flags::REMOVED, true);
                }
                continue;
            };
            if !p.gliding {
                // Vanilla discards a rocket whose rider stopped gliding.
                if let Some(f) = self.entities.flags_mut(key) {
                    f.set(ferrite_entity::Flags::REMOVED, true);
                }
                continue;
            }
            let (yaw, pitch) = (
                f64::from(p.yaw).to_radians(),
                f64::from(p.pitch).to_radians(),
            );
            let look = ferrite_math::Vec3::new(
                -yaw.sin() * pitch.cos(),
                -pitch.sin(),
                yaw.cos() * pitch.cos(),
            );
            let rocket_at = self.entities.pos(key).unwrap_or_default();
            let velocity = ferrite_entity::firework::rider_velocity(
                rocket_at,
                ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]),
                look,
            );
            send(
                p,
                &SetEntityMotion {
                    id: VarInt(p.entity),
                    velocity: ferrite_protocol::types::LpVec3([velocity.x, velocity.y, velocity.z]),
                },
            );
            self.entities
                .set_pos(key, ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]));
        }
        for burst in tick.bursts {
            // Vanilla `FireworkRocketEntity.explode` broadcasts entity event 17.
            self.broadcast_status(ctx, burst.key, 17);
            let Some(damage) = ferrite_entity::firework::burst_damage(burst.explosions) else {
                continue;
            };
            for p in self.players.values_mut() {
                let d = [
                    p.pos[0] - burst.pos.x,
                    p.pos[1] - burst.pos.y,
                    p.pos[2] - burst.pos.z,
                ];
                if (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() <= 5.0 && p.gamemode != 3 {
                    p.hurt(DamageKind::Generic, damage);
                }
            }
        }
    }

    /// Eyes of ender: a new one looks for the nearest stronghold, then flies; when its life ends it
    /// drops as an item or shatters.
    fn tick_eyes(&mut self, ctx: &TickContext) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let throws: Vec<[f64; 3]> = self
            .players
            .values_mut()
            .flat_map(|p| std::mem::take(&mut p.eye_throws))
            .collect();
        for at in throws {
            let record = ferrite_entity::eye::new_eye(
                ferrite_math::Vec3::new(at[0], at[1], at[2]),
                &mut self.rng,
            );
            self.entities.insert(record);
        }
        for (key, pos) in ferrite_entity::eye::untargeted(&self.entities) {
            // Vanilla searches 100 chunks for a stronghold.
            let target = match source.locate_structure(
                "minecraft:stronghold",
                pos.x.floor() as i32,
                pos.z.floor() as i32,
                100,
            ) {
                Some(Ok(Some((x, y, z)))) => Some(ferrite_math::Vec3::new(
                    f64::from(x) + 0.5,
                    f64::from(y),
                    f64::from(z) + 0.5,
                )),
                _ => None,
            };
            ferrite_entity::eye::set_target(&mut self.entities, key, target);
        }
        for ended in ferrite_entity::eye::tick_eyes(&mut self.entities) {
            if ended.drops
                && let Some(stack) = ferrite_item::ItemStack::from_name("minecraft:ender_eye")
            {
                let record = ferrite_entity::item::new_item_stack(
                    ended.pos,
                    ferrite_math::Vec3::default(),
                    &stack,
                    &mut self.rng,
                );
                self.entities.insert(record);
            }
        }
    }

    fn broadcast_status(&self, ctx: &TickContext, key: ferrite_entity::EntityKey, status: u8) {
        let Some(id) = self.entities.data(key).map(|data| data.id) else {
            return;
        };
        let players = &self.players;
        tracker::broadcast(
            &self.entities,
            key,
            &ferrite_protocol::packets::play::EntityEvent {
                entity_id: id,
                status,
            },
            &mut |viewer, bytes| deliver(players, ctx, viewer, bytes),
        );
    }

    /// Right-clicks on entities: food puts an adult animal in love or speeds a baby up
    /// (vanilla `Animal.mobInteract`); the item is used up outside creative mode.
    /// The variant of a cat or wolf, picked from the biome it stands in the first time it is
    /// needed (vanilla picks it when the animal spawns; this server has no spawn-time hook for
    /// every spawn path). Returns its registry id.
    fn settle_variant(
        &mut self,
        ctx: &TickContext,
        key: ferrite_entity::EntityKey,
    ) -> Option<&'static str> {
        let (kind, variant) = {
            let state = ferrite_entity::mob::state(&self.entities, key)?;
            (state.kind, state.variant)
        };
        ferrite_entity::variant::names(kind)?;
        let index = match variant {
            Some(index) => index,
            None => {
                let at = self.entities.pos(key)?;
                let biome = ctx.source.as_ref().and_then(|source| {
                    source.biome_name(
                        at.x.floor() as i32,
                        at.y.floor() as i32,
                        at.z.floor() as i32,
                    )
                });
                let day_time = lock_game_state(&ctx.state, ctx.profiler.as_deref()).clock_time;
                let index = super::variants::select(
                    kind,
                    biome.as_deref(),
                    super::variants::moon_brightness(day_time),
                    &mut self.rng,
                )?;
                ferrite_entity::mob::state_mut(&mut self.entities, key)?.variant = Some(index);
                index
            }
        };
        ferrite_entity::variant::name(kind, index)
    }

    fn resolve_uses(&mut self, ctx: &TickContext) {
        let uuids: Vec<Uuid> = self.players.keys().copied().collect();
        for uuid in uuids {
            let Some(p) = self.players.get_mut(&uuid) else {
                continue;
            };
            let uses = std::mem::take(&mut p.uses);
            for (id, hand) in uses {
                let Some(p) = self.players.get(&uuid) else {
                    break;
                };
                if p.survival.dead || p.gamemode == 3 || !(0..=1).contains(&hand) {
                    continue;
                }
                let Some(key) = self.entities.key_of(id) else {
                    continue;
                };
                let Some(bbox) = self.entities.bbox(key) else {
                    continue;
                };
                let feet = ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]);
                if !combat::within_reach(combat::eye_position(feet), bbox, combat::ENTITY_RANGE) {
                    continue;
                }
                let slot = p.hand_slot(hand);
                let held = p.inventory.get(slot).clone();
                let name = held.name().unwrap_or("minecraft:air");
                let gamemode = p.gamemode;
                let sneaking = p.sneaking;
                // A sneaking use on a tamed parrot lifts it onto a free shoulder.
                if sneaking && self.capture_shoulder(uuid, key) {
                    continue;
                }
                if self.use_lead(ctx, uuid, key, name, slot, gamemode) {
                    continue;
                }
                if self.use_trader(uuid, key) {
                    continue;
                }
                if self.use_shears_on_sheep(uuid, key, name, slot, gamemode) {
                    continue;
                }
                if self.use_shears_on_snow_golem(uuid, key, name, slot, gamemode) {
                    continue;
                }
                if self.use_shears_on_mooshroom(uuid, key, name, slot, gamemode) {
                    continue;
                }
                if self.use_dye_on_sheep(uuid, key, name, slot, gamemode) {
                    continue;
                }
                if self.use_iron_ingot_on_golem(uuid, key, name, slot, gamemode) {
                    continue;
                }
                if self.use_bucket_on_milkable(uuid, key, name, slot, gamemode) {
                    continue;
                }
                if self.use_bowl_on_mooshroom(uuid, key, name, slot, gamemode) {
                    continue;
                }
                if self.use_gold_on_piglin(uuid, key, name, slot, gamemode) {
                    continue;
                }
                // A sneaking use on a tame mount opens its inventory (`AbstractHorse.mobInteract`).
                if sneaking
                    && ferrite_entity::mob::state(&self.entities, key)
                        .is_some_and(|s| s.kind.is_equine() && s.owner.is_some())
                {
                    self.open_mount(uuid, key);
                    continue;
                }
                if ferrite_entity::mob::state(&self.entities, key)
                    .is_some_and(|s| s.kind.is_equine())
                    && self.use_horse(ctx, uuid, key, name, slot, gamemode)
                {
                    continue;
                }
                if ferrite_entity::mob::state(&self.entities, key)
                    .is_some_and(|s| s.kind == ferrite_entity::mob::MobKind::Pig)
                    && self.use_pig(ctx, uuid, key, name, slot, gamemode)
                {
                    continue;
                }
                if ferrite_entity::breed::feed(&mut self.entities, key, name, uuid)
                    == ferrite_entity::breed::Fed::No
                {
                    // Not food: a wolf may be tamed with a bone or told to sit.
                    let used = ferrite_entity::tame::use_on(
                        &mut self.entities,
                        key,
                        name,
                        uuid,
                        &mut self.rng,
                    );
                    // Entity events: 7 hearts when tamed, 6 smoke when a taming attempt failed.
                    if let Some(status) = used.status() {
                        self.broadcast_status(ctx, key, status);
                    }
                    if used == ferrite_entity::tame::Used::Tamed {
                        let variant = self.settle_variant(ctx, key);
                        let kind = ferrite_entity::mob::state(&self.entities, key)
                            .map_or("", |state| state.kind.name());
                        super::recipe_unlock::TAMED
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push((uuid, kind, variant));
                    }
                    if used != ferrite_entity::tame::Used::Consumed
                        && used != ferrite_entity::tame::Used::Tamed
                    {
                        continue;
                    }
                }
                if held.is_empty() {
                    continue;
                }
                if gamemode != 1
                    && let Some(p) = self.players.get_mut(&uuid)
                {
                    p.inventory.set(slot, held.copy_with_count(held.count - 1));
                }
            }
        }
    }

    /// Vanilla `Sheep.mobInteract` with shears: a ready sheep drops one to three wool of its
    /// colour from `shearing/sheep`, becomes sheared and wears the shears by one; a lamb or an
    /// already sheared sheep consumes the click and does nothing. Not modelled: the
    /// `GameEvent.SHEAR` vibration.
    fn use_shears_on_sheep(
        &mut self,
        player: Uuid,
        key: ferrite_entity::EntityKey,
        held: &str,
        slot: usize,
        gamemode: i32,
    ) -> bool {
        if held != "minecraft:shears" {
            return false;
        }
        match self.shear_sheep(key) {
            None => return false,
            Some(false) => return true,
            Some(true) => {}
        }
        if let Some(pos) = self.entities.pos(key) {
            self.play_sound_near(pos, "minecraft:entity.sheep.shear", 1.0, 1.0);
        }
        self.wear_shears(player, slot, gamemode);
        true
    }

    /// Vanilla `ItemStack.hurtAndBreak(1, ...)` for the shears in `slot`; creative keeps them.
    fn wear_shears(&mut self, player: Uuid, slot: usize, gamemode: i32) {
        if gamemode == 1 {
            return;
        }
        let Some(p) = self.players.get_mut(&player) else {
            return;
        };
        let stack = p.inventory.get(slot).clone();
        let max = ferrite_item::item_defaults(stack.item).and_then(|d| d.max_damage);
        let damage = stack.damage() + 1;
        let worn = match max {
            Some(max) if damage >= max => Some(ferrite_item::ItemStack::default()),
            _ => worn_stack(stack, damage),
        };
        if let Some(worn) = worn {
            p.inventory.set(slot, worn);
            send(
                p,
                &SetPlayerInventory {
                    slot: VarInt(slot as i32),
                    item: p.inventory.get(slot).clone(),
                },
            );
        }
    }

    /// Vanilla `SnowGolem.mobInteract` with shears: a golem wearing a pumpkin plays
    /// `entity.snow_golem.shear`, loses the pumpkin, drops `shearing/snow_golem` at eye height
    /// and wears the shears by one. A sheared golem leaves the click unused (vanilla `PASS`).
    /// Not modelled: the `GameEvent.SHEAR` vibration.
    fn use_shears_on_snow_golem(
        &mut self,
        player: Uuid,
        key: ferrite_entity::EntityKey,
        held: &str,
        slot: usize,
        gamemode: i32,
    ) -> bool {
        if held != "minecraft:shears" {
            return false;
        }
        match ferrite_entity::golem::shear(&mut self.entities, key) {
            None | Some(false) => return false,
            Some(true) => {}
        }
        if let Some(pos) = self.entities.pos(key) {
            self.play_sound_near(pos, ferrite_entity::golem::SNOW_GOLEM_SHEAR_SOUND, 1.0, 1.0);
            let eye = f64::from(MobKind::SnowGolem.props().eye_height);
            for stack in super::drops::snow_golem_shearing_drops(&mut self.rng) {
                let at = ferrite_math::Vec3::new(pos.x, pos.y + eye, pos.z);
                self.entities.insert(item::new_item_stack(
                    at,
                    ferrite_math::Vec3::ZERO,
                    &stack,
                    &mut self.rng,
                ));
            }
        }
        self.wear_shears(player, slot, gamemode);
        true
    }

    /// Vanilla `MushroomCow.shear`: shearing an adult mooshroom drops 5 red or brown mushrooms,
    /// plays `entity.mooshroom.shear`, wears the shears, and replaces the mooshroom with a cow.
    fn use_shears_on_mooshroom(
        &mut self,
        player: Uuid,
        key: ferrite_entity::EntityKey,
        held: &str,
        slot: usize,
        gamemode: i32,
    ) -> bool {
        if held != "minecraft:shears" || !mob::is_mob(&self.entities, key) {
            return false;
        }
        let Some(state) = mob::state(&self.entities, key).copied() else {
            return false;
        };
        if state.kind != MobKind::Mooshroom || state.age < 0 || state.is_dead() {
            return false;
        }
        let pos = self.entities.pos(key).unwrap_or_default();
        let vel = self.entities.vel(key).unwrap_or_default();
        let (yaw, pitch, custom_name) = self.entities.data(key).map_or((0.0, 0.0, None), |d| {
            (d.yaw, d.pitch, d.custom_name.clone())
        });
        let brown = state.variant == Some(1);
        self.play_sound_near(pos, "minecraft:entity.mooshroom.shear", 1.0, 1.0);
        let height = f64::from(MobKind::Mooshroom.props().height);
        for stack in super::drops::mooshroom_shearing_drops(brown, &mut self.rng) {
            let at = ferrite_math::Vec3::new(pos.x, pos.y + height, pos.z);
            self.entities.insert(item::new_item_stack(
                at,
                ferrite_math::Vec3::ZERO,
                &stack,
                &mut self.rng,
            ));
        }
        if let Some(flags) = self.entities.flags_mut(key) {
            flags.set(ferrite_entity::Flags::REMOVED, true);
        }
        let mut cow = mob::new_mob(MobKind::Cow, pos, &mut self.rng);
        cow.data.yaw = yaw;
        cow.data.head_yaw = yaw;
        cow.data.pitch = pitch;
        cow.data.custom_name = custom_name;
        if let Some(cow_state) = cow.data.mob.as_mut() {
            cow_state.health = state.health.min(MobKind::Cow.max_health());
            cow_state.age = state.age;
            cow_state.no_ai = state.no_ai;
            cow_state.love = state.love;
        }
        mob::sync_state_metadata(&mut cow.data);
        let cow_key = self.entities.insert(cow);
        self.entities.set_vel(cow_key, vel);
        self.wear_shears(player, slot, gamemode);
        true
    }

    /// `DyeItem.interactLivingEntity` on a sheep: a live, unsheared sheep of another colour takes
    /// the dye's colour, the click plays `item.dye.use` and one dye is used up (creative keeps
    /// the stack, as `Player.interactOn` restores it). Any other click is not used here and
    /// falls through to feeding. Not modelled: the dye-use game event.
    fn use_dye_on_sheep(
        &mut self,
        player: Uuid,
        key: ferrite_entity::EntityKey,
        held: &str,
        slot: usize,
        gamemode: i32,
    ) -> bool {
        let Some(color) = ferrite_entity::sheep::dye_color(held) else {
            return false;
        };
        if !ferrite_entity::sheep::dye(&mut self.entities, key, color) {
            return false;
        }
        if let Some(pos) = self.entities.pos(key) {
            self.play_sound_near(pos, "minecraft:item.dye.use", 1.0, 1.0);
        }
        if let Some(p) = self.players.get_mut(&player) {
            let stack = p.inventory.get(slot).clone();
            let rest = stack_after_dye(&stack, gamemode);
            if rest != stack {
                p.inventory.set(slot, rest);
                send(
                    p,
                    &SetPlayerInventory {
                        slot: VarInt(slot as i32),
                        item: p.inventory.get(slot).clone(),
                    },
                );
            }
        }
        true
    }

    /// `BreakDoorGoal.tick` on its last tick: both halves of the wooden door vanish without a
    /// drop (`removeBlock`), with the zombie's door-breaking sound. The crack-stage packets
    /// (`destroyBlockProgress`) are not sent.
    fn break_door(&mut self, ctx: &TickContext, lower: [i32; 3]) {
        let chunk = ChunkPos(lower[0].div_euclid(16), lower[2].div_euclid(16));
        if !self.held_chunks.contains_key(&chunk) {
            return;
        }
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let is_door = |at: [i32; 3]| {
            source
                .block(at[0], at[1], at[2])
                .and_then(|state| u16::try_from(state).ok())
                .is_some_and(|state| ferrite_entity::sun::is_closed_wooden_door(Some(state)))
        };
        let upper = [lower[0], lower[1] + 1, lower[2]];
        let mut removed = Vec::new();
        if is_door(lower) {
            removed.push((lower, 0));
        }
        if is_door(upper) {
            removed.push((upper, 0));
        }
        if removed.is_empty() {
            return;
        }
        self.place_blocks(ctx, &removed);
        self.play_sound_near(
            ferrite_math::Vec3::new(
                f64::from(lower[0]) + 0.5,
                f64::from(lower[1]) + 0.5,
                f64::from(lower[2]) + 0.5,
            ),
            "minecraft:entity.zombie.break_wooden_door",
            1.0,
            1.0,
        );
    }

    /// `InteractWithDoor`: opens or closes both halves of a hand-openable door owned by this
    /// region. Guards on `held_chunks` before reading or mutating block state so a mob near a
    /// region boundary never mutates another region's door.
    fn set_door_open(&mut self, ctx: &TickContext, lower: [i32; 3], open: bool) {
        let chunk = ChunkPos(lower[0].div_euclid(16), lower[2].div_euclid(16));
        if !self.held_chunks.contains_key(&chunk) {
            return;
        }
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let target_open = if open { "true" } else { "false" };
        let upper = [lower[0], lower[1] + 1, lower[2]];
        let mut updates = Vec::new();
        let mut door_name = None;
        for pos in [lower, upper] {
            let Some(state) = source
                .block(pos[0], pos[1], pos[2])
                .and_then(|s| u16::try_from(s).ok())
            else {
                continue;
            };
            if !ferrite_entity::path::is_openable_door(state) {
                continue;
            }
            if super::blocks::property(state, "open") == Some(target_open) {
                continue;
            }
            if let Some(next) = super::blocks::with_props(state, &[("open", target_open)]) {
                door_name = ferrite_data::blocks::state(state).map(|(name, _)| name);
                updates.push((pos, next));
            }
        }
        if updates.is_empty() {
            return;
        }
        self.place_blocks(ctx, &updates);
        let is_copper = door_name.is_some_and(|name| name.ends_with("copper_door"));
        let sound = match (is_copper, open) {
            (true, true) => "minecraft:block.copper_door.open",
            (true, false) => "minecraft:block.copper_door.close",
            (false, true) => "minecraft:block.wooden_door.open",
            (false, false) => "minecraft:block.wooden_door.close",
        };
        self.play_sound_near(
            ferrite_math::Vec3::new(
                f64::from(lower[0]) + 0.5,
                f64::from(lower[1]) + 0.5,
                f64::from(lower[2]) + 0.5,
            ),
            sound,
            1.0,
            1.0,
        );
    }

    /// `Spider.finalizeSpawn`: one natural spider in a hundred comes with a skeleton riding it.
    /// Seats the skeleton (through [`super::ride`], which keeps it on the spider) and returns its
    /// record for the caller to insert next to the spider; the spider is marked as carrying a
    /// rider so it stops leaping.
    fn spider_jockey(
        &mut self,
        spider: &mut ferrite_entity::EntityRecord,
    ) -> Option<ferrite_entity::EntityRecord> {
        let is_spider = spider
            .data
            .mob
            .as_ref()
            .is_some_and(|state| state.kind == MobKind::Spider);
        if !is_spider || !ferrite_entity::spider::jockey_roll(&mut self.rng) {
            return None;
        }
        let rider = mob::new_mob(MobKind::Skeleton, spider.pos, &mut self.rng);
        super::ride::mount_in(
            rider.data.uuid,
            spider.data.uuid,
            false,
            Some("minecraft:spider"),
        )
        .ok()?;
        if let Some(state) = spider.data.mob.as_mut() {
            state.has_rider = true;
        }
        Some(rider)
    }

    /// Whether the live `mob_griefing` rule allows a mob block change.
    fn mob_griefing(ctx: &TickContext) -> bool {
        ctx.rules
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_bool("mob_griefing")
            .unwrap_or(true)
    }

    /// Vanilla `MushroomCow.mobInteract` with a bowl: an adult mooshroom fills a bowl into
    /// `minecraft:mushroom_stew` (`ItemUtils.createFilledResult`) and plays
    /// `minecraft:entity.mooshroom.milk`. Baby mooshrooms ignore the bowl.
    fn use_bowl_on_mooshroom(
        &mut self,
        player: Uuid,
        key: ferrite_entity::EntityKey,
        held: &str,
        slot: usize,
        gamemode: i32,
    ) -> bool {
        if held != "minecraft:bowl" {
            return false;
        }
        let is_adult_mooshroom = ferrite_entity::mob::state(&self.entities, key)
            .is_some_and(|s| s.kind == ferrite_entity::mob::MobKind::Mooshroom && s.age >= 0);
        if !is_adult_mooshroom {
            return false;
        }
        let Some(stew) = ferrite_item::ItemStack::from_name("minecraft:mushroom_stew") else {
            return false;
        };
        if let Some(pos) = self.entities.pos(key) {
            self.play_sound_near(pos, "minecraft:entity.mooshroom.milk", 1.0, 1.0);
        }
        if let Some(p) = self.players.get_mut(&player) {
            let stack = p.inventory.get(slot).clone();
            let (updated_slot, extra_stew) = mooshroom_bowl_result(&stack, gamemode, &stew);
            if updated_slot != stack {
                p.inventory.set(slot, updated_slot);
                send(
                    p,
                    &SetPlayerInventory {
                        slot: VarInt(slot as i32),
                        item: p.inventory.get(slot).clone(),
                    },
                );
            }
            if let Some(mut extra) = extra_stew {
                p.inventory.add(&mut extra, gamemode == 1);
                if !extra.is_empty() {
                    let drop_pos = ferrite_math::Vec3::new(p.pos[0], p.pos[1] + 0.5, p.pos[2]);
                    let record = item::new_item_stack(
                        drop_pos,
                        ferrite_math::Vec3::ZERO,
                        &extra,
                        &mut self.rng,
                    );
                    self.entities.insert(record);
                }
            }
        }
        true
    }

    /// Vanilla `IronGolem.mobInteract` with an iron ingot: heal 25 health, play
    /// `entity.iron_golem.repair` with a random pitch, and consume one ingot. A full-health golem
    /// leaves the click unused. Creative keeps the ingot, as `ItemStack.consume` does.
    fn use_iron_ingot_on_golem(
        &mut self,
        player: Uuid,
        key: ferrite_entity::EntityKey,
        held: &str,
        slot: usize,
        gamemode: i32,
    ) -> bool {
        if held != "minecraft:iron_ingot" {
            return false;
        }
        let Some((kind, health, max_health)) = ferrite_entity::mob::state(&self.entities, key)
            .map(|state| (state.kind, state.health, state.kind.max_health()))
        else {
            return false;
        };
        if kind != MobKind::IronGolem || health <= 0.0 || health >= max_health {
            return false;
        }
        if let Some(state) = ferrite_entity::mob::state_mut(&mut self.entities, key) {
            state.health = (health + 25.0).min(max_health);
        }
        if let Some(pos) = self.entities.pos(key) {
            let pitch = 1.0 + (self.rng.next_f32() - self.rng.next_f32()) * 0.2;
            self.play_sound_near(pos, "minecraft:entity.iron_golem.repair", 1.0, pitch);
        }
        if gamemode != 1
            && let Some(p) = self.players.get_mut(&player)
        {
            let stack = p.inventory.get(slot).clone();
            let rest = stack.copy_with_count(stack.count - 1);
            if rest != stack {
                p.inventory.set(slot, rest);
                send(
                    p,
                    &SetPlayerInventory {
                        slot: VarInt(slot as i32),
                        item: p.inventory.get(slot).clone(),
                    },
                );
            }
        }
        true
    }

    /// Vanilla `AbstractCow.mobInteract` and `Goat.mobInteract` with a bucket: an adult cow,
    /// mooshroom or goat fills a milk bucket. A baby leaves the click unused. Goats use their
    /// screaming sound variant when applicable.
    fn use_bucket_on_milkable(
        &mut self,
        player: Uuid,
        key: ferrite_entity::EntityKey,
        held: &str,
        slot: usize,
        gamemode: i32,
    ) -> bool {
        if held != "minecraft:bucket" {
            return false;
        }
        let Some((kind, adult, screaming)) = ferrite_entity::mob::state(&self.entities, key)
            .map(|state| (state.kind, state.age >= 0, state.screaming))
        else {
            return false;
        };
        if !adult || !matches!(kind, MobKind::Cow | MobKind::Mooshroom | MobKind::Goat) {
            return false;
        }
        let Some(milk) = ferrite_item::ItemStack::from_name("minecraft:milk_bucket") else {
            return false;
        };
        let sound = match kind {
            MobKind::Goat if screaming => "minecraft:entity.goat.screaming.milk",
            MobKind::Goat => "minecraft:entity.goat.milk",
            _ => "minecraft:entity.cow.milk",
        };
        if let Some(pos) = self.entities.pos(key) {
            self.play_sound_near(pos, sound, 1.0, 1.0);
        }
        if let Some(p) = self.players.get_mut(&player) {
            let stack = p.inventory.get(slot).clone();
            let (updated_slot, extra_milk) = filled_container_result(&stack, gamemode, &milk);
            if updated_slot != stack {
                p.inventory.set(slot, updated_slot);
                send(
                    p,
                    &SetPlayerInventory {
                        slot: VarInt(slot as i32),
                        item: p.inventory.get(slot).clone(),
                    },
                );
            }
            if let Some(mut extra) = extra_milk {
                p.inventory.add(&mut extra, gamemode == 1);
                if !extra.is_empty() {
                    let drop_pos = ferrite_math::Vec3::new(p.pos[0], p.pos[1] + 0.5, p.pos[2]);
                    let record = item::new_item_stack(
                        drop_pos,
                        ferrite_math::Vec3::ZERO,
                        &extra,
                        &mut self.rng,
                    );
                    self.entities.insert(record);
                }
            }
        }
        super::recipe_unlock::push_plain(
            player,
            super::recipe_unlock::PlainEvent {
                trigger: "minecraft:filled_bucket",
                item: Some("minecraft:milk_bucket".to_owned()),
                ..Default::default()
            },
        );
        true
    }

    /// Vanilla `Piglin.mobInteract` / `PiglinAi.mobInteract`: a living, idle adult piglin offered
    /// `minecraft:gold_ingot` takes one ingot into its off hand and admires it for
    /// [`ferrite_entity::piglin::ADMIRE_TICKS`] ticks before rolling
    /// `minecraft:gameplay/piglin_bartering`. Babies, dead piglins, and piglins already admiring
    /// leave the player's stack untouched.
    fn use_gold_on_piglin(
        &mut self,
        player: Uuid,
        key: ferrite_entity::EntityKey,
        held: &str,
        slot: usize,
        gamemode: i32,
    ) -> bool {
        if !ferrite_entity::piglin::can_admire(&self.entities, key, held) {
            return false;
        }
        let Some(admiring) = ferrite_entity::piglin::start_admiring(&mut self.entities, key) else {
            return false;
        };
        if let Some(pos) = self.entities.pos(key) {
            self.play_sound_near_with_source(
                pos,
                "minecraft:entity.piglin.admiring_item",
                1.0,
                1.0,
                5, // SoundSource.HOSTILE
            );
            if let Some(spilled) = admiring.spilled {
                let mut record =
                    item::new_item_stack(pos, ferrite_math::Vec3::ZERO, &spilled, &mut self.rng);
                if let Some(item_state) = record.data.item.as_mut() {
                    item_state.pickup_delay = ferrite_entity::item::DEFAULT_PICKUP_DELAY;
                }
                self.entities.insert(record);
            }
        }
        if gamemode != 1
            && let Some(p) = self.players.get_mut(&player)
        {
            let stack = p.inventory.get(slot).clone();
            let rest = stack.copy_with_count(stack.count - 1);
            if rest != stack {
                p.inventory.set(slot, rest);
                send(
                    p,
                    &SetPlayerInventory {
                        slot: VarInt(slot as i32),
                        item: p.inventory.get(slot).clone(),
                    },
                );
            }
        }
        true
    }

    /// Completes a piglin's barter (`PiglinAi.stopHoldingOffHandItem` + `throwItems`): rolls
    /// `minecraft:gameplay/piglin_bartering`, swings the off hand, and throws each stack toward
    /// the nearest visible player (or a random nearby spot).
    fn finish_piglin_barter(
        &mut self,
        ctx: &TickContext,
        key: ferrite_entity::EntityKey,
        at: ferrite_math::Vec3,
        toward: Option<ferrite_math::Vec3>,
    ) {
        let stacks = super::drops::piglin_barter(&mut self.rng);
        if stacks.is_empty() {
            return;
        }
        let (entity_id, piglin_uuid) = self
            .entities
            .data(key)
            .map_or((None, None), |data| (Some(data.id), Some(data.uuid)));
        if let Some(id) = entity_id {
            let players = &self.players;
            tracker::broadcast(
                &self.entities,
                key,
                &Animate {
                    id: VarInt(id),
                    action: 3, // SWING_OFF_HAND
                },
                &mut |viewer, bytes| deliver(players, ctx, viewer, bytes),
            );
        }
        let target = toward.unwrap_or_else(|| {
            let dx = self.rng.range(-4.0, 4.0);
            let dy = self.rng.range(-2.0, 2.0);
            let dz = self.rng.range(-4.0, 4.0);
            ferrite_math::Vec3::new(at.x + dx, at.y + dy, at.z + dz)
        });
        let origin = ferrite_entity::piglin::throw_origin(at);
        let vel = ferrite_entity::piglin::throw_velocity(at, target);
        for stack in stacks {
            let mut record = item::new_item_stack(origin, vel, &stack, &mut self.rng);
            if let Some(item_state) = record.data.item.as_mut() {
                item_state.pickup_delay = ferrite_entity::item::DEFAULT_PICKUP_DELAY;
                item_state.thrower = piglin_uuid;
            }
            self.entities.insert(record);
        }
    }

    /// Angers idle adult piglins in this region within 16 blocks of `pos` (`PiglinAi.angerNearbyPiglins`).
    pub(super) fn anger_nearby_piglins(
        &mut self,
        source: Option<&super::anvil::AnvilChunkSource>,
        player: Uuid,
        pos: [f64; 3],
        require_los: bool,
    ) -> usize {
        let blocks = TickBlocks {
            source,
            seen: Default::default(),
        };
        ferrite_entity::piglin::anger_nearby(
            &mut self.entities,
            &blocks,
            player,
            ferrite_math::Vec3::new(pos[0], pos[1], pos[2]),
            require_los,
        )
    }

    /// Angers nearby piglins in this region and forwards the alert to any neighbouring regions
    /// overlapping the 16-block guard radius.
    fn trigger_piglin_guard_alert(
        &mut self,
        ctx: &TickContext,
        blocks: &TickBlocks<'_>,
        player: Uuid,
        pos: [f64; 3],
        require_los: bool,
    ) {
        ferrite_entity::piglin::anger_nearby(
            &mut self.entities,
            blocks,
            player,
            ferrite_math::Vec3::new(pos[0], pos[1], pos[2]),
            require_los,
        );
        let margin = ferrite_entity::piglin::ANGER_RADIUS + 0.3;
        let min_cx = (pos[0] - margin).floor() as i32 >> 4;
        let max_cx = (pos[0] + margin).floor() as i32 >> 4;
        let min_cz = (pos[2] - margin).floor() as i32 >> 4;
        let max_cz = (pos[2] + margin).floor() as i32 >> 4;
        let mut remote_chunks = Vec::new();
        for cx in min_cx..=max_cx {
            for cz in min_cz..=max_cz {
                let chunk = ChunkPos(cx, cz);
                if !self.held_chunks.contains_key(&chunk) {
                    remote_chunks.push(chunk);
                }
            }
        }
        if !remote_chunks.is_empty() {
            let _ = ctx.signals.send(Signal::PiglinGuardAlert {
                world_id: ctx.world_id,
                player,
                pos,
                require_los,
                chunks: remote_chunks,
            });
        }
    }

    /// `EatBlockGoal.tick`: `mobGriefing` gates the block change (a plant is destroyed without
    /// drops; grass becomes dirt), not the sheep's `ate()`, which the AI already did.
    fn eat_block(&mut self, ctx: &TickContext, at: [i32; 3], grass: bool) {
        if !Self::mob_griefing(ctx) {
            return;
        }
        let state = if grass {
            ferrite_data::blocks::default_state("minecraft:dirt")
        } else {
            Some(0)
        };
        if let Some(state) = state {
            self.place_blocks(ctx, &[(at, state)]);
        }
    }

    /// `SnowGolem.aiStep` snow trail: places a `minecraft:snow` layer at `at` only when
    /// `mobGriefing` is enabled, this region owns the chunk, `at` is air, and the block below
    /// can support a snow layer.
    fn place_snow_trail(&mut self, ctx: &TickContext, at: [i32; 3]) {
        if !Self::mob_griefing(ctx) {
            return;
        }
        if !self
            .held_chunks
            .contains_key(&ChunkPos(at[0].div_euclid(16), at[2].div_euclid(16)))
        {
            return;
        }
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        if source.block(at[0], at[1], at[2]) != Some(0) {
            return;
        }
        let Some(below) = source
            .block(at[0], at[1] - 1, at[2])
            .and_then(|s| u16::try_from(s).ok())
        else {
            return;
        };
        if below == 0
            || usize::from(below) >= ferrite_data::blocks::STATE_COUNT
            || ferrite_data::block_props::get(below).is_air
            || ferrite_data::block_props::get(below).fluid.is_some()
            || ferrite_data::block_props::collision_shape(below).is_empty()
        {
            return;
        }
        if ferrite_data::blocks::state(below).is_some_and(|(name, _)| {
            matches!(
                name,
                "minecraft:ice" | "minecraft:packed_ice" | "minecraft:barrier" | "minecraft:snow"
            )
        }) {
            return;
        }
        if let Some(snow) = ferrite_data::blocks::default_state("minecraft:snow") {
            self.place_blocks(ctx, &[(at, snow)]);
        }
    }

    /// `EndermanTakeBlockGoal`: remove a tagged block without drops and store its block's
    /// default state. The AI already checked the random roll and line of sight; this is the
    /// authoritative `mobGriefing` and tag check.
    fn take_enderman_block(
        &mut self,
        ctx: &TickContext,
        key: ferrite_entity::EntityKey,
        at: [i32; 3],
    ) {
        if !Self::mob_griefing(ctx) {
            return;
        }
        if !self
            .held_chunks
            .contains_key(&ChunkPos(at[0].div_euclid(16), at[2].div_euclid(16)))
        {
            return;
        }
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let Some(state) = source
            .block(at[0], at[1], at[2])
            .and_then(|state| u16::try_from(state).ok())
        else {
            return;
        };
        let Some((name, _)) = ferrite_data::blocks::state(state) else {
            return;
        };
        if !ferrite_data::tags::contains("block", "enderman_holdable", name) {
            return;
        }
        let Some(carried) = ferrite_data::blocks::default_state(name) else {
            return;
        };
        if !self.entity_change_allowed(ctx, key, at, name, "minecraft:air") {
            return;
        }
        self.place_blocks(ctx, &[(at, 0)]);
        if let Some(mob) = mob::state_mut(&mut self.entities, key) {
            mob.carried = Some(carried);
        }
        ferrite_entity::enderman::sync(&mut self.entities, key);
    }

    /// `EndermanLeaveBlockGoal`: place a carried default state only on an air cell above a full,
    /// non-bedrock support, where the state survives and no other entity occupies the cube.
    fn place_enderman_block(
        &mut self,
        ctx: &TickContext,
        key: ferrite_entity::EntityKey,
        at: [i32; 3],
        state: u16,
    ) {
        if !Self::mob_griefing(ctx) {
            return;
        }
        if !self
            .held_chunks
            .contains_key(&ChunkPos(at[0].div_euclid(16), at[2].div_euclid(16)))
        {
            return;
        }
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let Some(current) = source
            .block(at[0], at[1], at[2])
            .and_then(|state| u16::try_from(state).ok())
        else {
            return;
        };
        if !ferrite_data::block_props::get(current).is_air {
            return;
        }
        let Some(below) = source
            .block(at[0], at[1] - 1, at[2])
            .and_then(|state| u16::try_from(state).ok())
        else {
            return;
        };
        let Some((below_name, _)) = ferrite_data::blocks::state(below) else {
            return;
        };
        if below_name == "minecraft:bedrock"
            || !ferrite_data::block_props::get(below).full_collision
        {
            return;
        }
        let world = super::fluid::RegionFluidWorld::new(source, &self.held_chunks, &ctx.tracking);
        if !super::neighbors::can_survive(state, at, &world) {
            return;
        }
        let area = ferrite_math::AABB::new(
            f64::from(at[0]),
            f64::from(at[1]),
            f64::from(at[2]),
            f64::from(at[0]) + 1.0,
            f64::from(at[1]) + 1.0,
            f64::from(at[2]) + 1.0,
        );
        if self
            .entities
            .query_vec(area)
            .iter()
            .any(|&other| other != key)
        {
            return;
        }
        let Some((name, _)) = ferrite_data::blocks::state(state) else {
            return;
        };
        if !self.entity_change_allowed(ctx, key, at, "minecraft:air", name) {
            return;
        }
        self.place_blocks(ctx, &[(at, state)]);
        if let Some(mob) = mob::state_mut(&mut self.entities, key) {
            mob.carried = None;
        }
        ferrite_entity::enderman::sync(&mut self.entities, key);
    }

    /// The world half of shearing: `None` when `key` is not a sheep, `Some(false)` when it is a
    /// lamb or already sheared (the click is consumed), `Some(true)` when it was sheared and its
    /// wool was dropped.
    fn shear_sheep(&mut self, key: ferrite_entity::EntityKey) -> Option<bool> {
        let state = mob::state(&self.entities, key).copied()?;
        if state.kind != MobKind::Sheep {
            return None;
        }
        let facts = ferrite_entity::sheep::loot_properties(&state);
        if ferrite_entity::sheep::shear(&mut self.entities, key).is_none() {
            return Some(false);
        }
        let pos = self.entities.pos(key)?;
        for stack in super::drops::shearing_drops(&facts, &mut self.rng) {
            // `Sheep.shear` spawns each wool on its own, a block above the feet, with a little
            // extra push on top of the item's own random toss.
            for _ in 0..stack.count {
                let mut one = stack.clone();
                one.count = 1;
                let vel = ferrite_math::Vec3::new(
                    f64::from(self.rng.next_f32() * 0.2 - 0.1)
                        + f64::from((self.rng.next_f32() - self.rng.next_f32()) * 0.1),
                    0.2 + f64::from(self.rng.next_f32() * 0.05),
                    f64::from(self.rng.next_f32() * 0.2 - 0.1)
                        + f64::from((self.rng.next_f32() - self.rng.next_f32()) * 0.1),
                );
                let at = ferrite_math::Vec3::new(pos.x, pos.y + 1.0, pos.z);
                let record = item::new_item_stack(at, vel, &one, &mut self.rng);
                self.entities.insert(record);
            }
        }
        Some(true)
    }

    /// `EnderMan.teleport`: the old position's teleport sound and 32 portal particles.
    fn play_enderman_teleport(
        &self,
        ctx: &TickContext,
        from: ferrite_math::Vec3,
        _to: ferrite_math::Vec3,
    ) {
        self.play_sound_near_with_source(
            from,
            "minecraft:entity.enderman.teleport",
            1.0,
            1.0,
            5, // SoundSource.HOSTILE
        );
        let Some(particle) =
            ferrite_data::registries::id("minecraft:particle_type", "minecraft:portal")
        else {
            return;
        };
        let update = packet(&LevelParticles {
            particle: ferrite_protocol::types::ParticleOptions::new(particle),
            long_distance: false,
            always_show: false,
            x: from.x,
            y: from.y + 1.0,
            z: from.z,
            velocity_x: 0.5,
            velocity_y: 0.5,
            velocity_z: 0.5,
            speed: 0.0,
            speed_y: 0.0,
            speed_z: 0.0,
            count: VarInt(32),
            randomization: VarInt(0),
        });
        let chunk = (
            (from.x.floor() as i32).div_euclid(16),
            (from.z.floor() as i32).div_euclid(16),
        );
        let tracking = ctx.tracking.lock().unwrap();
        broadcast_tracked_chunk(&tracking, ctx.world_id, chunk, &update);
    }

    /// Plays a sound event at `pos` for every player of this region within 16 blocks.
    fn play_sound_near(&self, pos: ferrite_math::Vec3, name: &str, volume: f32, pitch: f32) {
        self.play_sound_near_with_source(pos, name, volume, pitch, 7); // SoundSource.PLAYERS
    }

    /// [`play_sound_near`] with an explicit `SoundSource` id.
    fn play_sound_near_with_source(
        &self,
        pos: ferrite_math::Vec3,
        name: &str,
        volume: f32,
        pitch: f32,
        source: i32,
    ) {
        let Some(sound) = ferrite_data::registries::id("minecraft:sound_event", name) else {
            return;
        };
        for p in self.players.values() {
            let dx = p.pos[0] - pos.x;
            let dz = p.pos[2] - pos.z;
            if dx * dx + dz * dz > 256.0 {
                continue;
            }
            send(
                p,
                &SoundEffect {
                    sound: super::commands_extras::sound_holder(sound),
                    source: VarInt(source),
                    x: (pos.x * 8.0) as i32,
                    y: (pos.y * 8.0) as i32,
                    z: (pos.z * 8.0) as i32,
                    volume,
                    pitch,
                    seed: 0,
                },
            );
        }
    }

    /// Lifts a tamed parrot from the entity store onto the player's first free shoulder. The
    /// entity leaves the store (vanilla discards it) and its NBT is written immediately, so a
    /// disconnect right after the pickup cannot lose it.
    fn capture_shoulder(&mut self, player: Uuid, key: ferrite_entity::EntityKey) -> bool {
        let Some(data) = self.entities.data(key) else {
            return false;
        };
        let Some(parrot) = ShoulderParrot::from_data(data, player) else {
            return false;
        };
        let Some(p) = self.players.get(&player) else {
            return false;
        };
        let Some(side) = super::ride::shoulder_put(player, parrot) else {
            return false;
        };
        self.entities.remove(key);
        self.save_shoulders_for(p);
        self.send_shoulder(p, side, Some(parrot.variant));
        true
    }

    /// `Player#setShoulderEntityLeft/Right` and the release methods, applied to the (detached) player the
    /// region is ticking. See [`ShoulderOp`]: seating lifts a live parrot out of the world (a uuid with no
    /// entity becomes a fresh shoulder parrot), a parrot already on the other shoulder moves over, one that was
    /// on `side` is released into the world; `Clear` empties the shoulder without a parrot; `Release` puts the
    /// parrot into the world. The change is saved at once and announced to the player and the viewers. `false`
    /// when nothing changed (an empty shoulder, or an entity that is not a parrot).
    fn apply_shoulder(
        &mut self,
        p: &Player,
        side: ShoulderSide,
        op: crate::game::plugin_types::ShoulderOp,
    ) -> bool {
        use crate::game::plugin_types::ShoulderOp;
        let at = ferrite_math::Vec3::new(p.pos[0], p.pos[1] + 0.05, p.pos[2]);
        let changes = match op {
            ShoulderOp::Seat(parrot) => self.change_shoulder(p.uuid, at, side, Some(parrot)),
            ShoulderOp::Release => self.change_shoulder(p.uuid, at, side, None),
            ShoulderOp::Clear => match super::ride::shoulder_release(p.uuid, side) {
                Some(_) => vec![(side, None)],
                None => Vec::new(),
            },
        };
        for (side, variant) in &changes {
            self.send_shoulder(p, *side, *variant);
        }
        if !changes.is_empty() {
            self.save_shoulders_for(p);
        }
        !changes.is_empty()
    }

    /// [`Self::apply_shoulder`] for a player still in the region's table (tests).
    #[cfg(test)]
    pub fn set_shoulder(&mut self, player: Uuid, side: ShoulderSide, parrot: Option<Uuid>) -> bool {
        let Some(p) = self.players.remove(&player) else {
            return false;
        };
        let op = parrot.map_or(
            crate::game::plugin_types::ShoulderOp::Release,
            crate::game::plugin_types::ShoulderOp::Seat,
        );
        let changed = self.apply_shoulder(&p, side, op);
        self.players.insert(player, p);
        changed
    }

    /// Moves parrots between the entity store and the player's shoulders. Returns what to
    /// announce, as `(side, variant)` with `None` for an empty shoulder.
    fn change_shoulder(
        &mut self,
        player: Uuid,
        at: ferrite_math::Vec3,
        side: ShoulderSide,
        parrot: Option<Uuid>,
    ) -> Vec<(ShoulderSide, Option<i32>)> {
        let mut changes = Vec::new();
        let Some(uuid) = parrot else {
            if let Some(old) = super::ride::shoulder_release(player, side) {
                self.entities.insert(old.to_record(at, &mut self.rng));
                changes.push((side, None));
            }
            return changes;
        };
        let live = self.entities.keys().iter().copied().find(|key| {
            self.entities
                .data(*key)
                .is_some_and(|data| data.uuid == uuid)
        });
        let other = match side {
            ShoulderSide::Left => ShoulderSide::Right,
            ShoulderSide::Right => ShoulderSide::Left,
        };
        // A parrot already on the other shoulder keeps its variant and owner when it moves over.
        let moving = super::ride::shoulders_of(player)
            .get(other)
            .filter(|sitting| sitting.uuid == uuid);
        let seated = match live.filter(|_| moving.is_none()) {
            Some(key) => {
                let state = self.entities.data(key).and_then(|data| data.mob);
                let Some(state) = state.filter(|s| s.kind == MobKind::Parrot && !s.is_dead())
                else {
                    return changes;
                };
                self.entities.remove(key);
                ShoulderParrot {
                    uuid,
                    variant: 0,
                    owner: state.owner,
                }
            }
            None => moving.unwrap_or(ShoulderParrot {
                uuid,
                variant: 0,
                owner: None,
            }),
        };
        if moving.is_some() {
            super::ride::shoulder_release(player, other);
            changes.push((other, None));
        }
        if let Some(old) = super::ride::shoulder_set(player, side, Some(seated))
            && old.uuid != uuid
        {
            self.entities.insert(old.to_record(at, &mut self.rng));
        }
        changes.push((side, Some(seated.variant)));
        changes
    }

    /// Writes the player's current shoulders next to their player data.
    fn save_shoulders_for(&self, player: &Player) {
        super::entity_save::save_shoulders(
            player.playerdata.as_deref(),
            player.uuid,
            &super::ride::shoulders_of(player.uuid),
        );
    }

    /// Tells this player and the other same-dimension viewers about one shoulder. The packet is
    /// `set_entity_data` (101) with player metadata index 19/20 and the 26.3
    /// `OPTIONAL_VAR_INT` serializer (19). It is built here because the shared protocol crate
    /// does not model player shoulder metadata yet.
    fn send_shoulder(&self, player: &Player, side: ShoulderSide, variant: Option<i32>) {
        let bytes = shoulder_metadata_bytes(player.entity, side, variant);
        let _ = player.outbound.send(Outbound::Packet(bytes.clone().into()));
        broadcast_shoulder_bytes(&player.tracking, player.world_id, player.uuid, &bytes);
    }

    /// Announces both shoulders when a player logs in with saved parrots.
    fn announce_shoulders(&self, player: &Player) {
        for (side, parrot) in super::ride::shoulders_of(player.uuid).iter() {
            self.send_shoulder(player, side, Some(parrot.variant));
        }
    }

    fn release_shoulder(&mut self, player: &Player, side: ShoulderSide) -> bool {
        let Some(parrot) = super::ride::shoulder_release(player.uuid, side) else {
            return false;
        };
        let pos = ferrite_math::Vec3::new(player.pos[0], player.pos[1] + 0.05, player.pos[2]);
        self.entities.insert(parrot.to_record(pos, &mut self.rng));
        self.send_shoulder(player, side, None);
        true
    }

    /// Releases both shoulders (death or sneak-jump).
    fn release_shoulders(&mut self, player: &Player) {
        let mut released = false;
        for side in [ShoulderSide::Left, ShoulderSide::Right] {
            released |= self.release_shoulder(player, side);
        }
        if released {
            self.save_shoulders_for(player);
        }
    }

    /// Pushes players inside a wind charge's radius away from the impact and deals one damage.
    /// Mobs are pushed by [`ferrite_entity::arrow::wind_burst`].
    fn wind_charge_burst(
        &mut self,
        center: ferrite_math::Vec3,
        owner: Option<Uuid>,
        players: &[(Uuid, ferrite_math::AABB)],
    ) {
        ferrite_entity::arrow::wind_burst(&mut self.entities, center);
        let radius = arrow::WIND_CHARGE_RADIUS;
        for (uuid, box_) in players {
            if owner == Some(*uuid) {
                continue;
            }
            let at = box_.center();
            let dx = at.x - center.x;
            let dy = at.y - center.y;
            let dz = at.z - center.z;
            if dx * dx + dy * dy + dz * dz > radius * radius {
                continue;
            }
            let length = (dx * dx + dy * dy + dz * dz).sqrt().max(1.0e-4);
            let push = arrow::WIND_CHARGE_PUSH;
            if let Some(p) = self.players.get_mut(uuid) {
                p.hurt(DamageKind::Generic, arrow::WIND_CHARGE_DAMAGE);
                send(
                    p,
                    &SetEntityMotion {
                        id: VarInt(p.entity),
                        velocity: ferrite_protocol::types::LpVec3([
                            dx / length * push,
                            (dy / length).max(0.2) * push,
                            dz / length * push,
                        ]),
                    },
                );
            }
        }
    }

    /// A right-click on a horse: a saddle goes on a tame horse; anything else mounts it.
    /// Returns whether the click was used up.
    /// Opens the inventory screen of a tame horse, donkey, mule or llama for `player`.
    fn open_mount(&mut self, player: Uuid, key: ferrite_entity::EntityKey) {
        let (Some(data), Some(state)) = (
            self.entities.data(key),
            ferrite_entity::mob::state(&self.entities, key),
        ) else {
            return;
        };
        if state.is_dead() {
            return;
        }
        let (uuid, id) = (data.uuid, data.id);
        let columns = ferrite_entity::horse::chest_columns(&self.entities, key);
        let saddle = if state.saddled {
            ferrite_item::ItemStack::from_name("minecraft:saddle").unwrap_or_default()
        } else {
            ferrite_item::ItemStack::empty()
        };
        let armor = if state.armor != 0 {
            ferrite_item::ItemStack::with_count(state.armor, 1)
        } else {
            ferrite_item::ItemStack::empty()
        };
        // Trader llamas wear carpets too; only horses use the horse-armour rule.
        let llama = state.kind.is_llama();
        if let Some(p) = self.players.get_mut(&player) {
            p.open_mount(uuid, id, columns, llama, saddle, armor);
        }
    }

    fn use_horse(
        &mut self,
        ctx: &TickContext,
        uuid: Uuid,
        key: ferrite_entity::EntityKey,
        held: &str,
        slot: usize,
        gamemode: i32,
    ) -> bool {
        let item = ferrite_item::item_id(held);
        let wear = item.is_some_and(|id| {
            ferrite_entity::horse::wears(&self.entities, key, held)
                && ferrite_entity::horse::wear_armor(
                    &mut self.entities,
                    key,
                    id,
                    ferrite_entity::horse::armor_points(held),
                )
        });
        if wear || ferrite_entity::horse::treat(&mut self.entities, key, held) {
            if gamemode != 1
                && let Some(p) = self.players.get_mut(&uuid)
            {
                p.inventory.remove(slot, 1);
                send(
                    p,
                    &SetPlayerInventory {
                        slot: VarInt(slot as i32),
                        item: p.inventory.get(slot).clone(),
                    },
                );
            }
            self.send_saddle(ctx, key);
            return true;
        }
        if held == "minecraft:chest" && ferrite_entity::horse::add_chest(&mut self.entities, key) {
            if gamemode != 1
                && let Some(p) = self.players.get_mut(&uuid)
            {
                p.inventory.remove(slot, 1);
                send(
                    p,
                    &SetPlayerInventory {
                        slot: VarInt(slot as i32),
                        item: p.inventory.get(slot).clone(),
                    },
                );
            }
            return true;
        }
        if held == "minecraft:saddle" && ferrite_entity::horse::saddle(&mut self.entities, key) {
            if gamemode != 1
                && let Some(p) = self.players.get_mut(&uuid)
            {
                p.inventory.remove(slot, 1);
                send(
                    p,
                    &SetPlayerInventory {
                        slot: VarInt(slot as i32),
                        item: p.inventory.get(slot).clone(),
                    },
                );
            }
            self.send_saddle(ctx, key);
            return true;
        }
        let Some(horse) = self.entities.data(key).map(|d| d.uuid) else {
            return false;
        };
        if super::ride::mount(uuid, horse, false).is_err() {
            return false;
        }
        let steerable = ferrite_entity::horse::is_steerable(&self.entities, key);
        super::ride::set_controlled(horse, steerable);
        if let Some(state) = ferrite_entity::mob::state_mut(&mut self.entities, key) {
            state.no_ai = true;
        }
        self.send_saddle(ctx, key);
        self.send_stats(ctx, key);
        self.send_passengers(ctx, key);
        self.record_started_riding(uuid, key);
        true
    }

    fn record_started_riding(&self, rider: Uuid, vehicle_key: ferrite_entity::EntityKey) {
        let Some(data) = self.entities.data(vehicle_key) else {
            return;
        };
        let vehicle_kind = ferrite_data::entity_props::get(data.kind).key.to_owned();
        let passengers = super::ride::passengers_of(data.uuid)
            .into_iter()
            .filter_map(|p| {
                if self.players.contains_key(&p) || p == rider {
                    Some("minecraft:player".to_owned())
                } else {
                    self.entities.keys().iter().find_map(|&k| {
                        let d = self.entities.data(k)?;
                        (d.uuid == p)
                            .then(|| ferrite_data::entity_props::get(d.kind).key.to_owned())
                    })
                }
            })
            .collect();
        super::recipe_unlock::push_event(
            rider,
            super::recipe_unlock::Event::StartedRiding {
                vehicle: vehicle_kind,
                passengers,
            },
        );
    }

    /// Vanilla `Pig.mobInteract`: a saddle equips an adult unsaddled pig; otherwise a saddled
    /// adult pig seats the player (unless holding pig food to breed it).
    fn use_pig(
        &mut self,
        ctx: &TickContext,
        uuid: Uuid,
        key: ferrite_entity::EntityKey,
        held: &str,
        slot: usize,
        gamemode: i32,
    ) -> bool {
        let Some(state) = ferrite_entity::mob::state(&self.entities, key).copied() else {
            return false;
        };
        if state.age < 0 {
            return false;
        }
        if held == "minecraft:saddle"
            && ferrite_entity::pig::set_saddled(&mut self.entities, key, true)
        {
            if gamemode != 1
                && let Some(p) = self.players.get_mut(&uuid)
            {
                p.inventory.remove(slot, 1);
                send(
                    p,
                    &SetPlayerInventory {
                        slot: VarInt(slot as i32),
                        item: p.inventory.get(slot).clone(),
                    },
                );
            }
            self.send_saddle(ctx, key);
            return true;
        }
        if !state.saddled || ferrite_entity::breed::is_food(ferrite_entity::mob::MobKind::Pig, held)
        {
            return false;
        }
        let Some(pig) = self.entities.data(key).map(|d| d.uuid) else {
            return false;
        };
        if super::ride::mount(uuid, pig, false).is_err() {
            return false;
        }
        self.send_saddle(ctx, key);
        self.send_passengers(ctx, key);
        self.record_started_riding(uuid, key);
        true
    }

    /// Sends the horse's inherited speed and jump strength, which the riding client steers with.
    fn send_stats(&self, ctx: &TickContext, key: ferrite_entity::EntityKey) {
        let (Some(data), Some(state)) = (
            self.entities.data(key),
            ferrite_entity::mob::state(&self.entities, key),
        ) else {
            return;
        };
        if state.speed <= 0.0 {
            return;
        }
        let attribute = |name: &str, base: f32| {
            ferrite_data::registries::id("minecraft:attribute", name).map(|id| AttributeSnapshot {
                attribute: VarInt(id),
                base: f64::from(base),
                modifiers: Vec::new(),
            })
        };
        let attributes: Vec<_> = [
            attribute("minecraft:movement_speed", state.speed),
            attribute("minecraft:jump_strength", state.jump),
        ]
        .into_iter()
        .flatten()
        .collect();
        let players = &self.players;
        tracker::broadcast(
            &self.entities,
            key,
            &UpdateAttributes {
                entity_id: VarInt(data.id),
                attributes,
            },
            &mut |viewer, bytes| deliver(players, ctx, viewer, bytes),
        );
    }

    /// Tells everyone tracking the horse what it wears: the saddle (the client only lets the
    /// rider steer a saddled horse) and its armour.
    fn send_saddle(&self, ctx: &TickContext, key: ferrite_entity::EntityKey) {
        let Some(id) = self.entities.data(key).map(|d| d.id) else {
            return;
        };
        let Some(state) = ferrite_entity::mob::state(&self.entities, key) else {
            return;
        };
        let saddle = if state.saddled {
            ferrite_item::ItemStack::from_name("minecraft:saddle").unwrap_or_default()
        } else {
            ferrite_item::ItemStack::empty()
        };
        let armor = if state.armor != 0 {
            ferrite_item::ItemStack::with_count(state.armor, 1)
        } else {
            ferrite_item::ItemStack::empty()
        };
        let slots = vec![
            (ferrite_entity::horse::SADDLE_SLOT, saddle),
            (ferrite_entity::horse::BODY_SLOT, armor),
        ];
        let players = &self.players;
        tracker::broadcast(
            &self.entities,
            key,
            &SetEquipment {
                entity_id: VarInt(id),
                slots,
            },
            &mut |viewer, bytes| deliver(players, ctx, viewer, bytes),
        );
    }

    /// Sends the horse's current rider list (empty after a dismount).
    fn send_passengers(&self, ctx: &TickContext, key: ferrite_entity::EntityKey) {
        let Some(data) = self.entities.data(key) else {
            return;
        };
        let passengers = super::ride::passengers_of(data.uuid)
            .into_iter()
            .filter_map(|rider| self.players.get(&rider).map(|p| VarInt(p.entity)))
            .collect();
        let players = &self.players;
        tracker::broadcast(
            &self.entities,
            key,
            &SetPassengers {
                vehicle: VarInt(data.id),
                passengers,
            },
            &mut |viewer, bytes| deliver(players, ctx, viewer, bytes),
        );
    }

    /// Moves ridden horses to where their riders steered them, and lets wild horses buck or
    /// accept their riders.
    fn tick_horses(&mut self, ctx: &TickContext) {
        let askers: Vec<Uuid> = self
            .players
            .iter()
            .filter(|(_, p)| p.open_animal_chest)
            .map(|(uuid, _)| *uuid)
            .collect();
        for rider in askers {
            if let Some(p) = self.players.get_mut(&rider) {
                p.open_animal_chest = false;
            }
            let Some(vehicle) = super::ride::vehicle_of(rider) else {
                continue;
            };
            let Some(key) = self.entities.keys().iter().copied().find(|&k| {
                self.entities.data(k).is_some_and(|d| d.uuid == vehicle)
                    && ferrite_entity::mob::state(&self.entities, k)
                        .is_some_and(|s| s.kind.is_equine() && s.owner.is_some())
            }) else {
                continue;
            };
            self.open_mount(rider, key);
        }
        self.spin_attacks();
        for p in self.players.values_mut() {
            if p.spin_ticks > 0 {
                p.spin_ticks -= 1;
                if p.spin_ticks == 0 {
                    p.spin_damage = 0.0;
                    p.spin_hit.clear();
                    p.broadcast_spin(false);
                }
            }
        }
        // What a player moved in an open mount screen reaches the animal. Gear the animal
        // cannot wear is handed back to the player instead of vanishing on close.
        let viewers: Vec<(Uuid, Uuid, ferrite_item::ItemStack, ferrite_item::ItemStack)> = self
            .players
            .iter()
            .filter_map(|(&player, p)| {
                p.mount_gear()
                    .map(|(animal, saddle, armor)| (player, animal, saddle, armor))
            })
            .collect();
        for (player, animal, saddle, armor) in viewers {
            let Some(key) = self.entities.keys().iter().copied().find(|&k| {
                self.entities.data(k).is_some_and(|d| d.uuid == animal)
                    && mob::state(&self.entities, k).is_some_and(|s| !s.is_dead())
            }) else {
                // The animal died or left the region: vanilla's `stillValid` closes the
                // screen instead of leaving a copy of its gear to take.
                if let Some(p) = self.players.get_mut(&player) {
                    if let Some(window) = p.menus.open.as_ref().map(|menu| menu.container_id()) {
                        send(
                            p,
                            &ClientboundContainerClose {
                                window: VarInt(window),
                            },
                        );
                    }
                    p.close_container();
                }
                continue;
            };
            let Some(kind) = mob::state(&self.entities, key).map(|s| s.kind) else {
                continue;
            };
            let mut saddle = saddle;
            let mut armor = armor;
            let bad_saddle = !saddle.is_empty() && saddle.name() != Some("minecraft:saddle");
            let bad_armor =
                !armor.is_empty() && !ferrite_entity::horse::body_armor_fits(kind, armor.item);
            if bad_saddle {
                let rejected = std::mem::take(&mut saddle);
                if let Some(p) = self.players.get_mut(&player) {
                    if let Some(menu) = p.menus.open.as_mut()
                        && let Some(slot) = menu.container_mut().first_mut()
                    {
                        *slot = ferrite_item::ItemStack::empty();
                    }
                    p.menus.thrown.push(rejected);
                }
            }
            if bad_armor {
                let rejected = std::mem::take(&mut armor);
                if let Some(p) = self.players.get_mut(&player) {
                    if let Some(menu) = p.menus.open.as_mut()
                        && let Some(slot) = menu.container_mut().get_mut(1)
                    {
                        *slot = ferrite_item::ItemStack::empty();
                    }
                    p.menus.thrown.push(rejected);
                }
            }
            let name = armor.name().unwrap_or("");
            if ferrite_entity::horse::set_gear(
                &mut self.entities,
                key,
                !saddle.is_empty(),
                if armor.is_empty() { 0 } else { armor.item },
                ferrite_entity::horse::armor_points(name),
            ) {
                self.send_saddle(ctx, key);
            }
        }
        let riders: Vec<Uuid> = self.players.keys().copied().collect();
        for rider in riders {
            let moves = self
                .players
                .get_mut(&rider)
                .map(|p| std::mem::take(&mut p.vehicle_moves))
                .unwrap_or_default();
            let (Some(last), Some(vehicle)) = (moves.last(), super::ride::vehicle_of(rider)) else {
                continue;
            };
            let Some(key) = self
                .entities
                .keys()
                .iter()
                .copied()
                .find(|&k| self.entities.data(k).is_some_and(|d| d.uuid == vehicle))
            else {
                continue;
            };
            if ferrite_entity::horse::is_steerable(&self.entities, key)
                || ferrite_entity::mob::state(&self.entities, key).is_none()
            {
                self.entities
                    .set_pos(key, ferrite_math::Vec3::new(last[0], last[1], last[2]));
                if let Some(data) = self.entities.data_mut(key) {
                    data.yaw = last[3] as f32;
                }
            }
        }
        let horses: Vec<ferrite_entity::EntityKey> = self
            .entities
            .keys()
            .iter()
            .copied()
            .filter(|&k| {
                ferrite_entity::mob::state(&self.entities, k).is_some_and(|s| s.kind.is_equine())
            })
            .collect();
        for key in horses {
            let Some(horse) = self.entities.data(key).map(|d| d.uuid) else {
                continue;
            };
            let rider = super::ride::passengers_of(horse).into_iter().next();
            let Some(rider) = rider else {
                if ferrite_entity::mob::state(&self.entities, key).is_some_and(|s| s.no_ai) {
                    super::ride::set_controlled(horse, false);
                    if let Some(state) = ferrite_entity::mob::state_mut(&mut self.entities, key) {
                        state.no_ai = false;
                    }
                }
                continue;
            };
            // A saddled tame horse ridden by a player (also one put there with `/ride`) is
            // steered by the rider's packets, not by its own AI or by the rider following it.
            // A rider that is a mob of this region cannot steer.
            let rider_is_mob = self
                .entities
                .keys()
                .iter()
                .any(|&k| self.entities.data(k).is_some_and(|d| d.uuid == rider));
            if !rider_is_mob
                && ferrite_entity::horse::is_steerable(&self.entities, key)
                && !super::ride::is_controlled(horse)
            {
                super::ride::set_controlled(horse, true);
                if let Some(state) = ferrite_entity::mob::state_mut(&mut self.entities, key) {
                    state.no_ai = true;
                }
            }
            let outcome = ferrite_entity::horse::tick_rider(
                &mut self.entities,
                key,
                Some(rider),
                &mut self.rng,
            );
            if outcome != ferrite_entity::horse::Bucked::No {
                super::ride::dismount(rider);
                super::ride::set_controlled(horse, false);
                if let Some(state) = ferrite_entity::mob::state_mut(&mut self.entities, key) {
                    state.no_ai = false;
                }
                self.send_passengers(ctx, key);
                // Entity events: 7 hearts when tamed, 6 smoke when the horse threw its rider.
                self.broadcast_status(
                    ctx,
                    key,
                    if outcome == ferrite_entity::horse::Bucked::Tamed {
                        7
                    } else {
                        6
                    },
                );
            }
        }
        let blocks = TickBlocks {
            source: ctx.source.as_deref(),
            seen: Default::default(),
        };
        let pigs: Vec<ferrite_entity::EntityKey> = self
            .entities
            .keys()
            .iter()
            .copied()
            .filter(|&k| {
                ferrite_entity::mob::state(&self.entities, k)
                    .is_some_and(|s| s.kind == ferrite_entity::mob::MobKind::Pig && !s.is_dead())
            })
            .collect();
        for key in pigs {
            let Some(pig_uuid) = self.entities.data(key).map(|d| d.uuid) else {
                continue;
            };
            let rider = super::ride::passengers_of(pig_uuid).into_iter().next();
            let Some(rider) = rider else {
                if ferrite_entity::mob::state(&self.entities, key).is_some_and(|s| s.no_ai)
                    && super::ride::is_controlled(pig_uuid)
                {
                    super::ride::set_controlled(pig_uuid, false);
                    if let Some(state) = ferrite_entity::mob::state_mut(&mut self.entities, key) {
                        state.no_ai = false;
                    }
                }
                continue;
            };
            let Some(p) = self.players.get(&rider) else {
                continue;
            };
            let saddled =
                ferrite_entity::mob::state(&self.entities, key).is_some_and(|s| s.saddled);
            let main_stick =
                p.inventory.get(p.hand_slot(0)).name() == Some("minecraft:carrot_on_a_stick");
            let off_stick =
                p.inventory.get(p.hand_slot(1)).name() == Some("minecraft:carrot_on_a_stick");
            if saddled && (main_stick || off_stick) {
                let rider_yaw = p.yaw;
                let boost_active = self
                    .entities
                    .data(key)
                    .and_then(|d| d.metadata.get(ferrite_entity::pig::BOOST_TIME_INDEX))
                    .is_some_and(|v| matches!(v, MetaValue::Int(t) if *t > 0));
                let steer = ferrite_entity::pig::steer_velocity(true, rider_yaw, boost_active);
                if let Some(state) = ferrite_entity::mob::state_mut(&mut self.entities, key) {
                    state.no_ai = true;
                }
                super::ride::set_controlled(pig_uuid, true);
                let mut vel = self.entities.vel(key).unwrap_or_default();
                vel.x = steer.x;
                vel.z = steer.z;
                self.entities.set_vel(key, vel);
                ferrite_entity::physics::move_entity(&mut self.entities, key, vel, &blocks);
                if let Some(data) = self.entities.data_mut(key) {
                    data.yaw = rider_yaw;
                    data.head_yaw = rider_yaw;
                }
            } else if super::ride::is_controlled(pig_uuid) {
                super::ride::set_controlled(pig_uuid, false);
                if let Some(state) = ferrite_entity::mob::state_mut(&mut self.entities, key) {
                    state.no_ai = false;
                }
            }
        }
    }

    fn resolve_attacks(&mut self, ctx: &TickContext) -> Vec<(Uuid, ferrite_entity::EntityKey)> {
        let mut owner_attacks = Vec::new();
        for uuid in std::mem::take(&mut self.pending_hurts) {
            let key = self.entities.keys().iter().copied().find(|key| {
                self.entities
                    .data(*key)
                    .is_some_and(|data| data.uuid == uuid)
            });
            let Some(key) = key else { continue };
            let Some(data) = self.entities.data(key) else {
                continue;
            };
            let (id, yaw) = (VarInt(data.id), data.yaw);
            let players = &self.players;
            tracker::broadcast(
                &self.entities,
                key,
                &HurtAnimation { id, yaw },
                &mut |viewer, bytes| deliver(players, ctx, viewer, bytes),
            );
        }
        for (uuid, action) in std::mem::take(&mut self.pending_swings) {
            let key = self.entities.keys().iter().copied().find(|key| {
                self.entities
                    .data(*key)
                    .is_some_and(|data| data.uuid == uuid)
            });
            let Some(key) = key else { continue };
            let Some(id) = self.entities.data(key).map(|data| VarInt(data.id)) else {
                continue;
            };
            let players = &self.players;
            tracker::broadcast(
                &self.entities,
                key,
                &Animate { id, action },
                &mut |viewer, bytes| deliver(players, ctx, viewer, bytes),
            );
        }
        struct Pending {
            uuid: Uuid,
            attacks: Vec<i32>,
            ticks: i32,
            bow: Option<(f32, f32, f32)>,
            trident: bool,
        }
        let mut pending = Vec::new();
        for p in self.players.values_mut() {
            let pick_ids = std::mem::take(&mut p.pick_entities);
            if !pick_ids.is_empty() {
                let picked_stacks: Vec<ferrite_item::ItemStack> = pick_ids
                    .into_iter()
                    .filter_map(|net_id| {
                        let key = self.entities.key_of(net_id)?;
                        let data = self.entities.data(key)?;
                        let props = ferrite_data::entity_props::get(data.kind);
                        let short = props.key.strip_prefix("minecraft:").unwrap_or(props.key);
                        let egg_name = format!("minecraft:{short}_spawn_egg");
                        let item_id = ferrite_item::item_id(&egg_name).or_else(|| match short {
                            "painting" => ferrite_item::item_id("minecraft:painting"),
                            "item_frame" => ferrite_item::item_id("minecraft:item_frame"),
                            "glow_item_frame" => ferrite_item::item_id("minecraft:glow_item_frame"),
                            "armor_stand" => ferrite_item::item_id("minecraft:armor_stand"),
                            "end_crystal" => ferrite_item::item_id("minecraft:end_crystal"),
                            _ => None,
                        })?;
                        Some(ferrite_item::ItemStack::with_count(item_id, 1))
                    })
                    .collect();
                for stack in picked_stacks {
                    p.give_picked_item(stack);
                }
            }
        }
        for (&uuid, p) in self.players.iter_mut() {
            let attacks = std::mem::take(&mut p.attacks);
            let released = std::mem::take(&mut p.bow_released);
            if attacks.is_empty() && !released {
                continue;
            }
            let trident = released && p.trident_draw;
            let bow = released.then(|| {
                let charge = (p.bow_charge as f32 / 20.0).clamp(0.0, 1.0);
                (charge, p.yaw, p.pitch)
            });
            // Every swing resets the attack-strength ticker, even when nothing is hit.
            let ticks = p.attack_ticks;
            p.attack_ticks = 0;
            pending.push(Pending {
                uuid,
                attacks,
                ticks,
                bow,
                trident,
            });
        }
        for Pending {
            uuid,
            attacks,
            ticks,
            bow,
            trident,
        } in pending
        {
            if let Some((charge, yaw, pitch)) = bow {
                if trident {
                    if charge * 20.0 >= arrow::TRIDENT_MIN_DRAW as f32 {
                        let wet = ctx
                            .state
                            .lock()
                            .is_ok_and(|s| s.weather != super::commands::Weather::Clear);
                        self.throw_trident(uuid, yaw, pitch, wet);
                    }
                } else if charge >= 0.1 {
                    self.fire_bow(uuid, charge, yaw, pitch);
                }
            }
            if attacks.is_empty() {
                continue;
            }
            let Some(p) = self.players.get(&uuid) else {
                continue;
            };
            if p.survival.dead || p.gamemode == 3 {
                continue;
            }
            let stack = p.inventory.selected_item().clone();
            let stats = combat::weapon_stats(&stack);
            let strength = combat::attack_strength(ticks, stats.attack_speed);
            let sword = combat::is_sword(&stack);
            let mut damage = stats.attack_damage * combat::damage_scale(strength);
            let critical =
                combat::can_critical(p.ground, p.survival.fall_distance, p.survival.sprinting);
            if critical {
                damage *= combat::CRITICAL_MULTIPLIER;
            }
            let feet = ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]);
            let eye = combat::eye_position(feet);
            let look = combat::look_vector(p.yaw, p.pitch);
            let range = if p.gamemode == 1 {
                combat::CREATIVE_ENTITY_RANGE
            } else {
                combat::ENTITY_RANGE
            };
            let mut hurt_keys = Vec::new();
            let mut crystals = Vec::new();
            for id in attacks {
                let Some(key) = self.entities.key_of(id) else {
                    continue;
                };
                let (Some(bbox), Some(target)) = (self.entities.bbox(key), self.entities.pos(key))
                else {
                    continue;
                };
                if !combat::within_reach(eye, bbox, range) || !combat::facing(eye, look, target) {
                    continue;
                }
                let mut hit_damage = damage as f32;
                if let Some(dragon) = self
                    .entities
                    .data(key)
                    .and_then(|data| data.mob)
                    .filter(|mob| mob.kind == mob::MobKind::EnderDragon)
                {
                    let yaw = self.entities.data(key).map_or(0.0, |data| data.yaw);
                    let Some(part) = ferrite_entity::dragon::part_boxes_with_flight(
                        target,
                        yaw,
                        &dragon.dragon_flight,
                        dragon.dragon_phase.is_sitting(),
                    )
                    .iter()
                    .find(|(_, part_box)| {
                        combat::within_reach(eye, *part_box, range)
                            && combat::facing(eye, look, part_box.center())
                    })
                    .map(|(part, _)| *part) else {
                        continue;
                    };
                    hit_damage = ferrite_entity::dragon::damage_on_part(part, hit_damage);
                }
                if ferrite_entity::dragon::is_crystal(&self.entities, key) {
                    // Any hit breaks an end crystal; it blows up (`EndCrystal.hurt`).
                    if let Some(flags) = self.entities.flags_mut(key) {
                        flags.set(ferrite_entity::Flags::REMOVED, true);
                    }
                    crystals.push(target);
                    continue;
                }
                let Some(hurt) = mob::hurt(&mut self.entities, key, hit_damage, Some(feet)) else {
                    continue;
                };
                let died = matches!(hurt, mob::Hurt::Died);
                let swept = matches!(hurt, mob::Hurt::Survived { .. } | mob::Hurt::Died)
                    && combat::can_sweep(sword, strength, p.ground, p.survival.sprinting);
                owner_attacks.push((uuid, key));
                mob::credit(&mut self.entities, key, uuid);
                mob::retaliate(&mut self.entities, key, uuid);
                hurt_keys.push((key, critical, swept, died, strength));
                // Sweeping: a sword at full charge on the ground hits everything around the
                // target for 1.0 (`Player.attack`'s `sweepAttack`).
                if swept {
                    for other in self.entities.query_vec(bbox.inflate(1.0, 0.25, 1.0)) {
                        if other == key || !mob::is_mob(&self.entities, other) {
                            continue;
                        }
                        mob::hurt(&mut self.entities, other, combat::SWEEP_DAMAGE, Some(feet));
                        mob::credit(&mut self.entities, other, uuid);
                    }
                }
            }
            for (key, critical, swept, died, strength) in hurt_keys {
                let Some(data) = self.entities.data(key) else {
                    continue;
                };
                let id = VarInt(data.id);
                let yaw = data.yaw;
                let mob_kind = data.mob.map(|m| m.kind);
                let mob_pos = self.entities.pos(key);
                let players = &self.players;
                tracker::broadcast(
                    &self.entities,
                    key,
                    &HurtAnimation { id, yaw },
                    &mut |viewer, bytes| deliver(players, ctx, viewer, bytes),
                );
                if critical {
                    tracker::broadcast(
                        &self.entities,
                        key,
                        &Animate { id, action: 4 },
                        &mut |viewer, bytes| deliver(players, ctx, viewer, bytes),
                    );
                }
                if let Some(pos) = mob_pos {
                    let attack_sound = if critical {
                        "minecraft:entity.player.attack.crit"
                    } else if swept {
                        "minecraft:entity.player.attack.sweep"
                    } else if strength > 0.9 {
                        "minecraft:entity.player.attack.strong"
                    } else {
                        "minecraft:entity.player.attack.weak"
                    };
                    self.play_sound_near(pos, attack_sound, 1.0, 1.0);
                    if let Some(kind) = mob_kind {
                        let sound = if died {
                            mob_death_sound(kind)
                        } else {
                            mob_hurt_sound(kind)
                        };
                        let source = if kind.is_hostile() { 5 } else { 6 };
                        self.play_sound_near_with_source(pos, sound, 1.0, 1.0, source);
                    }
                }
            }
            // A broken end crystal explodes with power 6 and no fire.
            for at in crystals {
                self.explode_by([at.x, at.y + 1.0, at.z], 6.0, true, Some(uuid), ctx);
            }
        }
        owner_attacks
    }

    /// A riptide spin with damage (`LivingEntity.checkAutoSpinAttack`) hurts the mobs in the
    /// spinner's box, widened by one block sideways; each mob is hit once per spin. shortcut:
    /// vanilla re-hits every tick and leaves the spacing to invulnerability frames.
    fn spin_attacks(&mut self) {
        let spinners: Vec<(Uuid, [f64; 3], f32)> = self
            .players
            .values()
            .filter(|p| p.spin_ticks > 0 && p.spin_damage > 0.0 && !p.survival.dead)
            .map(|p| (p.uuid, p.pos, p.spin_damage))
            .collect();
        for (uuid, pos, damage) in spinners {
            let bbox = ferrite_math::AABB::new(
                pos[0] - 0.3,
                pos[1],
                pos[2] - 0.3,
                pos[0] + 0.3,
                pos[1] + 1.8,
                pos[2] + 0.3,
            )
            .inflate(1.0, 0.0, 1.0);
            let origin = ferrite_math::Vec3::new(pos[0], pos[1], pos[2]);
            let Some(p) = self.players.get_mut(&uuid) else {
                continue;
            };
            for (key, target) in spin_victims(&self.entities, bbox, &mut p.spin_hit) {
                if mob::hurt(&mut self.entities, key, damage, Some(origin)).is_some() {
                    mob::credit(&mut self.entities, key, uuid);
                    mob::retaliate(&mut self.entities, key, uuid);
                    self.pending_hurts.push(target);
                }
            }
        }
    }

    /// Fires an arrow from a released bow (vanilla `BowItem.releaseUsing`): speed and damage
    /// scale with the draw, and a full draw is critical.
    fn fire_bow(&mut self, uuid: Uuid, charge: f32, yaw: f32, pitch: f32) {
        let Some(p) = self.players.get(&uuid) else {
            return;
        };
        let feet = ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]);
        let eye = combat::eye_position(feet);
        let direction = combat::look_vector(yaw, pitch);
        let speed = f64::from(charge) * arrow::SPEED;
        let vel = ferrite_math::Vec3::new(
            direction.x * speed,
            direction.y * speed,
            direction.z * speed,
        );
        let start = ferrite_math::Vec3::new(
            eye.x + direction.x * 0.3,
            eye.y + direction.y * 0.3 - 0.1,
            eye.z + direction.z * 0.3,
        );
        let damage = combat::arrow_damage(charge, charge >= 1.0);
        let record = arrow::new_arrow(start, vel, damage, Some(uuid), &mut self.rng);
        self.entities.insert(record);
    }

    /// `TridentItem.releaseUsing`: the held trident leaves the hand and flies where the player
    /// looks. shortcut: loyalty returns it the moment it lands (no flight back); riptide launches the
    /// player and starts the spin pose; channeling calls lightning on a mob during thunder. Without loyalty it drops as an item where it ends.
    fn throw_trident(&mut self, uuid: Uuid, yaw: f32, pitch: f32, raining: bool) {
        let Some(p) = self.players.get_mut(&uuid) else {
            return;
        };
        let slot = p.hand_slot(0);
        let stack = p.inventory.get(slot).clone();
        if stack.name() != Some("minecraft:trident") {
            return;
        }
        let riptide = match stack
            .components
            .get(ferrite_item::component_ids::ENCHANTMENTS)
        {
            Some(ferrite_item::Component::Enchantments(e)) => {
                e.level("minecraft:riptide").unwrap_or(0)
            }
            _ => 0,
        };
        if riptide > 0 {
            // `TridentItem.releaseUsing`: only in water or rain; the player is launched along
            // the look direction and keeps the trident (one point of wear).
            let (x, y, z) = (
                p.pos[0].floor() as i32,
                p.pos[1].floor() as i32,
                p.pos[2].floor() as i32,
            );
            let in_water = p
                .source
                .as_ref()
                .and_then(|s| s.block(x, y, z))
                .and_then(|s| u16::try_from(s).ok())
                .and_then(ferrite_data::blocks::state)
                .is_some_and(|(name, _)| name == "minecraft:water");
            if !(in_water || raining) {
                return;
            }
            let direction = combat::look_vector(yaw, pitch);
            let scale = 3.0 * (1.0 + f64::from(riptide)) / 4.0;
            send(
                p,
                &SetEntityMotion {
                    id: VarInt(p.entity),
                    velocity: ferrite_protocol::types::LpVec3([
                        direction.x * scale,
                        direction.y * scale + if in_water { 0.0 } else { 1.2 },
                        direction.z * scale,
                    ]),
                },
            );
            // The living-entity flag 0x04 plays the spin for the twenty ticks of the launch.
            p.spin_ticks = 20;
            p.broadcast_spin(true);
            if p.gamemode != 1
                && let Some(worn) = worn_stack(stack.clone(), stack.damage() + 1)
            {
                p.inventory.set(slot, worn);
                send(
                    p,
                    &SetPlayerInventory {
                        slot: VarInt(slot as i32),
                        item: p.inventory.get(slot).clone(),
                    },
                );
            }
            return;
        }
        if !matches!(p.gamemode, 1 | 3) {
            p.inventory.remove(slot, 1);
            send(
                p,
                &SetPlayerInventory {
                    slot: VarInt(slot as i32),
                    item: p.inventory.get(slot).clone(),
                },
            );
        }
        let feet = ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]);
        let eye = combat::eye_position(feet);
        let direction = combat::look_vector(yaw, pitch);
        let vel = ferrite_math::Vec3::new(
            direction.x * arrow::TRIDENT_SPEED,
            direction.y * arrow::TRIDENT_SPEED,
            direction.z * arrow::TRIDENT_SPEED,
        );
        let start = ferrite_math::Vec3::new(
            eye.x + direction.x * 0.3,
            eye.y + direction.y * 0.3 - 0.1,
            eye.z + direction.z * 0.3,
        );
        let mut record = arrow::new_trident(start, vel, Some(uuid));
        if let Some(state) = &mut record.data.arrow {
            state.channeling = matches!(
                stack.components.get(ferrite_item::component_ids::ENCHANTMENTS),
                Some(ferrite_item::Component::Enchantments(e))
                    if e.level("minecraft:channeling").is_some_and(|l| l > 0)
            );
        }
        self.thrown_tridents
            .insert(record.data.uuid, (uuid, stack.copy_with_count(1)));
        self.entities.insert(record);
    }

    /// Drops the item of every trident that ended its flight this tick.
    fn land_tridents(&mut self) {
        let ended: Vec<_> = self
            .entities
            .keys()
            .iter()
            .copied()
            .filter(|&key| {
                self.entities
                    .data(key)
                    .is_some_and(|d| arrow::kind_of(d) == Some(arrow::Kind::Trident))
                    && self
                        .entities
                        .flags(key)
                        .is_some_and(|f| f.has(ferrite_entity::Flags::REMOVED))
            })
            .collect();
        for key in ended {
            let (Some(uuid), Some(pos)) = (
                self.entities.data(key).map(|d| d.uuid),
                self.entities.pos(key),
            ) else {
                continue;
            };
            if let Some((owner, stack)) = self.thrown_tridents.remove(&uuid) {
                let loyal = matches!(
                    stack.components.get(ferrite_item::component_ids::ENCHANTMENTS),
                    Some(ferrite_item::Component::Enchantments(e)) if e.level("minecraft:loyalty").is_some_and(|l| l > 0)
                );
                if loyal && let Some(p) = self.players.get_mut(&owner) {
                    let leftover = p.inventory.place_back(stack);
                    p.sync_containers();
                    let Some(stack) = leftover else {
                        continue;
                    };
                    self.entities.insert(item::new_item_stack(
                        pos,
                        ferrite_math::Vec3::ZERO,
                        &stack,
                        &mut self.rng,
                    ));
                    continue;
                }
                let mut record =
                    item::new_item_stack(pos, ferrite_math::Vec3::ZERO, &stack, &mut self.rng);
                if let Some(state) = &mut record.data.item {
                    state.pickup_delay = 10;
                }
                self.entities.insert(record);
            }
        }
    }

    /// Applies damage to a player of this region, honouring a raised shield (vanilla
    /// `LivingEntity.hurt` after `isDamageSourceBlocked`).
    fn damage_player(
        &mut self,
        uuid: Uuid,
        amount: f32,
        kind: DamageKind,
        from: ferrite_math::Vec3,
        attacker: Option<ferrite_entity::EntityKey>,
    ) {
        self.damage_player_with(uuid, amount, kind, from, attacker, 0.4, 0.0, 0);
    }

    #[allow(clippy::too_many_arguments)]
    fn damage_player_with(
        &mut self,
        uuid: Uuid,
        amount: f32,
        kind: DamageKind,
        from: ferrite_math::Vec3,
        attacker: Option<ferrite_entity::EntityKey>,
        knockback_xz: f64,
        launch_y: f64,
        poison_ticks: i32,
    ) {
        let Some(p) = self.players.get_mut(&uuid) else {
            return;
        };
        if p.blocks_damage_from([from.x, from.y, from.z]) {
            // A blocked hit costs no health: the shield is disabled for its cooldown and the
            // attacker is knocked back (`LivingEntity.blockUsingShield`).
            p.shield_disabled = 5;
            let shield_pos = ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]);
            if let Some(attacker) = attacker {
                mob::knockback(&mut self.entities, attacker, shield_pos);
            }
            self.play_sound_near(shield_pos, "minecraft:item.shield.block", 1.0, 1.0);
            return;
        }
        if p.hurt(kind, amount) {
            // Vanilla `Mob.doHurtTarget` knocks the hurt player away from the attacker.
            let dx = p.pos[0] - from.x;
            let dz = p.pos[2] - from.z;
            let length = (dx * dx + dz * dz).sqrt().max(1.0e-4);
            let y = if p.ground { 0.4 } else { 0.0 } + launch_y;
            send(
                p,
                &SetEntityMotion {
                    id: VarInt(p.entity),
                    velocity: ferrite_protocol::types::LpVec3([
                        dx / length * knockback_xz,
                        y,
                        dz / length * knockback_xz,
                    ]),
                },
            );
            if poison_ticks > 0 {
                p.apply_effect(MobEffectInstance::new(
                    ferrite_entity::effect::Effect::Poison.id(),
                    poison_ticks,
                    0,
                ));
            }
        }
    }

    /// What the dragon does to the world: it smashes the non-immune blocks inside its body.
    fn tick_dragon_effects(&mut self, ctx: &TickContext) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        if source.dimension() != super::Dimension::End {
            return;
        }
        let mut smashed = Vec::new();
        for key in self.entities.keys().to_vec() {
            if !mob::state(&self.entities, key).is_some_and(|m| {
                m.kind == mob::MobKind::EnderDragon
                    && !m.is_dead()
                    && u64::from(m.wander_timer)
                        .is_multiple_of(ferrite_entity::dragon::SMASH_INTERVAL)
            }) {
                continue;
            }
            let Some(bbox) = self.entities.bbox(key) else {
                continue;
            };
            for x in bbox.min.x.floor() as i32..=bbox.max.x.floor() as i32 {
                for y in bbox.min.y.floor() as i32..=bbox.max.y.floor() as i32 {
                    for z in bbox.min.z.floor() as i32..=bbox.max.z.floor() as i32 {
                        let Some(state) = source.block(x, y, z).and_then(|s| u16::try_from(s).ok())
                        else {
                            continue;
                        };
                        if state != 0
                            && ferrite_data::blocks::state(state).is_some_and(|(name, _)| {
                                !ferrite_entity::dragon::IMMUNE.contains(&name)
                                    && name != "minecraft:air"
                            })
                        {
                            smashed.push(([x, y, z], 0));
                        }
                    }
                }
            }
        }
        if !smashed.is_empty() {
            self.place_blocks(ctx, &smashed);
        }
    }

    /// Advances arrow flight and applies arrow hits to the players of this region.
    /// Channeling: a trident that hit a mob during a thunderstorm calls a lightning bolt on it
    /// (`ThrownTrident.onHitEntity`). shortcut: the bolt hurts mobs within three blocks for 5
    /// and does not set fires or need open sky.
    fn strike_channeled(&mut self, ctx: &TickContext) {
        let hits: Vec<_> =
            std::mem::take(&mut *arrow::CHANNELED.lock().unwrap_or_else(|e| e.into_inner()));
        let thunder = ctx.dimension == super::Dimension::Overworld
            && ctx
                .state
                .lock()
                .is_ok_and(|s| s.weather == super::commands::Weather::Thunder);
        if !thunder {
            return;
        }
        for (at, thrower) in hits {
            let near = self.entities.query_vec(ferrite_math::AABB::new(
                at.x - 3.0,
                at.y - 3.0,
                at.z - 3.0,
                at.x + 3.0,
                at.y + 3.0,
                at.z + 3.0,
            ));
            let mut victims = Vec::new();
            for key in near {
                if mob::is_mob(&self.entities, key) {
                    // `Creeper.thunderHit`: a bolt charges a creeper (and hurts it all the same).
                    ferrite_entity::creeper::thunder_hit(&mut self.entities, key);
                    if let Some(state) = mob::state_mut(&mut self.entities, key)
                        && state.kind == MobKind::Mooshroom
                    {
                        state.variant = Some(1 - state.variant.unwrap_or(0));
                        if let Some(data) = self.entities.data_mut(key) {
                            mob::sync_state_metadata(data);
                        }
                    }
                    mob::hurt(&mut self.entities, key, 5.0, None);
                    if let Some(state) = mob::state(&self.entities, key) {
                        victims.push(state.kind.name().to_owned());
                    }
                }
            }
            // `LightningBolt.tick` -> `CHANNELED_LIGHTNING` for the player who channeled it.
            if let Some(thrower) = thrower {
                super::recipe_unlock::push_plain(
                    thrower,
                    super::recipe_unlock::PlainEvent {
                        trigger: "minecraft:channeled_lightning",
                        victims,
                        ..Default::default()
                    },
                );
            }
            static NEXT: std::sync::atomic::AtomicI32 =
                std::sync::atomic::AtomicI32::new(1_800_000_000);
            let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            for p in self.players.values() {
                send(
                    p,
                    &AddEntity {
                        id: VarInt(id),
                        uuid: Uuid::from_u128(u128::from(id as u32)),
                        entity_type: VarInt(78),
                        x: at.x,
                        y: at.y,
                        z: at.z,
                        velocity: ferrite_protocol::types::LpVec3::ZERO,
                        pitch: 0,
                        yaw: 0,
                        head_yaw: 0,
                        data: VarInt(0),
                    },
                );
            }
        }
    }

    fn route_ender_pearl_landing(
        &mut self,
        signals: &UnboundedSender<Signal>,
        dimension: super::Dimension,
        owner: Uuid,
        pos: [f64; 3],
    ) {
        if let Some(player) = self.players.get_mut(&owner)
            && player.dimension == dimension
            && player.land_ender_pearl(pos)
        {
            return;
        }
        let _ = signals.send(Signal::EnderPearlTeleport {
            dimension,
            player: owner,
            pos,
        });
    }

    fn tick_arrows(&mut self, ctx: &TickContext) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let blocks = TickBlocks {
            source: Some(source),
            seen: Default::default(),
        };
        let players: Vec<(Uuid, ferrite_math::AABB)> = self
            .players
            .values()
            .map(|p| {
                (
                    p.uuid,
                    ferrite_math::AABB::new(
                        p.pos[0] - 0.3,
                        p.pos[1],
                        p.pos[2] - 0.3,
                        p.pos[0] + 0.3,
                        p.pos[1] + 1.8,
                        p.pos[2] + 0.3,
                    ),
                )
            })
            .collect();
        let dragon_parts: Vec<_> = self
            .entities
            .keys()
            .iter()
            .filter_map(|&key| {
                let data = self.entities.data(key)?;
                let mob = data.mob?;
                if mob.kind != mob::MobKind::EnderDragon || mob.is_dead() {
                    return None;
                }
                let pos = self.entities.pos(key)?;
                Some((
                    key,
                    ferrite_entity::dragon::part_boxes_with_flight(
                        pos,
                        data.yaw,
                        &mob.dragon_flight,
                        mob.dragon_phase.is_sitting(),
                    )
                    .to_vec(),
                ))
            })
            .collect();
        let mut blocked_arrow_pos: Option<ferrite_math::Vec3> = None;
        for hit in
            arrow::tick_arrows_with_parts(&mut self.entities, &blocks, &players, &dragon_parts)
        {
            if let Some(p) = self.players.get_mut(&hit.uuid) {
                if p.blocking && p.shield_disabled == 0 {
                    p.shield_disabled = 5;
                    blocked_arrow_pos = Some(ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]));
                    super::recipe_unlock::push_plain(
                        hit.uuid,
                        super::recipe_unlock::PlainEvent {
                            trigger: "minecraft:entity_hurt_player",
                            projectile: true,
                            blocked: true,
                            ..Default::default()
                        },
                    );
                } else if p.hurt(DamageKind::Arrow, hit.damage)
                    && let Some(effect) = hit.effect
                {
                    p.apply_effect(effect);
                }
            }
        }
        if let Some(pos) = blocked_arrow_pos {
            self.play_sound_near(pos, "minecraft:item.shield.block", 1.0, 1.0);
        }
        self.strike_channeled(ctx);
        for (owner, kind) in
            std::mem::take(&mut *arrow::HURT.lock().unwrap_or_else(|e| e.into_inner()))
        {
            // `ServerPlayer.awardHit`-style `PLAYER_HURT_ENTITY`: the projectile is the damage's
            // direct entity, and every arrow and trident is a `#minecraft:is_projectile` source.
            super::recipe_unlock::push_plain(
                owner,
                super::recipe_unlock::PlainEvent {
                    trigger: "minecraft:player_hurt_entity",
                    direct: Some(format!("minecraft:{kind}")),
                    projectile: true,
                    ..Default::default()
                },
            );
        }
        let shot: Vec<_> =
            std::mem::take(&mut *arrow::KILLS.lock().unwrap_or_else(|e| e.into_inner()));
        super::recipe_unlock::KILLS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend(shot);
        let mut fires: Vec<[i32; 3]> = Vec::new();
        for landing in arrow::tick_thrown(&mut self.entities, &blocks, &players) {
            // Vanilla large fireball: a power-1 explosion that sets fires.
            if matches!(
                landing.kind,
                arrow::Kind::Fireball | arrow::Kind::WitherSkull
            ) {
                // A wither skull explodes with power 1 and starts no fires.
                let fire = landing.kind == arrow::Kind::Fireball;
                self.queued_explosions.push((
                    [landing.pos.x, landing.pos.y, landing.pos.z],
                    1.0,
                    fire,
                    true,
                ));
            }
            // Vanilla ender pearl impact: the thrower arrives where the pearl landed and takes
            // 5 fall damage.
            if landing.kind == arrow::Kind::DragonFireball {
                let cloud =
                    ferrite_entity::dragon::fireball_breath_cloud(landing.pos, landing.owner);
                self.entities.insert(cloud.spawn_record());
                self.potion_clouds.push(cloud);
            }
            // Vanilla shulker bullet: 4 damage to what it hits (no homing here).
            if landing.kind == arrow::Kind::ShulkerBullet {
                let at = landing.pos;
                let reach = ferrite_math::AABB::new(
                    at.x - 1.0,
                    at.y - 1.0,
                    at.z - 1.0,
                    at.x + 1.0,
                    at.y + 1.0,
                    at.z + 1.0,
                );
                for key in self.entities.query_vec(reach) {
                    mob::hurt(&mut self.entities, key, 4.0, Some(at));
                }
            }
            // Vanilla small fireball: 5 damage + 5 s fire on hit entity; places fire on block hit
            // when `mobGriefing` allows (or when fired by a non-mob source such as a dispenser).
            if landing.kind == arrow::Kind::SmallFireball {
                let at = landing.pos;
                match landing.hit {
                    arrow::HitTarget::Player(target) => {
                        if let Some(p) = self.players.get_mut(&target) {
                            if p.blocks_damage_from([at.x, at.y, at.z]) {
                                p.shield_disabled = 5;
                            } else {
                                let prev_fire = p.survival.fire_ticks;
                                p.survival.fire_ticks = prev_fire.max(100);
                                if !p.hurt(DamageKind::Fireball, 5.0) {
                                    p.survival.fire_ticks = prev_fire;
                                }
                            }
                        }
                    }
                    arrow::HitTarget::Mob(target) => {
                        let prev_fire = mob::state(&self.entities, target).map_or(0, |m| m.fire);
                        mob::ignite(&mut self.entities, target, 100);
                        let hurt = mob::hurt(&mut self.entities, target, 5.0, Some(at));
                        if !hurt.is_some_and(|h| !matches!(h, mob::Hurt::Immune))
                            && let Some(m) = mob::state_mut(&mut self.entities, target)
                        {
                            m.fire = prev_fire;
                        }
                    }
                    arrow::HitTarget::Block { adjacent } => {
                        let mob_owner = landing.owner.is_some();
                        if !mob_owner || Self::mob_griefing(ctx) {
                            fires.push(adjacent);
                        }
                    }
                }
            }
            if landing.kind == arrow::Kind::SplashPotion
                && let Some(bottle) = self.take_thrown_potion(landing.uuid)
            {
                self.splash_potion_owned(
                    [landing.pos.x, landing.pos.y, landing.pos.z],
                    &bottle,
                    landing.owner,
                );
            }
            if landing.kind == arrow::Kind::LingeringPotion
                && let Some(bottle) = self.take_thrown_potion(landing.uuid)
            {
                self.linger_potion_owned(landing.pos, &bottle, landing.owner);
            }
            if landing.kind == arrow::Kind::Egg {
                ferrite_entity::breed::hatch_egg(&mut self.entities, landing.pos, &mut self.rng);
            }
            // A thrown bottle awards 3-11 experience where it breaks.
            if landing.kind == arrow::Kind::ExperienceBottle {
                let amount = 3 + self.rng.next_int(5) + self.rng.next_int(5);
                for orb in experience::award(landing.pos, amount) {
                    self.entities.insert(orb);
                }
            }
            // A wind charge bursts on impact and pushes nearby players; mobs are handled by
            // `arrow::wind_burst` inside the entity store.
            if matches!(
                landing.kind,
                arrow::Kind::WindCharge | arrow::Kind::BreezeWindCharge
            ) {
                self.wind_charge_burst(landing.pos, landing.owner, &players);
            }
            if landing.kind == arrow::Kind::EnderPearl
                && let Some(owner) = landing.owner
            {
                self.route_ender_pearl_landing(
                    &ctx.signals,
                    ctx.dimension,
                    owner,
                    [landing.pos.x, landing.pos.y, landing.pos.z],
                );
            }
        }
        for pos in fires {
            self.ignite_block(pos, ctx);
        }
        // An arrow that reaches an end crystal breaks it and is used up.
        let mut blasts = Vec::new();
        for (crystal, arrow, pos) in ferrite_entity::dragon::arrows_in_crystals(&self.entities) {
            for hit in [crystal, arrow] {
                if let Some(flags) = self.entities.flags_mut(hit) {
                    flags.set(ferrite_entity::Flags::REMOVED, true);
                }
            }
            blasts.push(pos);
        }
        for pos in blasts {
            self.explode([pos.x, pos.y + 1.0, pos.z], 6.0, true, ctx);
        }
    }

    /// Runs the neighbor updates of this tick's block changes (vanilla
    /// `Level.markAndNotifyBlock` -> `Block.updateNeighbourShapes`). Unsupported blocks break
    /// here; their loot drops as item entities through the normal loot tables.
    fn tick_block_updates(&mut self, ctx: &TickContext) -> Vec<[i32; 3]> {
        let mut changed = Vec::new();
        for p in self.players.values_mut() {
            changed.append(&mut p.changed_blocks);
        }
        let Some(source) = ctx.source.as_deref() else {
            return changed;
        };
        for pos in &changed {
            self.updates.block_changed(*pos);
        }
        let mut removed = Vec::new();
        self.apply_neighbor_updates(source, ctx, &mut removed);
        for (pos, state) in removed {
            self.spawn_block_drops(source, pos, state);
            self.wake_fluid(ctx, pos);
            changed.push(pos);
        }
        changed
    }

    /// A block vanished at `pos` outside the player path: wake the fluids around it, in this
    /// region or the one that owns them (vanilla `Level.setBlock` -> `neighborChanged`).
    fn wake_fluid(&self, ctx: &TickContext, pos: [i32; 3]) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let _ = ctx.signals.send(Signal::FluidChange {
            dimension: source.dimension(),
            x: pos[0],
            y: pos[1],
            z: pos[2],
        });
    }

    /// Runs the queued shape updates against the region's blocks.
    fn apply_neighbor_updates(
        &mut self,
        source: &super::anvil::AnvilChunkSource,
        ctx: &TickContext,
        removed: &mut Vec<([i32; 3], u16)>,
    ) {
        let mut world =
            super::fluid::RegionFluidWorld::new(source, &self.held_chunks, &ctx.tracking)
                .with_signals(ctx.signals.clone());
        self.updates.apply(&mut world, removed);
    }

    /// `World#spawnEntity` / `spawnFallingBlock`: a falling block, primed TNT or a modelled mob.
    /// Returns whether something was spawned.
    pub fn plugin_spawn(
        &mut self,
        center: [f64; 3],
        kind: &str,
        block: Option<&str>,
        velocity: [f64; 3],
        uuid: Uuid,
        owner: Option<Uuid>,
    ) -> bool {
        use ferrite_math::Vec3;
        if !center
            .iter()
            .all(|value| value.is_finite() && value.abs() <= 30_000_000.0)
        {
            return false;
        }
        let pos = Vec3::new(center[0], center[1], center[2]);
        let record = match kind {
            "minecraft:falling_block" => {
                let Some(state) = block.and_then(ferrite_data::blocks::default_state) else {
                    return false;
                };
                ferrite_entity::falling::new_falling(pos, state)
            }
            "minecraft:tnt" => ferrite_entity::tnt::new_primed_tnt(pos, 80),
            "minecraft:eye_of_ender" => ferrite_entity::eye::new_eye(pos, &mut self.rng),
            "minecraft:firework_rocket" => {
                ferrite_entity::firework::new_firework(pos, 1, 0, None, &mut self.rng)
            }
            thrown if arrow::Kind::thrown_from_name(thrown).is_some() => arrow::new_thrown(
                arrow::Kind::thrown_from_name(thrown).unwrap(),
                pos,
                Vec3::new(velocity[0], velocity[1], velocity[2]),
                owner,
            ),
            "minecraft:trident" => {
                let (Some(owner), Some(stack)) = (
                    owner,
                    ferrite_item::ItemStack::from_name("minecraft:trident"),
                ) else {
                    return false;
                };
                let record = arrow::new_trident(
                    pos,
                    Vec3::new(velocity[0], velocity[1], velocity[2]),
                    Some(owner),
                );
                self.thrown_tridents.insert(uuid, (owner, stack));
                record
            }
            "minecraft:spectral_arrow" => ferrite_entity::arrow::new_spectral_arrow(
                pos,
                Vec3::new(velocity[0], velocity[1], velocity[2]),
                2.0,
                owner,
                &mut self.rng,
            ),
            "minecraft:arrow" => ferrite_entity::arrow::new_arrow(
                pos,
                Vec3::new(velocity[0], velocity[1], velocity[2]),
                2.0,
                None,
                &mut self.rng,
            ),
            other => {
                let Some(mob) = ferrite_entity::mob::MobKind::from_name(other) else {
                    return false;
                };
                mob::new_mob(mob, pos, &mut self.rng)
            }
        };
        let mut record = record;
        record.data.uuid = uuid;
        self.entities.insert(record);
        true
    }

    /// `World#ferriteSpawnSnapshot`: the entity an SNBT snapshot describes, keeping its NBT.
    pub fn plugin_spawn_snapshot(
        &mut self,
        _snbt: &str,
        _uuid: Uuid,
        _pos: [f64; 3],
        _yaw: f32,
    ) -> bool {
        false
    }

    /// `World#dropItem`: an item entity at `center`; the natural variant scatters it a little
    /// and pops it up like a block drop. An unknown item is ignored.
    pub fn drop_item(
        &mut self,
        center: [f64; 3],
        item: &str,
        amount: u8,
        natural: bool,
        uuid: Uuid,
    ) {
        use ferrite_math::Vec3;
        let Some(id) = ferrite_item::item_id(item) else {
            return;
        };
        let stack = ferrite_protocol::packets::play::ItemStack {
            count: i32::from(amount),
            item_id: i32::from(id),
        };
        let (pos, vel) = if natural {
            (
                Vec3::new(
                    center[0] + self.rng.range(-0.25, 0.25),
                    center[1] + self.rng.range(-0.25, 0.25),
                    center[2] + self.rng.range(-0.25, 0.25),
                ),
                Vec3::new(
                    self.rng.next_f64() * 0.2 - 0.1,
                    0.2,
                    self.rng.next_f64() * 0.2 - 0.1,
                ),
            )
        } else {
            (
                Vec3::new(center[0], center[1], center[2]),
                Vec3::new(0.0, 0.0, 0.0),
            )
        };
        let mut record = item::new_item(pos, vel, stack, &mut self.rng);
        if let Some(state) = &mut record.data.item {
            state.pickup_delay = 10;
        }
        record.data.uuid = uuid;
        self.entities.insert(record);
    }

    /// Drops what a block removed by a neighbor, gravity, piston or explosion update leaves
    /// behind: its loot table (a shulker box with its contents in the item) and the contents of
    /// any other container, which spill like a player-broken one.
    fn spawn_block_drops(
        &mut self,
        source: &super::anvil::AnvilChunkSource,
        pos: [i32; 3],
        state: u16,
    ) {
        let empty = ferrite_item::ItemStack::empty();
        let is_shulker = ferrite_data::blocks::state(state)
            .is_some_and(|(name, _)| super::block_container::is_shulker_box(name));
        let is_hive = ferrite_data::blocks::state(state)
            .is_some_and(|(name, _)| super::beehive::is_hive(name));
        let entity = if is_shulker {
            super::shulker::take_components(source, pos)
        } else if is_hive {
            let (components, data) = super::beehive::take(source, pos);
            self.pending_hive_breaks.push((pos, state, data));
            components
        } else {
            std::collections::BTreeMap::new()
        };
        for stack in super::drops::block_drop_stacks(state, &empty, &mut self.rng, &entity) {
            let record = item::pop_resource_stack(pos, &stack, &mut self.rng);
            self.entities.insert(record);
        }
        if !is_shulker {
            for stack in super::block_container::take_all_contents(source, pos, u32::from(state)) {
                let record = item::pop_resource_stack(pos, &stack, &mut self.rng);
                self.entities.insert(record);
            }
        }
    }

    /// Vanilla `FallingBlock.onPlace`/`neighborChanged`: schedules a free gravity block. The
    /// changed block and its six neighbors are the blocks vanilla notifies.
    fn schedule_gravity(&mut self, source: &super::anvil::AnvilChunkSource, changed: &[[i32; 3]]) {
        for &pos in changed {
            for (dx, dy, dz) in [
                (0, 0, 0),
                (1, 0, 0),
                (-1, 0, 0),
                (0, 1, 0),
                (0, -1, 0),
                (0, 0, 1),
                (0, 0, -1),
            ] {
                let target = [pos[0] + dx, pos[1] + dy, pos[2] + dz];
                let chunk = ChunkPos(target[0].div_euclid(16), target[2].div_euclid(16));
                if !self.held_chunks.contains_key(&chunk) {
                    continue;
                }
                let Some(state) = source
                    .block(target[0], target[1], target[2])
                    .and_then(|s| u16::try_from(s).ok())
                else {
                    continue;
                };
                if !super::gravity::is_gravity(state) {
                    continue;
                }
                let below = source
                    .block(target[0], target[1] - 1, target[2])
                    .and_then(|s| u16::try_from(s).ok());
                if super::gravity::is_free(below) {
                    self.gravity.schedule(target);
                }
            }
        }
    }

    /// Vanilla `ConcretePowderBlock.updateShape`/`getStateForPlacement`: powder that touches
    /// water (or sits in it) becomes concrete. Checks the changed blocks and their neighbors.
    fn solidify_powder(&mut self, ctx: &TickContext, changed: &[[i32; 3]]) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let block = |pos: [i32; 3]| {
            source
                .block(pos[0], pos[1], pos[2])
                .and_then(|s| u16::try_from(s).ok())
        };
        let mut hardened = Vec::new();
        for &pos in changed {
            for (dx, dy, dz) in [
                (0, 0, 0),
                (1, 0, 0),
                (-1, 0, 0),
                (0, 1, 0),
                (0, -1, 0),
                (0, 0, 1),
                (0, 0, -1),
            ] {
                let target = [pos[0] + dx, pos[1] + dy, pos[2] + dz];
                if !self.held_chunks.contains_key(&ChunkPos(
                    target[0].div_euclid(16),
                    target[2].div_euclid(16),
                )) {
                    continue;
                }
                let Some(state) = block(target) else {
                    continue;
                };
                if let Some(concrete) = super::gravity::solidified(state, target, block) {
                    hardened.push((target, concrete));
                }
            }
        }
        hardened.sort_unstable();
        hardened.dedup();
        if !hardened.is_empty() {
            self.place_blocks(ctx, &hardened);
        }
    }

    /// Runs due gravity checks, then the neighbor updates of the blocks they removed.
    fn tick_gravity(&mut self, ctx: &TickContext) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let removed =
            self.gravity
                .tick(source, &self.held_chunks, &ctx.tracking, &mut self.entities);
        if removed.is_empty() {
            return;
        }
        for pos in &removed {
            self.updates.block_changed(*pos);
            self.wake_fluid(ctx, *pos);
        }
        // Vanilla `FallingBlock.updateShape` schedules the block above a vacated cell, so a
        // stack falls one block per two ticks instead of leaving the upper blocks hanging.
        self.schedule_gravity(source, &removed);
        let mut broken = Vec::new();
        self.apply_neighbor_updates(source, ctx, &mut broken);
        for (pos, state) in broken {
            self.spawn_block_drops(source, pos, state);
            self.wake_fluid(ctx, pos);
        }
    }

    /// Ticks the falling block entities against this region's blocks.
    fn tick_falling(&mut self, ctx: &TickContext) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let blocks = TickBlocks {
            source: Some(source),
            seen: Default::default(),
        };
        let world = super::fluid::RegionFluidWorld::new(source, &self.held_chunks, &ctx.tracking);
        let mut adapter = FallingAdapter {
            read: &blocks,
            world,
        };
        ferrite_entity::falling::tick_falling_blocks(&mut self.entities, &mut adapter, -64, 320);
    }
    /// Spawns the loot of the blocks a player broke in survival as item entities.
    fn drop_broken_blocks(&mut self, p: &mut Player, ctx: &TickContext) {
        let held = p.inventory.selected_item();
        for (pos, state) in p.broken.drain(..) {
            let Ok(state) = u16::try_from(state) else {
                continue;
            };
            let at = [pos.x, pos.y, pos.z];
            let entity = p.menus.broken_entities.remove(&at).unwrap_or_default();
            for stack in super::drops::block_drop_stacks(state, held, &mut self.rng, &entity) {
                let record = item::pop_resource_stack(at, &stack, &mut self.rng);
                self.entities.insert(record);
            }
            // `TurtleEggBlock.playerDestroy` removes one egg from the cluster after the
            // normal drop roll. The loot table only drops an item with Silk Touch.
            if p.gamemode == 0
                && let Some((eggs, hatch)) = ferrite_entity::turtle::egg_props(state)
            {
                if eggs > 1
                    && let Some(next) = ferrite_entity::turtle::egg_state(eggs - 1, hatch)
                {
                    self.place_blocks(ctx, &[(at, next)]);
                    p.changed_blocks.push(at);
                }
                let pitch = 0.9 + self.rng.next_f32() * 0.2;
                self.play_sound_near(
                    block_sound_center(at),
                    ferrite_entity::turtle::EGG_BREAK_SOUND,
                    0.7,
                    pitch,
                );
                turtle_egg_break_particles(&ctx.tracking, ctx.world_id, at, state);
            }
        }
    }
    /// Lets every non-spectator player pick up the items it touches.
    fn pick_up_items(&mut self, ctx: &TickContext) {
        let mut sent = Vec::new();
        for p in self.players.values_mut() {
            if p.gamemode == 3 {
                continue;
            }
            let mut picker = item::Picker {
                uuid: p.uuid,
                entity_id: p.entity,
                pos: ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]),
                inventory: &mut p.inventory,
                infinite: p.gamemode == 1,
            };
            item::pick_up(&mut self.entities, &mut picker, &mut |viewer, bytes| {
                sent.push((viewer, bytes.to_vec()))
            });
            arrow::pick_up_arrows(&mut self.entities, &mut picker, &mut |viewer, bytes| {
                sent.push((viewer, bytes.to_vec()))
            });
        }
        // Like vanilla: the take animation first, the inventory sync at the end of the tick.
        for (viewer, bytes) in sent {
            deliver(&self.players, ctx, viewer, &bytes);
        }
    }
    /// Spawns primed TNT at a block position (vanilla `PrimedTnt` entity).
    fn spawn_tnt(&mut self, pos: [i32; 3], fuse: i32) {
        self.spawn_tnt_owned(pos, fuse, None);
    }

    fn spawn_tnt_owned(&mut self, pos: [i32; 3], fuse: i32, owner: Option<Uuid>) {
        let center = ferrite_math::Vec3::new(
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]),
            f64::from(pos[2]) + 0.5,
        );
        self.entities
            .insert(ferrite_entity::tnt::new_primed_tnt_owned(
                center, fuse, owner,
            ));
    }

    /// Awards the experience orbs each player touches (`ExperienceOrb.playerTouch`) and syncs
    /// the bar. The take animation packets go out after every player was processed.
    fn pick_up_experience(&mut self, ctx: &TickContext) {
        let mut sent = Vec::new();
        for p in self.players.values_mut() {
            if p.gamemode == 3 || p.survival.dead {
                continue;
            }
            let feet = ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]);
            let gained = experience::player_touch(
                &mut self.entities,
                p.entity,
                feet,
                &mut |viewer, bytes| sent.push((viewer, bytes.to_vec())),
            );
            if gained > 0 {
                p.set_experience_total(i64::from(p.experience_total) + i64::from(gained));
                send(
                    p,
                    &SetExperience {
                        progress: p.experience_progress,
                        level: VarInt(p.experience_level),
                        total: VarInt(p.experience_total),
                    },
                );
            }
        }
        for (viewer, bytes) in sent {
            deliver(&self.players, ctx, viewer, &bytes);
        }
    }
    /// Spawns the end crystals players placed.
    fn spawn_crystals(&mut self, p: &mut Player) {
        for [x, y, z] in p.crystals.drain(..) {
            let at = ferrite_math::Vec3::new(f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5);
            self.entities
                .insert(ferrite_entity::dragon::new_crystal(at));
        }
    }

    /// Spawns the TNT a player lit (vanilla `PrimedTnt` at the block position).
    fn spawn_primed_tnt(&mut self, p: &mut Player) {
        let igniter = Some(p.uuid);
        for (pos, fuse) in p.primed_tnt.drain(..) {
            self.spawn_tnt_owned(pos, fuse, igniter);
        }
    }

    /// Applies `CarvedPumpkinBlock.onPlace` for each pumpkin this player placed this tick.
    fn build_golems(&mut self, p: &mut Player, ctx: &TickContext) {
        for pumpkin in p.placed_pumpkins.drain(..) {
            self.construct_golem(ctx, pumpkin);
        }
    }

    /// Matches a golem pattern around `pumpkin`, clears it and spawns the golem. Every pattern
    /// cell must be held by this region; a structure that crosses into another region's chunk is
    /// refused instead of being partially cleared or handed off.
    fn construct_golem(&mut self, ctx: &TickContext, pumpkin: [i32; 3]) -> bool {
        let Some(source) = ctx.source.as_deref() else {
            return false;
        };
        let Some(build) = super::golem::construction_at(pumpkin, |x, y, z| {
            source
                .block(x, y, z)
                .and_then(|state| u16::try_from(state).ok())
        }) else {
            return false;
        };
        if !build.cells.iter().all(|at| {
            self.held_chunks
                .contains_key(&ChunkPos(at[0].div_euclid(16), at[2].div_euclid(16)))
        }) {
            return false;
        }
        if build.kind == MobKind::Wither
            && ctx
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .difficulty
                == 0
        {
            // Vanilla `WitherSkullBlock.checkSpawn` refuses to summon the Wither in Peaceful.
            return false;
        }
        let removed = build.removed.clone();
        let cleared: Vec<([i32; 3], u16)> = removed.iter().map(|(at, _)| (*at, 0)).collect();
        self.place_blocks(ctx, &cleared);
        for (at, state) in removed {
            self.block_break_effect(ctx, at, state);
        }
        let at = ferrite_math::Vec3::new(
            f64::from(build.spawn[0]) + 0.5,
            f64::from(build.spawn[1]) + 0.05,
            f64::from(build.spawn[2]) + 0.5,
        );
        let mut record = mob::new_mob(build.kind, at, &mut self.rng);
        // `spawnGolemInWorld` snaps the golem to the pattern's bottom block with yaw 0, pitch 0.
        record.data.yaw = 0.0;
        record.data.pitch = 0.0;
        if build.kind == MobKind::IronGolem
            && let Some(state) = record.data.mob.as_mut()
        {
            state.player_created = true;
        }
        if build.kind == MobKind::Wither {
            if let Some(state) = record.data.mob.as_mut() {
                state.special_timer = 220;
                state.health = 100.0;
            }
            self.play_sound_near(at, "minecraft:entity.wither.spawn", 1.0, 1.0);
        }
        mob::sync_state_metadata(&mut record.data);
        self.entities.insert(record);
        for (&player_id, p) in &self.players {
            let dx = p.pos[0] - at.x;
            let dy = p.pos[1] - at.y;
            let dz = p.pos[2] - at.z;
            if dx * dx + dy * dy + dz * dz <= 25.0 {
                super::recipe_unlock::push_plain(
                    player_id,
                    super::recipe_unlock::PlainEvent {
                        trigger: "minecraft:summoned_entity",
                        entity: Some(build.kind.name().to_owned()),
                        ..Default::default()
                    },
                );
            }
        }
        true
    }

    /// `clearPatternBlocks` sends level event 2001 (block break sound and particles) with the
    /// removed state's id.
    fn block_break_effect(&self, ctx: &TickContext, at: [i32; 3], state: u16) {
        let update = packet(&LevelEvent {
            event: 2001,
            location: ferrite_protocol::types::BlockPos::new(at[0], at[1], at[2]).pack(),
            data: i32::from(state),
            global: false,
        });
        let tracking = ctx.tracking.lock().unwrap();
        broadcast_tracked_chunk(
            &tracking,
            ctx.world_id,
            (at[0].div_euclid(16), at[2].div_euclid(16)),
            &update,
        );
    }

    /// Runs this region's command blocks after redstone settled for the tick (vanilla
    /// `Level.tickBlockEntities` -> `CommandBlockEntity.tick`), then sends each changed block
    /// entity to its viewers through `block_entity_data`.
    fn tick_command_blocks(&mut self, ctx: &TickContext, changed: &[[i32; 3]]) {
        let Some(source) = ctx.source.as_ref() else {
            return;
        };
        let Some(tree) = ctx.tree.as_ref() else {
            return;
        };
        let Self {
            command_blocks,
            redstone,
            held_chunks,
            tick_clock,
            ..
        } = self;
        command_blocks.blocks_changed(source, changed);
        let world = super::fluid::RegionFluidWorld::new(source, held_chunks, &ctx.tracking);
        let power = |pos| redstone.power_at(&world, pos);
        let mut runner = BlockCommands {
            source,
            tracking: &ctx.tracking,
            state: &ctx.state,
            dimension: ctx.dimension,
            tree,
        };
        let updated = command_blocks.tick(source, &power, &mut runner, *tick_clock);
        if updated.is_empty() {
            return;
        }
        let tracking = ctx.tracking.lock().unwrap();
        for (pos, entity) in updated {
            let chunk = (pos[0].div_euclid(16), pos[2].div_euclid(16));
            let ferrite_world::BlockEntity { type_id, mut data } = entity;
            data.shift_remove("x");
            data.shift_remove("y");
            data.shift_remove("z");
            let update = packet(&BlockEntityData {
                location: ferrite_protocol::types::BlockPos::new(pos[0], pos[1], pos[2]).pack(),
                block_type: VarInt(i32::try_from(type_id).unwrap_or(0)),
                nbt: data,
            });
            broadcast_tracked_chunk(&tracking, ctx.world_id, chunk, &update);
        }
    }

    /// Replaces a held final update for the same position, so a reversal before the two-tick
    /// settle cannot deliver an older extension after a newer retraction.
    fn queue_delayed_update(&mut self, update: DelayedUpdate) {
        self.delayed_updates.retain(|old| old.pos != update.pos);
        self.delayed_updates.push(update);
    }

    /// Ticks the region's redstone network after this tick's block changes, mirroring
    /// `Level.tickBlockEntities` + `Level.tickBlocks` + scheduled block ticks.
    fn tick_redstone(&mut self, ctx: &TickContext, changed: &mut Vec<[i32; 3]>) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let now = self.tick_clock;
        let mut world =
            super::fluid::RegionFluidWorld::new(source, &self.held_chunks, &ctx.tracking)
                .with_signals(ctx.signals.clone());
        let (due, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.delayed_updates)
            .into_iter()
            .partition(|update| update.due <= now);
        self.delayed_updates = waiting;
        let mut settled_pumpkins = Vec::new();
        for update in due {
            let current = super::redstone::RedstoneWorld::block(&world, update.pos);
            if !delayed_piston_update_matches_current(update.expected_state, current) {
                continue;
            }
            broadcast_tracked_chunk(
                &ctx.tracking.lock().unwrap(),
                ctx.dimension,
                update.chunk,
                &update.packet,
            );
            if super::golem::is_pumpkin_state(update.expected_state as u16) {
                settled_pumpkins.push(update.pos);
            }
        }
        self.moving_pistons.advance();
        self.redstone.blocks_changed(&mut world, changed);
        if self.redstone.has_plates() || !self.entities.keys().is_empty() {
            // A player's box follows the pose: 1.5 tall sneaking, 0.2 sleeping (a vanilla box
            // never rotates, so the facing does not matter).
            let players = self.players.values().map(|p| {
                super::redstone::Occupant::player_in_pose(p.pos, p.sleeping.is_some(), p.sneaking)
            });
            let occupants = players
                .chain(self.entities.keys().iter().filter_map(|&key| {
                    let at = self.entities.pos(key)?;
                    let data = self.entities.data(key)?;
                    Some(super::redstone::Occupant {
                        pos: [at.x, at.y, at.z],
                        half: data.width / 2.0,
                        height: data.height,
                        living: data.mob.is_some(),
                    })
                }))
                .collect();
            self.redstone.set_occupants(occupants);
        }
        if !self.entities.keys().is_empty() || self.redstone.has_frames() {
            self.redstone
                .set_item_frames(&item_frame_outputs(&self.entities));
        }
        if self.redstone.has_chests() || self.players.values().any(|p| p.menus.open_at.is_some()) {
            self.redstone.set_chest_viewers(&trapped_chest_viewers(
                self.players.values().map(|p| &p.menus),
            ));
        }
        self.redstone.tick(&mut world, &[]);
        let actions = self.redstone.take_piston_actions(&world);
        let mut piston_changes = Vec::new();
        let mut destroyed = Vec::new();
        let border = lock_game_state(&ctx.state, ctx.profiler.as_deref())
            .border
            .clone();
        for (pos, facing, extending) in actions {
            let Some(operation) = super::piston::operation(&world, pos, facing, extending) else {
                continue;
            };
            if operation.edits.iter().any(|(at, _, _)| {
                !super::piston::within_border(&border, *at)
                    || !super::piston::within_height(source.dimension(), *at)
            }) {
                continue;
            }
            if let Err(error) = source.compare_set_blocks(&operation.edits) {
                tracing::warn!(?pos, %error, "piston movement rejected");
                continue;
            }
            let chunk = (pos[0].div_euclid(16), pos[2].div_euclid(16));
            if let Some(state) = super::redstone::RedstoneWorld::block(&world, pos)
                && let Some((name, _)) = ferrite_data::blocks::state(state)
                && let Some(block) = ferrite_data::registries::id("minecraft:block", name)
            {
                let direction = match facing {
                    ferrite_math::Direction::Down => 0,
                    ferrite_math::Direction::Up => 1,
                    ferrite_math::Direction::North => 2,
                    ferrite_math::Direction::South => 3,
                    ferrite_math::Direction::West => 4,
                    ferrite_math::Direction::East => 5,
                };
                let event = BlockEvent {
                    pos: ferrite_protocol::types::BlockPos::new(pos[0], pos[1], pos[2]),
                    block: VarInt(block),
                    param_a: if extending { 0 } else { 1 },
                    param_b: direction,
                };
                broadcast_tracked_chunk(
                    &ctx.tracking.lock().unwrap(),
                    ctx.dimension,
                    chunk,
                    &packet(&event),
                );
            }
            let affected: HashSet<_> = operation.edits.iter().map(|&(at, _, _)| at).collect();
            let mut moving = Vec::new();
            for &moved in &operation.moves {
                moving.push(super::piston::MovingBlock::new(
                    moved.source,
                    moved.destination,
                    moved.state,
                    facing,
                    extending,
                    false,
                ));
            }
            if let Some(head) = operation.head {
                moving.push(super::piston::MovingBlock::new(
                    head.source,
                    head.destination,
                    head.state,
                    facing,
                    extending,
                    true,
                ));
            }
            let destinations: HashSet<_> = moving.iter().map(|block| block.destination).collect();
            // Clear the cells that do not receive a moving block immediately. The base changes
            // now; the animated destinations keep their moving_piston overlay until completion.
            for &(at, _, state) in &operation.edits {
                if at == pos || !destinations.contains(&at) {
                    let update = BlockUpdate {
                        location: ferrite_protocol::types::BlockPos::new(at[0], at[1], at[2])
                            .pack(),
                        block_state: VarInt(state as i32),
                    };
                    broadcast_tracked_chunk(
                        &ctx.tracking.lock().unwrap(),
                        ctx.dimension,
                        (at[0].div_euclid(16), at[2].div_euclid(16)),
                        &packet(&update),
                    );
                }
            }
            // `PistonMovingBlockEntity` is attached to the destination cell; its NBT carries the
            // moved state, direction and progress the client interpolates over two ticks.
            for block in &moving {
                let Some((update, entity)) = moving_piston_packets(block) else {
                    continue;
                };
                let chunk = (
                    block.destination[0].div_euclid(16),
                    block.destination[2].div_euclid(16),
                );
                broadcast_tracked_chunk(
                    &ctx.tracking.lock().unwrap(),
                    ctx.dimension,
                    chunk,
                    &packet(&update),
                );
                if let Some(entity) = entity {
                    broadcast_tracked_chunk(
                        &ctx.tracking.lock().unwrap(),
                        ctx.dimension,
                        chunk,
                        &packet(&entity),
                    );
                }
            }
            for &(at, _, state) in &operation.edits {
                let update = BlockUpdate {
                    location: ferrite_protocol::types::BlockPos::new(at[0], at[1], at[2]).pack(),
                    block_state: VarInt(state as i32),
                };
                self.delayed_updates.retain(|old| old.pos != at);
                self.delayed_updates.push(DelayedUpdate {
                    pos: at,
                    due: self.tick_clock.wrapping_add(2),
                    chunk: (at[0].div_euclid(16), at[2].div_euclid(16)),
                    expected_state: state,
                    packet: packet(&update),
                });
                piston_changes.push(at);
            }
            self.moving_pistons.start(moving, &affected);
            {
                // Entities standing where a block was pushed are carried one block along, when
                // there is room for them there (vanilla `PistonMovingBlockEntity.moveCollidedEntities`).
                // A pull moves the block toward the piston, and whatever stands on it follows.
                let (dx, dy, dz) = facing.step();
                let sign = if extending { 1 } else { -1 };
                let delta = ferrite_math::Vec3::new(
                    f64::from(dx * sign),
                    f64::from(dy * sign),
                    f64::from(dz * sign),
                );
                for &at in &operation.entity_push_cells {
                    // These coordinates are the occupied destination (extension) or source
                    // (retraction) cells. Retraction also reaches slightly above the cell for
                    // entities standing on top.
                    let cell = piston_entity_collision_box(at, extending);
                    for key in self.entities.query_vec(cell) {
                        let (Some(pos), Some(bbox)) =
                            (self.entities.pos(key), self.entities.bbox(key))
                        else {
                            continue;
                        };
                        if !bbox.intersects(cell)
                            || self
                                .entities
                                .flags(key)
                                .is_some_and(|f| f.has(ferrite_entity::Flags::REMOVED))
                        {
                            continue;
                        }
                        let moved = pos + delta;
                        // Vanilla `PistonMovingBlockEntity.moveCollidedEntities`: only a
                        // destination with no collision shape for the entity's full box moves
                        // it. Water and plants are replaceable and do not block the push.
                        let moved_bbox = ferrite_math::AABB::new(
                            bbox.min.x + delta.x,
                            bbox.min.y + delta.y,
                            bbox.min.z + delta.z,
                            bbox.max.x + delta.x,
                            bbox.max.y + delta.y,
                            bbox.max.z + delta.z,
                        );
                        if super::piston::entity_space_clear(&world, moved_bbox) {
                            self.entities.set_pos(key, moved);
                            if let Some(flags) = self.entities.flags_mut(key) {
                                flags.set(ferrite_entity::Flags::TELEPORTED, true);
                                flags.set(ferrite_entity::Flags::NEEDS_SYNC, true);
                            }
                        }
                    }
                    // Players are pushed the same way; their client is told with a position sync.
                    for player in self.players.values_mut() {
                        let bbox = ferrite_math::AABB::new(
                            player.pos[0] - 0.3,
                            player.pos[1],
                            player.pos[2] - 0.3,
                            player.pos[0] + 0.3,
                            player.pos[1] + 1.8,
                            player.pos[2] + 0.3,
                        );
                        if !bbox.intersects(cell) {
                            continue;
                        }
                        let moved = [
                            player.pos[0] + delta.x,
                            player.pos[1] + delta.y,
                            player.pos[2] + delta.z,
                        ];
                        let moved_bbox = ferrite_math::AABB::new(
                            moved[0] - 0.3,
                            moved[1],
                            moved[2] - 0.3,
                            moved[0] + 0.3,
                            moved[1] + 1.8,
                            moved[2] + 0.3,
                        );
                        if super::piston::entity_space_clear(&world, moved_bbox) {
                            player.pos = moved;
                            player.teleport();
                        }
                    }
                }
            }
            destroyed.extend(operation.destroyed);
        }
        if !piston_changes.is_empty() {
            self.redstone.blocks_changed(&mut world, &piston_changes);
        }
        for &(pos, state) in &destroyed {
            self.spawn_block_drops(source, pos, state);
        }
        for &pos in &piston_changes {
            self.updates.block_changed(pos);
        }
        changed.extend(piston_changes);
        for (pos, fuse) in self.redstone.take_primed() {
            self.spawn_tnt(pos, fuse);
        }
        for pos in settled_pumpkins {
            self.construct_golem(ctx, pos);
        }
    }
    /// Fires droppers and dispensers whose redstone input rose this tick (vanilla
    /// `DispenserBlock.neighborChanged`).
    fn tick_dispensers(&mut self, ctx: &TickContext) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let actions = {
            let Self {
                held_chunks,
                dispensers,
                redstone,
                ..
            } = self;
            let world = super::fluid::RegionFluidWorld::new(source, held_chunks, &ctx.tracking);
            dispensers.tick(source, redstone, &world)
        };
        for (_, action) in actions {
            match action {
                super::dispenser::Action::Arrow { pos, velocity } => {
                    let record = ferrite_entity::arrow::new_arrow(
                        ferrite_math::Vec3::new(pos[0], pos[1], pos[2]),
                        ferrite_math::Vec3::new(velocity[0], velocity[1], velocity[2]),
                        2.0,
                        None,
                        &mut self.rng,
                    );
                    self.entities.insert(record);
                }
                super::dispenser::Action::Throw { .. } | super::dispenser::Action::Drop { .. } => {
                    if let Some(record) = super::dispenser::item_entity(&action, &mut self.rng) {
                        self.entities.insert(record);
                    }
                }
                super::dispenser::Action::Splash { pos, stack } => {
                    self.splash_potion(pos, &stack);
                }
                super::dispenser::Action::Fire { pos, velocity } => {
                    self.entities.insert(arrow::new_thrown(
                        arrow::Kind::SmallFireball,
                        ferrite_math::Vec3::new(pos[0], pos[1], pos[2]),
                        ferrite_math::Vec3::new(velocity[0], velocity[1], velocity[2]),
                        None,
                    ));
                }
                super::dispenser::Action::PlaceBlock { pos, state } => {
                    self.place_blocks(ctx, &[(pos, state)]);
                    self.updates.block_changed(pos);
                    self.construct_golem(ctx, pos);
                }
                super::dispenser::Action::Nothing => {}
            }
        }
    }

    fn player_snapshot(p: &Player) -> ai::PlayerSnapshot {
        let wearing_gold = [
            ferrite_inventory::FEET,
            ferrite_inventory::LEGS,
            ferrite_inventory::CHEST,
            ferrite_inventory::HEAD,
        ]
        .into_iter()
        .any(|slot| {
            matches!(
                p.inventory.get(slot).name(),
                Some(
                    "minecraft:golden_boots"
                        | "minecraft:golden_leggings"
                        | "minecraft:golden_chestplate"
                        | "minecraft:golden_helmet"
                )
            )
        });
        let vel = if p.survival.last_pos != [0.0, 0.0, 0.0] {
            let dv = ferrite_math::Vec3::new(
                p.pos[0] - p.survival.last_pos[0],
                p.pos[1] - p.survival.last_pos[1],
                p.pos[2] - p.survival.last_pos[2],
            );
            if dv.length_sqr() <= 4.0 {
                dv
            } else {
                ferrite_math::Vec3::ZERO
            }
        } else {
            ferrite_math::Vec3::ZERO
        };
        ai::PlayerSnapshot {
            health: p.survival.health,
            has_slowness: p.effects.has(ferrite_entity::effect::Effect::Slowness),
            has_poison: p.effects.has(ferrite_entity::effect::Effect::Poison),
            has_weakness: p.effects.has(ferrite_entity::effect::Effect::Weakness),
            wearing_gold,
            sneaking: p.sneaking,
            vel,
        }
    }

    fn take_thrown_potion(&mut self, uuid: Uuid) -> Option<ferrite_item::ItemStack> {
        if let Some(stack) = self.thrown_potions.remove(&uuid) {
            return Some(stack);
        }
        let key = self
            .entities
            .keys()
            .iter()
            .copied()
            .find(|&key| self.entities.data(key).is_some_and(|d| d.uuid == uuid))?;
        self.entities
            .data_mut(key)
            .and_then(|d| d.thrown_potion.take())
    }

    /// Vanilla `ThrownSplashPotion.onHitAsPotion`: every living entity whose box (grown by the
    /// projectile margin) is within four blocks of the potion's box, found in the box inflated by
    /// 4 sideways and 2 up and down, gets the effects scaled by `1 - distance / 4`. Lasting effects
    /// last `scale * duration + 0.5` ticks (and are skipped when that is a second or less);
    /// instant ones act at `scale` of their strength.
    fn splash_potion(&mut self, center: [f64; 3], stack: &ferrite_item::ItemStack) {
        self.splash_potion_owned(center, stack, None);
    }

    fn splash_potion_owned(
        &mut self,
        center: [f64; 3],
        stack: &ferrite_item::ItemStack,
        owner: Option<Uuid>,
    ) {
        let effects = Self::bottle_effects(stack);
        if effects.is_empty() {
            return;
        }
        // `ProjectileUtil.computeMargin`: 0.3 once the potion has flown a few ticks.
        const MARGIN: f64 = 0.3;
        let reach = (
            [
                center[0] - 0.125 - 4.0,
                center[1] - 2.0,
                center[2] - 0.125 - 4.0,
            ],
            [
                center[0] + 0.125 + 4.0,
                center[1] + 0.25 + 2.0,
                center[2] + 0.125 + 4.0,
            ],
        );
        let inside = |min: [f64; 3], max: [f64; 3]| {
            (0..3).all(|axis| min[axis] < reach.1[axis] && max[axis] > reach.0[axis])
        };
        for p in self.players.values_mut() {
            let (half, height) = (0.3, if p.sneaking { 1.5 } else { 1.8 });
            let min = [p.pos[0] - half, p.pos[1], p.pos[2] - half];
            let max = [p.pos[0] + half, p.pos[1] + height, p.pos[2] + half];
            if !inside(min, max) {
                continue;
            }
            let Some(scale) = ferrite_entity::potion::splash_scale(center, (min, max), MARGIN)
            else {
                continue;
            };
            for instance in &effects {
                if instance.kind().is_instant() {
                    p.apply_instant_scaled(*instance, scale);
                } else if let Some(instance) =
                    ferrite_entity::potion::splash_duration(*instance, scale)
                {
                    p.apply_effect(instance);
                }
            }
        }
        let bounds = ferrite_math::AABB::new(
            reach.0[0], reach.0[1], reach.0[2], reach.1[0], reach.1[1], reach.1[2],
        );
        for key in self.entities.query_vec(bounds) {
            let (Some(pos), Some(data)) = (self.entities.pos(key), self.entities.data(key)) else {
                continue;
            };
            let mob_uuid = data.uuid;
            let mob_kind = data.mob.as_ref().map(|m| m.kind);
            let half = data.width / 2.0;
            let min = [pos.x - half, pos.y, pos.z - half];
            let max = [pos.x + half, pos.y + data.height, pos.z + half];
            if !inside(min, max) {
                continue;
            }
            let Some(scale) = ferrite_entity::potion::splash_scale(center, (min, max), MARGIN)
            else {
                continue;
            };
            for instance in &effects {
                if instance.kind().is_instant() {
                    // `Witch.getDamageAfterMagicAbsorb`: 0 damage when `source.getEntity() == this`.
                    if instance.kind() == ferrite_entity::effect::Effect::InstantDamage
                        && mob_kind == Some(MobKind::Witch)
                        && owner == Some(mob_uuid)
                    {
                        continue;
                    }
                    mob::apply_instant_scaled(&mut self.entities, key, *instance, scale);
                } else if let Some(instance) =
                    ferrite_entity::potion::splash_duration(*instance, scale)
                {
                    self.give_mob_effect(key, instance);
                }
            }
        }
    }

    /// Gives a mob an effect and queues the client update for whoever tracks it.
    fn give_mob_effect(
        &mut self,
        key: ferrite_entity::EntityKey,
        instance: ferrite_entity::effect::MobEffectInstance,
    ) {
        let Some(uuid) = self.entities.data(key).map(|data| data.uuid) else {
            return;
        };
        if let Some(stored) = mob::add_effect(&mut self.entities, key, instance) {
            self.pending_mob_effects
                .push((uuid, MobEffectNotice::Added(stored)));
        }
    }

    /// Ticks the effects of every mob that has any (wandering traders do their own in
    /// [`Self::tick_trader_drinks`]) and tells trackers about what was added or expired.
    fn tick_mob_effects(&mut self, ctx: &TickContext) {
        let mut keys = std::mem::take(&mut self.scratch_effect_keys);
        keys.clear();
        keys.extend(self.entities.keys().iter().copied().filter(|key| {
            self.entities.data(*key).is_some_and(|data| {
                !data.effects.is_empty()
                    && data
                        .mob
                        .as_ref()
                        .is_some_and(|mob| mob.kind != MobKind::WanderingTrader)
            })
        }));
        for &key in &keys {
            let Some(uuid) = self.entities.data(key).map(|data| data.uuid) else {
                continue;
            };
            for expired in mob::tick_effects(&mut self.entities, key) {
                self.pending_mob_effects
                    .push((uuid, MobEffectNotice::Removed(expired.effect)));
            }
        }
        self.scratch_effect_keys = keys;
        for (uuid, notice) in std::mem::take(&mut self.pending_mob_effects) {
            let Some(key) = self
                .entities
                .keys()
                .iter()
                .copied()
                .find(|key| self.entities.data(*key).is_some_and(|d| d.uuid == uuid))
            else {
                continue;
            };
            let Some(id) = self.entities.data(key).map(|data| VarInt(data.id)) else {
                continue;
            };
            let players = &self.players;
            let mut send_to = |viewer, bytes: &[u8]| deliver(players, ctx, viewer, bytes);
            match notice {
                MobEffectNotice::Added(instance) => tracker::broadcast(
                    &self.entities,
                    key,
                    &UpdateMobEffect {
                        entity_id: id,
                        effect: VarInt(instance.effect),
                        amplifier: VarInt(instance.amplifier),
                        duration: VarInt(instance.duration),
                        flags: instance.flags(),
                    },
                    &mut send_to,
                ),
                MobEffectNotice::Removed(effect) => tracker::broadcast(
                    &self.entities,
                    key,
                    &RemoveMobEffect {
                        entity_id: id,
                        effect: VarInt(effect),
                    },
                    &mut send_to,
                ),
            }
        }
    }

    /// The effects a potion bottle carries, or none when it is not a potion.
    fn bottle_effects(
        stack: &ferrite_item::ItemStack,
    ) -> Vec<ferrite_entity::effect::MobEffectInstance> {
        match stack.effective_component(ferrite_item::component_ids::POTION_CONTENTS) {
            Some(ferrite_item::Component::PotionContents(contents)) => {
                ferrite_entity::potion::effects_from_component(&contents)
            }
            Some(ferrite_item::Component::Opaque(tag)) => PotionContents::from_tag(&tag)
                .map(|contents| contents.all_effects())
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// Vanilla `ThrownPotion.makeAreaOfEffectCloud`: a lingering potion leaves a cloud where it
    /// lands. A bottle with no effects leaves nothing.
    #[cfg(test)]
    fn linger_potion(&mut self, center: ferrite_math::Vec3, stack: &ferrite_item::ItemStack) {
        self.linger_potion_owned(center, stack, None);
    }

    fn linger_potion_owned(
        &mut self,
        center: ferrite_math::Vec3,
        stack: &ferrite_item::ItemStack,
        owner: Option<Uuid>,
    ) {
        let effects = Self::bottle_effects(stack);
        if effects.is_empty() {
            return;
        }
        let color = match stack.effective_component(ferrite_item::component_ids::POTION_CONTENTS) {
            Some(ferrite_item::Component::PotionContents(contents)) => contents.custom_color,
            Some(ferrite_item::Component::Opaque(tag)) => {
                PotionContents::from_tag(&tag).and_then(|contents| contents.custom_color)
            }
            _ => None,
        };
        let cloud = ferrite_entity::potion::PotionCloud::lingering(center, &effects)
            .with_color(color)
            .with_owner(owner);
        self.entities.insert(cloud.spawn_record());
        self.potion_clouds.push(cloud);
    }

    /// Ticks every area effect cloud (lingering potions and dragon breath) and applies what it
    /// reaches to the players and mobs of this region.
    fn tick_potion_clouds(&mut self) {
        if self.potion_clouds.is_empty() {
            return;
        }
        let mut candidates: Vec<(Uuid, [f64; 3], f64, f64, bool)> = self
            .players
            .values()
            .filter(|p| p.gamemode != 3 && !p.survival.dead)
            .map(|p| {
                (
                    p.uuid,
                    p.pos,
                    0.6,
                    if p.sneaking { 1.5 } else { 1.8 },
                    false,
                )
            })
            .collect();
        candidates.extend(self.entities.keys().iter().filter_map(|&key| {
            let data = self.entities.data(key)?;
            let is_dragon = data
                .mob
                .as_ref()
                .is_some_and(|mob| mob.kind == MobKind::EnderDragon);
            let live = self
                .entities
                .flags(key)
                .is_some_and(|f| !f.has(Flags::REMOVED))
                && data.mob.as_ref().is_some_and(|mob| !mob.is_dead());
            let pos = self.entities.pos(key)?;
            live.then_some((
                data.uuid,
                [pos.x, pos.y, pos.z],
                data.width,
                data.height,
                is_dragon,
            ))
        }));
        let mut clouds = std::mem::take(&mut self.potion_clouds);
        let entities = &mut self.entities;
        let mut pending = Vec::new();
        clouds.retain_mut(|cloud| {
            let dragon_breath = cloud.is_dragon_breath();
            let Some(applied) = cloud.tick_boxes(
                candidates
                    .iter()
                    .filter(|&&(_, _, _, _, is_dragon)| !(dragon_breath && is_dragon))
                    .map(|&(id, pos, width, height, _)| (id, pos, width, height)),
            ) else {
                cloud.despawn(entities);
                return false;
            };
            cloud.sync(entities);
            for (uuid, effect) in applied {
                // A cloud applies instant effects at half strength.
                if effect.kind().is_instant() {
                    if let Some(p) = self.players.get_mut(&uuid) {
                        p.apply_instant_scaled(effect, 0.5);
                    } else if let Some(key) = entities
                        .keys()
                        .iter()
                        .copied()
                        .find(|key| entities.data(*key).is_some_and(|d| d.uuid == uuid))
                    {
                        mob::apply_instant_scaled(entities, key, effect, 0.5);
                    }
                } else if let Some(p) = self.players.get_mut(&uuid) {
                    p.apply_effect(effect);
                } else if let Some(key) = entities
                    .keys()
                    .iter()
                    .copied()
                    .find(|key| entities.data(*key).is_some_and(|d| d.uuid == uuid))
                    && let Some(stored) = mob::add_effect(entities, key, effect)
                {
                    pending.push((uuid, MobEffectNotice::Added(stored)));
                }
            }
            true
        });
        self.potion_clouds = clouds;
        self.pending_mob_effects.extend(pending);
    }

    /// A fire charge: light the facing block when fire may be placed there.
    fn ignite_block(&mut self, pos: [i32; 3], ctx: &TickContext) {
        if let Some((hooks, _)) = ctx.hooks.as_deref().and_then(SharedPluginHooks::load)
            && !hooks
                .fire_spread_allowed(ctx.dimension.key(), pos[0], pos[1], pos[2])
                .unwrap_or(true)
        {
            return;
        }
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let placed = {
            let mut world =
                super::fluid::RegionFluidWorld::new(source, &self.held_chunks, &ctx.tracking)
                    .with_signals(ctx.signals.clone());
            super::fire::placement_state(&world, pos).map(|state| {
                ferrite_fluid::FluidWorld::set_block(
                    &mut world,
                    pos[0],
                    pos[1],
                    pos[2],
                    u32::from(state),
                );
            })
        };
        if placed.is_some() {
            self.updates.block_changed(pos);
        }
    }

    /// Ticks primed TNT and runs its `Explosion` where a fuse ends.
    fn tick_tnt(&mut self, ctx: &TickContext) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let blocks = TickBlocks {
            source: Some(source),
            seen: Default::default(),
        };
        let centers = ferrite_entity::tnt::tick_tnt(&mut self.entities, &blocks);
        if centers.is_empty() {
            return;
        }
        for center in centers {
            self.explode(
                [center.x, center.y, center.z],
                super::explosion::TNT_POWER,
                true,
                ctx,
            );
        }
        // Vanilla `Explosion.finalizeExplosion` sets blocks with `Block.UPDATE_NEIGHBORS`.
        let mut broken = Vec::new();
        self.apply_neighbor_updates(source, ctx, &mut broken);
        for (pos, state) in broken {
            self.spawn_block_drops(source, pos, state);
        }
    }
    /// Fires Bukkit's `EntityChangeBlockEvent` for a non-player entity before it changes a
    /// block. A missing bridge allows the change; a failed event follows the configured policy.
    fn entity_change_allowed(
        &self,
        ctx: &TickContext,
        key: ferrite_entity::EntityKey,
        at: [i32; 3],
        from: &str,
        to: &str,
    ) -> bool {
        let Some(data) = self.entities.data(key) else {
            return true;
        };
        let Some(mob) = data.mob.as_ref() else {
            return true;
        };
        let Some((hooks, policy)) = self.damage_hooks(ctx) else {
            return true;
        };
        event_allows(Some(hooks.as_ref()), policy, |plugins| {
            plugins.entity_changed_block(
                data.uuid,
                mob.kind.name(),
                super::plugin_world_id(ctx.world_id),
                crate::game::plugin_types::BlockPosition {
                    x: at[0],
                    y: at[1],
                    z: at[2],
                },
                from,
                to,
            )
        })
    }

    /// The plugin bridge for events about non-player entities: the live handle on the tick
    /// context first (it works in a region without players), else a player's own handle.
    fn damage_hooks(
        &self,
        ctx: &TickContext,
    ) -> Option<(
        Arc<dyn crate::game::plugin_types::ServerHooks>,
        PluginEventPolicy,
    )> {
        resolve_damage_hooks(
            ctx.hooks.as_deref(),
            self.players.values().filter_map(|p| {
                p.plugins
                    .clone()
                    .map(|hooks| (hooks, p.plugin_event_errors))
            }),
        )
    }
    /// Vanilla `Explosion` with no player to credit.
    fn explode(&mut self, center: [f64; 3], power: f32, breaks: bool, ctx: &TickContext) {
        self.explode_by(center, power, breaks, None, ctx);
    }

    /// Vanilla `ServerExplosion` finalization: level event 2000 at the centre, with data 1 for a
    /// blast of radius 2 or more (the larger particle); the client plays the explosion sound from
    /// the same event, so every player within 64 blocks hears and sees it.
    fn explosion_effects(&self, center: [f64; 3], power: f32) {
        let at = ferrite_protocol::types::BlockPos::new(
            center[0].floor() as i32,
            center[1].floor() as i32,
            center[2].floor() as i32,
        );
        for p in self.players.values() {
            let dx = p.pos[0] - center[0];
            let dy = p.pos[1] - center[1];
            let dz = p.pos[2] - center[2];
            if dx * dx + dy * dy + dz * dz > 64.0 * 64.0 {
                continue;
            }
            send(
                p,
                &LevelEvent {
                    event: 2000,
                    location: at.pack(),
                    data: i32::from(power >= 2.0),
                    global: false,
                },
            );
        }
    }

    /// Vanilla `Explosion`: entity knockback and exposure first, then the ray cast blocks and
    /// their loot-table drops. `killer` receives credit for mobs the blast kills.
    fn explode_by(
        &mut self,
        center: [f64; 3],
        power: f32,
        breaks: bool,
        killer: Option<Uuid>,
        ctx: &TickContext,
    ) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        // Players are not in the entity store; vanilla pushes them like any other entity.
        let hits: Vec<(Uuid, Option<[f64; 3]>, f32)> = self
            .players
            .values()
            .filter_map(|p| {
                let bbox = ferrite_math::AABB::new(
                    p.pos[0] - 0.3,
                    p.pos[1],
                    p.pos[2] - 0.3,
                    p.pos[0] + 0.3,
                    p.pos[1] + 1.8,
                    p.pos[2] + 0.3,
                );
                let feet = ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]);
                let impulse = super::explosion::impulse(source, center, power, feet, bbox);
                let damage = super::explosion::damage(source, center, power, feet, bbox);
                (impulse.is_some() || damage > 0.0).then_some((p.uuid, impulse, damage))
            })
            .collect();
        for (uuid, impulse, damage) in hits {
            if let Some(p) = self.players.get_mut(&uuid) {
                if damage > 0.0 {
                    p.hurt(DamageKind::Explosion, damage);
                }
                if let Some(impulse) = impulse {
                    send(
                        p,
                        &SetEntityMotion {
                            id: VarInt(p.entity),
                            velocity: ferrite_protocol::types::LpVec3(impulse),
                        },
                    );
                }
            }
        }
        // Entity push (vanilla `Entity.push`).
        let radius = f64::from(power) * 2.0;
        let area = ferrite_math::AABB::new(
            center[0] - radius,
            center[1] - radius,
            center[2] - radius,
            center[0] + radius,
            center[1] + radius,
            center[2] + radius,
        );
        let mut chained = Vec::new();
        for key in self.entities.query_vec(area) {
            if self
                .entities
                .flags(key)
                .is_some_and(|flags| flags.has(ferrite_entity::Flags::REMOVED))
            {
                continue;
            }
            let (Some(pos), Some(bbox)) = (self.entities.pos(key), self.entities.bbox(key)) else {
                continue;
            };
            let damage = super::explosion::damage(source, center, power, pos, bbox);
            if damage > 0.0 {
                let hooks = self.damage_hooks(ctx);
                let hooks = hooks
                    .as_ref()
                    .map(|(hooks, policy)| (hooks.as_ref(), *policy));
                if super::explosion::hurt_mob(&mut self.entities, key, damage, hooks).is_some()
                    && let Some(killer) = killer
                {
                    mob::credit(&mut self.entities, key, killer);
                }
            }
            if let Some(impulse) = super::explosion::impulse(source, center, power, pos, bbox) {
                let vel = self.entities.vel(key).unwrap_or_default();
                self.entities.set_vel(
                    key,
                    ferrite_math::Vec3::new(
                        vel.x + impulse[0],
                        vel.y + impulse[1],
                        vel.z + impulse[2],
                    ),
                );
                if let Some(flags) = self.entities.flags_mut(key) {
                    flags.set(ferrite_entity::Flags::NEEDS_SYNC, true);
                }
            }
            // An explosion breaks any end crystal it reaches, and that one blows up too.
            if ferrite_entity::dragon::is_crystal(&self.entities, key) {
                if let Some(flags) = self.entities.flags_mut(key) {
                    flags.set(ferrite_entity::Flags::REMOVED, true);
                }
                chained.push(pos);
            }
        }
        for pos in chained {
            self.explode_by([pos.x, pos.y + 1.0, pos.z], 6.0, true, killer, ctx);
        }
        // Vanilla `ServerExplosion` finalizes the explosion even when blocks are kept: level
        // event 2000 makes the client show the particles and play the explosion sound.
        self.explosion_effects(center, power);
        if !breaks {
            return;
        }
        // Destroyed blocks, with the state read before it turns into air.
        let air = u32::from(ferrite_data::blocks::default_state("air").expect("vanilla air"));
        let mut destroyed;
        let mut removed = Vec::new();
        {
            let mut world =
                super::fluid::RegionFluidWorld::new(source, &self.held_chunks, &ctx.tracking);
            destroyed = super::explosion::explode_blocks(&world, center, power, &mut self.rng);
            // Bukkit fires BlockExplodeEvent before any block is removed; a cancel leaves the
            // terrain untouched. The event sees the same blocks the ray cast selected.
            if let Some((hooks, policy)) = self.damage_hooks(ctx) {
                let positions: Vec<crate::game::plugin_types::BlockPosition> = destroyed
                    .iter()
                    .map(|[x, y, z]| crate::game::plugin_types::BlockPosition {
                        x: *x,
                        y: *y,
                        z: *z,
                    })
                    .collect();
                let center_pos = crate::game::plugin_types::BlockPosition {
                    x: center[0].floor() as i32,
                    y: center[1].floor() as i32,
                    z: center[2].floor() as i32,
                };
                match hooks.blocks_exploded(
                    super::plugin_world_id(ctx.world_id),
                    center_pos,
                    &positions,
                    power,
                ) {
                    Ok(Some(kept)) => {
                        let kept: std::collections::HashSet<[i32; 3]> =
                            kept.iter().map(|p| [p.x, p.y, p.z]).collect();
                        destroyed.retain(|pos| kept.contains(pos));
                    }
                    Ok(None) => {}
                    Err(error) => {
                        let allow = policy == PluginEventPolicy::Allow;
                        tracing::warn!(%error, allow, "plugin block explode event failed");
                        if !allow {
                            destroyed.clear();
                        }
                    }
                }
            }
            for pos in &destroyed {
                // Never touch another region's blocks.
                let chunk = ChunkPos(pos[0].div_euclid(16), pos[2].div_euclid(16));
                if !self.held_chunks.contains_key(&chunk) {
                    continue;
                }
                let Some(state) =
                    super::neighbors::BlockWorld::block(&world, pos[0], pos[1], pos[2])
                else {
                    continue;
                };
                ferrite_fluid::FluidWorld::set_block(&mut world, pos[0], pos[1], pos[2], air);
                removed.push((*pos, state));
            }
        }
        for (pos, state) in removed {
            self.spawn_block_drops(source, pos, state);
        }
        for pos in &destroyed {
            self.updates.block_changed(*pos);
        }
        self.schedule_gravity(source, &destroyed);
    }
    /// Spawns what a player threw (Q, clicks outside a screen, overflowing crafts) as item
    /// entities in front of it. The entities carry the stacks' components.
    fn spawn_thrown(&mut self, p: &mut Player) {
        for (block, stack) in p.menus.spilled.drain(..) {
            for record in item::drop_from_container_stack(block, &stack, &mut self.rng) {
                self.entities.insert(record);
            }
        }
        for (kind, contents) in p.menus.launched.drain(..) {
            let feet = ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]);
            let eye = combat::eye_position(feet);
            // Vanilla `shootFromRotation(.., 1.5, 1.0)`; a potion is lobbed: 20 degrees upward
            // at half the speed (`ThrowablePotionItem`).
            let (pitch, power) = if kind.is_potion() {
                (p.pitch - 20.0, 0.5)
            } else {
                (p.pitch, 1.5)
            };
            let direction = combat::look_vector(p.yaw, pitch);
            let vel = ferrite_math::Vec3::new(
                direction.x * power,
                direction.y * power,
                direction.z * power,
            );
            let mut record = arrow::new_thrown(kind, eye, vel, Some(p.uuid));
            if let Some(contents) = contents {
                record.data.thrown_potion = Some(contents.clone());
                self.thrown_potions.insert(record.data.uuid, contents);
            }
            self.entities.insert(record);
        }
        for (side, op) in std::mem::take(&mut p.menus.shoulders) {
            self.apply_shoulder(p, side, op);
        }
        if let Some(entity) = p.bobber_cast.take()
            && !p.survival.dead
            && bobber::find_for(&self.entities, p.uuid).is_none()
        {
            let feet = ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]);
            let eye = combat::eye_position(feet);
            let mut record = bobber::cast(p.uuid, p.entity, eye, p.yaw, p.pitch, 0, &mut self.rng);
            if let Some(entity) = entity {
                record.data.uuid = entity;
            }
            self.entities.insert(record);
        }
        for slot in std::mem::take(&mut p.menus.fishing) {
            self.use_fishing_rod(p, slot);
        }
        for (at, amount) in std::mem::take(&mut p.menus.orbs) {
            // `ExperienceOrb.award(level, Vec3.atCenterOf(pos), amount)` then
            // `levelEvent(1042, pos, 0)` (the grindstone's use sound).
            let center = ferrite_math::Vec3::new(
                f64::from(at[0]) + 0.5,
                f64::from(at[1]) + 0.5,
                f64::from(at[2]) + 0.5,
            );
            for orb in experience::award(center, amount) {
                self.entities.insert(orb);
            }
            send(
                p,
                &LevelEvent {
                    event: 1042,
                    location: ferrite_protocol::types::BlockPos::new(at[0], at[1], at[2]).pack(),
                    data: 0,
                    global: false,
                },
            );
        }
        for stack in p.menus.thrown.drain(..) {
            let feet = ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]);
            let record =
                item::thrown_by_player_stack(feet, 1.62, p.yaw, p.pitch, &stack, &mut self.rng);
            self.entities.insert(record);
        }
    }
    /// A right click with a fishing rod: reels in the player's bobber, or casts a new one.
    fn use_fishing_rod(&mut self, p: &mut Player, slot: usize) {
        let rod = p.inventory.get(slot).clone();
        if p.survival.dead || !matches!(rod.name(), Some("minecraft:fishing_rod")) {
            return;
        }
        let feet = ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]);
        let eye = combat::eye_position(feet);
        let Some(key) = bobber::find_for(&self.entities, p.uuid) else {
            let lure = match rod.effective_component(ferrite_item::component_ids::ENCHANTMENTS) {
                Some(ferrite_item::Component::Enchantments(e)) => {
                    e.level("minecraft:lure").unwrap_or(0)
                }
                _ => 0,
            };
            let record = bobber::cast(p.uuid, p.entity, eye, p.yaw, p.pitch, lure, &mut self.rng);
            self.entities.insert(record);
            return;
        };
        let at = self.entities.pos(key).unwrap_or(eye);
        let Some(outcome) = bobber::reel(&mut self.entities, key, feet) else {
            return;
        };
        if let bobber::Reel::Caught { .. } = outcome {
            for stack in super::drops::fishing_catch(&rod, &mut self.rng) {
                // `FishingHook.retrieve` -> `FISHING_ROD_HOOKED`: the `item` predicate is tried
                // against each caught stack.
                super::recipe_unlock::push_plain(
                    p.uuid,
                    super::recipe_unlock::PlainEvent {
                        trigger: "minecraft:fishing_rod_hooked",
                        item: u16::try_from(stack.item_id)
                            .ok()
                            .and_then(ferrite_item::item_name)
                            .map(str::to_owned),
                        ..Default::default()
                    },
                );
                let vel = bobber::catch_velocity(at, eye);
                self.entities
                    .insert(item::new_item(at, vel, stack, &mut self.rng));
            }
            let experience = 1 + self.rng.next_int(6);
            for orb in experience::award(feet, experience) {
                self.entities.insert(orb);
            }
        }
        Self::wear_rod(p, slot, outcome.damage());
    }

    /// Wears the rod in `slot` by `damage` points outside creative mode; it breaks at its last.
    fn wear_rod(p: &mut Player, slot: usize, damage: i32) {
        if damage <= 0 || p.gamemode == 1 {
            return;
        }
        let stack = p.inventory.get(slot).clone();
        let Some(max) = ferrite_item::item_defaults(stack.item).and_then(|d| d.max_damage) else {
            return;
        };
        let worn = stack.damage() + damage;
        if worn >= max {
            p.inventory.remove(slot, 1);
        } else if let Some(stack) = worn_stack(stack, worn) {
            p.inventory.set(slot, stack);
        }
        send(
            p,
            &SetPlayerInventory {
                slot: VarInt(slot as i32),
                item: p.inventory.get(slot).clone(),
            },
        );
    }

    /// Ticks every fishing bobber. One whose owner left, died, is more than 32 blocks away or no
    /// longer holds a rod is discarded (`FishingHook.shouldStopFishing`).
    fn tick_bobbers(&mut self, blocks: &TickBlocks) {
        let keys: Vec<_> = self
            .entities
            .keys()
            .iter()
            .copied()
            .filter(|key| self.entities.data(*key).is_some_and(|d| d.bobber.is_some()))
            .collect();
        for key in keys {
            let Some(state) = self.entities.data(key).and_then(|d| d.bobber) else {
                continue;
            };
            let at = self.entities.pos(key).unwrap_or_default();
            let keeps_fishing = self.players.get(&state.owner).is_some_and(|p| {
                let holds_rod = [p.hand_slot(0), p.hand_slot(1)].into_iter().any(|slot| {
                    matches!(p.inventory.get(slot).name(), Some("minecraft:fishing_rod"))
                });
                let dx = p.pos[0] - at.x;
                let dy = p.pos[1] - at.y;
                let dz = p.pos[2] - at.z;
                !p.survival.dead
                    && holds_rod
                    && dx * dx + dy * dy + dz * dz <= bobber::MAX_DISTANCE_SQ
            });
            if !keeps_fishing {
                bobber::discard(&mut self.entities, key);
                continue;
            }
            bobber::tick(&mut self.entities, key, blocks, &mut self.rng);
        }
    }

    /// Runs the entity tracker against this region's players.
    fn track_entities(&mut self, ctx: &TickContext) {
        // `track_arc_batched` already returns immediately for an empty store. Return before
        // building the per-player viewer and sender vectors, which otherwise costs O(players)
        // every region tick even when there is nothing to track.
        if self.entities.is_empty() {
            return;
        }
        // Vanilla sends a vehicle's passenger list in `ServerEntity.sendPairingData`. The ride
        // table is keyed by UUID; publish the network ids on the tracker state before pairing.
        if super::ride::is_empty() {
            for data in self.entities.tracking_slices_mut().3 {
                data.tracker.passengers.clear();
            }
        } else {
            self.scratch_entity_ids.clear();
            for (uuid, player) in &self.players {
                self.scratch_entity_ids.insert(*uuid, player.entity);
            }
            for data in self.entities.tracking_slices_mut().3.iter() {
                self.scratch_entity_ids.insert(data.uuid, data.id);
            }
            let ids = &self.scratch_entity_ids;
            for data in self.entities.tracking_slices_mut().3 {
                data.tracker.passengers = super::ride::passengers_of(data.uuid)
                    .into_iter()
                    .filter_map(|rider| ids.get(&rider).copied())
                    .collect();
            }
        }
        let mut viewers = Vec::with_capacity(self.players.len());
        let mut outbound_senders = Vec::with_capacity(self.players.len());
        for p in self.players.values() {
            viewers.push(Viewer {
                key: (p.uuid, p.session),
                pos: ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]),
                view_distance: p.view,
                chunks: &p.loaded,
            });
            outbound_senders.push(&p.outbound);
        }
        let players = &self.players;
        tracker::track_arc_batched(&mut self.entities, &viewers, &mut |vi, viewer, pkts| {
            if let Some(vi) = vi {
                let outbound = outbound_senders[vi];
                if !pkts.is_empty() {
                    let _ = outbound.send(Outbound::Batch(pkts.to_vec()));
                }
            } else {
                for pkt in pkts {
                    deliver_arc(players, ctx, viewer, pkt);
                }
            }
        });
    }
    /// Vanilla `ServerPlayer.die` + `dropAllDeathLoot` + `Player.deathMessage`: drop the
    /// inventory and experience unless `keepInventory`, send the death screen and announce it.
    fn process_death(&mut self, p: &mut Player, ctx: &TickContext, rules: &SurvivalRules) {
        if p.survival.death_processed {
            return;
        }
        // Shoulder parrots leave before the inventory drops, so death cannot swallow them.
        self.release_shoulders(p);
        p.stop_spin();
        p.survival.death_processed = true;
        p.survival.eating = None;
        p.breaking = None;
        p.close_container();
        let survival_mode = matches!(p.gamemode, 0 | 2);
        let drop_allowed = !rules.keep_inventory && survival_mode;
        let initial_exp = if drop_allowed {
            p.experience_level.saturating_mul(7).min(100)
        } else {
            0
        };
        let mut dropped_stacks = Vec::new();
        let mut initial_drops = Vec::new();
        if drop_allowed {
            for slot in 0..ferrite_inventory::SIZE {
                let stack = p.inventory.get(slot);
                let Some(material) = (!stack.is_empty())
                    .then(|| ferrite_item::item_name(stack.item))
                    .flatten()
                else {
                    continue;
                };
                dropped_stacks.push(stack.clone());
                initial_drops.push(crate::game::plugin_types::ItemSnapshot {
                    material: material.to_owned(),
                    amount: u8::try_from(stack.count).unwrap_or(u8::MAX),
                });
            }
        }
        let kind = p.survival.death_cause.unwrap_or(DamageKind::GenericKill);
        if matches!(kind, DamageKind::Mob | DamageKind::Arrow) {
            super::recipe_unlock::push_event(p.uuid, super::recipe_unlock::Event::KilledByEntity);
        }
        let message = p
            .survival
            .death_message
            .take()
            .unwrap_or_else(|| kind.death_message(&p.name));
        let initial = crate::game::plugin_types::PlayerDeath {
            drops: initial_drops,
            dropped_exp: initial_exp,
            keep_inventory: !drop_allowed,
            message: message.clone(),
        };
        let mut outcome = crate::game::plugin_types::PlayerDeathOutcome {
            drops: initial.drops.clone(),
            dropped_exp: initial.dropped_exp,
            keep_inventory: initial.keep_inventory,
        };
        if let Some(plugins) = p.plugins.clone() {
            match plugins.player_died(p.uuid, &p.name, damage_cause(kind), &initial) {
                Ok(modified) => outcome = modified,
                Err(error) => {
                    tracing::warn!(%error, "plugin death event failed; using vanilla drops");
                }
            }
        }
        let keep_inventory = outcome.keep_inventory || !drop_allowed;
        if !keep_inventory {
            let pos = [
                p.pos[0].floor() as i32,
                p.pos[1].floor() as i32,
                p.pos[2].floor() as i32,
            ];
            for slot in 0..ferrite_inventory::SIZE {
                p.inventory.set(slot, ferrite_item::ItemStack::empty());
            }
            for stack in super::item_keep::drop_stacks(dropped_stacks, &outcome.drops) {
                match stack {
                    Ok(stack) => {
                        self.entities
                            .insert(item::pop_resource_stack(pos, &stack, &mut self.rng));
                    }
                    Err(dropped) => {
                        let _ = dropped;
                    }
                }
            }
            // `ServerPlayer.dropExperience`: `min(level * 7, 100)` points, split by
            // `ExperienceOrb.award` and spawned where the player died.
            let dropped = outcome.dropped_exp.max(0);
            if dropped > 0 {
                let feet = ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]);
                for record in experience::award(feet, dropped) {
                    self.entities.insert(record);
                }
            }
            p.experience_level = 0;
            p.experience_progress = 0.0;
            p.experience_total = 0;
            send(
                p,
                &SetExperience {
                    progress: 0.0,
                    level: VarInt(0),
                    total: VarInt(0),
                },
            );
            tracing::info!(player = %p.name, dropped, "player died; experience orbs dropped");
        }
        // `ServerPlayer.die` stops riding: the world loop detaches the corpse on its next plan.
        super::ride::request_dismount(p.uuid);
        send(
            p,
            &PlayerCombatKill {
                player_id: VarInt(p.entity),
                message: ferrite_protocol::types::PlainText(message.clone()),
            },
        );
        p.sync_health();
        if rules.show_death_messages {
            let update = packet(&SystemChat {
                content: ferrite_protocol::types::PlainText(message),
                overlay: false,
            });
            for (_, _, outbound) in ctx.tracking.lock().unwrap().values() {
                let _ = outbound.send(update.clone());
            }
        }
        tracing::info!(player = %p.name, ?kind, "player died");
    }

    pub fn tick(&mut self, ctx: &TickContext) {
        self.tick_clock = self.tick_clock.wrapping_add(1);
        let timings = self.timings.clone();
        let _tick = timings.as_ref().map(|t| t.scope("region-tick"));
        // Closes the previous stage scope and opens the next, so the profile shows where a region
        // tick goes; each stage is a child of `region-tick`.
        let mut stage = None;
        macro_rules! stage {
            ($name:expr) => {
                drop(stage.take());
                stage.replace(timings.as_ref().map(|t| t.scope($name)));
            };
        }
        let rules = SurvivalRules::from(&ctx.rules.lock().unwrap());
        let blocks = TickBlocks {
            source: ctx.source.as_deref(),
            seen: Default::default(),
        };
        stage!("players");
        let mut ids = std::mem::take(&mut self.scratch_ids);
        ids.clear();
        ids.extend(self.players.keys().copied());
        let mut explosions: Vec<PendingExplosion> = std::mem::take(&mut self.queued_explosions)
            .into_iter()
            .map(|(center, power, fire, breaks)| (center, power, fire, breaks, None))
            .collect();
        // Snapshot the live overlay so chunk streaming can replay it without borrowing the
        // region's animation state while players are removed for their turn.
        let moving_pistons = if self.moving_pistons.active().is_empty() {
            Vec::new()
        } else {
            self.moving_pistons.active().to_vec()
        };
        for id in ids.drain(..) {
            let mut p = self.players.remove(&id).unwrap();
            // The first tick of a login reads the parrot save, then announces it to viewers.
            // Shoulder state is keyed by player UUID, so it survives region and dimension
            // hand-offs without living in any one region's entity store.
            if super::ride::ensure_shoulder_entry(p.uuid) {
                let shoulders = super::entity_save::load_shoulders(p.playerdata.as_deref(), p.uuid);
                if !shoulders.is_empty() {
                    super::ride::restore_shoulders(p.uuid, shoulders);
                }
                self.announce_shoulders(&p);
            }
            // Vanilla drops the parrots when the player sneaks in the air.
            if p.sneaking && !p.ground {
                self.release_shoulders(&p);
            }
            let updated = {
                let mut trader = TraderSync {
                    entities: &mut self.entities,
                };
                // A restock or another viewer's sale may have changed the merchant since this
                // menu was last looked at. A menu whose merchant moved to another region is
                // closed here before any queued click is processed.
                let _ = trader.refresh(&mut p);
                match &timings {
                    Some(timings) => {
                        let _scope = timings.scope("player-update");
                        p.update(&mut trader)
                    }
                    None => p.update(&mut trader),
                }
            };
            if updated {
                let gateway_blocked = !self.gateway_accepts_teleport([
                    p.pos[0].floor() as i32,
                    p.pos[1].floor() as i32,
                    p.pos[2].floor() as i32,
                ]);
                p.tick_portal(gateway_blocked);
            }
            if updated {
                self.trample_turtle_eggs_for_player(&mut p, ctx);
                p.survival_tick(&blocks, &rules);
                if p.survival.dead {
                    self.process_death(&mut p, ctx, &rules);
                    if p.survival.dead && (p.respawn_requested || rules.immediate_respawn) {
                        p.respawn(ctx.spawn);
                    }
                }
            }
            if let Some(at) = p.gateway_used.take() {
                self.gateway_teleported(ctx, at);
            }
            self.spawn_thrown(&mut p);
            self.spawn_primed_tnt(&mut p);
            self.spawn_crystals(&mut p);
            self.build_golems(&mut p, ctx);
            let uuid = p.uuid;
            explosions.extend(
                p.explosions
                    .drain(..)
                    .map(move |(c, power, fire)| (c, power, fire, true, Some(uuid))),
            );
            for (pos, delay) in p.button_press.drain(..) {
                self.buttons.schedule(pos, delay);
            }
            for pos in p.ignited.drain(..) {
                let delay = super::fire::tick_delay(&mut self.rng);
                self.fire.schedule(pos, delay);
            }
            for require_los in p.piglin_alerts.drain(..) {
                self.trigger_piglin_guard_alert(
                    ctx,
                    &blocks,
                    uuid,
                    [p.pos[0], p.pos[1], p.pos[2]],
                    require_los,
                );
            }
            if !updated {
                let held = p.loaded.iter().map(|(x, z)| ChunkPos(*x, *z)).collect();
                let _ = p.signals.send(Signal::Leave {
                    world: p.world_id,
                    uuid: p.uuid,
                    chunk: p.chunk,
                    held,
                });
                continue;
            }
            if p.dimension != ctx.dimension || p.world_id != ctx.world_id {
                // A dimension or world transfer leaves this region family; the world loop owns
                // inserting the player into the target world's independent scheduler.
                let signals = p.signals.clone();
                let _ = signals.send(Signal::ChangeDimension {
                    player: Box::new(p),
                });
                continue;
            }
            let next = ChunkPos(
                (p.pos[0].floor() as i32).div_euclid(16),
                (p.pos[2].floor() as i32).div_euclid(16),
            );
            if next != p.chunk {
                p.chunk = next;
                p.refresh_chunks();
                let tx = p.signals.clone();
                let _ = tx.send(Signal::Transfer {
                    to: next,
                    player: Box::new(p),
                });
                continue;
            }
            match &timings {
                Some(timings) => {
                    let _scope = timings.scope("chunk-stream");
                    p.chunks_tick(&mut self.generator, &moving_pistons);
                }
                None => p.chunks_tick(&mut self.generator, &moving_pistons),
            }
            self.drop_broken_blocks(&mut p, ctx);
            self.players.insert(id, p);
        }
        self.scratch_ids = ids;
        stage!("explosions");
        // Vanilla `BedBlock.explodeBed` runs the ordinary `Explosion` and leaves fire behind.
        for (center, power, fire, breaks, killer) in explosions {
            self.explode_by(center, power, breaks, killer, ctx);
            if fire && let Some(source) = ctx.source.as_deref() {
                let pos = [
                    center[0].floor() as i32,
                    center[1].floor() as i32,
                    center[2].floor() as i32,
                ];
                let mut world =
                    super::fluid::RegionFluidWorld::new(source, &self.held_chunks, &ctx.tracking)
                        .with_signals(ctx.signals.clone());
                if let Some(state) = super::fire::placement_state(&world, pos) {
                    ferrite_fluid::FluidWorld::set_block(
                        &mut world,
                        pos[0],
                        pos[1],
                        pos[2],
                        u32::from(state),
                    );
                    self.updates.block_changed(pos);
                }
            }
        }
        stage!("buttons-and-block-updates");
        // Vanilla `ButtonBlock` scheduled release ticks.
        let mut button_changed = Vec::new();
        if let Some(source) = ctx.source.as_deref() {
            let mut world =
                super::fluid::RegionFluidWorld::new(source, &self.held_chunks, &ctx.tracking);
            self.buttons.tick(&mut world, &mut button_changed);
        }
        for pos in &button_changed {
            self.updates.block_changed(*pos);
        }
        // A chunk this region just took may have fluid, support or gravity cells that were
        // stopped at its edge while the neighbour was unloaded.
        if let Some(source) = ctx.source.as_deref() {
            self.wake_loaded_chunks(source, ctx.signals.clone());
        }
        let mut changed = self.tick_block_updates(ctx);
        changed.extend(button_changed);
        stage!("vaults");
        // Vanilla `VaultBlockEntity.Server.tick`: a vault wakes up near players and runs its
        // unlocking/ejecting timers. Only vaults in chunks this region holds are ticked.
        let mut vault_changed = Vec::new();
        if let Some(source) = ctx.source.as_deref() {
            self.vaults.discover(source, &self.held_chunks);
            if !changed.is_empty() {
                self.vaults
                    .blocks_changed(source, &changed, &self.held_chunks);
            }
            let mut players = std::mem::take(&mut self.scratch_vault_players);
            players.clear();
            players.extend(self.players.values().map(|p| super::vault::VaultPlayer {
                pos: p.pos,
                spectator: p.gamemode == 3,
            }));
            let vault_updates =
                self.vaults
                    .tick_with_delivered(source, &players, &self.held_chunks, |token| {
                        self.entities.keys().iter().any(|&key| {
                            self.entities
                                .data(key)
                                .is_some_and(|data| data.uuid == token)
                        })
                    });
            self.scratch_vault_players = players;
            for update in vault_updates {
                let super::vault::VaultUpdate {
                    pos,
                    block_state,
                    events,
                    items,
                    pending_items,
                } = update;
                if let Some(state) = block_state {
                    {
                        let mut world = super::fluid::RegionFluidWorld::new(
                            source,
                            &self.held_chunks,
                            &ctx.tracking,
                        );
                        ferrite_fluid::FluidWorld::set_block(
                            &mut world,
                            pos[0],
                            pos[1],
                            pos[2],
                            u32::from(state),
                        );
                    }
                    self.updates.block_changed(pos);
                    vault_changed.push(pos);
                }
                for event in events {
                    let update = packet(&LevelEvent {
                        event: event.event,
                        location: ferrite_protocol::types::BlockPos::new(
                            event.pos[0],
                            event.pos[1],
                            event.pos[2],
                        )
                        .pack(),
                        data: event.data,
                        global: false,
                    });
                    let tracking = ctx.tracking.lock().unwrap();
                    broadcast_tracked_chunk(
                        &tracking,
                        source.world_id(),
                        (event.pos[0].div_euclid(16), event.pos[2].div_euclid(16)),
                        &update,
                    );
                }
                let mut spawned = Vec::new();
                for reward in items {
                    let record = super::vault::ejected_item(pos, &reward, &mut self.rng);
                    spawned.push(self.entities.insert(record));
                }
                // The vault's ledger is written only after its item entities exist, so a
                // crash in between re-ejects a reward instead of losing it. A failed write
                // rolls the new entities back to keep the reward pending.
                if let Some(items) = pending_items
                    && !super::vault::commit_ejection(source, pos, &items)
                {
                    for key in spawned {
                        let _ = self.entities.remove(key);
                    }
                }
            }
        }
        changed.extend(vault_changed);
        stage!("hoppers");
        // Vanilla `HopperBlockEntity` ticks as a block entity; a region keeps only the
        // hoppers it has seen and ticks them every game tick (their own cooldown gates moves).
        if let Some(source) = ctx.source.as_deref() {
            if !changed.is_empty() {
                self.hoppers.blocks_changed(source, &changed);
                self.dispensers.blocks_changed(source, &changed);
            }
            let Self {
                hoppers,
                entities,
                rng,
                ..
            } = self;
            hoppers.tick(source, entities, rng);
        }
        let (is_raining, entities_run, day_time) = {
            let state = lock_game_state(&ctx.state, ctx.profiler.as_deref());
            (
                ctx.dimension == super::Dimension::Overworld
                    && state.weather != super::commands::Weather::Clear,
                state.more.tick.simulating(),
                state.clock_time,
            )
        };
        stage!("fire");
        // Vanilla `FireBlock.tick` runs on scheduled ticks and changes blocks through the same
        // neighbor-update path as any other block change.
        let mut fire_changed = Vec::new();
        if let Some(source) = ctx.source.as_deref() {
            let mut world =
                super::fluid::RegionFluidWorld::new(source, &self.held_chunks, &ctx.tracking)
                    .with_signals(ctx.signals.clone());
            self.fire.tick(
                &mut world,
                ctx.dimension,
                is_raining,
                &mut self.rng,
                &mut fire_changed,
            );
        }
        if !fire_changed.is_empty() {
            for pos in &fire_changed {
                self.updates.block_changed(*pos);
            }
            if let Some(source) = ctx.source.as_deref() {
                let mut broken = Vec::new();
                self.apply_neighbor_updates(source, ctx, &mut broken);
                for (pos, state) in broken {
                    self.spawn_block_drops(source, pos, state);
                }
            }
            changed.extend(fire_changed);
        }
        let remote = std::mem::take(&mut self.remote_changes);
        self.solidify_powder(ctx, &changed);
        self.solidify_powder(ctx, &remote);
        if let Some(source) = ctx.source.as_deref() {
            self.schedule_gravity(source, &changed);
            self.schedule_gravity(source, &remote);
        }
        stage!("gravity-redstone");
        self.tick_gravity(ctx);
        self.tick_redstone(ctx, &mut changed);
        self.tick_dispensers(ctx);
        self.tick_command_blocks(ctx, &changed);
        let blocks = TickBlocks {
            source: ctx.source.as_deref(),
            seen: Default::default(),
        };
        // `/tick freeze` stops every entity but players (vanilla `TickRateManager.isEntityFrozen`).
        if entities_run {
            item::tick_items(&mut self.entities, &blocks, &mut self.rng);
        }
        stage!("entities");
        let owner_attacks = self.resolve_attacks(ctx);
        self.resolve_uses(ctx);
        if entities_run {
            self.tick_arrows(ctx);
            self.tick_bobbers(&blocks);
            ferrite_entity::vehicle::tick_vehicles_with_riders(
                &mut self.entities,
                &blocks,
                &|uuid| !super::ride::passengers_of(uuid).is_empty(),
            );
            self.tick_horses(ctx);
            self.sync_trades();
            self.land_tridents();
            self.tick_dragon_effects(ctx);
            self.tick_potion_clouds();
            for player in self.players.values_mut() {
                player.send_held_maps();
            }
            self.tick_mob_effects(ctx);
            self.tick_mobs_with_owner_attacks(ctx, &blocks, &owner_attacks);
            self.raids.tick(
                &mut self.entities,
                &mut self.players,
                &mut self.rng,
                &blocks,
                ctx.dimension,
            );
            self.trial_spawners.tick(
                &mut self.entities,
                &mut self.players,
                &mut self.rng,
                &blocks,
                ctx.source.as_deref(),
            );
            for pos in self.trial_spawners.take_changed() {
                if let Some(source) = ctx.source.as_deref()
                    && let Some(state) = source.block(pos[0], pos[1], pos[2])
                {
                    let update = packet(&BlockUpdate {
                        location: ferrite_protocol::types::BlockPos::new(pos[0], pos[1], pos[2])
                            .pack(),
                        block_state: ferrite_protocol::VarInt(state as i32),
                    });
                    let chunk = (pos[0].div_euclid(16), pos[2].div_euclid(16));
                    broadcast_tracked_chunk(
                        &ctx.tracking.lock().unwrap(),
                        ctx.dimension,
                        chunk,
                        &update,
                    );
                }
            }
        }
        let mut alive = std::mem::take(&mut self.scratch_alive);
        alive.clear();
        alive.extend(
            self.players
                .values()
                .filter(|p| p.gamemode != 3 && !p.survival.dead)
                .map(|p| ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2])),
        );
        if entities_run {
            experience::tick_orbs(&mut self.entities, &blocks, &alive, &mut self.rng);
            self.pick_up_experience(ctx);
            self.pick_up_items(ctx);
            self.tick_falling(ctx);
            self.tick_tnt(ctx);
            self.tick_fireworks(ctx);
            self.tick_eyes(ctx);
        }
        self.scratch_alive = alive;
        stage!("containers");
        self.tick_beacons(ctx);
        self.tick_campfires(ctx);
        self.tick_beehives_at(ctx, day_time);
        for p in self.players.values_mut() {
            p.tick_containers();
        }
        stage!("track-entities");
        self.track_entities(ctx);
        self.entities.sweep_removed();
        for entity in self.entities.take_crossing() {
            let (x, z) = entity.chunk();
            let _ = ctx.signals.send(Signal::EntityTransfer {
                world_id: ctx.world_id,
                to: ChunkPos(x, z),
                entity: Box::new(entity),
            });
        }
        stage!("finish");
        self.publish_command_entities(&ctx.command_entities, ctx.dimension, ctx.world_id);
    }

    /// Vanilla `CampfireBlockEntity`: every loaded campfire cooks its food for one tick (lit)
    /// or cools it down (unlit). A finished item is dropped at the block like vanilla's
    /// `Containers.dropItemStack`, not left in the slot.
    fn tick_campfires(&mut self, ctx: &TickContext) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let held = &self.held_chunks;
        let campfires = source.block_entity_positions_in_held(held, |_pos, entity| {
            matches!(
                entity.data.get("id"),
                Some(ferrite_nbt::Tag::String(id)) if id == "minecraft:campfire"
            )
        });
        for pos in campfires {
            let Some(state) = source
                .block(pos[0], pos[1], pos[2])
                .and_then(|state| u16::try_from(state).ok())
            else {
                continue;
            };
            let Some((name, props)) = ferrite_data::blocks::state(state) else {
                continue;
            };
            if !matches!(name, "minecraft:campfire" | "minecraft:soul_campfire") {
                continue;
            }
            let lit = props
                .iter()
                .any(|(key, value)| *key == "lit" && *value == "true");
            if lit {
                for stack in
                    super::campfire::tick(source, pos, &|item| super::campfire_recipe(item))
                {
                    let record = item::pop_resource_stack(pos, &stack, &mut self.rng);
                    self.entities.insert(record);
                }
            } else {
                super::campfire::cool(source, pos);
            }
        }
    }

    /// Vanilla `BeehiveBlockEntity.serverTick` and the bee side of hive home behaviour.
    ///
    /// Hive state lives in the block entity of a chunk this region holds. A bee entering a hive
    /// leaves the entity store as one complete record; one leaving is inserted at the exit and
    /// handed to the owning region when that exit is in another region section. Nothing here
    /// reads another region's blocks or entities.
    #[cfg(test)]
    fn tick_beehives(&mut self, ctx: &TickContext) {
        let day_time = lock_game_state(&ctx.state, ctx.profiler.as_deref()).clock_time;
        self.tick_beehives_at(ctx, day_time);
    }

    fn tick_beehives_at(&mut self, ctx: &TickContext, day_time: i64) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let daylight = is_bright_outside(ctx.dimension, day_time);
        // `EnvironmentAttributes.BEES_STAY_IN_HIVE`: night or rain keeps bees inside. A sedated
        // hive still releases on schedule; the campfire only calms anger from a harvest.
        let stay_in_hive = !daylight || ctx.raining;

        // Non-player destruction (explosion, piston, fluid) took the block entity and queued the
        // occupants for this tick, where a `TickContext` exists for the hand-off.
        let breaks = std::mem::take(&mut self.pending_hive_breaks);
        for (pos, state, data) in breaks {
            self.release_hive_data(ctx, pos, state, &data, false);
        }

        // Player destruction: the block entity was taken before the loot roll, so the dropped
        // item can still carry its bees.
        let broken: Vec<_> = self
            .players
            .values_mut()
            .flat_map(|player| player.broken_hives.drain(..))
            .collect();
        for (pos, state, player, data) in broken {
            // Vanilla angers the bees already outside before the occupants join them.
            self.anger_nearby_bees(pos, player);
            self.release_hive_data(ctx, pos, state, &data, false);
        }

        // `BeehiveBlock.useItemOn`: harvesting without a lit campfire below empties the hive and
        // angers nearby bees; shears and bottles both reach this path.
        let harvests: Vec<_> = self
            .players
            .values_mut()
            .flat_map(|player| player.hive_harvests.drain(..))
            .collect();
        for (pos, player, shears) in harvests {
            if shears {
                self.play_sound_near(
                    ferrite_math::Vec3::new(
                        f64::from(pos[0]) + 0.5,
                        f64::from(pos[1]) + 0.5,
                        f64::from(pos[2]) + 0.5,
                    ),
                    "minecraft:block.beehive.shear",
                    1.0,
                    1.0,
                );
            }
            if super::beehive::is_sedated(source, pos) {
                continue;
            }
            let data = super::beehive::load(source, pos);
            if data.occupants.is_empty() {
                continue;
            }
            let state = source
                .block(pos[0], pos[1], pos[2])
                .and_then(|state| u16::try_from(state).ok())
                .unwrap_or(0);
            self.anger_nearby_bees(pos, player);
            self.release_hive_data(ctx, pos, state, &data, true);
        }

        // Only hives in chunks this region holds are read or written.
        let held = &self.held_chunks;
        let hives = source.block_entity_positions_in_held(held, |_pos, entity| {
            matches!(
                entity.data.get("id"),
                Some(ferrite_nbt::Tag::String(id)) if id == super::beehive::BLOCK_ENTITY
            )
        });
        for pos in &hives {
            self.tick_hive_occupants(ctx, *pos, stay_in_hive);
        }
        self.tick_bee_citizens(ctx, &hives, stay_in_hive);
    }

    /// `BeehiveBlockEntity.tickOccupants`: every occupant ages one tick; one whose minimum stay
    /// is over leaves (unless night/rain holds it in), delivering honey when it carried nectar.
    fn tick_hive_occupants(&mut self, ctx: &TickContext, pos: [i32; 3], stay_in_hive: bool) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        if !self
            .held_chunks
            .contains_key(&ChunkPos(pos[0].div_euclid(16), pos[2].div_euclid(16)))
        {
            return;
        }
        let Some(state) = source
            .block(pos[0], pos[1], pos[2])
            .and_then(|state| u16::try_from(state).ok())
        else {
            return;
        };
        if !ferrite_data::blocks::state(state)
            .is_some_and(|(name, _)| super::beehive::is_hive(name))
        {
            return;
        }
        let mut data = super::beehive::load(source, pos);
        if data.occupants.is_empty() {
            return;
        }
        let mut honey = super::beehive::honey_level(state).unwrap_or(0);
        let mut honey_changed = false;
        let mut released = Vec::new();
        let mut index = 0;
        while index < data.occupants.len() {
            let due = data.occupants[index].tick();
            if !due || stay_in_hive {
                index += 1;
                continue;
            }
            let occupant = data.occupants.remove(index);
            let nectar = occupant.has_nectar();
            let Some(mut record) = self.release_record(ctx, pos, state, &occupant, false) else {
                // A blocked exit or an exit in a chunk this region does not own keeps the bee
                // inside, exactly like vanilla's refused `releaseOccupant`.
                data.occupants.insert(index, occupant);
                index += 1;
                continue;
            };
            if let Some(mob) = record.data.mob.as_mut() {
                if mob.hive_pos.is_none() {
                    mob.hive_pos = Some(pos);
                }
                if mob.flower_pos.is_none() && self.rng.next_int(10) != 0 {
                    mob.flower_pos = data.flower_pos;
                }
                if nectar {
                    mob.has_nectar = false;
                    mob.crops_grown = 0;
                    mob.ticks_since_pollination = 0;
                }
            }
            super::beehive::sync_record(&mut record);
            if nectar {
                honey = super::beehive::delivered_honey(honey, &mut self.rng);
                honey_changed = true;
                self.play_sound_near(
                    ferrite_math::Vec3::new(
                        f64::from(pos[0]) + 0.5,
                        f64::from(pos[1]) + 0.5,
                        f64::from(pos[2]) + 0.5,
                    ),
                    "minecraft:block.beehive.work",
                    1.0,
                    1.0,
                );
            }
            released.push(record);
        }
        // `BeeData.tick` mutates every tick, so flush the occupant clocks periodically as well
        // as whenever an occupant leaves. This keeps a restart from losing the whole stay while
        // not re-encoding the chunk for a quiet hive every tick.
        if !released.is_empty() || self.tick_clock.rem_euclid(20) == 0 {
            super::beehive::store(source, pos, &data);
        }
        if honey_changed && let Some(next) = super::beehive::with_honey_level(state, honey) {
            self.place_blocks(ctx, &[(pos, next)]);
        }
        for record in released {
            self.spawn_released_bee(ctx, pos, record);
        }
    }

    /// Releases every occupant of a destroyed or emptied hive. `keep_hive` leaves an empty block
    /// entity behind for a harvest; a broken hive has none.
    fn release_hive_data(
        &mut self,
        ctx: &TickContext,
        pos: [i32; 3],
        state: u16,
        data: &super::beehive::HiveData,
        keep_hive: bool,
    ) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let sedated = super::beehive::is_sedated(source, pos);
        for occupant in &data.occupants {
            let Some(mut record) = self.release_record(ctx, pos, state, occupant, true) else {
                continue;
            };
            if let Some(mob) = record.data.mob.as_mut() {
                if mob.hive_pos.is_none() {
                    mob.hive_pos = Some(pos);
                }
                if mob.flower_pos.is_none() && self.rng.next_int(10) != 0 {
                    mob.flower_pos = data.flower_pos;
                }
                if sedated {
                    mob.stay_out_of_hive = super::beehive::MIN_TICKS_BEFORE_REENTERING_HIVE;
                }
            }
            super::beehive::sync_record(&mut record);
            self.spawn_released_bee(ctx, pos, record);
        }
        if keep_hive {
            super::beehive::store(
                source,
                pos,
                &super::beehive::HiveData {
                    occupants: Vec::new(),
                    flower_pos: data.flower_pos,
                },
            );
        }
    }

    /// Builds the live bee at a hive exit. A non-emergency release refuses an exit in a chunk
    /// this region does not own; an emergency release falls back to the hive centre so a broken
    /// hive cannot swallow its bees.
    fn release_record(
        &mut self,
        ctx: &TickContext,
        pos: [i32; 3],
        state: u16,
        occupant: &super::beehive::Occupant,
        emergency: bool,
    ) -> Option<EntityRecord> {
        let source = ctx.source.as_deref()?;
        let mut record = super::beehive::release(source, pos, state, occupant, emergency)?;
        let chunk = (
            (record.pos.x.floor() as i32).div_euclid(16),
            (record.pos.z.floor() as i32).div_euclid(16),
        );
        if !self.held_chunks.contains_key(&ChunkPos(chunk.0, chunk.1)) {
            if !emergency {
                return None;
            }
            let height = f64::from(MobKind::Bee.props().height);
            record = occupant.record_at([
                f64::from(pos[0]) + 0.5,
                f64::from(pos[1]) + 0.5 - height / 2.0,
                f64::from(pos[2]) + 0.5,
            ])?;
        }
        Some(record)
    }

    /// Inserts a released bee in this region, or hands it to the region that owns the exit
    /// chunk. The test is a region-section comparison (`EntityStore`'s shift is four chunks), so
    /// a hive near a region seam does not produce an entity in a region that does not own it.
    fn spawn_released_bee(&mut self, ctx: &TickContext, hive: [i32; 3], record: EntityRecord) {
        self.play_sound_near(
            ferrite_math::Vec3::new(
                f64::from(hive[0]) + 0.5,
                f64::from(hive[1]) + 0.5,
                f64::from(hive[2]) + 0.5,
            ),
            "minecraft:block.beehive.exit",
            1.0,
            1.0,
        );
        let at = record.pos;
        let chunk = (
            (at.x.floor() as i32).div_euclid(16),
            (at.z.floor() as i32).div_euclid(16),
        );
        let home_section = (hive[0].div_euclid(16) >> 4, hive[2].div_euclid(16) >> 4);
        let target_section = (chunk.0 >> 4, chunk.1 >> 4);
        if target_section != home_section {
            let _ = ctx.signals.send(Signal::EntityTransfer {
                world_id: ctx.world_id,
                to: ChunkPos(chunk.0, chunk.1),
                entity: Box::new(record),
            });
        } else {
            self.entities.insert(record);
        }
    }

    /// `BeehiveBlock.angerNearbyBees`: bees in the box around a disturbed hive target the
    /// nearest player, unless they already have a target. Stung bees are handled by the AI.
    fn anger_nearby_bees(&mut self, hive: [i32; 3], player: Uuid) {
        let cx = f64::from(hive[0]) + 0.5;
        let cy = f64::from(hive[1]) + 0.5;
        let cz = f64::from(hive[2]) + 0.5;
        let keys: Vec<_> = self.entities.keys().to_vec();
        for key in keys {
            let Some(at) = self.entities.pos(key) else {
                continue;
            };
            if (at.x - cx).abs() > 8.0 || (at.y - cy).abs() > 6.0 || (at.z - cz).abs() > 8.0 {
                continue;
            }
            let Some(state) = mob::state(&self.entities, key) else {
                continue;
            };
            if state.kind != MobKind::Bee || state.target.is_some() {
                continue;
            }
            if let Some(state) = mob::state_mut(&mut self.entities, key) {
                state.target = Some(player);
                state.anger = 400 + self.rng.next_int(380) as u16;
            }
        }
    }

    /// The bee side of hive behaviour: assign a home, enter at night/rain or with nectar, find a
    /// flower in daylight, and grow `#bee_growables` while carrying nectar.
    fn tick_bee_citizens(&mut self, ctx: &TickContext, hives: &[[i32; 3]], stay_in_hive: bool) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let bees: Vec<_> = self
            .entities
            .keys()
            .iter()
            .copied()
            .filter(|&key| {
                self.entities
                    .data(key)
                    .and_then(|data| data.mob)
                    .is_some_and(|mob| mob.kind == MobKind::Bee)
            })
            .collect();
        for key in bees {
            let Some(pos) = self.entities.pos(key) else {
                continue;
            };
            let Some(mut state) = self.entities.data(key).and_then(|data| data.mob) else {
                continue;
            };
            if state.stay_out_of_hive > 0 {
                let stay = state.stay_out_of_hive - 1;
                if let Some(mob) = mob::state_mut(&mut self.entities, key) {
                    mob.stay_out_of_hive = stay;
                }
                state.stay_out_of_hive = stay;
            }
            if state.hive_pos.is_none()
                && let Some(hive) =
                    nearest_hive(pos, hives, ferrite_entity::bee::HIVE_SEARCH_DISTANCE)
            {
                if let Some(mob) = mob::state_mut(&mut self.entities, key) {
                    mob.hive_pos = Some(hive);
                }
                state.hive_pos = Some(hive);
            }
            if let Some(hive) = state.hive_pos {
                let distance = hive_distance(pos, hive);
                let wants_home_without_nectar = u32::from(state.ticks_without_nectar)
                    >= ferrite_entity::bee::TICKS_WITHOUT_NECTAR_BEFORE_GOING_HOME;
                if state.stay_out_of_hive == 0
                    && distance <= ferrite_entity::bee::HIVE_CLOSE_ENOUGH_DISTANCE + 1.0
                    && (stay_in_hive || state.has_nectar || wants_home_without_nectar)
                    && self.enter_hive(ctx, key, hive)
                {
                    continue;
                }
                if distance > ferrite_entity::bee::TOO_FAR_DISTANCE {
                    if let Some(mob) = mob::state_mut(&mut self.entities, key) {
                        mob.hive_pos = None;
                    }
                    state.hive_pos = None;
                }
            }
            if stay_in_hive || state.no_ai {
                continue;
            }
            if !state.has_nectar {
                let mut flower = state.flower_pos.filter(|&at| is_flower(source, at));
                if flower.is_none() && state.flower_pos.is_some() {
                    if let Some(mob) = mob::state_mut(&mut self.entities, key) {
                        mob.flower_pos = None;
                    }
                    state.flower_pos = None;
                }
                if flower.is_none()
                    && state.ticks_without_nectar % 20 == 0
                    && let Some(found) = find_flower(source, pos, 5)
                {
                    if let Some(mob) = mob::state_mut(&mut self.entities, key) {
                        mob.flower_pos = Some(found);
                        mob.ticks_since_pollination = 0;
                    }
                    flower = Some(found);
                    state.flower_pos = Some(found);
                    state.ticks_since_pollination = 0;
                }
                if let Some(flower) = flower {
                    let target = ferrite_math::Vec3::new(
                        f64::from(flower[0]) + 0.5,
                        f64::from(flower[1]) + 0.5,
                        f64::from(flower[2]) + 0.5,
                    );
                    if pos.distance_to_sqr(target) <= 4.0 {
                        if state.ticks_since_pollination >= ferrite_entity::bee::POLLINATE_TICKS {
                            ferrite_entity::bee::set_has_nectar(&mut self.entities, key, true);
                            state.has_nectar = true;
                            state.ticks_since_pollination = 0;
                            state.ticks_without_nectar = 0;
                        } else {
                            let ticks = state.ticks_since_pollination.saturating_add(1);
                            if let Some(mob) = mob::state_mut(&mut self.entities, key) {
                                mob.ticks_since_pollination = ticks;
                            }
                            state.ticks_since_pollination = ticks;
                        }
                    } else {
                        if state.ticks_since_pollination != 0 {
                            if let Some(mob) = mob::state_mut(&mut self.entities, key) {
                                mob.ticks_since_pollination = 0;
                            }
                            state.ticks_since_pollination = 0;
                        }
                        let ticks = state.ticks_without_nectar.saturating_add(1);
                        if let Some(mob) = mob::state_mut(&mut self.entities, key) {
                            mob.ticks_without_nectar = ticks;
                        }
                        state.ticks_without_nectar = ticks;
                    }
                } else {
                    let ticks = state.ticks_without_nectar.saturating_add(1);
                    if let Some(mob) = mob::state_mut(&mut self.entities, key) {
                        mob.ticks_without_nectar = ticks;
                    }
                    state.ticks_without_nectar = ticks;
                }
            }
            if state.has_nectar {
                if let Some(mob) = mob::state_mut(&mut self.entities, key) {
                    mob.ticks_since_pollination = mob.ticks_since_pollination.saturating_add(1);
                }
                self.try_grow_bee_crop(ctx, key);
            }
        }
    }

    /// Moves a bee into a hive block entity and out of the entity store.
    fn enter_hive(
        &mut self,
        ctx: &TickContext,
        key: ferrite_entity::EntityKey,
        hive: [i32; 3],
    ) -> bool {
        let Some(source) = ctx.source.as_deref() else {
            return false;
        };
        if !self
            .held_chunks
            .contains_key(&ChunkPos(hive[0].div_euclid(16), hive[2].div_euclid(16)))
        {
            return false;
        }
        let Some(state) = source
            .block(hive[0], hive[1], hive[2])
            .and_then(|state| u16::try_from(state).ok())
        else {
            return false;
        };
        if !ferrite_data::blocks::state(state)
            .is_some_and(|(name, _)| super::beehive::is_hive(name))
        {
            return false;
        }
        let mut data = super::beehive::load(source, hive);
        if data.occupants.len() >= super::beehive::MAX_OCCUPANTS {
            return false;
        }
        let Some(record) = self.entities.remove(key) else {
            return false;
        };
        if let Some(flower) = record.data.mob.as_ref().and_then(|mob| mob.flower_pos)
            && (data.flower_pos.is_none() || self.rng.next_int(2) == 0)
        {
            data.flower_pos = Some(flower);
        }
        if !data.add(&record) {
            self.entities.insert(record);
            return false;
        }
        super::beehive::store(source, hive, &data);
        self.play_sound_near(
            ferrite_math::Vec3::new(
                f64::from(hive[0]) + 0.5,
                f64::from(hive[1]) + 0.5,
                f64::from(hive[2]) + 0.5,
            ),
            "minecraft:block.beehive.enter",
            1.0,
            1.0,
        );
        true
    }

    /// `BeeGrowCropGoal`: one attempt in thirty, a `#bee_growables` block one or two below the
    /// bee, at most ten crops between pollinations.
    fn try_grow_bee_crop(&mut self, ctx: &TickContext, key: ferrite_entity::EntityKey) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let Some(state) = mob::state(&self.entities, key) else {
            return;
        };
        if state.crops_grown >= ferrite_entity::bee::MAX_CROPS_GROWN || self.rng.next_int(30) != 0 {
            return;
        }
        let Some(at) = self.entities.pos(key) else {
            return;
        };
        let x = at.x.floor() as i32;
        let y = at.y.floor() as i32;
        let z = at.z.floor() as i32;
        for below in 1..=2 {
            let target = [x, y - below, z];
            if !self.held_chunks.contains_key(&ChunkPos(
                target[0].div_euclid(16),
                target[2].div_euclid(16),
            )) {
                continue;
            }
            let Some(block) = source
                .block(target[0], target[1], target[2])
                .and_then(|state| u16::try_from(state).ok())
            else {
                continue;
            };
            let Some(next) = super::beehive::grow_crop(block) else {
                continue;
            };
            self.place_blocks(ctx, &[(target, next)]);
            if let Some(mob) = mob::state_mut(&mut self.entities, key) {
                mob.crops_grown = mob.crops_grown.saturating_add(1);
            }
            break;
        }
    }

    /// Vanilla `BeaconBlockEntity`'s 80-tick timer: recompute each beacon's pyramid, resend the
    /// beam height and refresh the effects of the players inside the range.
    fn tick_beacons(&mut self, ctx: &TickContext) {
        self.beacon_clock = self.beacon_clock.wrapping_add(1);
        if !self
            .beacon_clock
            .is_multiple_of(super::beacon::EFFECT_INTERVAL)
        {
            return;
        }
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let Some(block) = ferrite_data::registries::id("minecraft:block", "minecraft:beacon")
        else {
            return;
        };
        let held = &self.held_chunks;
        let beacons = source.block_entity_positions_in_held(held, |_pos, entity| {
            matches!(
                entity.data.get("id"),
                Some(ferrite_nbt::Tag::String(id)) if id == "minecraft:beacon"
            )
        });
        for pos in beacons {
            let mut data = super::beacon::load(source, pos);
            let levels = super::beacon::scan_levels(source, pos);
            if data.levels != levels {
                let was_active = data.levels > 0;
                data.levels = levels;
                super::beacon::store(source, pos, &data);
                if !was_active && levels > 0 {
                    // `BeaconBlockEntity.tick`: on activation every player within the box
                    // `(x, y - 4, z)` grown by (10, 5, 10) gets `construct_beacon`.
                    for player in self.players.values() {
                        if super::beacon::near_activation(pos, player.pos) {
                            super::recipe_unlock::push_plain(
                                player.uuid,
                                super::recipe_unlock::PlainEvent {
                                    trigger: "minecraft:construct_beacon",
                                    level: Some(levels),
                                    ..Default::default()
                                },
                            );
                        }
                    }
                }
            }
            if levels <= 0 {
                continue;
            }
            // Vanilla `BeaconBlockEntity.triggerEvent`: event 1 sets the client's beam height.
            let event = ferrite_protocol::packets::play::BlockEvent {
                pos: ferrite_protocol::types::BlockPos::new(pos[0], pos[1], pos[2]),
                block: VarInt(block),
                param_a: 1,
                param_b: u8::try_from(levels).unwrap_or(0),
            };
            for player in self.players.values() {
                send(player, &event);
            }
            let effects = super::beacon::effects(&data);
            for player in self.players.values_mut() {
                if super::beacon::in_range(pos, levels, player.pos) {
                    for effect in &effects {
                        player.apply_effect(*effect);
                    }
                }
            }
        }
    }

    /// Publishes this region's entities for command selectors. Folia keeps the store region
    /// local; commands read the snapshot and route mutations back to this region.
    fn publish_command_entities(
        &mut self,
        index: &Arc<Mutex<super::entity_index::EntityIndex>>,
        dimension: super::Dimension,
        _world_id: super::WorldId,
    ) {
        // Nothing was published and there is nothing to unpublish.
        if self.entities.is_empty() && self.published_entities.is_empty() {
            return;
        }
        let prepared = super::entity_index::EntityIndex::prepare_sync(&self.entities, dimension);
        index
            .lock()
            .unwrap()
            .apply_prepared(&mut self.published_entities, prepared);
    }

    /// Marks one entity for removal (`/kill`). The tracker still sends the remove packet and
    /// the normal sweep drops it on this tick or the next.
    /// Hurts one mob (`/damage`) through the same path player attacks use. Returns whether the
    /// entity was found in this region.
    #[cfg(test)]
    pub fn damage_entity(
        &mut self,
        uuid: Uuid,
        amount: f32,
        origin: Option<[f64; 3]>,
        aggro: Option<Uuid>,
        credit: Option<Uuid>,
    ) -> bool {
        self.damage_entity_with(uuid, amount, origin, aggro, credit, false)
    }

    pub fn damage_entity_with(
        &mut self,
        uuid: Uuid,
        amount: f32,
        origin: Option<[f64; 3]>,
        aggro: Option<Uuid>,
        credit: Option<Uuid>,
        bypass_armor: bool,
    ) -> bool {
        let key = self.entities.keys().iter().copied().find(|key| {
            self.entities
                .data(*key)
                .is_some_and(|data| data.uuid == uuid)
        });
        let Some(key) = key else {
            return false;
        };
        let source = origin.map(|p| ferrite_math::Vec3::new(p[0], p[1], p[2]));
        // The causing player gets the kill (loot with `killed_by_player`, advancements).
        if let Some(player) = credit {
            mob::credit(&mut self.entities, key, player);
        }
        if mob::hurt_with(&mut self.entities, key, amount, source, bypass_armor).is_some() {
            if let Some(player) = aggro {
                mob::retaliate(&mut self.entities, key, player);
            }
            self.pending_hurts.push(uuid);
        }
        true
    }

    /// Queues an arm swing (`/swing`) of one entity of this region for the next tick, when the
    /// tracker can broadcast it. Returns whether the entity is here.
    pub fn swing_entity(&mut self, uuid: Uuid, action: u8) -> bool {
        let found = self.entities.keys().iter().any(|key| {
            self.entities
                .data(*key)
                .is_some_and(|data| data.uuid == uuid)
        });
        if found {
            self.pending_swings.push((uuid, action));
        }
        found
    }

    /// A plugin's operation on one entity of this region. Returns whether the entity was here.
    #[cfg(test)]
    pub fn plugin_entity_op(
        &mut self,
        uuid: Uuid,
        op: &crate::game::plugin_types::EntityOperation,
    ) -> bool {
        use crate::game::plugin_types::EntityOperation as Op;
        use ferrite_math::Vec3;
        let Some(key) = self.entities.keys().iter().copied().find(|key| {
            self.entities
                .data(*key)
                .is_some_and(|data| data.uuid == uuid)
        }) else {
            return false;
        };
        match op {
            Op::Remove => return self.kill_entity(uuid),
            Op::SetVelocity(v) => self.entities.set_vel(key, Vec3::new(v[0], v[1], v[2])),
            Op::Teleport(p) => {
                let (yaw, pitch) = self
                    .entities
                    .data(key)
                    .map_or((0.0, 0.0), |data| (data.yaw, data.pitch));
                return self.teleport_entity(uuid, *p, yaw, pitch);
            }
            Op::Damage(amount) => {
                mob::hurt(&mut self.entities, key, *amount, None);
            }
            Op::SetItem { material, amount } => {
                let Some(item_id) = ferrite_item::item_id(material) else {
                    return false;
                };
                let Some(data) = self.entities.data_mut(key) else {
                    return false;
                };
                ferrite_entity::item::set_stack(
                    data,
                    ferrite_protocol::packets::play::ItemStack {
                        count: i32::from(*amount),
                        item_id: i32::from(item_id),
                    },
                );
            }
            Op::SetPickupDelay(delay) => {
                if let Some(item) = self
                    .entities
                    .data_mut(key)
                    .and_then(|data| data.item.as_mut())
                {
                    item.pickup_delay = *delay;
                }
            }
            Op::SetOwner(owner) => {
                if let Some(item) = self
                    .entities
                    .data_mut(key)
                    .and_then(|data| data.item.as_mut())
                {
                    item.target = *owner;
                }
            }
            Op::SetThrower(thrower) => {
                if let Some(item) = self
                    .entities
                    .data_mut(key)
                    .and_then(|data| data.item.as_mut())
                {
                    item.thrower = *thrower;
                }
            }
            Op::SetUnlimitedLifetime(unlimited) => {
                if let Some(item) = self
                    .entities
                    .data_mut(key)
                    .and_then(|data| data.item.as_mut())
                {
                    item.unlimited_lifetime = *unlimited;
                    item.age = if *unlimited {
                        ferrite_entity::item::INFINITE_LIFETIME
                    } else {
                        0
                    };
                }
            }
            Op::SetCanMobPickup(allowed) => {
                if let Some(item) = self
                    .entities
                    .data_mut(key)
                    .and_then(|data| data.item.as_mut())
                {
                    item.can_mob_pickup = *allowed;
                }
            }
            Op::SetCanPlayerPickup(allowed) => {
                if let Some(item) = self
                    .entities
                    .data_mut(key)
                    .and_then(|data| data.item.as_mut())
                {
                    item.can_player_pickup = *allowed;
                }
            }
            Op::SetWillAge(ages) => {
                if let Some(item) = self
                    .entities
                    .data_mut(key)
                    .and_then(|data| data.item.as_mut())
                {
                    item.will_age = *ages;
                }
            }
            Op::SetItemHealth(health) => {
                if let Some(item) = self
                    .entities
                    .data_mut(key)
                    .and_then(|data| data.item.as_mut())
                {
                    item.health = *health;
                }
            }
            Op::SetFireworkLife(life) => {
                if let Some(rocket) = self
                    .entities
                    .data_mut(key)
                    .and_then(|data| data.firework.as_mut())
                {
                    rocket.life = *life;
                }
            }
            Op::SetFireworkMaxLife(max) => {
                if let Some(rocket) = self
                    .entities
                    .data_mut(key)
                    .and_then(|data| data.firework.as_mut())
                {
                    rocket.life_time = *max;
                }
            }
        }
        true
    }

    pub fn kill_entity(&mut self, uuid: Uuid) -> bool {
        let key = self.entities.keys().iter().copied().find(|key| {
            self.entities
                .data(*key)
                .is_some_and(|data| data.uuid == uuid)
        });
        let Some(key) = key else {
            return false;
        };
        if let Some(data) = self.entities.data(key) {
            self.published_facts.remove(&data.uuid);
        }
        // `/kill` is lethal damage, not just a tracker removal: the death pass must still run
        // loot, the dragon fight, and advancements.
        if !mob::kill(&mut self.entities, key)
            && let Some(flags) = self.entities.flags_mut(key)
        {
            // `/kill` also applies to non-living entities such as projectiles and dropped items.
            flags.set(Flags::REMOVED, true);
        }
        if let Some(flags) = self.entities.flags_mut(key) {
            flags.set(Flags::NEEDS_SYNC, true);
        }
        true
    }

    /// Moves one entity (`/tp`). When the destination left this region section, the normal
    /// `take_crossing` pass hands it to the owning region by value on the next tick.
    pub fn teleport_entity(&mut self, uuid: Uuid, pos: [f64; 3], yaw: f32, pitch: f32) -> bool {
        let key = self.entities.keys().iter().copied().find(|key| {
            self.entities
                .data(*key)
                .is_some_and(|data| data.uuid == uuid)
        });
        let Some(key) = key else {
            return false;
        };
        self.entities
            .set_pos(key, ferrite_math::Vec3::new(pos[0], pos[1], pos[2]));
        if let Some(data) = self.entities.data_mut(key) {
            data.yaw = yaw;
            data.pitch = pitch;
            data.head_yaw = yaw;
        }
        if let Some(flags) = self.entities.flags_mut(key) {
            flags.set(Flags::TELEPORTED, true);
            flags.set(Flags::NEEDS_SYNC, true);
        }
        true
    }
}

/// The nearest hive centre within `limit` blocks (vanilla `Bee.HIVE_SEARCH_DISTANCE`).
fn nearest_hive(pos: ferrite_math::Vec3, hives: &[[i32; 3]], limit: f64) -> Option<[i32; 3]> {
    hives
        .iter()
        .copied()
        .map(|hive| (hive_distance(pos, hive), hive))
        .filter(|(distance, _)| *distance <= limit)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, hive)| hive)
}

fn hive_distance(pos: ferrite_math::Vec3, hive: [i32; 3]) -> f64 {
    let dx = pos.x - (f64::from(hive[0]) + 0.5);
    let dy = pos.y - (f64::from(hive[1]) + 0.5);
    let dz = pos.z - (f64::from(hive[2]) + 0.5);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// The closest flower block for a bee (`#minecraft:bee_attractive` with air above it). The
/// vanilla pollinate goal searches a 3x3 around the saved flower; this scan is the equivalent
/// local check while a bee is away from its hive.
fn is_flower(source: &super::anvil::AnvilChunkSource, [x, y, z]: [i32; 3]) -> bool {
    let Some(state) = source
        .block(x, y, z)
        .and_then(|state| u16::try_from(state).ok())
    else {
        return false;
    };
    let Some((name, _)) = ferrite_data::blocks::state(state) else {
        return false;
    };
    if !ferrite_data::tags::contains("block", "bee_attractive", name) {
        return false;
    }
    source
        .block(x, y + 1, z)
        .and_then(|state| u16::try_from(state).ok())
        .is_some_and(|above| ferrite_data::block_props::get(above).is_air)
}

fn find_flower(
    source: &super::anvil::AnvilChunkSource,
    pos: ferrite_math::Vec3,
    radius: i32,
) -> Option<[i32; 3]> {
    let x = pos.x.floor() as i32;
    let y = pos.y.floor() as i32;
    let z = pos.z.floor() as i32;
    let mut closest = None;
    let mut closest_distance = f64::INFINITY;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let at = [x + dx, y + dy, z + dz];
                if is_flower(source, at) {
                    let distance = f64::from(dx * dx + dy * dy + dz * dz);
                    if distance < closest_distance {
                        closest = Some(at);
                        closest_distance = distance;
                    }
                }
            }
        }
    }
    closest
}

/// `SpawnEnv` wrapper that applies vanilla time-of-day and weather sky darkening so hostile
/// monsters do not spawn under open daytime sky while dark caves remain spawnable.
struct RegionSpawnEnv<'a> {
    blocks: &'a TickBlocks<'a>,
    sky_darken: u8,
}

impl BlockGetter for RegionSpawnEnv<'_> {
    fn brightness(&self, x: i32, y: i32, z: i32) -> Option<u8> {
        self.blocks.brightness(x, y, z)
    }

    fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
        self.blocks.block_state(x, y, z)
    }
}

impl SpawnEnv for RegionSpawnEnv<'_> {
    fn creature_table(&self, x: i32, z: i32) -> Option<&[ferrite_entity::spawn::SpawnEntry]> {
        self.blocks.creature_table(x, z)
    }

    fn monster_table(&self, x: i32, z: i32) -> Option<&[ferrite_entity::spawn::SpawnEntry]> {
        self.blocks.monster_table(x, z)
    }

    fn light(&self, x: i32, y: i32, z: i32) -> u8 {
        if let Some(source) = self.blocks.source
            && let Some((sky, block)) = source.sky_and_block(x, y, z)
        {
            return sky.saturating_sub(self.sky_darken).max(block);
        }
        // Superflat / fallback without stored light: open sky is 15 minus sky_darken.
        for scan in y..=y.saturating_add(64) {
            match self.blocks.block_state(x, scan, z) {
                Some(state)
                    if usize::from(state) < ferrite_data::blocks::STATE_COUNT
                        && ferrite_data::block_props::get(state).is_air => {}
                _ => return 0,
            }
        }
        15u8.saturating_sub(self.sky_darken)
    }
}

/// Vanilla sky darkness (`Level.getSkyDarken` approximation): 0 in clear Overworld day,
/// transitioning through dusk/dawn to 11 at night (and +2 during rain).
fn sky_darken(dimension: super::Dimension, day_time: i64, raining: bool) -> u8 {
    if dimension != super::Dimension::Overworld {
        return 0;
    }
    let t = day_time.rem_euclid(24_000);
    let base = if !(12_000..23_800).contains(&t) {
        0
    } else if t < 13_188 {
        (((t - 12_000) * 11) / (13_188 - 12_000)) as u8
    } else if t <= 22_812 {
        11
    } else {
        (((23_800 - t) * 11) / (23_800 - 22_812)) as u8
    };
    (base + if raining { 2 } else { 0 }).min(11)
}

/// Vanilla `Level.isBrightOutside`: true during overworld daytime (`[0..12542)` or `[23460..24000)`).
fn is_bright_outside(dimension: super::Dimension, day_time: i64) -> bool {
    let time_of_day = day_time.rem_euclid(24_000);
    dimension == super::Dimension::Overworld && !(12_542..23_460).contains(&time_of_day)
}

fn mob_hurt_sound(kind: MobKind) -> &'static str {
    match kind {
        MobKind::Creeper => "minecraft:entity.creeper.hurt",
        MobKind::Skeleton | MobKind::Stray | MobKind::Bogged | MobKind::WitherSkeleton => {
            "minecraft:entity.skeleton.hurt"
        }
        MobKind::Zombie | MobKind::Husk | MobKind::Drowned => "minecraft:entity.zombie.hurt",
        MobKind::Spider | MobKind::CaveSpider => "minecraft:entity.spider.hurt",
        MobKind::Slime => "minecraft:entity.slime.hurt",
        MobKind::MagmaCube => "minecraft:entity.magma_cube.hurt",
        MobKind::Enderman => "minecraft:entity.enderman.hurt",
        MobKind::Cow | MobKind::Mooshroom => "minecraft:entity.cow.hurt",
        MobKind::Pig => "minecraft:entity.pig.hurt",
        MobKind::Sheep => "minecraft:entity.sheep.hurt",
        MobKind::Chicken => "minecraft:entity.chicken.hurt",
        MobKind::Wolf => "minecraft:entity.wolf.hurt",
        MobKind::Villager | MobKind::WanderingTrader => "minecraft:entity.villager.hurt",
        MobKind::Witch => "minecraft:entity.witch.hurt",
        MobKind::Blaze => "minecraft:entity.blaze.hurt",
        MobKind::Piglin | MobKind::PiglinBrute => "minecraft:entity.piglin.hurt",
        MobKind::ZombifiedPiglin => "minecraft:entity.zombified_piglin.hurt",
        MobKind::IronGolem => "minecraft:entity.iron_golem.hurt",
        MobKind::SnowGolem => "minecraft:entity.snow_golem.hurt",
        MobKind::EnderDragon => "minecraft:entity.ender_dragon.hurt",
        _ => "minecraft:entity.generic.hurt",
    }
}

fn mob_death_sound(kind: MobKind) -> &'static str {
    match kind {
        MobKind::Creeper => "minecraft:entity.creeper.death",
        MobKind::Skeleton | MobKind::Stray | MobKind::Bogged | MobKind::WitherSkeleton => {
            "minecraft:entity.skeleton.death"
        }
        MobKind::Zombie | MobKind::Husk | MobKind::Drowned => "minecraft:entity.zombie.death",
        MobKind::Spider | MobKind::CaveSpider => "minecraft:entity.spider.death",
        MobKind::Slime => "minecraft:entity.slime.death",
        MobKind::MagmaCube => "minecraft:entity.magma_cube.death",
        MobKind::Enderman => "minecraft:entity.enderman.death",
        MobKind::Cow | MobKind::Mooshroom => "minecraft:entity.cow.death",
        MobKind::Pig => "minecraft:entity.pig.death",
        MobKind::Sheep => "minecraft:entity.sheep.death",
        MobKind::Chicken => "minecraft:entity.chicken.death",
        MobKind::Wolf => "minecraft:entity.wolf.death",
        MobKind::Villager | MobKind::WanderingTrader => "minecraft:entity.villager.death",
        MobKind::Witch => "minecraft:entity.witch.death",
        MobKind::Blaze => "minecraft:entity.blaze.death",
        MobKind::Piglin | MobKind::PiglinBrute => "minecraft:entity.piglin.death",
        MobKind::ZombifiedPiglin => "minecraft:entity.zombified_piglin.death",
        MobKind::IronGolem => "minecraft:entity.iron_golem.death",
        MobKind::SnowGolem => "minecraft:entity.snow_golem.death",
        MobKind::EnderDragon => "minecraft:entity.ender_dragon.death",
        _ => "minecraft:entity.generic.death",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sky_darken_is_zero_during_day_and_eleven_at_midnight() {
        assert_eq!(
            sky_darken(super::super::Dimension::Overworld, 1000, false),
            0
        );
        assert_eq!(
            sky_darken(super::super::Dimension::Overworld, 6000, false),
            0
        );
        assert_eq!(
            sky_darken(super::super::Dimension::Overworld, 18000, false),
            11
        );
        assert_eq!(
            sky_darken(super::super::Dimension::Overworld, 6000, true),
            2
        );
        assert_eq!(sky_darken(super::super::Dimension::Nether, 6000, false), 0);
    }

    #[test]
    fn a_plugin_can_point_only_a_filled_map_at_a_saved_map() {
        let filled = ferrite_item::ItemStack::from_name("minecraft:filled_map").unwrap();
        let pointed = with_map_id(filled, 12).unwrap();
        assert_eq!(ferrite_item::map_id(&pointed), Some(12));
        let again = with_map_id(pointed, 13).unwrap();
        assert_eq!(ferrite_item::map_id(&again), Some(13));
        let dirt = ferrite_item::ItemStack::from_name("minecraft:dirt").unwrap();
        assert!(with_map_id(dirt, 1).is_none());
        let empty_map = ferrite_item::ItemStack::from_name("minecraft:map").unwrap();
        assert!(with_map_id(empty_map, 1).is_none());
    }

    #[test]
    fn remote_ender_pearl_landing_is_handed_to_the_world_loop() {
        let (signals, mut received) = tokio::sync::mpsc::unbounded_channel();
        let owner = Uuid::from_u128(0xfeed);
        let pos = [32.5, 70.0, -16.5];

        Region::default().route_ender_pearl_landing(
            &signals,
            super::super::Dimension::Nether,
            owner,
            pos,
        );

        assert!(matches!(
            received.try_recv(),
            Ok(Signal::EnderPearlTeleport {
                dimension: super::super::Dimension::Nether,
                player,
                pos: landing,
            }) if player == owner && landing == pos
        ));
    }

    #[test]
    fn retracting_piston_checks_the_moved_blocks_source_cell() {
        let source = [2, 64, 0];
        let cell = piston_entity_collision_box(source, false);
        assert_eq!(cell.min.x, 2.0);
        assert_eq!(cell.min.y, 64.0);
        assert_eq!(cell.min.z, 0.0);
        assert_eq!(cell.max.y, 65.05);
    }

    #[test]
    fn stale_concurrent_trade_menu_cannot_roll_back_or_double_count_sales() {
        let mut entities = EntityStore::default();
        let key = entities.insert(mob::new_mob(
            MobKind::Villager,
            ferrite_math::Vec3::new(20.5, 64.0, 0.5),
            &mut Rng::new(7),
        ));
        let player = Uuid::from_u128(44);
        let villager = entities.data_mut(key).unwrap().villager.as_mut().unwrap();
        villager.offers = vec![ferrite_inventory::Trade::new(
            ferrite_item::ItemStack::from_name("emerald").unwrap(),
            ferrite_item::ItemStack::from_name("bread").unwrap(),
            12,
            1,
        )];

        // Two player regions opened the same villager at uses=0. One sale arrives, then the
        // other player's stale pre-sale snapshot, then the first player's repeated snapshot.
        apply_trade_sync(&mut entities, key, player, &[(1, 1)]);
        apply_trade_sync(&mut entities, key, Uuid::from_u128(45), &[(0, 0)]);
        apply_trade_sync(&mut entities, key, player, &[(1, 1)]);

        let villager = entities.data(key).unwrap().villager.as_ref().unwrap();
        assert_eq!(villager.offers[0].uses, 1);
        assert_eq!(villager.offers[0].demand, 1);
        assert_eq!(
            villager
                .gossips
                .get(player, ferrite_entity::villager::GossipType::Trading),
            2
        );
    }

    #[test]
    fn delayed_piston_updates_for_one_position_keep_only_the_latest_state() {
        let pos = [3, 64, -2];
        let extended =
            ferrite_data::blocks::state_id("piston", &[("facing", "east"), ("extended", "true")])
                .unwrap();
        let retracted =
            ferrite_data::blocks::state_id("piston", &[("facing", "east"), ("extended", "false")])
                .unwrap();
        let mut updates = Vec::new();

        queue_delayed_piston_update(&mut updates, 12, pos, u32::from(extended));
        queue_delayed_piston_update(&mut updates, 13, pos, u32::from(retracted));

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].0, 13);
        assert_eq!(updates[0].1, (0, -1));
        assert_eq!(updates[0].2, pos);
        assert_eq!(updates[0].3, u32::from(retracted));
        assert!(!delayed_piston_update_matches_current(
            updates[0].3,
            Some(extended)
        ));
        assert!(delayed_piston_update_matches_current(
            updates[0].3,
            Some(retracted)
        ));
    }

    #[test]
    fn wandering_trader_spawning_obeys_its_gamerule() {
        let mut rules = ferrite_command::gamerule::GameRules::default();
        assert!(trader_spawning_enabled(&rules));
        assert!(trader_spawning_enabled_in_dimension(
            &rules,
            super::super::Dimension::Overworld
        ));
        assert!(!trader_spawning_enabled_in_dimension(
            &rules,
            super::super::Dimension::Nether
        ));
        assert!(!trader_spawning_enabled_in_dimension(
            &rules,
            super::super::Dimension::End
        ));

        rules.set("spawn_wandering_traders", "false").unwrap();
        assert!(!trader_spawning_enabled(&rules));
        assert!(!trader_spawning_enabled_in_dimension(
            &rules,
            super::super::Dimension::Overworld
        ));
    }

    #[test]
    fn expiring_a_trader_removes_it_without_drops_and_detaches_its_llama() {
        let mut region = Region::default();
        let trader = region.entities.insert(mob::new_mob(
            MobKind::WanderingTrader,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        let trader_uuid = region.entities.data(trader).unwrap().uuid;
        mob::state_mut(&mut region.entities, trader)
            .unwrap()
            .lifetime = 1;

        let llama = region.entities.insert(mob::new_mob(
            MobKind::TraderLlama,
            ferrite_math::Vec3::new(1.5, 64.0, 0.5),
            &mut Rng::new(2),
        ));
        {
            let state = mob::state_mut(&mut region.entities, llama).unwrap();
            state.lifetime = 5;
            state.leash = Some((trader_uuid, 1));
            state.caravan = Some(trader_uuid);
        }

        region.tick_trader_lifetimes(&HashSet::new(), &tick_context());

        let trader_state = mob::state(&region.entities, trader).unwrap();
        assert!(trader_state.is_dead());
        assert!(trader_state.loot_dropped, "expiry must suppress mob loot");
        assert!(region.entities.flags(trader).unwrap().has(Flags::REMOVED));
        let llama_state = mob::state(&region.entities, llama).unwrap();
        assert_eq!(llama_state.leash, None);
        assert_eq!(llama_state.caravan, None);
        assert_eq!(llama_state.lifetime, 4);
        assert!(dropped_items(&region).is_empty());
    }

    #[test]
    fn an_open_trade_keeps_a_trader_alive_until_the_trade_closes() {
        let mut region = Region::default();
        let trader = region.entities.insert(mob::new_mob(
            MobKind::WanderingTrader,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(3),
        ));
        let uuid = region.entities.data(trader).unwrap().uuid;
        mob::state_mut(&mut region.entities, trader)
            .unwrap()
            .lifetime = 1;

        region.tick_trader_lifetimes(&HashSet::from([uuid]), &tick_context());
        assert_eq!(mob::state(&region.entities, trader).unwrap().lifetime, 1);
        assert!(!region.entities.flags(trader).unwrap().has(Flags::REMOVED));

        region.tick_trader_lifetimes(&HashSet::new(), &tick_context());
        assert!(region.entities.flags(trader).unwrap().has(Flags::REMOVED));
    }

    #[test]
    fn stale_second_merchant_snapshot_cannot_rewind_or_oversell() {
        let mut store = EntityStore::default();
        let record = mob::new_mob(
            MobKind::Villager,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(1),
        );
        let uuid = record.data.uuid;
        let key = store.insert(record);
        let alice = Uuid::from_u128(11);
        let bob = Uuid::from_u128(12);
        let wants = ferrite_item::ItemStack::from_name("emerald")
            .expect("emerald")
            .copy_with_count(32);
        let gives = ferrite_item::ItemStack::from_name("bread").expect("bread");
        let mut offer = ferrite_inventory::Trade::new(wants, gives, 2, 1);
        offer.price_multiplier = 0.05;
        let villager = store.data_mut(key).unwrap().villager.as_mut().unwrap();
        villager.offers = vec![offer];
        villager.gossips.add(
            alice,
            ferrite_entity::villager::GossipType::MajorPositive,
            20,
        );

        let snapshot = |store: &EntityStore, player| {
            store
                .data(key)
                .unwrap()
                .villager
                .as_ref()
                .unwrap()
                .offers_for_player(player)
        };
        let mut alice_menu = snapshot(&store, alice);
        let bob_menu = snapshot(&store, bob);
        assert_eq!(
            alice_menu[0].price(),
            27,
            "Alice's cure reputation gives her a lower price"
        );
        assert_eq!(bob_menu[0].price(), 32, "Bob has neutral reputation");

        // Alice's screen completes one sale; only Alice gets the trading gossip.
        alice_menu[0].on_sale();
        assert_eq!(merge_trader_trades(&mut store, alice, uuid, &alice_menu), 1);
        let villager = store.data(key).unwrap().villager.as_ref().unwrap();
        assert_eq!((villager.offers[0].uses, villager.offers[0].demand), (1, 1));
        assert_eq!(
            villager
                .gossips
                .get(alice, ferrite_entity::villager::GossipType::Trading),
            ferrite_entity::villager::TRADING_GOSSIP
        );

        // Bob's old snapshot still says uses = 0 and demand = 0. It must neither rewind the
        // villager nor credit Alice's sale to Bob.
        assert_eq!(merge_trader_trades(&mut store, bob, uuid, &bob_menu), 0);
        let villager = store.data(key).unwrap().villager.as_ref().unwrap();
        assert_eq!((villager.offers[0].uses, villager.offers[0].demand), (1, 1));
        assert_eq!(
            villager
                .gossips
                .get(bob, ferrite_entity::villager::GossipType::Trading),
            0
        );

        // The region refreshes Bob's screen before his next click. One use is left, and a
        // second merge cannot push the offer past max_uses or duplicate the gossip.
        let mut bob_refreshed = snapshot(&store, bob);
        assert_eq!(bob_refreshed[0].uses, 1);
        assert_eq!(
            bob_refreshed[0].price(),
            32,
            "refreshing must not copy Alice's reputation price"
        );
        bob_refreshed[0].on_sale();
        assert_eq!(
            merge_trader_trades(&mut store, bob, uuid, &bob_refreshed),
            1
        );
        assert_eq!(
            merge_trader_trades(&mut store, bob, uuid, &bob_refreshed),
            0
        );
        let villager = store.data(key).unwrap().villager.as_ref().unwrap();
        assert_eq!((villager.offers[0].uses, villager.offers[0].demand), (2, 2));
        assert!(
            villager
                .offers_for_player(alice)
                .first()
                .is_some_and(ferrite_inventory::Trade::is_out_of_stock)
        );
        assert_eq!(villager.offers_for_player(alice)[0].special_price, -5);
        assert_eq!(villager.offers_for_player(bob)[0].special_price, 0);
        assert_eq!(
            villager
                .gossips
                .get(bob, ferrite_entity::villager::GossipType::Trading),
            ferrite_entity::villager::TRADING_GOSSIP
        );
    }

    #[test]
    fn a_refreshed_second_menu_cannot_take_an_already_sold_offer() {
        use ferrite_inventory::{ClickInput, Clicker, Menu, NoCrafting, PlayerInventory};

        let mut store = EntityStore::default();
        let record = mob::new_mob(
            MobKind::Villager,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(1),
        );
        let uuid = record.data.uuid;
        let key = store.insert(record);
        let alice = Uuid::from_u128(21);
        let bob = Uuid::from_u128(22);
        let emeralds = |count| {
            ferrite_item::ItemStack::from_name("emerald")
                .expect("emerald")
                .copy_with_count(count)
        };
        let bread = ferrite_item::ItemStack::from_name("bread").expect("bread");
        let mut offer = ferrite_inventory::Trade::new(emeralds(2), bread.clone(), 1, 1);
        offer.price_multiplier = 0.05;
        store
            .data_mut(key)
            .unwrap()
            .villager
            .as_mut()
            .unwrap()
            .offers = vec![offer];

        let snapshot = |store: &EntityStore, player| {
            store
                .data(key)
                .unwrap()
                .villager
                .as_ref()
                .unwrap()
                .offers_for_player(player)
        };
        let bob_stale = snapshot(&store, bob);

        // Alice buys the only use through her copy of the menu.
        let mut alice_menu = Menu::merchant(1, snapshot(&store, alice));
        alice_menu.container_mut()[0] = emeralds(2);
        assert!(alice_menu.select_trade(0));
        assert_eq!(alice_menu.result(), &bread);
        {
            let crafter = NoCrafting;
            let mut inventory = PlayerInventory::new();
            let mut clicker = Clicker::new(&mut inventory, false, &crafter);
            alice_menu.clicked(&mut clicker, 2, 0, ClickInput::Pickup);
        }
        assert!(alice_menu.trades()[0].is_out_of_stock());
        assert_eq!(
            merge_trader_trades(&mut store, alice, uuid, alice_menu.trades()),
            1
        );

        // Bob's screen was opened before Alice bought; it still shows the offer.
        let mut bob_menu = Menu::merchant(2, bob_stale);
        bob_menu.container_mut()[0] = emeralds(2);
        assert!(bob_menu.select_trade(0));
        assert_eq!(
            bob_menu.result(),
            &bread,
            "the stale snapshot still shows the offer"
        );

        // The region refreshes it before the click, so the result disappears.
        bob_menu.set_trades(snapshot(&store, bob));
        assert!(bob_menu.result().is_empty());
        {
            let crafter = NoCrafting;
            let mut inventory = PlayerInventory::new();
            let mut clicker = Clicker::new(&mut inventory, false, &crafter);
            bob_menu.clicked(&mut clicker, 2, 0, ClickInput::Pickup);
            assert_eq!(clicker.xp, 0.0, "no sale, no trade experience");
        }
        assert_eq!(
            merge_trader_trades(&mut store, bob, uuid, bob_menu.trades()),
            0
        );
        let villager = store.data(key).unwrap().villager.as_ref().unwrap();
        assert_eq!((villager.offers[0].uses, villager.offers[0].demand), (1, 1));
        assert_eq!(
            villager
                .gossips
                .get(bob, ferrite_entity::villager::GossipType::Trading),
            0,
            "Bob did not complete a sale"
        );
        assert_eq!(
            villager
                .gossips
                .get(alice, ferrite_entity::villager::GossipType::Trading),
            ferrite_entity::villager::TRADING_GOSSIP
        );
    }

    #[test]
    fn merchant_trade_xp_levels_the_villager() {
        let mut store = ferrite_entity::EntityStore::default();
        let key = store.insert(mob::new_mob(
            mob::MobKind::Villager,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut ferrite_entity::Rng::new(1),
        ));
        let uuid = store.data(key).unwrap().uuid;
        {
            let villager = store.data_mut(key).unwrap().villager.as_mut().unwrap();
            villager.profession = ferrite_entity::villager::Profession::Farmer;
            villager.offers = ferrite_entity::villager::offers_for(villager.profession, 1, uuid);
        }
        add_trader_xp(&mut store, uuid, ferrite_entity::villager::NEXT_LEVEL_XP[0]);
        let villager = store.data(key).unwrap().villager.as_ref().unwrap();
        assert_eq!(villager.level, 2);
        assert_eq!(villager.xp, ferrite_entity::villager::NEXT_LEVEL_XP[0]);
        assert_eq!(villager.offers.len(), 4);
    }

    pub(super) fn tick_context() -> TickContext {
        let (signals, _rx) = tokio::sync::mpsc::unbounded_channel();
        TickContext {
            signals,
            tracking: Default::default(),
            positions: Default::default(),
            source: None,
            dimension: super::super::Dimension::Overworld,
            world_id: crate::game::WorldId::Vanilla(super::super::Dimension::Overworld),
            raining: false,
            rules: Default::default(),
            spawn: [0.0, 65.0, 0.0],
            command_entities: Default::default(),
            state: Default::default(),
            trader_spawner: Default::default(),
            profiler: None,
            tree: None,
            hooks: None,
        }
    }

    #[test]
    fn hostile_mob_sees_a_nearby_player_in_another_region() {
        let ctx = tick_context();
        let player = Uuid::from_u128(0x1234);
        let (outbound, _receiver) = OutboundSender::channel(4096);
        ctx.tracking.lock().unwrap().insert(
            player,
            (
                super::super::WorldId::Vanilla(super::super::Dimension::Overworld),
                HashSet::new(),
                outbound,
            ),
        );
        ctx.positions
            .lock()
            .unwrap()
            .insert(player, [129.0, 65.0, 0.5]);
        let mut region = Region::default();
        let zombie = region.entities.insert(mob::new_mob(
            MobKind::Zombie,
            ferrite_math::Vec3::new(127.0, 65.0, 0.5),
            &mut region.rng,
        ));
        let blocks = TickBlocks {
            source: None,
            seen: Default::default(),
        };
        region.tick_mobs(&ctx, &blocks);
        assert!(!region.entities.flags(zombie).unwrap().has(Flags::REMOVED));
        ctx.tracking.lock().unwrap().get_mut(&player).unwrap().0 =
            super::super::WorldId::Vanilla(super::super::Dimension::Nether);
        region.tick_mobs(&ctx, &blocks);
        assert!(region.entities.flags(zombie).unwrap().has(Flags::REMOVED));
    }

    #[test]
    fn an_enderman_is_angered_by_a_stare_from_a_player_in_another_region() {
        let ctx = tick_context();
        let player = Uuid::from_u128(0xe1de_0001);
        let (outbound, _receiver) = OutboundSender::channel(4096);
        ctx.tracking.lock().unwrap().insert(
            player,
            (
                super::super::WorldId::Vanilla(super::super::Dimension::Overworld),
                HashSet::new(),
                outbound,
            ),
        );
        // The player belongs to another region: this region sees only the shared position
        // snapshot and the gaze that region published.
        let feet = [0.5, 65.0, 0.5];
        ctx.positions.lock().unwrap().insert(player, feet);
        let mut region = Region::default();
        let enderman = region.entities.insert(mob::new_mob(
            MobKind::Enderman,
            ferrite_math::Vec3::new(10.5, 65.0, 0.5),
            &mut region.rng,
        ));
        let eye = region.entities.pos(enderman).unwrap()
            + ferrite_math::Vec3::new(0.0, f64::from(MobKind::Enderman.props().eye_height), 0.0);
        let look = (eye
            - ferrite_entity::combat::eye_position(ferrite_math::Vec3::new(
                feet[0], feet[1], feet[2],
            )))
        .normalize();
        let blocks = TickBlocks {
            source: None,
            seen: Default::default(),
        };
        super::super::targeting::publish_gaze(
            player,
            ferrite_entity::enderman::Gaze {
                look,
                pumpkin: false,
            },
        );
        for _ in 0..2_000 {
            region.tick_mobs(&ctx, &blocks);
            if mob::state(&region.entities, enderman).unwrap().target == Some(player) {
                break;
            }
        }
        assert_eq!(
            mob::state(&region.entities, enderman).unwrap().target,
            Some(player),
            "a stare from another region must anger it"
        );
        // The same player under a carved pumpkin is not noticed.
        let calm = region.entities.insert(mob::new_mob(
            MobKind::Enderman,
            ferrite_math::Vec3::new(10.5, 65.0, 0.5),
            &mut region.rng,
        ));
        super::super::targeting::publish_gaze(
            player,
            ferrite_entity::enderman::Gaze {
                look,
                pumpkin: true,
            },
        );
        for _ in 0..1_000 {
            region.tick_mobs(&ctx, &blocks);
        }
        assert_eq!(mob::state(&region.entities, calm).unwrap().target, None);
        super::super::targeting::forget(player);
    }

    #[test]
    fn a_cross_region_target_survives_a_region_split_and_merge() {
        let ctx = tick_context();
        let player = Uuid::from_u128(0x1235);
        let (outbound, _receiver) = OutboundSender::channel(4096);
        ctx.tracking.lock().unwrap().insert(
            player,
            (
                super::super::WorldId::Vanilla(super::super::Dimension::Overworld),
                HashSet::new(),
                outbound,
            ),
        );
        ctx.positions
            .lock()
            .unwrap()
            .insert(player, [129.0, 65.0, 0.5]);
        let mut region = Region::default();
        let zombie = region.entities.insert(mob::new_mob(
            MobKind::Zombie,
            ferrite_math::Vec3::new(127.0, 65.0, 0.5),
            &mut region.rng,
        ));
        let blocks = TickBlocks {
            source: None,
            seen: Default::default(),
        };
        region.tick_mobs(&ctx, &blocks);
        let zombie_uuid = region.entities.data(zombie).unwrap().uuid;
        assert_eq!(
            mob::state(&region.entities, zombie).unwrap().target,
            Some(player)
        );
        // A split and a merge reinsert entities, so find the mob by UUID rather than by key.
        let target_of = |entities: &ferrite_entity::EntityStore| -> Option<Uuid> {
            entities.keys().iter().copied().find_map(|key| {
                let data = entities.data(key)?;
                if data.uuid != zombie_uuid {
                    return None;
                }
                mob::state(entities, key).and_then(|state| state.target)
            })
        };

        let mut taken = region.split(&[SectionPos(0, 0)].into());
        assert!(
            region.entities.keys().is_empty(),
            "the mob moved with its section"
        );
        taken.tick_mobs(&ctx, &blocks);
        assert_eq!(
            target_of(&taken.entities),
            Some(player),
            "the target is kept by the region that owns the mob"
        );
        region.merge(taken);
        region.tick_mobs(&ctx, &blocks);
        assert_eq!(
            target_of(&region.entities),
            Some(player),
            "and survives the merge back"
        );
    }

    #[test]
    fn a_chicken_lay_egg_event_spawns_an_egg_item() {
        let ctx = tick_context();
        let mut region = Region::default();
        let chicken = region.entities.insert(mob::new_mob(
            MobKind::Chicken,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut region.rng,
        ));
        mob::state_mut(&mut region.entities, chicken)
            .unwrap()
            .egg_time = 1;
        let blocks = TickBlocks {
            source: None,
            seen: Default::default(),
        };
        region.tick_mobs(&ctx, &blocks);
        assert_eq!(dropped_items(&region), ["minecraft:egg"]);
    }

    /// A tick context whose signal receiver the test keeps.
    fn listening_context() -> (
        TickContext,
        tokio::sync::mpsc::UnboundedReceiver<Signal>,
        Uuid,
    ) {
        let mut ctx = tick_context();
        let (signals, rx) = tokio::sync::mpsc::unbounded_channel();
        ctx.signals = signals;
        let player = Uuid::from_u128(0xabcd);
        let (outbound, _receiver) = OutboundSender::channel(4096);
        ctx.tracking.lock().unwrap().insert(
            player,
            (
                super::super::WorldId::Vanilla(super::super::Dimension::Overworld),
                HashSet::new(),
                outbound,
            ),
        );
        (ctx, rx, player)
    }

    #[test]
    fn a_zombie_hits_a_player_across_a_region_boundary_and_names_itself() {
        let (ctx, mut signals, player) = listening_context();
        // 8 blocks past a region section edge: the player is owned by another region.
        ctx.positions
            .lock()
            .unwrap()
            .insert(player, [128.5, 65.0, 0.5]);
        let mut region = Region::default();
        let zombie = region.entities.insert(mob::new_mob(
            MobKind::Zombie,
            ferrite_math::Vec3::new(127.5, 65.0, 0.5),
            &mut region.rng,
        ));
        let uuid = region.entities.data(zombie).unwrap().uuid;
        let blocks = TickBlocks {
            source: None,
            seen: Default::default(),
        };
        let mut hit = None;
        for _ in 0..40 {
            region.tick_mobs(&ctx, &blocks);
            while let Ok(signal) = signals.try_recv() {
                if let Signal::MobMelee {
                    attacker,
                    player: target,
                    ..
                } = signal
                {
                    hit = Some((attacker, target));
                }
            }
        }
        assert_eq!(
            hit,
            Some((uuid, player)),
            "the hit reaches the player's region and says which mob swung"
        );
        assert!(!region.entities.flags(zombie).unwrap().has(Flags::REMOVED));
    }

    #[test]
    fn a_mob_walking_toward_a_cross_region_player_is_not_despawned_when_the_snapshot_moves() {
        let (ctx, _signals, player) = listening_context();
        let mut region = Region::default();
        let zombie = region.entities.insert(mob::new_mob(
            MobKind::Zombie,
            ferrite_math::Vec3::new(0.5, 65.0, 0.5),
            &mut region.rng,
        ));
        let blocks = TickBlocks {
            source: None,
            seen: Default::default(),
        };
        // The player crosses the boundary: the mob keeps seeing the fresh snapshot each tick.
        for x in [100.5, 120.5, 126.5, 110.5] {
            ctx.positions.lock().unwrap().insert(player, [x, 65.0, 0.5]);
            region.tick_mobs(&ctx, &blocks);
            assert!(!region.entities.flags(zombie).unwrap().has(Flags::REMOVED));
        }
        // Only a player beyond 128 blocks lets it despawn.
        ctx.positions
            .lock()
            .unwrap()
            .insert(player, [400.5, 65.0, 0.5]);
        region.tick_mobs(&ctx, &blocks);
        assert!(region.entities.flags(zombie).unwrap().has(Flags::REMOVED));
    }

    #[test]
    fn knock_back_by_uuid_pushes_only_the_named_mob() {
        let mut region = Region::default();
        let pos = ferrite_math::Vec3::new(8.5, 64.0, 8.5);
        let first = region
            .entities
            .insert(mob::new_mob(MobKind::Zombie, pos, &mut region.rng));
        let second = region
            .entities
            .insert(mob::new_mob(MobKind::Zombie, pos, &mut region.rng));
        let uuid = region.entities.data(first).unwrap().uuid;
        assert!(region.knock_back_by_uuid(uuid, ferrite_math::Vec3::new(6.5, 64.0, 8.5)));
        assert!(region.entities.vel(first).unwrap().x > 0.3);
        assert_eq!(region.entities.vel(second).unwrap().x, 0.0);
        assert!(
            !region.knock_back_by_uuid(Uuid::from_u128(1), pos),
            "gone mobs are ignored"
        );
    }

    #[test]
    fn a_potions_bottle_follows_it_through_a_region_split_and_merge() {
        let bottle = ferrite_item::ItemStack::with_count(
            ferrite_item::item_id("minecraft:splash_potion").unwrap(),
            1,
        );
        let mut region = Region::default();
        let potion = arrow::new_thrown(
            arrow::Kind::SplashPotion,
            ferrite_math::Vec3::new(8.5, 70.0, 8.5),
            ferrite_math::Vec3::new(0.0, 0.0, 0.5),
            None,
        );
        let uuid = potion.data.uuid;
        region.entities.insert(potion);
        region.thrown_potions.insert(uuid, bottle);

        // A split that does not take the potion's section leaves the bottle where it is.
        let elsewhere: HashSet<_> = [SectionPos(40, 40)].into();
        let other = region.split(&elsewhere);
        assert!(other.thrown_potions.is_empty());
        assert!(region.thrown_potions.contains_key(&uuid));

        // One that takes it moves the bottle along with the entity, and a merge brings it home.
        let here: HashSet<_> = [SectionPos(0, 0)].into();
        let mut taken = region.split(&here);
        assert!(region.thrown_potions.is_empty());
        assert!(taken.thrown_potions.contains_key(&uuid));
        taken.merge(region);
        assert!(taken.thrown_potions.contains_key(&uuid));
    }

    fn lingering_bottle(potion: &str) -> ferrite_item::ItemStack {
        let mut bottle = ferrite_item::ItemStack::from_name("minecraft:lingering_potion")
            .expect("vanilla lingering potion");
        bottle.components.insert(
            ferrite_item::component_ids::POTION_CONTENTS,
            ferrite_item::Component::PotionContents(ferrite_item::PotionContents {
                potion: Some(potion.into()),
                ..Default::default()
            }),
        );
        bottle
    }

    #[test]
    fn a_lingering_bottle_with_effects_leaves_a_cloud_and_water_leaves_none() {
        let mut region = Region::default();
        let at = ferrite_math::Vec3::new(8.5, 64.0, 8.5);
        region.linger_potion(at, &lingering_bottle("minecraft:water"));
        assert!(region.potion_clouds.is_empty());
        region.linger_potion(at, &lingering_bottle("minecraft:swiftness"));
        assert_eq!(region.potion_clouds.len(), 1);
        let cloud = &region.potion_clouds[0];
        assert_eq!(cloud.center, at);
        assert!(!cloud.effects.is_empty());
        // Swiftness lasts 3:00 in a bottle; the cloud gives a quarter of that.
        assert_eq!(cloud.effects[0].duration, 3600 / 4);
    }

    #[test]
    fn a_potion_cloud_follows_its_section_through_a_region_split_and_merge() {
        let mut region = Region::default();
        region.linger_potion(
            ferrite_math::Vec3::new(8.5, 64.0, 8.5),
            &lingering_bottle("minecraft:swiftness"),
        );
        let elsewhere: HashSet<_> = [SectionPos(40, 40)].into();
        let other = region.split(&elsewhere);
        assert!(other.potion_clouds.is_empty());
        assert_eq!(region.potion_clouds.len(), 1);

        let here: HashSet<_> = [SectionPos(0, 0)].into();
        let mut taken = region.split(&here);
        assert!(region.potion_clouds.is_empty());
        assert_eq!(taken.potion_clouds.len(), 1);
        taken.merge(region);
        assert_eq!(taken.potion_clouds.len(), 1);
    }

    #[test]
    fn ticking_an_empty_cloud_list_is_harmless_and_a_cloud_expires() {
        let mut region = Region::default();
        region.tick_potion_clouds();
        region.linger_potion(
            ferrite_math::Vec3::new(8.5, 64.0, 8.5),
            &lingering_bottle("minecraft:swiftness"),
        );
        for _ in 0..(ferrite_entity::potion::PotionCloud::WAIT
            + ferrite_entity::potion::PotionCloud::DURATION)
        {
            region.tick_potion_clouds();
        }
        assert!(region.potion_clouds.is_empty());
    }

    fn cloud_entities(region: &Region) -> Vec<Uuid> {
        let kind = ferrite_entity::potion::cloud_type();
        region
            .entities
            .keys()
            .iter()
            .filter_map(|&key| region.entities.data(key))
            .filter(|data| data.kind == kind)
            .map(|data| data.uuid)
            .collect()
    }

    #[test]
    fn a_lingering_potion_spawns_a_client_visible_cloud_entity_and_removes_it_on_expiry() {
        let mut region = Region::default();
        region.linger_potion(
            ferrite_math::Vec3::new(8.5, 64.0, 8.5),
            &lingering_bottle("minecraft:water"),
        );
        assert!(cloud_entities(&region).is_empty());
        region.linger_potion(
            ferrite_math::Vec3::new(8.5, 64.0, 8.5),
            &lingering_bottle("minecraft:swiftness"),
        );
        assert_eq!(
            cloud_entities(&region),
            vec![region.potion_clouds[0].uuid()]
        );
        let mut peak = 0;
        for _ in 0..(ferrite_entity::potion::PotionCloud::WAIT
            + ferrite_entity::potion::PotionCloud::DURATION)
        {
            region.tick_potion_clouds();
            peak = peak.max(cloud_entities(&region).len());
        }
        assert_eq!(peak, 1);
        assert!(region.potion_clouds.is_empty());
        region.entities.sweep_removed();
        assert!(cloud_entities(&region).is_empty());
    }

    #[test]
    fn a_cloud_entity_travels_with_its_cloud_through_region_split_and_merge() {
        let mut region = Region::default();
        region.linger_potion(
            ferrite_math::Vec3::new(8.5, 64.0, 8.5),
            &lingering_bottle("minecraft:swiftness"),
        );
        let uuid = region.potion_clouds[0].uuid();
        let mut taken = region.split(&[SectionPos(0, 0)].into());
        assert!(cloud_entities(&region).is_empty() && region.potion_clouds.is_empty());
        assert_eq!(cloud_entities(&taken), vec![uuid]);
        taken.merge(region);
        assert_eq!(cloud_entities(&taken), vec![uuid]);
        // The merged region still ticks the cloud and still finds its entity to update.
        for _ in 0..ferrite_entity::potion::PotionCloud::WAIT + 1 {
            taken.tick_potion_clouds();
        }
        let key = taken.entities.keys()[0];
        let data = taken.entities.data(key).unwrap();
        assert_eq!(
            data.metadata
                .get(ferrite_entity::metadata::area_effect_cloud::WAITING),
            Some(&ferrite_protocol::packets::play::MetaValue::Boolean(false))
        );
    }

    fn potion_bottle(item: &str, potion: &str) -> ferrite_item::ItemStack {
        let mut bottle = ferrite_item::ItemStack::from_name(item).expect("vanilla potion item");
        bottle.components.insert(
            ferrite_item::component_ids::POTION_CONTENTS,
            ferrite_item::Component::PotionContents(ferrite_item::PotionContents {
                potion: Some(potion.into()),
                ..Default::default()
            }),
        );
        bottle
    }

    fn cow_at(region: &mut Region, x: f64, z: f64) -> ferrite_entity::EntityKey {
        region.entities.insert(mob::new_mob(
            MobKind::Cow,
            ferrite_math::Vec3::new(x, 64.0, z),
            &mut ferrite_entity::Rng::new(3),
        ))
    }

    /// `Firework#setLife`, `setMaxLife` and `detonate` reach the live rocket, and a rocket aged
    /// past its life bursts on the next tick.
    #[test]
    fn plugin_firework_operations_change_the_rocket_and_detonate_bursts_it() {
        use crate::game::plugin_types::EntityOperation as Op;
        let mut region = Region::default();
        let key = region
            .entities
            .insert(ferrite_entity::firework::new_firework(
                ferrite_math::Vec3::new(0.5, 90.0, 0.5),
                3,
                1,
                None,
                &mut ferrite_entity::Rng::new(5),
            ));
        let uuid = region.entities.data(key).unwrap().uuid;
        let state = |region: &Region| region.entities.data(key).unwrap().firework.clone().unwrap();
        assert!(region.plugin_entity_op(uuid, &Op::SetFireworkMaxLife(60)));
        assert!(region.plugin_entity_op(uuid, &Op::SetFireworkLife(12)));
        assert_eq!((state(&region).life, state(&region).life_time), (12, 60));
        // Not a rocket: nothing to change, and nothing breaks.
        let cow = cow_at(&mut region, 3.0, 3.0);
        let cow_uuid = region.entities.data(cow).unwrap().uuid;
        assert!(region.plugin_entity_op(cow_uuid, &Op::SetFireworkLife(5)));
        // Detonate: aged past its life (without overflowing when ticked), it bursts next tick.
        assert!(region.plugin_entity_op(uuid, &Op::SetFireworkLife(i32::MAX - 1)));
        struct Open;
        impl ferrite_entity::physics::BlockGetter for Open {
            fn block_state(&self, _x: i32, _y: i32, _z: i32) -> Option<u16> {
                ferrite_data::blocks::default_state("air")
            }
        }
        let tick = ferrite_entity::firework::tick_fireworks(&mut region.entities, &Open);
        assert_eq!(tick.bursts.len(), 1);
        assert_eq!(tick.bursts[0].explosions, 1);
    }

    fn effect_level(region: &Region, key: ferrite_entity::EntityKey, effect: Effect) -> i32 {
        region.entities.data(key).unwrap().effects.level(effect)
    }

    #[test]
    fn a_splash_potion_affects_mobs_in_range_with_falloff_and_not_those_outside() {
        let mut region = Region::default();
        let at_center = cow_at(&mut region, 8.0, 8.0);
        let half_way = cow_at(&mut region, 10.0, 8.0);
        let far = cow_at(&mut region, 11.5, 8.0);
        let outside = cow_at(&mut region, 14.0, 8.0);
        region.splash_potion(
            [8.0, 64.0, 8.0],
            &potion_bottle("minecraft:splash_potion", "minecraft:swiftness"),
        );
        let duration = |key| {
            region
                .entities
                .data(key)
                .unwrap()
                .effects
                .get(Effect::Speed)
                .map(|e| e.duration)
        };
        assert_eq!(duration(at_center), Some(3600));
        // Falloff is by the gap between the boxes (cow 0.9 wide, margin 0.3, potion 0.25 wide):
        // 2 blocks of centre distance leave a 1.125 gap, 3.5 blocks leave 2.625.
        let near = duration(half_way).unwrap();
        assert!((2586..=2588).contains(&near), "{near}");
        let far_duration = duration(far).unwrap();
        assert!((1237..=1239).contains(&far_duration), "{far_duration}");
        assert_eq!(duration(outside), None);
        // Each hit mob has an update waiting for its trackers.
        assert_eq!(region.pending_mob_effects.len(), 3);
    }

    /// Regression: an instant effect from a splash used to land at full strength wherever the
    /// mob stood; vanilla scales it by the distance (`(int)(scale * 6 + 0.5)` for harming) and a
    /// lingering cloud applies half.
    #[test]
    fn instant_effects_are_scaled_by_distance_and_halved_by_a_cloud() {
        let mut region = Region::default();
        let edge = cow_at(&mut region, 10.0, 8.0);
        region.splash_potion(
            [8.0, 64.0, 8.0],
            &potion_bottle("minecraft:splash_potion", "minecraft:harming"),
        );
        // Gap 1.125 -> scale 0.71875 -> (int)(4.3125 + 0.5) = 4 of the cow's 10 health.
        assert_eq!(mob::state(&region.entities, edge).unwrap().health, 6.0);

        let mut region = Region::default();
        let inside = cow_at(&mut region, 8.5, 8.5);
        region.linger_potion(
            ferrite_math::Vec3::new(8.5, 64.0, 8.5),
            &lingering_bottle("minecraft:harming"),
        );
        for _ in 0..ferrite_entity::potion::PotionCloud::WAIT {
            region.tick_potion_clouds();
        }
        // `(int)(0.5 * 6 + 0.5)` = 3.
        assert_eq!(mob::state(&region.entities, inside).unwrap().health, 7.0);
    }

    #[test]
    fn a_harming_splash_hurts_a_mob_and_a_healing_one_heals_it() {
        let mut region = Region::default();
        let hurt = cow_at(&mut region, 8.0, 8.0);
        region.splash_potion(
            [8.0, 64.0, 8.0],
            &potion_bottle("minecraft:splash_potion", "minecraft:harming"),
        );
        assert_eq!(mob::state(&region.entities, hurt).unwrap().health, 4.0);
        // Instant effects are applied, not stored, and nothing is announced.
        assert!(region.entities.data(hurt).unwrap().effects.is_empty());
        assert!(region.pending_mob_effects.is_empty());
        region.splash_potion(
            [8.0, 64.0, 8.0],
            &potion_bottle("minecraft:splash_potion", "minecraft:healing"),
        );
        assert_eq!(mob::state(&region.entities, hurt).unwrap().health, 8.0);
    }

    #[test]
    fn a_lingering_cloud_affects_mobs_standing_in_it() {
        let mut region = Region::default();
        let inside = cow_at(&mut region, 8.5, 8.5);
        let outside = cow_at(&mut region, 30.0, 30.0);
        region.linger_potion(
            ferrite_math::Vec3::new(8.5, 64.0, 8.5),
            &lingering_bottle("minecraft:swiftness"),
        );
        // Vanilla: a cloud does nothing while it waits (ten ticks), then hits on the fifth-tick checks.
        for _ in 0..ferrite_entity::potion::PotionCloud::WAIT - 1 {
            region.tick_potion_clouds();
        }
        assert_eq!(
            effect_level(&region, inside, Effect::Speed),
            0,
            "still waiting"
        );
        region.tick_potion_clouds();
        assert_eq!(effect_level(&region, inside, Effect::Speed), 1);
        assert_eq!(effect_level(&region, outside, Effect::Speed), 0);
        // The cloud entity itself is never a candidate.
        let cloud = cloud_entities(&region)[0];
        assert!(
            region
                .pending_mob_effects
                .iter()
                .all(|(uuid, _)| *uuid != cloud)
        );
        assert_eq!(region.pending_mob_effects.len(), 1);
    }

    fn end_tick_context() -> TickContext {
        let mut ctx = tick_context();
        ctx.dimension = super::super::Dimension::End;
        ctx.world_id = crate::game::WorldId::Vanilla(super::super::Dimension::End);
        ctx.rules
            .lock()
            .unwrap()
            .set("natural_health_regeneration", "false")
            .unwrap();
        ctx
    }

    fn test_player(
        uuid: Uuid,
        entity: i32,
        pos: [f64; 3],
        gamemode: i32,
        ctx: &TickContext,
    ) -> Player {
        let (outbound, _rx) = OutboundSender::channel(64);
        let chunk = ChunkPos(pos[0].floor() as i32 >> 4, pos[2].floor() as i32 >> 4);
        Player {
            uuid,
            session: 1,
            entity,
            broken: Vec::new(),
            changed_blocks: Vec::new(),
            primed_tnt: Vec::new(),
            firework_launches: Vec::new(),
            eye_throws: Vec::new(),
            bobber_cast: None,
            enchant_seed: 1,
            crystals: Vec::new(),
            ignited: Vec::new(),
            placed_pumpkins: Vec::new(),
            dimension: ctx.dimension,
            world_id: ctx.world_id,
            sleeping: None,
            spawn: None,
            explosions: Vec::new(),
            button_press: Vec::new(),
            editing_sign: None,
            name: format!("p_{entity}"),
            permission: 0,
            plugins: None,
            plugin_event_errors: PluginEventPolicy::Allow,
            positions: ctx.positions.clone(),
            tracking: ctx.tracking.clone(),
            source: None,
            playerdata: None,
            pos,
            yaw: 0.0,
            pitch: 0.0,
            ground: true,
            gliding: false,
            chunk,
            mailbox: Arc::new(Mutex::new(VecDeque::new())),
            outbound,
            signals: ctx.signals.clone(),
            biome: 0,
            gamemode,
            experience_level: 0,
            experience_progress: 0.0,
            experience_total: 0,
            inventory: ferrite_inventory::PlayerInventory::new(),
            menus: Default::default(),
            breaking: None,
            view: 8,
            pending_teleport: None,
            next_teleport: 1,
            loaded: FxSet::default(),
            queue: VecDeque::new(),
            prefetched: 0,
            prefetch_wait: 0,
            awaiting_batch: false,
            rate: 12.0,
            survival: Survival::new(),
            effects: Default::default(),
            dimension_type: ctx.dimension.dimension_type_id(),
            seed: 0,
            flat: true,
            respawn_requested: false,
            attacks: Vec::new(),
            uses: Vec::new(),
            pick_entities: Vec::new(),
            flying: false,
            attack_ticks: 0,
            blocking: false,
            shield_disabled: 0,
            bow_charge: 0,
            using_bow: false,
            trident_draw: false,
            vehicle_moves: Vec::new(),
            open_animal_chest: false,
            sneaking: false,
            spin_ticks: 0,
            spin_damage: 0.0,
            spin_hit: Vec::new(),
            bow_released: false,
            piglin_alerts: Vec::new(),
            hive_harvests: Vec::new(),
            broken_hives: Vec::new(),
            dimensions: super::super::DimensionRegistry::with_source(
                ctx.dimension,
                super::super::anvil::AnvilChunkSource::new_memory_dimension(None, ctx.dimension),
                [0, 65, 0],
            ),
            portal_ticks: 0,
            portal_cooldown: 0,
            portal_pending: None,
            gateway_used: None,
            maps_sent: HashMap::new(),
            seen_credits: false,
            watching_credits: false,
            batch_chunk_holders: Arc::new(AtomicBool::new(true)),
        }
    }

    #[tokio::test]
    async fn perched_dragon_breath_applies_instant_damage_to_players_and_mobs_every_twenty_ticks() {
        use ferrite_entity::dragon::{DragonPhase, FLAME_WARMUP_TICKS};
        use ferrite_protocol::packets::play::MetaValue;

        let ctx = end_tick_context();
        let mut region = Region::default();
        let dragon = region.entities.insert(mob::new_mob(
            MobKind::EnderDragon,
            ferrite_math::Vec3::new(0.0, 68.0, 0.0),
            &mut region.rng,
        ));
        region.entities.data_mut(dragon).unwrap().yaw = 0.0;
        {
            let state = mob::state_mut(&mut region.entities, dragon).unwrap();
            state.dragon_phase = DragonPhase::PerchFlaming;
            state.dragon_phase_ticks = FLAME_WARMUP_TICKS - 1;
            state.perch_flames = 1;
        }
        let dragon_uuid = region.entities.data(dragon).unwrap().uuid;

        // Placed at the vanilla breath aim (0, 67.5, 9) and inside radius 5.
        let survival_id = Uuid::from_u128(101);
        let creative_id = Uuid::from_u128(102);
        region.players.insert(
            survival_id,
            test_player(survival_id, 1001, [0.0, 67.5, 9.0], 0, &ctx),
        );
        region.players.insert(
            creative_id,
            test_player(creative_id, 1002, [1.0, 67.5, 9.0], 1, &ctx),
        );
        let cow = region.entities.insert(mob::new_mob(
            MobKind::Cow,
            ferrite_math::Vec3::new(0.0, 67.5, 11.0),
            &mut region.rng,
        ));
        let cow_uuid = region.entities.data(cow).unwrap().uuid;

        // First region tick reaches FLAME_WARMUP_TICKS and spawns the AreaEffectCloud.
        region.tick(&ctx);
        assert_eq!(region.potion_clouds.len(), 1);
        assert_eq!(
            cloud_entities(&region),
            vec![region.potion_clouds[0].uuid()]
        );
        let cloud = &region.potion_clouds[0];
        assert_eq!(cloud.center, ferrite_math::Vec3::new(0.0, 67.5, 9.0));
        assert_eq!(cloud.owner(), Some(dragon_uuid));
        assert_eq!(cloud.radius(), 5.0);
        let cloud_key = region
            .entities
            .keys()
            .iter()
            .copied()
            .find(|&k| {
                region
                    .entities
                    .data(k)
                    .is_some_and(|d| d.uuid == cloud.uuid())
            })
            .unwrap();
        assert_eq!(
            region
                .entities
                .data(cloud_key)
                .unwrap()
                .metadata
                .get(ferrite_entity::metadata::area_effect_cloud::PARTICLE),
            Some(&MetaValue::Particle { id: 15, argb: None })
        );

        // 20-tick wait: ticks 1..=19 do not damage anyone.
        for _ in 1..20 {
            region.tick(&ctx);
            assert_eq!(region.players[&survival_id].survival.health, 20.0);
            assert_eq!(mob::state(&region.entities, cow).unwrap().health, 10.0);
        }

        // Tick 20: wait ends and InstantDamage (half scale = 3) hits survival player and cow,
        // while creative player and the dragon itself stay unharmed.
        region.tick(&ctx);
        assert_eq!(
            region
                .entities
                .data(cloud_key)
                .unwrap()
                .metadata
                .get(ferrite_entity::metadata::area_effect_cloud::WAITING),
            Some(&MetaValue::Boolean(false))
        );
        assert_eq!(region.potion_clouds[0].radius(), 5.0);
        assert_eq!(region.players[&survival_id].survival.health, 17.0);
        assert_eq!(mob::state(&region.entities, cow).unwrap().health, 7.0);
        assert_eq!(region.players[&creative_id].survival.health, 20.0);
        assert_eq!(mob::state(&region.entities, dragon).unwrap().health, 200.0);

        // Ticks 21..=39: per-victim 20-tick reapplication delay blocks intermediate 5-tick checks.
        for _ in 1..20 {
            region.tick(&ctx);
            assert_eq!(region.players[&survival_id].survival.health, 17.0);
            assert_eq!(mob::state(&region.entities, cow).unwrap().health, 7.0);
        }

        // Tick 40: repeated 20-tick application hits again.
        region.tick(&ctx);
        assert_eq!(region.players[&survival_id].survival.health, 14.0);
        assert_eq!(mob::state(&region.entities, cow).unwrap().health, 4.0);

        // Repeated 20-tick hits eventually kill both the cow and the survival player with Magic
        // attribution.
        for _ in 0..120 {
            region.tick(&ctx);
            if region.players[&survival_id].survival.dead {
                break;
            }
        }
        assert!(region.players[&survival_id].survival.dead);
        assert_eq!(
            region.players[&survival_id].survival.death_cause,
            Some(DamageKind::Magic)
        );
        assert!(
            region
                .entities
                .keys()
                .iter()
                .all(|&k| region.entities.data(k).is_none_or(|d| d.uuid != cow_uuid)),
            "cow died from repeated InstantDamage ticks and was swept"
        );
        assert_eq!(mob::state(&region.entities, dragon).unwrap().health, 200.0);
    }

    #[test]
    fn perched_dragon_breath_cloud_expires_after_200_ticks_at_constant_radius_5() {
        let mut region = Region::default();
        let cloud = ferrite_entity::dragon::perched_breath_cloud(
            ferrite_math::Vec3::new(0.0, 64.0, 9.0),
            None,
        );
        let uuid = cloud.uuid();
        region.entities.insert(cloud.spawn_record());
        region.potion_clouds.push(cloud);

        for _ in 1..ferrite_entity::dragon::PERCHED_BREATH_TICKS {
            region.tick_potion_clouds();
            assert_eq!(region.potion_clouds.len(), 1);
            assert_eq!(region.potion_clouds[0].radius(), 5.0);
            assert_eq!(cloud_entities(&region), vec![uuid]);
        }

        region.tick_potion_clouds();
        assert!(region.potion_clouds.is_empty());
        region.entities.sweep_removed();
        assert!(cloud_entities(&region).is_empty());
    }

    #[tokio::test]
    async fn perched_dragon_breath_cloud_and_owner_follow_region_split_and_merge() {
        let ctx = end_tick_context();
        let mut region = Region::default();
        // Dragon in section (-1, 0), cloud + targets in section (0, 0).
        let dragon = region.entities.insert(mob::new_mob(
            MobKind::EnderDragon,
            ferrite_math::Vec3::new(-8.0, 68.0, 8.0),
            &mut region.rng,
        ));
        let dragon_uuid = region.entities.data(dragon).unwrap().uuid;
        let cloud = ferrite_entity::dragon::perched_breath_cloud(
            ferrite_math::Vec3::new(8.5, 64.0, 8.5),
            Some(dragon_uuid),
        );
        let cloud_uuid = cloud.uuid();
        region.entities.insert(cloud.spawn_record());
        region.potion_clouds.push(cloud);

        let cow = region.entities.insert(mob::new_mob(
            MobKind::Cow,
            ferrite_math::Vec3::new(8.5, 64.0, 8.5),
            &mut region.rng,
        ));
        let cow_uuid = region.entities.data(cow).unwrap().uuid;
        let player_id = Uuid::from_u128(201);
        region.players.insert(
            player_id,
            test_player(player_id, 2001, [9.5, 64.0, 8.5], 0, &ctx),
        );

        // Unrelated split keeps the cloud and entity with the original region.
        let unrelated = region.split(&[SectionPos(40, 40)].into());
        assert!(unrelated.potion_clouds.is_empty());
        assert!(cloud_entities(&unrelated).is_empty());
        assert_eq!(region.potion_clouds.len(), 1);

        // Splitting section (0, 0) transfers the cloud, its entity, and its owner away from the
        // region still holding the dragon.
        let mut taken = region.split(&[SectionPos(0, 0)].into());
        assert!(region.potion_clouds.is_empty());
        assert!(cloud_entities(&region).is_empty());
        assert_eq!(region.entities.keys().len(), 1, "dragon remains in (-1, 0)");
        assert_eq!(taken.potion_clouds.len(), 1);
        assert_eq!(taken.potion_clouds[0].owner(), Some(dragon_uuid));
        assert_eq!(taken.potion_clouds[0].radius(), 5.0);
        assert_eq!(cloud_entities(&taken), vec![cloud_uuid]);

        // The new owning region ticks the cloud and applies InstantDamage after the 20-tick wait.
        for _ in 0..ferrite_entity::dragon::PERCHED_BREATH_WAIT_TICKS {
            taken.tick_potion_clouds();
        }
        let taken_cow = taken
            .entities
            .keys()
            .iter()
            .copied()
            .find(|&k| taken.entities.data(k).is_some_and(|d| d.uuid == cow_uuid))
            .expect("cow moved to split region");
        assert_eq!(taken.players[&player_id].survival.health, 17.0);
        assert_eq!(mob::state(&taken.entities, taken_cow).unwrap().health, 7.0);

        // Merging brings everything back cleanly and continues ticking.
        taken.merge(region);
        assert_eq!(taken.potion_clouds.len(), 1);
        assert_eq!(taken.potion_clouds[0].owner(), Some(dragon_uuid));
        assert_eq!(cloud_entities(&taken), vec![cloud_uuid]);
    }

    #[tokio::test]
    async fn charging_dragon_target_and_collision_exit_survive_region_split_and_merge() {
        use ferrite_entity::Flags;
        use ferrite_entity::dragon::{CHARGE_RECOVERY_TICKS, DragonPhase};

        let ctx = end_tick_context();
        let mut region = Region::default();
        let charge_target = ferrite_math::Vec3::new(90.0, 70.0, 8.0);
        let dragon = region.entities.insert(mob::new_mob(
            MobKind::EnderDragon,
            ferrite_math::Vec3::new(8.0, 68.0, 8.0),
            &mut region.rng,
        ));
        let dragon_uuid = region.entities.data(dragon).unwrap().uuid;
        {
            let state = mob::state_mut(&mut region.entities, dragon).unwrap();
            state.dragon_phase = DragonPhase::ChargePlayer;
            state.dragon_charge_target = Some(charge_target);
            state.dragon_charge_recovery = 0;
        }

        // Split section (0, 0) into a new region and verify the charge target and NO_PHYSICS flag
        // travel with the dragon by value.
        let mut taken = region.split(&[SectionPos(0, 0)].into());
        assert!(region.entities.keys().is_empty());
        let taken_dragon = taken
            .entities
            .keys()
            .iter()
            .copied()
            .find(|&k| {
                taken
                    .entities
                    .data(k)
                    .is_some_and(|d| d.uuid == dragon_uuid)
            })
            .expect("dragon moved to split region");
        assert!(
            taken
                .entities
                .flags(taken_dragon)
                .unwrap()
                .has(Flags::NO_PHYSICS)
        );
        assert_eq!(
            mob::state(&taken.entities, taken_dragon)
                .unwrap()
                .dragon_charge_target,
            Some(charge_target)
        );

        // Trigger a collision exit inside the split region, then merge back mid-recovery and
        // confirm recovery finishes cleanly in the merged region.
        taken
            .entities
            .flags_mut(taken_dragon)
            .unwrap()
            .set(Flags::HORIZONTAL_COLLISION, true);
        taken.tick(&ctx);
        assert_eq!(
            mob::state(&taken.entities, taken_dragon)
                .unwrap()
                .dragon_charge_recovery,
            1
        );
        assert!(
            !taken
                .entities
                .flags(taken_dragon)
                .unwrap()
                .has(Flags::HORIZONTAL_COLLISION)
        );

        region.merge(taken);
        let merged_dragon = region
            .entities
            .keys()
            .iter()
            .copied()
            .find(|&k| {
                region
                    .entities
                    .data(k)
                    .is_some_and(|d| d.uuid == dragon_uuid)
            })
            .expect("dragon returned on merge");
        for _ in 0..CHARGE_RECOVERY_TICKS {
            assert_eq!(
                mob::state(&region.entities, merged_dragon)
                    .unwrap()
                    .dragon_phase,
                DragonPhase::ChargePlayer
            );
            region.tick(&ctx);
        }
        let state = mob::state(&region.entities, merged_dragon).unwrap();
        assert_eq!(state.dragon_phase, DragonPhase::Holding);
        assert_eq!(state.dragon_charge_target, None);
        assert_eq!(state.dragon_charge_recovery, 0);
    }

    #[test]
    fn effects_on_mobs_tick_down_and_expiry_is_announced_once() {
        let mut region = Region::default();
        let key = cow_at(&mut region, 8.0, 8.0);
        region.give_mob_effect(
            key,
            ferrite_entity::effect::MobEffectInstance::new(Effect::Speed.id(), 2, 0),
        );
        let ctx = tick_context();
        region.tick_mob_effects(&ctx);
        assert!(region.pending_mob_effects.is_empty());
        assert_eq!(effect_level(&region, key, Effect::Speed), 1);
        region.tick_mob_effects(&ctx);
        assert_eq!(effect_level(&region, key, Effect::Speed), 0);
        // The expiry was queued and flushed in the same pass; a further tick has nothing left.
        region.pending_mob_effects.clear();
        region.tick_mob_effects(&ctx);
        assert!(region.pending_mob_effects.is_empty());
    }

    fn bobber_of(owner: Uuid, at: ferrite_math::Vec3) -> ferrite_entity::EntityRecord {
        let mut record = ferrite_entity::bobber::cast(
            owner,
            9,
            at,
            0.0,
            0.0,
            0,
            &mut ferrite_entity::Rng::new(1),
        );
        record.pos = at;
        record.vel = ferrite_math::Vec3::ZERO;
        record
    }

    #[test]
    fn a_bobber_whose_owner_is_not_here_is_discarded() {
        let mut region = Region::default();
        let key = region.entities.insert(bobber_of(
            Uuid::from_u128(77),
            ferrite_math::Vec3::new(8.5, 64.0, 8.5),
        ));
        let blocks = TickBlocks {
            source: None,
            seen: Default::default(),
        };
        region.tick_bobbers(&blocks);
        assert!(region.entities.flags(key).unwrap().has(Flags::REMOVED));
        region.entities.sweep_removed();
        assert!(ferrite_entity::bobber::find_for(&region.entities, Uuid::from_u128(77)).is_none());
    }

    #[test]
    fn a_bobber_keeps_its_state_through_region_split_and_merge() {
        let mut region = Region::default();
        let owner = Uuid::from_u128(78);
        region
            .entities
            .insert(bobber_of(owner, ferrite_math::Vec3::new(8.5, 64.0, 8.5)));
        let mut taken = region.split(&[SectionPos(0, 0)].into());
        assert!(ferrite_entity::bobber::find_for(&region.entities, owner).is_none());
        assert!(
            ferrite_entity::bobber::find_for(&taken.entities, owner).is_some(),
            "the bobber moved with its chunk"
        );
        taken.merge(region);
        let key = ferrite_entity::bobber::find_for(&taken.entities, owner).expect("still here");
        let state = taken.entities.data(key).unwrap().bobber.unwrap();
        assert_eq!(state.owner, owner);
        assert_eq!(state.phase, ferrite_entity::bobber::Phase::Flying);
    }

    #[test]
    fn a_bobber_is_found_only_by_its_own_owner() {
        let mut region = Region::default();
        region.entities.insert(bobber_of(
            Uuid::from_u128(1),
            ferrite_math::Vec3::new(8.5, 64.0, 8.5),
        ));
        assert!(ferrite_entity::bobber::find_for(&region.entities, Uuid::from_u128(2)).is_none());
        assert!(ferrite_entity::bobber::find_for(&region.entities, Uuid::from_u128(1)).is_some());
    }

    fn shoulder_side_parrot(player: Uuid, side: ShoulderSide) -> Option<ShoulderParrot> {
        super::super::ride::shoulders_of(player).get(side)
    }

    fn tamed_parrot(region: &mut Region, owner: Option<Uuid>) -> (ferrite_entity::EntityKey, Uuid) {
        let mut record = mob::new_mob(
            MobKind::Parrot,
            ferrite_math::Vec3::new(8.5, 64.0, 8.5),
            &mut ferrite_entity::Rng::new(4),
        );
        record.data.mob.as_mut().unwrap().owner = owner;
        let uuid = record.data.uuid;
        (region.entities.insert(record), uuid)
    }

    fn parrots_in_world(region: &Region) -> Vec<Uuid> {
        region
            .entities
            .keys()
            .iter()
            .filter_map(|&key| region.entities.data(key))
            .filter(|data| data.mob.is_some_and(|m| m.kind == MobKind::Parrot))
            .map(|data| data.uuid)
            .collect()
    }

    const FEET: ferrite_math::Vec3 = ferrite_math::Vec3::new(0.5, 64.05, 0.5);

    #[test]
    fn setting_a_shoulder_lifts_a_live_parrot_out_of_the_world_and_announces_the_side() {
        let player = Uuid::from_u128(0x5101);
        let mut region = Region::default();
        let (_, parrot) = tamed_parrot(&mut region, Some(player));
        let changes = region.change_shoulder(player, FEET, ShoulderSide::Right, Some(parrot));
        assert_eq!(changes, vec![(ShoulderSide::Right, Some(0))]);
        assert!(
            parrots_in_world(&region).is_empty(),
            "the parrot left the world"
        );
        let sitting = shoulder_side_parrot(player, ShoulderSide::Right).unwrap();
        assert_eq!((sitting.uuid, sitting.owner), (parrot, Some(player)));
        assert!(shoulder_side_parrot(player, ShoulderSide::Left).is_none());
        super::super::ride::forget_shoulders(player);
    }

    #[test]
    fn a_uuid_with_no_entity_becomes_a_shoulder_parrot_and_a_non_parrot_is_refused() {
        let player = Uuid::from_u128(0x5102);
        let mut region = Region::default();
        let unknown = Uuid::from_u128(0x9999);
        assert_eq!(
            region.change_shoulder(player, FEET, ShoulderSide::Left, Some(unknown)),
            vec![(ShoulderSide::Left, Some(0))]
        );
        assert_eq!(
            shoulder_side_parrot(player, ShoulderSide::Left)
                .unwrap()
                .uuid,
            unknown
        );
        let cow = region.entities.insert(mob::new_mob(
            MobKind::Cow,
            ferrite_math::Vec3::new(8.5, 64.0, 8.5),
            &mut ferrite_entity::Rng::new(4),
        ));
        let cow_uuid = region.entities.data(cow).unwrap().uuid;
        assert!(
            region
                .change_shoulder(player, FEET, ShoulderSide::Right, Some(cow_uuid))
                .is_empty()
        );
        assert!(shoulder_side_parrot(player, ShoulderSide::Right).is_none());
        assert!(region.entities.contains(cow), "the cow stays where it is");
        super::super::ride::forget_shoulders(player);
    }

    #[test]
    fn replacing_a_shoulder_puts_the_old_parrot_in_the_world_and_moving_one_empties_its_old_side() {
        let player = Uuid::from_u128(0x5103);
        let mut region = Region::default();
        let first = Uuid::from_u128(0xA1);
        let second = Uuid::from_u128(0xA2);
        region.change_shoulder(player, FEET, ShoulderSide::Left, Some(first));
        let changes = region.change_shoulder(player, FEET, ShoulderSide::Left, Some(second));
        assert_eq!(changes, vec![(ShoulderSide::Left, Some(0))]);
        assert_eq!(
            parrots_in_world(&region),
            vec![first],
            "the replaced parrot is released"
        );
        let key = region
            .entities
            .keys()
            .iter()
            .copied()
            .find(|k| region.entities.data(*k).unwrap().uuid == first)
            .unwrap();
        assert_eq!(region.entities.pos(key), Some(FEET));
        // The parrot on the left moves to the right: the left side is announced empty.
        let changes = region.change_shoulder(player, FEET, ShoulderSide::Right, Some(second));
        assert_eq!(
            changes,
            vec![(ShoulderSide::Left, None), (ShoulderSide::Right, Some(0))]
        );
        assert!(shoulder_side_parrot(player, ShoulderSide::Left).is_none());
        assert_eq!(
            shoulder_side_parrot(player, ShoulderSide::Right)
                .unwrap()
                .uuid,
            second
        );
        assert_eq!(
            parrots_in_world(&region),
            vec![first],
            "a move releases nothing"
        );
        super::super::ride::forget_shoulders(player);
    }

    #[test]
    fn releasing_a_shoulder_places_the_parrot_in_the_world_sitting_with_its_owner() {
        let player = Uuid::from_u128(0x5104);
        let mut region = Region::default();
        let (_, parrot) = tamed_parrot(&mut region, Some(player));
        region.change_shoulder(player, FEET, ShoulderSide::Left, Some(parrot));
        assert!(parrots_in_world(&region).is_empty());
        let changes = region.change_shoulder(player, FEET, ShoulderSide::Left, None);
        assert_eq!(changes, vec![(ShoulderSide::Left, None)]);
        assert!(shoulder_side_parrot(player, ShoulderSide::Left).is_none());
        let key = region.entities.key_of(
            region
                .entities
                .keys()
                .iter()
                .filter_map(|k| region.entities.data(*k))
                .find(|d| d.uuid == parrot)
                .expect("the parrot is back in the world")
                .id,
        );
        let data = region.entities.data(key.unwrap()).unwrap();
        let state = data.mob.unwrap();
        assert_eq!((state.owner, state.sitting), (Some(player), true));
        assert_eq!(region.entities.pos(key.unwrap()), Some(FEET));
        // Releasing an empty shoulder does nothing.
        assert!(
            region
                .change_shoulder(player, FEET, ShoulderSide::Left, None)
                .is_empty()
        );
        super::super::ride::forget_shoulders(player);
    }

    #[test]
    fn moving_a_shoulder_parrot_to_the_other_side_keeps_its_owner_and_variant() {
        let player = Uuid::from_u128(0x5106);
        let mut region = Region::default();
        let parrot = ShoulderParrot {
            uuid: Uuid::from_u128(0xB1),
            variant: 3,
            owner: Some(player),
        };
        super::super::ride::shoulder_set(player, ShoulderSide::Left, Some(parrot));
        let changes = region.change_shoulder(player, FEET, ShoulderSide::Right, Some(parrot.uuid));
        assert_eq!(
            changes,
            vec![(ShoulderSide::Left, None), (ShoulderSide::Right, Some(3))]
        );
        assert_eq!(
            shoulder_side_parrot(player, ShoulderSide::Right),
            Some(parrot)
        );
        assert_eq!(shoulder_side_parrot(player, ShoulderSide::Left), None);
        assert!(
            parrots_in_world(&region).is_empty(),
            "moving puts nothing in the world"
        );
        super::super::ride::forget_shoulders(player);
    }

    #[test]
    fn clearing_a_shoulder_leaves_no_parrot_while_releasing_one_does() {
        let player = Uuid::from_u128(0x5107);
        let mut region = Region::default();
        let parrot = Uuid::from_u128(0xB2);
        region.change_shoulder(player, FEET, ShoulderSide::Left, Some(parrot));
        region.change_shoulder(
            player,
            FEET,
            ShoulderSide::Right,
            Some(Uuid::from_u128(0xB3)),
        );
        // `shoulder_release` drops the seated entry without spawning; `change_shoulder(..., None)` spawns it.
        assert!(super::super::ride::shoulder_release(player, ShoulderSide::Left).is_some());
        assert!(parrots_in_world(&region).is_empty());
        assert_eq!(
            region.change_shoulder(player, FEET, ShoulderSide::Right, None),
            vec![(ShoulderSide::Right, None)]
        );
        assert_eq!(parrots_in_world(&region), vec![Uuid::from_u128(0xB3)]);
        super::super::ride::forget_shoulders(player);
    }

    #[test]
    fn a_seated_shoulder_parrot_survives_a_save_and_reload_and_an_offline_player_is_refused() {
        let player = Uuid::from_u128(0x5105);
        let mut region = Region::default();
        assert!(
            !region.set_shoulder(player, ShoulderSide::Left, None),
            "not online here"
        );
        assert!(
            region
                .change_shoulder(player, FEET, ShoulderSide::Left, None)
                .is_empty(),
            "empty shoulder"
        );
        let (_, parrot) = tamed_parrot(&mut region, Some(player));
        region.change_shoulder(player, FEET, ShoulderSide::Right, Some(parrot));
        let dir = std::env::temp_dir().join(format!("ferrite-shoulder-{}", std::process::id()));
        let shoulders = super::super::ride::shoulders_of(player);
        super::super::entity_save::save_shoulders(Some(&dir), player, &shoulders);
        // A restart forgets the in-memory table; the login reads the file back.
        super::super::ride::forget_shoulders(player);
        assert!(super::super::ride::shoulders_of(player).is_empty());
        let loaded = super::super::entity_save::load_shoulders(Some(&dir), player);
        assert_eq!(loaded, shoulders);
        assert_eq!(loaded.right.unwrap().uuid, parrot);
        super::super::entity_save::save_shoulders(Some(&dir), player, &Default::default());
        assert!(super::super::entity_save::load_shoulders(Some(&dir), player).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A viewer of chunk (0, 0) in the overworld, and the channel that receives its packets.
    fn gateway_viewer(ctx: &TickContext) -> ferrite_net::game::OutboundReceiver {
        let (outbound, receiver) = OutboundSender::channel(4096);
        ctx.tracking.lock().unwrap().insert(
            Uuid::from_u128(0xB0B),
            (
                super::super::WorldId::Vanilla(super::super::Dimension::Overworld),
                HashSet::from([(0, 0)]),
                outbound,
            ),
        );
        receiver
    }

    /// The `BlockEvent`s a receiver has queued.
    fn block_events(receiver: &mut ferrite_net::game::OutboundReceiver) -> Vec<BlockEvent> {
        let mut events = Vec::new();
        while let Some(message) = receiver.try_recv() {
            if let Some(bytes) = message.raw_bytes() {
                let mut body = bytes;
                if <VarInt as ferrite_protocol::Decode>::decode(&mut body)
                    .is_ok_and(|id| id.0 == BlockEvent::ID)
                {
                    events
                        .push(<BlockEvent as ferrite_protocol::Decode>::decode(&mut body).unwrap());
                }
            }
        }
        events
    }

    #[test]
    fn a_teleport_through_a_gateway_sends_one_purple_beam_event_to_the_chunks_viewers() {
        let ctx = tick_context();
        let mut viewer = gateway_viewer(&ctx);
        let mut region = Region::default();
        let at = [3, 75, 4];
        region.gateway_teleported(&ctx, at);
        let events = block_events(&mut viewer);
        assert_eq!(events.len(), 1);
        let gateway = crate::game::end_fight::gateway_block_id().unwrap();
        assert_eq!(
            (events[0].block.0, events[0].param_a, events[0].param_b),
            (gateway, 1, 0)
        );
        assert_eq!(
            events[0].pos,
            ferrite_protocol::types::BlockPos::new(3, 75, 4)
        );
        // A second player stepping through while the beam runs does not start another.
        region.gateway_teleported(&ctx, at);
        assert!(block_events(&mut viewer).is_empty());
    }

    #[test]
    fn a_gateway_refuses_teleports_while_its_beam_runs() {
        let ctx = tick_context();
        let mut region = Region::default();
        let at = [3, 75, 4];
        assert!(
            region.gateway_accepts_teleport(at),
            "a fresh gateway is ready"
        );
        region.gateway_teleported(&ctx, at);
        assert!(
            !region.gateway_accepts_teleport(at),
            "the purple beam blocks the next trip"
        );
        assert!(
            region.gateway_accepts_teleport([4, 75, 4]),
            "another gateway is unaffected"
        );
    }

    #[test]
    fn a_gateway_clock_follows_its_section_through_a_region_split_and_merge() {
        let ctx = tick_context();
        let mut region = Region::default();
        region.gateway_teleported(&ctx, [3, 75, 4]);
        assert!(
            region
                .split(&[SectionPos(40, 40)].into())
                .gateway_beams
                .is_empty()
        );
        let mut taken = region.split(&[SectionPos(0, 0)].into());
        assert!(region.gateway_beams.is_empty());
        assert!(taken.gateway_beams[&[3, 75, 4]].is_cooling_down());
        taken.merge(region);
        assert_eq!(taken.gateway_beams.len(), 1);
    }

    fn dropped_items(region: &Region) -> Vec<&'static str> {
        region
            .entities
            .keys()
            .iter()
            .filter_map(|&key| {
                let data = region.entities.data(key)?;
                data.item.as_ref()?;
                u16::try_from(ferrite_entity::item::stack(data).item_id)
                    .ok()
                    .and_then(ferrite_item::item_name)
            })
            .collect()
    }

    fn dropped_gear(region: &Region) -> Vec<&'static str> {
        dropped_items(region)
            .into_iter()
            .filter(|name| *name == "minecraft:saddle" || name.ends_with("_horse_armor"))
            .collect()
    }

    fn dead_equine(region: &mut Region, kind: MobKind) -> ferrite_entity::EntityKey {
        let key = region.entities.insert(mob::new_mob(
            kind,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut region.rng,
        ));
        if let Some(state) = mob::state_mut(&mut region.entities, key) {
            state.health = 0.0;
        }
        key
    }

    #[test]
    fn a_dead_equine_horse_drops_saddle_and_armor_once() {
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let key = dead_equine(&mut region, MobKind::Horse);
        let armor = ferrite_item::item_id("minecraft:diamond_horse_armor").unwrap();
        if let Some(state) = mob::state_mut(&mut region.entities, key) {
            state.saddled = true;
            state.armor = armor;
        }
        let ctx = tick_context();
        region.tick(&ctx);
        let mut names = dropped_gear(&region);
        names.sort();
        assert_eq!(names, ["minecraft:diamond_horse_armor", "minecraft:saddle"]);
        assert!(mob::state(&region.entities, key).unwrap().loot_dropped);
        region.tick(&ctx);
        assert_eq!(dropped_gear(&region).len(), 2, "gear drops exactly once");
    }

    fn sheep_region() -> (Region, ferrite_entity::EntityKey) {
        let mut region = Region::default();
        let key = region.entities.insert(mob::new_mob(
            MobKind::Sheep,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut region.rng,
        ));
        ferrite_entity::sheep::set_color(&mut region.entities, key, 11);
        (region, key)
    }

    #[test]
    fn shearing_a_sheep_drops_its_coloured_wool_and_a_second_shear_drops_nothing() {
        if super::super::drops::tables().is_none() {
            return;
        }
        let (mut region, key) = sheep_region();
        assert_eq!(region.shear_sheep(key), Some(true));
        let wool = dropped_items(&region);
        assert!((1..=3).contains(&wool.len()), "{wool:?}");
        assert!(wool.iter().all(|name| *name == "minecraft:blue_wool"));
        assert!(mob::state(&region.entities, key).unwrap().sheared);
        let before = region.entities.keys().len();
        assert_eq!(region.shear_sheep(key), Some(false));
        assert_eq!(region.entities.keys().len(), before, "nothing more dropped");
    }

    #[test]
    fn a_dye_recolours_an_unsheared_sheep_in_the_region_and_updates_its_wool_metadata() {
        let (mut region, key) = sheep_region();
        // The click path: item name -> colour -> recolour, then the sound; the sheep is blue (11).
        let red = ferrite_entity::sheep::dye_color("minecraft:red_dye").unwrap();
        assert!(region.use_dye_on_sheep(Uuid::nil(), key, "minecraft:red_dye", 0, 0));
        assert_eq!(mob::state(&region.entities, key).unwrap().wool_color, red);
        let wool = region
            .entities
            .data(key)
            .unwrap()
            .metadata
            .get(ferrite_entity::sheep::WOOL_INDEX);
        assert_eq!(
            wool,
            Some(&ferrite_protocol::packets::play::MetaValue::Byte(red as i8)),
            "the wool metadata byte carries the new colour"
        );
        // The same colour, a sheared sheep and a non-dye leave the click unused.
        assert!(!region.use_dye_on_sheep(Uuid::nil(), key, "minecraft:red_dye", 0, 0));
        assert!(!region.use_dye_on_sheep(Uuid::nil(), key, "minecraft:wheat", 0, 0));
        mob::state_mut(&mut region.entities, key).unwrap().sheared = true;
        assert!(!region.use_dye_on_sheep(Uuid::nil(), key, "minecraft:lime_dye", 0, 0));
        assert_eq!(mob::state(&region.entities, key).unwrap().wool_color, red);
    }

    #[test]
    fn a_dye_is_used_up_in_survival_and_kept_in_creative() {
        let dye = ferrite_item::ItemStack::from_name("minecraft:red_dye")
            .unwrap()
            .copy_with_count(5);
        assert_eq!(stack_after_dye(&dye, 0).count, 4);
        assert_eq!(stack_after_dye(&dye, 2).count, 4);
        assert_eq!(stack_after_dye(&dye, 1).count, 5);
        assert!(stack_after_dye(&dye.copy_with_count(1), 0).is_empty());
    }

    #[test]
    fn a_lamb_is_not_sheared_and_a_cow_is_not_a_sheep() {
        let (mut region, key) = sheep_region();
        mob::state_mut(&mut region.entities, key).unwrap().age = -500;
        assert_eq!(region.shear_sheep(key), Some(false));
        assert!(!mob::state(&region.entities, key).unwrap().sheared);
        let cow = region.entities.insert(mob::new_mob(
            MobKind::Cow,
            ferrite_math::Vec3::new(4.5, 64.0, 0.5),
            &mut region.rng,
        ));
        assert_eq!(region.shear_sheep(cow), None);
    }

    async fn ensure_source_chunk(
        source: &std::sync::Arc<super::super::anvil::AnvilChunkSource>,
        cx: i32,
        cz: i32,
    ) {
        for _ in 0..2_000 {
            if let Some(result) = source.get(cx, cz, 0) {
                result.expect("chunk load");
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("chunk never loaded");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn placing_a_pumpkin_on_two_snow_blocks_builds_and_clears_the_golem() {
        let source = loaded_source().await;
        let snow = u32::from(ferrite_data::blocks::default_state("minecraft:snow_block").unwrap());
        let pumpkin =
            u32::from(ferrite_data::blocks::default_state("minecraft:carved_pumpkin").unwrap());
        for y in 68..=69 {
            source.set_block(8, y, 8, snow).unwrap();
        }
        source.set_block(8, 70, 8, pumpkin).unwrap();

        let ctx = bare_context(&source);
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let mut player = test_player(Uuid::from_u128(0x901), 3001, [8.5, 66.0, 8.5], 0, &ctx);
        player.placed_pumpkins.push([8, 70, 8]);
        region.build_golems(&mut player, &ctx);

        for y in 68..=70 {
            assert_eq!(source.block(8, y, 8), Some(0), "pattern block {y} cleared");
        }
        let key = *region.entities.keys().first().expect("spawned snow golem");
        let state = mob::state(&region.entities, key).unwrap();
        assert_eq!(state.kind, MobKind::SnowGolem);
        assert!(!state.sheared, "a new golem wears its pumpkin");
        let pos = region.entities.pos(key).unwrap();
        assert_eq!((pos.x, pos.y, pos.z), (8.5, 68.05, 8.5));
        let data = region.entities.data(key).unwrap();
        assert_eq!((data.yaw, data.pitch), (0.0, 0.0));
        assert_eq!(
            data.metadata
                .get(ferrite_entity::metadata::snow_golem::PUMPKIN),
            Some(&ferrite_protocol::packets::play::MetaValue::Byte(
                ferrite_entity::metadata::snow_golem::FLAG_HAS_PUMPKIN
            ))
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_iron_t_builds_a_player_created_golem_at_the_bottom_block() {
        let source = loaded_source().await;
        let iron = u32::from(ferrite_data::blocks::default_state("minecraft:iron_block").unwrap());
        let pumpkin =
            u32::from(ferrite_data::blocks::default_state("minecraft:carved_pumpkin").unwrap());
        for x in 7..=9 {
            source.set_block(x, 69, 8, iron).unwrap();
        }
        source.set_block(8, 68, 8, iron).unwrap();
        source.set_block(8, 70, 8, pumpkin).unwrap();

        let ctx = bare_context(&source);
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        assert!(region.construct_golem(&ctx, [8, 70, 8]));

        for at in [[8, 70, 8], [7, 69, 8], [8, 69, 8], [9, 69, 8], [8, 68, 8]] {
            assert_eq!(source.block(at[0], at[1], at[2]), Some(0), "{at:?} cleared");
        }
        let key = *region.entities.keys().first().expect("spawned iron golem");
        let state = mob::state(&region.entities, key).unwrap();
        assert_eq!(state.kind, MobKind::IronGolem);
        assert!(state.player_created);
        let pos = region.entities.pos(key).unwrap();
        assert_eq!((pos.x, pos.y, pos.z), (8.5, 68.05, 8.5));
        let data = region.entities.data(key).unwrap();
        assert_eq!((data.yaw, data.pitch), (0.0, 0.0));
        assert_eq!(
            data.metadata
                .get(ferrite_entity::metadata::iron_golem::FLAGS),
            Some(&ferrite_protocol::packets::play::MetaValue::Byte(
                ferrite_entity::metadata::iron_golem::FLAG_PLAYER_CREATED
            ))
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn incomplete_or_obstructed_patterns_leave_the_pumpkin_in_place() {
        let source = loaded_source().await;
        let snow = u32::from(ferrite_data::blocks::default_state("minecraft:snow_block").unwrap());
        let iron = u32::from(ferrite_data::blocks::default_state("minecraft:iron_block").unwrap());
        let stone = u32::from(ferrite_data::blocks::default_state("minecraft:stone").unwrap());
        let pumpkin =
            u32::from(ferrite_data::blocks::default_state("minecraft:carved_pumpkin").unwrap());
        let ctx = bare_context(&source);
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));

        // One snow block is incomplete.
        source.set_block(8, 69, 8, snow).unwrap();
        source.set_block(8, 70, 8, pumpkin).unwrap();
        assert!(!region.construct_golem(&ctx, [8, 70, 8]));
        assert_eq!(source.block(8, 70, 8), Some(pumpkin));
        assert!(region.entities.keys().is_empty());

        // A complete T with stone beside the pumpkin does not match vanilla's air cells.
        for x in 7..=9 {
            source.set_block(x, 69, 8, iron).unwrap();
        }
        source.set_block(8, 68, 8, iron).unwrap();
        source.set_block(7, 70, 8, stone).unwrap();
        assert!(!region.construct_golem(&ctx, [8, 70, 8]));
        assert_eq!(source.block(8, 70, 8), Some(pumpkin));
        assert!(region.entities.keys().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_pattern_crossing_an_unowned_chunk_is_refused_until_both_chunks_are_held() {
        let source = loaded_source().await;
        ensure_source_chunk(&source, 1, 0).await;
        let iron = u32::from(ferrite_data::blocks::default_state("minecraft:iron_block").unwrap());
        let pumpkin =
            u32::from(ferrite_data::blocks::default_state("minecraft:carved_pumpkin").unwrap());
        for x in 14..=16 {
            source.set_block(x, 69, 8, iron).unwrap();
        }
        source.set_block(15, 68, 8, iron).unwrap();
        source.set_block(15, 70, 8, pumpkin).unwrap();

        let ctx = bare_context(&source);
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        assert!(!region.construct_golem(&ctx, [15, 70, 8]));
        assert_eq!(source.block(15, 70, 8), Some(pumpkin));
        assert!(region.entities.keys().is_empty());

        region.hold_chunk(ChunkPos(1, 0));
        assert!(region.construct_golem(&ctx, [15, 70, 8]));
        assert_eq!(source.block(16, 69, 8), Some(0));
        let key = *region.entities.keys().first().expect("spawned iron golem");
        assert_eq!(
            mob::state(&region.entities, key).unwrap().kind,
            MobKind::IronGolem
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shearing_a_snow_golem_drops_a_pumpkin_and_wears_the_shears() {
        if super::super::drops::tables().is_none() {
            return;
        }
        let ctx = tick_context();
        let mut region = Region::default();
        let key = region.entities.insert(mob::new_mob(
            MobKind::SnowGolem,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut region.rng,
        ));
        let uuid = Uuid::from_u128(0x902);
        let mut player = test_player(uuid, 3002, [1.5, 64.0, 0.5], 0, &ctx);
        let slot = player.hand_slot(0);
        player.inventory.set(
            slot,
            ferrite_item::ItemStack::from_name("minecraft:shears").unwrap(),
        );
        region.players.insert(uuid, player);

        assert!(region.use_shears_on_snow_golem(uuid, key, "minecraft:shears", slot, 0));
        assert!(mob::state(&region.entities, key).unwrap().sheared);
        assert_eq!(
            region
                .entities
                .data(key)
                .unwrap()
                .metadata
                .get(ferrite_entity::metadata::snow_golem::PUMPKIN),
            Some(&ferrite_protocol::packets::play::MetaValue::Byte(0))
        );
        assert_eq!(dropped_items(&region), vec!["minecraft:carved_pumpkin"]);
        assert_eq!(region.players[&uuid].inventory.get(slot).damage(), 1);

        // A second click is not used (vanilla `PASS`) and wears nothing more.
        assert!(!region.use_shears_on_snow_golem(uuid, key, "minecraft:shears", slot, 0));
        assert_eq!(region.players[&uuid].inventory.get(slot).damage(), 1);
        assert_eq!(dropped_items(&region), vec!["minecraft:carved_pumpkin"]);
    }

    #[test]
    fn a_dead_enderman_drops_its_carried_block() {
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let grass = ferrite_data::blocks::default_state("minecraft:grass_block").unwrap();
        let key = region.entities.insert(mob::new_mob(
            MobKind::Enderman,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut region.rng,
        ));
        {
            let state = mob::state_mut(&mut region.entities, key).unwrap();
            state.carried = Some(grass);
            state.health = 0.0;
        }
        region.tick(&tick_context());
        let drops = dropped_items(&region);
        assert!(
            drops.contains(&"minecraft:grass_block"),
            "the carried block is dropped: {drops:?}"
        );
    }

    #[test]
    fn a_dead_sheep_drops_wool_of_its_colour_unless_it_was_sheared() {
        if super::super::drops::tables().is_none() {
            return;
        }
        for (sheared, expect_wool) in [(false, true), (true, false)] {
            let (mut region, key) = sheep_region();
            region.hold_chunk(ChunkPos(0, 0));
            {
                let state = mob::state_mut(&mut region.entities, key).unwrap();
                state.sheared = sheared;
                state.health = 0.0;
                state.last_killer = Some(Uuid::from_u128(5));
            }
            region.tick(&tick_context());
            let drops = dropped_items(&region);
            assert_eq!(
                drops.contains(&"minecraft:blue_wool"),
                expect_wool,
                "sheared {sheared}: {drops:?}"
            );
        }
    }

    fn eat_context(
        source: &Arc<super::super::anvil::AnvilChunkSource>,
        griefing: bool,
    ) -> TickContext {
        let mut ctx = tick_context();
        ctx.source = Some(Arc::clone(source));
        if !griefing {
            ctx.rules
                .lock()
                .unwrap()
                .set("mob_griefing", "false")
                .unwrap();
        }
        ctx
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_eaten_grass_block_becomes_dirt_and_a_plant_disappears() {
        let source = loaded_source().await;
        let grass =
            u32::from(ferrite_data::blocks::default_state("minecraft:grass_block").unwrap());
        let plant =
            u32::from(ferrite_data::blocks::default_state("minecraft:short_grass").unwrap());
        source.set_block(2, 63, 2, grass).unwrap();
        source.set_block(4, 64, 4, plant).unwrap();
        let ctx = eat_context(&source, true);
        let mut region = Region::default();
        region.held_chunks.insert(ChunkPos(0, 0), 1);
        region.eat_block(&ctx, [2, 63, 2], true);
        region.eat_block(&ctx, [4, 64, 4], false);
        let dirt = u32::from(ferrite_data::blocks::default_state("minecraft:dirt").unwrap());
        assert_eq!(source.block(2, 63, 2), Some(dirt));
        assert_eq!(source.block(4, 64, 4), Some(0));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn mob_griefing_off_leaves_the_eaten_block_alone() {
        let source = loaded_source().await;
        let grass =
            u32::from(ferrite_data::blocks::default_state("minecraft:grass_block").unwrap());
        source.set_block(2, 63, 2, grass).unwrap();
        let ctx = eat_context(&source, false);
        let mut region = Region::default();
        region.held_chunks.insert(ChunkPos(0, 0), 1);
        region.eat_block(&ctx, [2, 63, 2], true);
        assert_eq!(source.block(2, 63, 2), Some(grass));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn snow_golem_trail_places_snow_only_in_owned_chunks_and_respects_mob_griefing() {
        let source = loaded_source().await;
        // Also load chunk (1, 0) in the chunk source, but do NOT add it to region.held_chunks.
        for _ in 0..2_000 {
            if let Some(result) = source.get(1, 0, 0) {
                result.expect("neighbor chunk load");
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let grass =
            u32::from(ferrite_data::blocks::default_state("minecraft:grass_block").unwrap());
        let ice = u32::from(ferrite_data::blocks::default_state("minecraft:ice").unwrap());
        let snow = u32::from(ferrite_data::blocks::default_state("minecraft:snow").unwrap());

        source.set_block(2, 63, 2, grass).unwrap();
        source.set_block(2, 64, 2, 0).unwrap();
        source.set_block(3, 63, 2, ice).unwrap();
        source.set_block(3, 64, 2, 0).unwrap();
        // Cross-region / unowned chunk block (chunk (1, 0)).
        source.set_block(18, 63, 2, grass).unwrap();
        source.set_block(18, 64, 2, 0).unwrap();

        let mut region = Region::default();
        region.held_chunks.insert(ChunkPos(0, 0), 1);

        // mobGriefing=false must not place snow.
        let ctx_off = eat_context(&source, false);
        region.place_snow_trail(&ctx_off, [2, 64, 2]);
        assert_eq!(source.block(2, 64, 2), Some(0));

        // mobGriefing=true places snow on grass in owned chunk, skips ice, skips unowned chunk.
        let ctx_on = eat_context(&source, true);
        region.place_snow_trail(&ctx_on, [2, 64, 2]);
        region.place_snow_trail(&ctx_on, [3, 64, 2]);
        region.place_snow_trail(&ctx_on, [18, 64, 2]);
        assert_eq!(source.block(2, 64, 2), Some(snow));
        assert_eq!(source.block(3, 64, 2), Some(0));
        assert_eq!(source.block(18, 64, 2), Some(0));
    }

    #[test]
    fn mooshroom_bowl_interaction_gives_mushroom_stew_for_adult_and_ignores_baby() {
        let bowl = ferrite_item::ItemStack::from_name("minecraft:bowl").unwrap();
        let stew = ferrite_item::ItemStack::from_name("minecraft:mushroom_stew").unwrap();

        // Single bowl in survival -> replaced in-hand by mushroom_stew.
        let (slot1, extra1) = mooshroom_bowl_result(&bowl.copy_with_count(1), 0, &stew);
        assert_eq!(slot1, stew);
        assert_eq!(extra1, None);

        // Stacked bowls in survival -> decremented by 1 and stew returned as extra.
        let (slot2, extra2) = mooshroom_bowl_result(&bowl.copy_with_count(4), 0, &stew);
        assert_eq!(slot2.count, 3);
        assert_eq!(extra2, Some(stew.clone()));

        // Creative keeps bowl stack and gives stew.
        let (slot3, extra3) = mooshroom_bowl_result(&bowl.copy_with_count(4), 1, &stew);
        assert_eq!(slot3.count, 4);
        assert_eq!(extra3, Some(stew));

        let mut region = Region::default();
        let adult = region.entities.insert(mob::new_mob(
            MobKind::Mooshroom,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut region.rng,
        ));
        let baby = region.entities.insert(mob::new_mob(
            MobKind::Mooshroom,
            ferrite_math::Vec3::new(2.5, 64.0, 0.5),
            &mut region.rng,
        ));
        mob::state_mut(&mut region.entities, baby).unwrap().age = -1000;
        let cow = region.entities.insert(mob::new_mob(
            MobKind::Cow,
            ferrite_math::Vec3::new(4.5, 64.0, 0.5),
            &mut region.rng,
        ));

        assert!(region.use_bowl_on_mooshroom(Uuid::nil(), adult, "minecraft:bowl", 0, 0));
        assert!(!region.use_bowl_on_mooshroom(Uuid::nil(), baby, "minecraft:bowl", 0, 0));
        assert!(!region.use_bowl_on_mooshroom(Uuid::nil(), cow, "minecraft:bowl", 0, 0));
        assert!(!region.use_bowl_on_mooshroom(Uuid::nil(), adult, "minecraft:wheat", 0, 0));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn iron_ingot_repairs_a_damaged_golem_and_is_not_used_at_full_health() {
        let ctx = tick_context();
        let mut region = Region::default();
        let golem = region.entities.insert(mob::new_mob(
            MobKind::IronGolem,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut region.rng,
        ));
        let max = MobKind::IronGolem.max_health();
        mob::state_mut(&mut region.entities, golem).unwrap().health = 50.0;

        let uuid = Uuid::from_u128(0x903);
        let mut player = test_player(uuid, 3003, [1.5, 64.0, 0.5], 0, &ctx);
        let slot = player.hand_slot(0);
        player.inventory.set(
            slot,
            ferrite_item::ItemStack::from_name("minecraft:iron_ingot")
                .unwrap()
                .copy_with_count(2),
        );
        region.players.insert(uuid, player);

        assert!(region.use_iron_ingot_on_golem(uuid, golem, "minecraft:iron_ingot", slot, 0));
        assert_eq!(mob::state(&region.entities, golem).unwrap().health, 75.0);
        assert_eq!(region.players[&uuid].inventory.get(slot).count, 1);

        // A full-health golem ignores the ingot and does not consume it.
        mob::state_mut(&mut region.entities, golem).unwrap().health = max;
        assert!(!region.use_iron_ingot_on_golem(uuid, golem, "minecraft:iron_ingot", slot, 0));
        assert_eq!(region.players[&uuid].inventory.get(slot).count, 1);
        assert_eq!(mob::state(&region.entities, golem).unwrap().health, max);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn milking_an_adult_cow_and_goat_fills_a_bucket_and_a_baby_ignores_it() {
        let ctx = tick_context();
        let mut region = Region::default();
        let cow = region.entities.insert(mob::new_mob(
            MobKind::Cow,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut region.rng,
        ));
        let goat = region.entities.insert(mob::new_mob(
            MobKind::Goat,
            ferrite_math::Vec3::new(2.5, 64.0, 0.5),
            &mut region.rng,
        ));
        let baby = region.entities.insert(mob::new_mob(
            MobKind::Mooshroom,
            ferrite_math::Vec3::new(4.5, 64.0, 0.5),
            &mut region.rng,
        ));
        mob::state_mut(&mut region.entities, baby).unwrap().age = -1000;

        let uuid = Uuid::from_u128(0x904);
        let mut player = test_player(uuid, 3004, [1.5, 64.0, 0.5], 0, &ctx);
        let slot = player.hand_slot(0);
        player.inventory.set(
            slot,
            ferrite_item::ItemStack::from_name("minecraft:bucket").unwrap(),
        );
        region.players.insert(uuid, player);

        let bucket = "minecraft:bucket";
        assert!(region.use_bucket_on_milkable(uuid, cow, bucket, slot, 0));
        assert_eq!(
            region.players[&uuid].inventory.get(slot).name(),
            Some("minecraft:milk_bucket")
        );

        // Re-fill the hand for the goat and the baby checks.
        for (key, expected) in [(goat, true), (baby, false)] {
            region
                .players
                .get_mut(&uuid)
                .unwrap()
                .inventory
                .set(slot, ferrite_item::ItemStack::from_name(bucket).unwrap());
            assert_eq!(
                region.use_bucket_on_milkable(uuid, key, bucket, slot, 0),
                expected
            );
            assert_eq!(
                region.players[&uuid].inventory.get(slot).name(),
                Some(if expected {
                    "minecraft:milk_bucket"
                } else {
                    "minecraft:bucket"
                })
            );
        }
    }

    #[test]
    fn cross_region_iron_golem_and_bee_special_melee_preserve_launch_and_poison_signals() {
        let (ctx, mut signals, player) = listening_context();
        // Player owned by another region just across the region edge.
        ctx.positions
            .lock()
            .unwrap()
            .insert(player, [128.5, 65.0, 0.5]);

        let mut region = Region::default();
        let golem = region.entities.insert(mob::new_mob(
            MobKind::IronGolem,
            ferrite_math::Vec3::new(127.5, 65.0, 0.5),
            &mut region.rng,
        ));
        assert!(mob::retaliate(&mut region.entities, golem, player));

        let bee = region.entities.insert(mob::new_mob(
            MobKind::Bee,
            ferrite_math::Vec3::new(127.5, 65.0, 0.5),
            &mut region.rng,
        ));
        assert!(mob::retaliate(&mut region.entities, bee, player));

        let blocks = TickBlocks {
            source: None,
            seen: Default::default(),
        };
        region.tick_mobs(&ctx, &blocks);

        let mut saw_golem_fling = false;
        let mut saw_bee_poison = false;
        while let Ok(signal) = signals.try_recv() {
            if let Signal::MobMelee {
                player: target,
                launch_y,
                poison_ticks,
                ..
            } = signal
            {
                assert_eq!(target, player);
                if (launch_y - ferrite_entity::golem::IRON_GOLEM_LAUNCH_Y).abs() < 1e-6 {
                    saw_golem_fling = true;
                }
                if poison_ticks == ferrite_entity::bee::POISON_TICKS_NORMAL {
                    saw_bee_poison = true;
                }
            }
        }
        assert!(
            saw_golem_fling,
            "cross-region Iron Golem melee must preserve +0.4 vertical launch"
        );
        assert!(
            saw_bee_poison,
            "cross-region Bee sting must preserve Poison duration"
        );
        assert!(
            mob::state(&region.entities, bee).unwrap().has_stung,
            "bee must be marked has_stung after cross-region sting"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_enderman_takes_and_places_a_holdable_block() {
        let source = loaded_source().await;
        let grass =
            u32::from(ferrite_data::blocks::default_state("minecraft:grass_block").unwrap());
        let stone = u32::from(ferrite_data::blocks::default_state("minecraft:stone").unwrap());
        source.set_block(2, 63, 2, stone).unwrap();
        source.set_block(2, 64, 2, grass).unwrap();
        source.set_block(3, 63, 2, stone).unwrap();
        source.set_block(3, 64, 2, 0).unwrap();
        let ctx = eat_context(&source, true);
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let key = region.entities.insert(mob::new_mob(
            MobKind::Enderman,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut region.rng,
        ));
        region.take_enderman_block(&ctx, key, [2, 64, 2]);
        assert_eq!(source.block(2, 64, 2), Some(0));
        let carried = mob::state(&region.entities, key).unwrap().carried;
        assert_eq!(carried, Some(u16::try_from(grass).unwrap()));
        assert_eq!(
            region
                .entities
                .data(key)
                .unwrap()
                .metadata
                .get(ferrite_entity::enderman::CARRIED_BLOCK_INDEX),
            Some(&MetaValue::OptionalBlockState(carried)),
            "the take event updates the synched carried block"
        );
        region.place_enderman_block(&ctx, key, [3, 64, 2], carried.unwrap());
        assert_eq!(source.block(3, 64, 2), Some(grass));
        assert_eq!(mob::state(&region.entities, key).unwrap().carried, None);
        assert_eq!(
            region
                .entities
                .data(key)
                .unwrap()
                .metadata
                .get(ferrite_entity::enderman::CARRIED_BLOCK_INDEX),
            Some(&MetaValue::OptionalBlockState(None)),
            "the place event clears the synched carried block"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn enderman_block_changes_obey_mob_griefing() {
        let source = loaded_source().await;
        let grass =
            u32::from(ferrite_data::blocks::default_state("minecraft:grass_block").unwrap());
        source.set_block(2, 63, 2, grass).unwrap();
        source.set_block(2, 64, 2, 0).unwrap();
        let ctx = eat_context(&source, false);
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let key = region.entities.insert(mob::new_mob(
            MobKind::Enderman,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut region.rng,
        ));
        region.take_enderman_block(&ctx, key, [2, 63, 2]);
        assert_eq!(source.block(2, 63, 2), Some(grass));
        assert_eq!(mob::state(&region.entities, key).unwrap().carried, None);
    }

    #[test]
    fn a_horse_mounted_with_ride_is_steered_by_its_player_rider() {
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let key = dead_equine(&mut region, MobKind::Horse);
        if let Some(state) = mob::state_mut(&mut region.entities, key) {
            state.health = 20.0;
            state.saddled = true;
            state.owner = Some(Uuid::from_u128(0x51d1));
        }
        let horse = region.entities.data(key).unwrap().uuid;
        let rider = Uuid::from_u128(0x51d2);
        assert!(!super::super::ride::is_controlled(horse));
        super::super::ride::mount(rider, horse, false).unwrap();
        region.tick(&tick_context());
        assert!(
            super::super::ride::is_controlled(horse),
            "steered by the rider"
        );
        assert!(mob::state(&region.entities, key).unwrap().no_ai);
        // Without a saddle the rider cannot steer it.
        super::super::ride::dismount(rider);
        super::super::ride::set_controlled(horse, false);
        if let Some(state) = mob::state_mut(&mut region.entities, key) {
            state.saddled = false;
            state.no_ai = false;
        }
        super::super::ride::mount(rider, horse, false).unwrap();
        region.tick(&tick_context());
        assert!(!super::super::ride::is_controlled(horse));
        super::super::ride::dismount(rider);
    }

    #[test]
    fn use_pig_equips_a_saddle_and_mounts_an_adult_saddled_pig() {
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let key = region.entities.insert(mob::new_mob(
            MobKind::Pig,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut region.rng,
        ));
        let pig_uuid = region.entities.data(key).unwrap().uuid;
        let rider = Uuid::from_u128(0x7161);
        let ctx = tick_context();

        // Unsaddled pig cannot be mounted with empty hand, but equips a saddle when used with one.
        assert!(!region.use_pig(&ctx, rider, key, "", 0, 1));
        assert!(region.use_pig(&ctx, rider, key, "minecraft:saddle", 0, 1));
        assert!(mob::state(&region.entities, key).unwrap().saddled);

        // Once saddled, right-clicking mounts the pig (unless holding pig food like carrot).
        assert!(!region.use_pig(&ctx, rider, key, "minecraft:carrot", 0, 1));
        assert!(region.use_pig(&ctx, rider, key, "", 0, 1));
        assert_eq!(super::super::ride::vehicle_of(rider), Some(pig_uuid));
        super::super::ride::dismount(rider);
    }

    #[test]
    fn a_plugin_launched_trident_keeps_identity_owner_velocity_and_returns_as_an_item() {
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let uuid = Uuid::from_u128(0x7121);
        let owner = Uuid::from_u128(0xa11);
        assert!(region.plugin_spawn(
            [8.0, 70.0, 8.0],
            "minecraft:trident",
            None,
            [0.0, 0.5, 2.5],
            uuid,
            Some(owner),
        ));
        let key = *region.entities.keys().first().expect("spawned");
        let data = region.entities.data(key).unwrap();
        assert_eq!(data.uuid, uuid);
        assert_eq!(arrow::kind_of(data), Some(arrow::Kind::Trident));
        let state = data.arrow.expect("flying state");
        assert_eq!(state.owner, Some(owner));
        assert_eq!(state.damage, arrow::TRIDENT_DAMAGE);
        let vel = region.entities.vel(key).unwrap();
        assert_eq!((vel.x, vel.y, vel.z), (0.0, 0.5, 2.5));
        let (kept_owner, stack) = region.thrown_tridents.get(&uuid).expect("tracked");
        assert_eq!(*kept_owner, owner);
        assert_eq!(stack.name(), Some("minecraft:trident"));
    }

    #[test]
    fn a_dead_equine_baby_drops_no_gear() {
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let key = dead_equine(&mut region, MobKind::Horse);
        let armor = ferrite_item::item_id("minecraft:iron_horse_armor").unwrap();
        if let Some(state) = mob::state_mut(&mut region.entities, key) {
            state.age = -1;
            state.saddled = true;
            state.armor = armor;
        }
        region.tick(&tick_context());
        assert!(dropped_gear(&region).is_empty());
    }

    #[test]
    fn a_dead_equine_llama_never_drops_horse_armor() {
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let key = dead_equine(&mut region, MobKind::Llama);
        let armor = ferrite_item::item_id("minecraft:iron_horse_armor").unwrap();
        mob::state_mut(&mut region.entities, key).unwrap().armor = armor;
        region.tick(&tick_context());
        assert!(dropped_gear(&region).is_empty());
    }

    #[test]
    fn a_dead_equine_llama_drops_its_carpet() {
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let key = dead_equine(&mut region, MobKind::Llama);
        let carpet = ferrite_item::item_id("minecraft:white_carpet").unwrap();
        mob::state_mut(&mut region.entities, key).unwrap().armor = carpet;
        region.tick(&tick_context());
        assert!(dropped_items(&region).contains(&"minecraft:white_carpet"));
    }

    #[test]
    fn a_dead_equine_trader_llama_drops_its_carpet_not_a_saddle() {
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let key = dead_equine(&mut region, MobKind::TraderLlama);
        let carpet = ferrite_item::item_id("minecraft:white_carpet").unwrap();
        mob::state_mut(&mut region.entities, key).unwrap().armor = carpet;
        region.tick(&tick_context());
        let names = dropped_items(&region);
        assert!(names.contains(&"minecraft:white_carpet"));
        assert!(!names.contains(&"minecraft:saddle"));
    }

    #[test]
    fn a_dead_equine_mule_drops_its_pack() {
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let key = dead_equine(&mut region, MobKind::Mule);
        let uuid = region.entities.data(key).unwrap().uuid;
        let mut chest = super::super::equine_chest::load(uuid, 15);
        chest[1] = ferrite_item::ItemStack::from_name("minecraft:bread").unwrap();
        super::super::equine_chest::store(uuid, &chest);
        mob::state_mut(&mut region.entities, key).unwrap().chest = true;
        region.tick(&tick_context());
        let names = dropped_items(&region);
        assert!(names.contains(&"minecraft:bread"));
        assert!(names.contains(&"minecraft:chest"));
    }

    #[test]
    fn cat_and_parrot_ownership_survives_an_entity_save() {
        let owner = Uuid::from_u128(9);
        for kind in [MobKind::Cat, MobKind::Parrot] {
            let mut record = mob::new_mob(
                kind,
                ferrite_math::Vec3::new(0.5, 64.0, 0.5),
                &mut ferrite_entity::Rng::new(1),
            );
            let uuid = record.data.uuid;
            if let Some(state) = record.data.mob.as_mut() {
                state.owner = Some(owner);
                state.sitting = true;
            }
            // `use_on` writes the same metadata while the entity is alive.
            ferrite_entity::tame::define_metadata(
                &mut record.data.metadata,
                record.data.mob.as_ref().unwrap(),
            );
            let loaded =
                super::super::entity_save::from_nbt(&super::super::entity_save::to_nbt(&[record]));
            assert_eq!(loaded.len(), 1, "{kind:?}");
            let record = &loaded[0];
            assert_eq!(record.data.uuid, uuid, "{kind:?}");
            let state = record.data.mob.as_ref().unwrap();
            assert_eq!(state.owner, Some(owner), "{kind:?}");
            assert!(state.sitting, "{kind:?}");
            assert_eq!(
                record.data.metadata.get(ferrite_entity::tame::FLAGS_INDEX),
                Some(&ferrite_protocol::packets::play::MetaValue::Byte(1 | 4)),
                "{kind:?}"
            );
            assert_eq!(
                record.data.metadata.get(ferrite_entity::tame::OWNER_INDEX),
                Some(&ferrite_protocol::packets::play::MetaValue::OptionalUuid(
                    Some(owner)
                )),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn taming_outcomes_route_the_vanilla_tame_and_failure_events() {
        assert_eq!(ferrite_entity::tame::Used::Tamed.status(), Some(7));
        assert_eq!(ferrite_entity::tame::Used::Consumed.status(), Some(6));
        assert_eq!(ferrite_entity::tame::Used::Toggled.status(), None);
    }

    #[test]
    fn a_dead_pack_equine_still_drops_chest_and_saddle() {
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let key = dead_equine(&mut region, MobKind::Donkey);
        let uuid = region.entities.data(key).unwrap().uuid;
        let mut chest = super::super::equine_chest::load(uuid, 15);
        chest[0] = ferrite_item::ItemStack::from_name("minecraft:bread").unwrap();
        super::super::equine_chest::store(uuid, &chest);
        if let Some(state) = mob::state_mut(&mut region.entities, key) {
            state.chest = true;
            state.saddled = true;
        }
        region.tick(&tick_context());
        let names = dropped_items(&region);
        assert!(names.contains(&"minecraft:bread"));
        assert!(names.contains(&"minecraft:chest"));
        assert!(names.contains(&"minecraft:saddle"));
        assert!(
            super::super::equine_chest::load(uuid, 15)
                .iter()
                .all(ferrite_item::ItemStack::is_empty),
            "the dropped pack is not kept for another drop or save"
        );
    }

    #[test]
    fn independent_menus_count_two_sales_once_and_credit_each_player() {
        use ferrite_inventory::{ClickInput, Clicker, Menu, NoCrafting, PlayerInventory};

        let mut store = EntityStore::default();
        let record = mob::new_mob(
            MobKind::Villager,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(1),
        );
        let uuid = record.data.uuid;
        let key = store.insert(record);
        let alice = Uuid::from_u128(31);
        let bob = Uuid::from_u128(32);
        let emeralds = |count| {
            ferrite_item::ItemStack::from_name("emerald")
                .expect("emerald")
                .copy_with_count(count)
        };
        let bread = ferrite_item::ItemStack::from_name("bread").expect("bread");
        let mut offer = ferrite_inventory::Trade::new(emeralds(32), bread.clone(), 2, 1);
        offer.price_multiplier = 0.05;
        {
            let villager = store.data_mut(key).unwrap().villager.as_mut().unwrap();
            villager.offers = vec![offer];
            villager.gossips.add(
                alice,
                ferrite_entity::villager::GossipType::MajorPositive,
                20,
            );
        }

        let snapshot = |store: &EntityStore, player| {
            store
                .data(key)
                .unwrap()
                .villager
                .as_ref()
                .unwrap()
                .offers_for_player(player)
        };
        let mut alice_menu = Menu::merchant(1, snapshot(&store, alice));
        let mut bob_menu = Menu::merchant(2, snapshot(&store, bob));
        assert_eq!(alice_menu.trades()[0].price(), 27);
        assert_eq!(bob_menu.trades()[0].price(), 32);

        // Both menus were opened independently. Alice buys one of two uses first.
        alice_menu.container_mut()[0] = emeralds(27);
        assert!(alice_menu.select_trade(0));
        {
            let crafter = NoCrafting;
            let mut inventory = PlayerInventory::new();
            let mut clicker = Clicker::new(&mut inventory, false, &crafter);
            alice_menu.clicked(&mut clicker, 2, 0, ClickInput::Pickup);
        }
        assert_eq!(alice_menu.trades()[0].uses, 1);
        assert_eq!(
            merge_trader_trades(&mut store, alice, uuid, alice_menu.trades()),
            1
        );

        // Bob's screen still has the old uses; refreshing it before the click shows the one
        // remaining use at Bob's neutral price, and the sale must count exactly once.
        bob_menu.container_mut()[0] = emeralds(32);
        assert!(bob_menu.select_trade(0));
        assert_eq!(bob_menu.result(), &bread);
        bob_menu.set_trades(snapshot(&store, bob));
        assert_eq!(bob_menu.trades()[0].uses, 1);
        assert_eq!(bob_menu.trades()[0].price(), 32);
        assert_eq!(bob_menu.result(), &bread);
        {
            let crafter = NoCrafting;
            let mut inventory = PlayerInventory::new();
            let mut clicker = Clicker::new(&mut inventory, false, &crafter);
            bob_menu.clicked(&mut clicker, 2, 0, ClickInput::Pickup);
        }
        assert_eq!(bob_menu.trades()[0].uses, 2);
        assert_eq!(
            merge_trader_trades(&mut store, bob, uuid, bob_menu.trades()),
            1
        );
        assert_eq!(
            merge_trader_trades(&mut store, bob, uuid, bob_menu.trades()),
            0,
            "reconciling the same accepted snapshot twice must be a no-op"
        );

        let villager = store.data(key).unwrap().villager.as_ref().unwrap();
        assert_eq!((villager.offers[0].uses, villager.offers[0].demand), (2, 2));
        assert!(villager.offers[0].is_out_of_stock());
        assert_eq!(
            villager
                .gossips
                .get(alice, ferrite_entity::villager::GossipType::Trading),
            ferrite_entity::villager::TRADING_GOSSIP
        );
        assert_eq!(
            villager
                .gossips
                .get(bob, ferrite_entity::villager::GossipType::Trading),
            ferrite_entity::villager::TRADING_GOSSIP
        );
    }

    #[test]
    fn a_region_handoff_reconciles_sales_in_the_new_owner_once() {
        use ferrite_inventory::{ClickInput, Clicker, Menu, NoCrafting, PlayerInventory};

        let mut old = EntityStore::default();
        let record = mob::new_mob(
            MobKind::Villager,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(1),
        );
        let uuid = record.data.uuid;
        let key = old.insert(record);
        let alice = Uuid::from_u128(41);
        let bob = Uuid::from_u128(42);
        let emeralds = |count| {
            ferrite_item::ItemStack::from_name("emerald")
                .expect("emerald")
                .copy_with_count(count)
        };
        let bread = ferrite_item::ItemStack::from_name("bread").expect("bread");
        let mut offer = ferrite_inventory::Trade::new(emeralds(2), bread.clone(), 2, 1);
        offer.price_multiplier = 0.05;
        old.data_mut(key).unwrap().villager.as_mut().unwrap().offers = vec![offer];

        let snapshot = |store: &EntityStore, key, player| {
            store
                .data(key)
                .unwrap()
                .villager
                .as_ref()
                .unwrap()
                .offers_for_player(player)
        };
        let mut alice_menu = Menu::merchant(1, snapshot(&old, key, alice));
        alice_menu.container_mut()[0] = emeralds(2);
        assert!(alice_menu.select_trade(0));
        {
            let crafter = NoCrafting;
            let mut inventory = PlayerInventory::new();
            let mut clicker = Clicker::new(&mut inventory, false, &crafter);
            alice_menu.clicked(&mut clicker, 2, 0, ClickInput::Pickup);
        }
        assert_eq!(
            merge_trader_trades(&mut old, alice, uuid, alice_menu.trades()),
            1
        );

        // The region hands the entity to another store. A stale reconciliation against the old
        // owner must not count or credit anything; the new owner has the authoritative uses.
        let record = old.remove(key).expect("villager record");
        let mut new = EntityStore::default();
        let new_key = new.insert(record);
        assert!(old.keys().is_empty());
        assert!(trader_key(&old, uuid).is_none());
        assert!(trader_key(&new, uuid).is_some());
        assert_eq!(
            merge_trader_trades(&mut old, alice, uuid, alice_menu.trades()),
            0,
            "a late commit against the old owner must not count"
        );
        assert_eq!(
            merge_trader_trades(&mut new, alice, uuid, alice_menu.trades()),
            0,
            "the sale was already merged before the hand-off"
        );

        // Bob's menu is refreshed from the new owner. The remaining use is accepted there and
        // counts exactly once, with Bob's own gossip.
        let mut bob_menu = Menu::merchant(3, snapshot(&new, new_key, bob));
        assert_eq!(bob_menu.trades()[0].uses, 1);
        bob_menu.container_mut()[0] = emeralds(2);
        assert!(bob_menu.select_trade(0));
        assert_eq!(bob_menu.result(), &bread);
        {
            let crafter = NoCrafting;
            let mut inventory = PlayerInventory::new();
            let mut clicker = Clicker::new(&mut inventory, false, &crafter);
            bob_menu.clicked(&mut clicker, 2, 0, ClickInput::Pickup);
        }
        assert_eq!(
            merge_trader_trades(&mut new, bob, uuid, bob_menu.trades()),
            1
        );
        assert_eq!(
            merge_trader_trades(&mut new, bob, uuid, bob_menu.trades()),
            0,
            "the same accepted snapshot must not be counted twice after a hand-off"
        );

        let villager = new.data(new_key).unwrap().villager.as_ref().unwrap();
        assert_eq!((villager.offers[0].uses, villager.offers[0].demand), (2, 2));
        assert!(villager.offers[0].is_out_of_stock());
        assert_eq!(
            villager
                .gossips
                .get(alice, ferrite_entity::villager::GossipType::Trading),
            ferrite_entity::villager::TRADING_GOSSIP
        );
        assert_eq!(
            villager
                .gossips
                .get(bob, ferrite_entity::villager::GossipType::Trading),
            ferrite_entity::villager::TRADING_GOSSIP
        );
    }

    #[test]
    fn shoulder_metadata_uses_the_player_shoulder_indices_and_dimension() {
        use super::super::Dimension::{Nether, Overworld};

        let left = shoulder_metadata_bytes(42, ShoulderSide::Left, Some(2));
        assert_eq!(
            left,
            [101, 42, 19, 19, 1, 2, 0xFF],
            "set_entity_data, player entity, index 19, optional-varint, variant 2"
        );
        assert_eq!(
            shoulder_metadata_bytes(42, ShoulderSide::Right, None),
            [101, 42, 20, 19, 0, 0xFF]
        );

        let source = Uuid::from_u128(1);
        let viewer = Uuid::from_u128(2);
        let (sender, mut rx) = OutboundSender::channel(16);
        let (nether, mut nether_rx) = OutboundSender::channel(16);
        let tracking: Tracking = Arc::new(Mutex::new(HashMap::from([
            (
                viewer,
                (
                    super::super::WorldId::Vanilla(Overworld),
                    HashSet::new(),
                    sender,
                ),
            ),
            (
                Uuid::from_u128(3),
                (
                    super::super::WorldId::Vanilla(Nether),
                    HashSet::new(),
                    nether,
                ),
            ),
        ])));
        broadcast_shoulder_bytes(&tracking, Overworld, source, &left);
        assert_eq!(outbound_packets(&mut rx), vec![left]);
        assert!(outbound_packets(&mut nether_rx).is_empty());
    }

    #[test]
    fn player_metadata_broadcast_stays_in_dimension_and_excludes_source() {
        let source = Uuid::from_u128(1);
        let viewer = Uuid::from_u128(2);
        assert!(should_broadcast_player_metadata(
            source,
            viewer,
            super::super::WorldId::Vanilla(super::super::Dimension::Overworld),
            super::super::WorldId::Vanilla(super::super::Dimension::Overworld),
        ));
        assert!(!should_broadcast_player_metadata(
            source,
            source,
            super::super::WorldId::Vanilla(super::super::Dimension::Overworld),
            super::super::WorldId::Vanilla(super::super::Dimension::Overworld),
        ));
        assert!(!should_broadcast_player_metadata(
            source,
            viewer,
            super::super::WorldId::Vanilla(super::super::Dimension::Overworld),
            super::super::WorldId::Vanilla(super::super::Dimension::Nether),
        ));
    }

    fn outbound_packets(rx: &mut ferrite_net::game::OutboundReceiver) -> Vec<Vec<u8>> {
        std::iter::from_fn(|| rx.try_recv())
            .flat_map(|out| match out {
                Outbound::Packet(bytes) => vec![bytes.to_vec()],
                Outbound::Batch(packets) => packets.iter().map(|bytes| bytes.to_vec()).collect(),
                Outbound::SharedFrame(sf) => vec![sf.raw.to_vec()],
                Outbound::Disconnect(reason) => panic!("unexpected disconnect: {reason}"),
            })
            .collect()
    }

    fn spin_packet(entity: i32, spin: bool) -> Vec<u8> {
        match packet(&spin_metadata(entity, spin)) {
            Outbound::Packet(bytes) => bytes.to_vec(),
            Outbound::Batch(_) => panic!("unexpected packet batch"),
            Outbound::SharedFrame(sf) => sf.raw.to_vec(),
            Outbound::Disconnect(reason) => panic!("unexpected disconnect: {reason}"),
        }
    }

    #[test]
    fn riptide_spin_metadata_reaches_all_same_dimension_viewers() {
        use super::super::Dimension::{Nether, Overworld};

        let source = Uuid::from_u128(1);
        let (first, mut first_rx) = OutboundSender::channel(1024);
        let (second, mut second_rx) = OutboundSender::channel(1024);
        let (nether, mut nether_rx) = OutboundSender::channel(1024);
        let tracking: Tracking = Arc::new(Mutex::new(HashMap::from([
            (
                Uuid::from_u128(2),
                (
                    super::super::WorldId::Vanilla(Overworld),
                    HashSet::new(),
                    first,
                ),
            ),
            (
                Uuid::from_u128(3),
                (
                    super::super::WorldId::Vanilla(Overworld),
                    HashSet::new(),
                    second,
                ),
            ),
            (
                Uuid::from_u128(4),
                (
                    super::super::WorldId::Vanilla(Nether),
                    HashSet::new(),
                    nether,
                ),
            ),
        ])));
        let entity = 42;

        broadcast_player_metadata(&tracking, Overworld, source, entity, true);
        assert_eq!(
            outbound_packets(&mut first_rx),
            vec![spin_packet(entity, true)]
        );
        assert_eq!(
            outbound_packets(&mut second_rx),
            vec![spin_packet(entity, true)]
        );
        assert!(outbound_packets(&mut nether_rx).is_empty());

        broadcast_player_metadata(&tracking, Overworld, source, entity, true);
        assert_eq!(
            outbound_packets(&mut first_rx),
            vec![spin_packet(entity, true)]
        );
        assert_eq!(
            outbound_packets(&mut second_rx),
            vec![spin_packet(entity, true)]
        );
        assert!(outbound_packets(&mut nether_rx).is_empty());

        broadcast_player_metadata(&tracking, Overworld, source, entity, false);
        assert_eq!(
            outbound_packets(&mut first_rx),
            vec![spin_packet(entity, false)]
        );
        assert_eq!(
            outbound_packets(&mut second_rx),
            vec![spin_packet(entity, false)]
        );
        assert!(outbound_packets(&mut nether_rx).is_empty());
    }

    #[test]
    fn ending_a_riptide_spin_clears_state_for_the_old_dimension() {
        use super::super::Dimension::{Nether, Overworld};

        let source = Uuid::from_u128(1);
        let (viewer, mut viewer_rx) = OutboundSender::channel(1024);
        let (nether, mut nether_rx) = OutboundSender::channel(1024);
        let tracking: Tracking = Arc::new(Mutex::new(HashMap::from([
            (
                Uuid::from_u128(2),
                (
                    super::super::WorldId::Vanilla(Overworld),
                    HashSet::new(),
                    viewer,
                ),
            ),
            (
                Uuid::from_u128(3),
                (
                    super::super::WorldId::Vanilla(Nether),
                    HashSet::new(),
                    nether,
                ),
            ),
        ])));
        let entity = 42;
        let mut spin_ticks = 20;

        broadcast_player_metadata(&tracking, Overworld, source, entity, true);
        assert_eq!(
            outbound_packets(&mut viewer_rx),
            vec![spin_packet(entity, true)]
        );

        assert!(end_player_spin(
            &mut spin_ticks,
            &tracking,
            Overworld,
            source,
            entity
        ));
        assert_eq!(spin_ticks, 0);
        assert_eq!(
            outbound_packets(&mut viewer_rx),
            vec![spin_packet(entity, false)]
        );
        assert!(outbound_packets(&mut nether_rx).is_empty());

        assert!(!end_player_spin(
            &mut spin_ticks,
            &tracking,
            Overworld,
            source,
            entity
        ));
        assert!(outbound_packets(&mut viewer_rx).is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn plugin_explosion_can_preserve_blocks_and_still_push_entities() {
        use ferrite_entity::{EntityData, EntityRecord, allocate_id, random_uuid};
        use ferrite_math::Vec3;

        let source = super::super::anvil::AnvilChunkSource::new_memory();
        let mut ready = false;
        for _ in 0..2_000 {
            if let Some(result) = source.get(0, 0, 0) {
                result.expect("chunk load");
                ready = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(ready, "chunk never loaded");
        let stone = u32::from(ferrite_data::blocks::default_state("minecraft:stone").unwrap());
        source.set_block(8, 65, 9, stone).unwrap();
        let mut region = Region::default();
        region.held_chunks.insert(ChunkPos(0, 0), 1);
        let key = region.entities.insert(EntityRecord {
            pos: Vec3::new(10.5, 66.0, 8.5),
            vel: Vec3::ZERO,
            flags: Flags::default(),
            data: EntityData::new(allocate_id(), random_uuid(), 71, 0.25, 0.25),
        });
        let cow = region.entities.insert(mob::new_mob(
            mob::MobKind::Cow,
            Vec3::new(15.0, 66.0, 8.5),
            &mut region.rng,
        ));
        let (signals, _rx) = tokio::sync::mpsc::unbounded_channel();
        let ctx = TickContext {
            signals,
            tracking: Default::default(),
            positions: Default::default(),
            source: Some(source.clone()),
            dimension: super::super::Dimension::Overworld,
            world_id: crate::game::WorldId::Vanilla(super::super::Dimension::Overworld),
            raining: false,
            rules: Default::default(),
            spawn: [0.0, 65.0, 0.0],
            command_entities: Default::default(),
            state: Default::default(),
            trader_spawner: Default::default(),
            profiler: None,
            tree: None,
            hooks: None,
        };
        region.explode([8.5, 66.5, 8.5], 4.0, false, &ctx);
        assert_eq!(source.block(8, 65, 9), Some(stone));
        assert_ne!(region.entities.vel(key).unwrap(), Vec3::ZERO);
        assert!(mob::state(&region.entities, cow).unwrap().health < mob::MobKind::Cow.max_health());
        region.explode([8.5, 66.5, 8.5], 4.0, true, &ctx);
        assert_ne!(source.block(8, 65, 9), Some(stone));
    }

    const SHULKER_BLOCKS: [&str; 17] = [
        "shulker_box",
        "white_shulker_box",
        "orange_shulker_box",
        "magenta_shulker_box",
        "light_blue_shulker_box",
        "yellow_shulker_box",
        "lime_shulker_box",
        "pink_shulker_box",
        "gray_shulker_box",
        "light_gray_shulker_box",
        "cyan_shulker_box",
        "purple_shulker_box",
        "blue_shulker_box",
        "brown_shulker_box",
        "green_shulker_box",
        "red_shulker_box",
        "black_shulker_box",
    ];

    /// The dropped item stacks lying in a region.
    fn ground_stacks(region: &Region) -> Vec<ferrite_item::ItemStack> {
        region
            .entities
            .keys()
            .iter()
            .filter_map(|key| region.entities.data(*key))
            .filter(|data| data.item.is_some())
            .map(ferrite_entity::item::full_stack)
            .collect()
    }

    fn stone_stack(count: i32) -> ferrite_item::ItemStack {
        ferrite_item::ItemStack::from_name("stone")
            .unwrap()
            .copy_with_count(count)
    }

    fn emeralds(count: i32) -> ferrite_item::ItemStack {
        ferrite_item::ItemStack::from_name("emerald")
            .unwrap()
            .copy_with_count(count)
    }

    fn container_of(stack: &ferrite_item::ItemStack) -> Vec<ferrite_item::ItemStack> {
        match stack.components.get(ferrite_item::component_ids::CONTAINER) {
            Some(ferrite_item::Component::Container(slots)) => slots
                .iter()
                .filter(|slot| !slot.is_empty())
                .cloned()
                .collect(),
            other => panic!("expected a typed container component, got {other:?}"),
        }
    }

    async fn loaded_source() -> std::sync::Arc<super::super::anvil::AnvilChunkSource> {
        let source = super::super::anvil::AnvilChunkSource::new_memory();
        for _ in 0..2_000 {
            if let Some(result) = source.get(0, 0, 0) {
                result.expect("chunk load");
                return source;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("chunk never loaded");
    }

    fn bare_context(source: &std::sync::Arc<super::super::anvil::AnvilChunkSource>) -> TickContext {
        let (signals, _rx) = tokio::sync::mpsc::unbounded_channel();
        TickContext {
            signals,
            tracking: Default::default(),
            positions: Default::default(),
            source: Some(source.clone()),
            dimension: super::super::Dimension::Overworld,
            world_id: crate::game::WorldId::Vanilla(super::super::Dimension::Overworld),
            raining: false,
            rules: Default::default(),
            spawn: [0.0, 65.0, 0.0],
            command_entities: Default::default(),
            state: Default::default(),
            trader_spawner: Default::default(),
            profiler: None,
            tree: None,
            hooks: None,
        }
    }

    fn fill_box(
        source: &super::super::anvil::AnvilChunkSource,
        pos: [i32; 3],
        block: &str,
        contents: &[(usize, ferrite_item::ItemStack)],
    ) -> u32 {
        let state =
            u32::from(ferrite_data::blocks::default_state(&format!("minecraft:{block}")).unwrap());
        source.set_block(pos[0], pos[1], pos[2], state).unwrap();
        let mut slots = vec![ferrite_item::ItemStack::empty(); 27];
        for (slot, stack) in contents {
            slots[*slot] = stack.clone();
        }
        super::super::block_container::store_slots(
            source,
            pos,
            &format!("minecraft:{block}"),
            &slots,
        );
        state
    }

    /// A spider that rolls the one-in-a-hundred jockey gets a skeleton seated on it, and stops
    /// leaping; every other spawn gets no rider.
    #[test]
    fn a_jockey_spider_gets_a_seated_skeleton() {
        let mut region = Region::default();
        let _turn = super::super::ride::test_turn();
        let mut jockeys = 0;
        for _ in 0..4_000 {
            let mut spider = mob::new_mob(
                MobKind::Spider,
                ferrite_math::Vec3::new(0.5, 65.0, 0.5),
                &mut region.rng,
            );
            let uuid = spider.data.uuid;
            if let Some(rider) = region.spider_jockey(&mut spider) {
                jockeys += 1;
                assert_eq!(rider.data.mob.as_ref().unwrap().kind, MobKind::Skeleton);
                assert!(spider.data.mob.as_ref().unwrap().has_rider);
                assert_eq!(super::super::ride::vehicle_of(rider.data.uuid), Some(uuid));
                super::super::ride::forget(rider.data.uuid);
            } else {
                assert!(!spider.data.mob.as_ref().unwrap().has_rider);
            }
        }
        assert!((15..70).contains(&jockeys), "{jockeys} jockeys in 4000");
    }

    /// A zombie that finishes chewing a closed wooden door removes both halves; an open door or
    /// an iron door stays.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_broken_door_loses_both_halves() {
        let source = loaded_source().await;
        let ctx = bare_context(&source);
        let mut region = Region::default();
        region.held_chunks.insert(ChunkPos(0, 0), 1);
        let door = |name: &str, half: &str| {
            let default = ferrite_data::blocks::default_state(name).unwrap();
            ferrite_data::blocks::state_id(name, &[("half", half)]).unwrap_or(default)
        };
        let lower = u32::from(door("minecraft:oak_door", "lower"));
        let upper = u32::from(door("minecraft:oak_door", "upper"));
        source.set_block(4, 65, 4, lower).unwrap();
        source.set_block(4, 66, 4, upper).unwrap();
        let iron = u32::from(ferrite_data::blocks::default_state("minecraft:iron_door").unwrap());
        source.set_block(6, 65, 4, iron).unwrap();
        region.break_door(&ctx, [4, 65, 4]);
        assert_eq!(source.block(4, 65, 4), Some(0));
        assert_eq!(source.block(4, 66, 4), Some(0));
        region.break_door(&ctx, [6, 65, 4]);
        assert_eq!(source.block(6, 65, 4), Some(iron));
    }

    /// An exploding shulker box (every colour, and the undyed one) drops one item that carries
    /// its contents; nothing spills loose and no other copy is made.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_explosion_keeps_a_shulker_boxs_contents_in_its_item() {
        let source = loaded_source().await;
        let ctx = bare_context(&source);
        for block in SHULKER_BLOCKS {
            let mut region = Region::default();
            region.held_chunks.insert(ChunkPos(0, 0), 1);
            let sword = {
                let mut sword = ferrite_item::ItemStack::from_name("diamond_sword").unwrap();
                sword.components.insert(
                    ferrite_item::component_ids::REPAIR_COST,
                    ferrite_item::Component::RepairCost(7),
                );
                sword
            };
            fill_box(
                &source,
                [8, 65, 9],
                block,
                &[(0, emeralds(40)), (26, sword.clone())],
            );
            region.explode([8.5, 66.5, 8.5], 4.0, true, &ctx);
            let air = u32::from(ferrite_data::blocks::default_state("minecraft:air").unwrap());
            assert_eq!(source.block(8, 65, 9), Some(air), "{block} was destroyed");
            // The blast also breaks the terrain around the box, so look at the box and at its
            // distinctive contents only.
            let ground = ground_stacks(&region);
            let boxes: Vec<_> = ground
                .iter()
                .filter(|stack| stack.name() == Some(format!("minecraft:{block}").as_str()))
                .collect();
            assert_eq!(boxes.len(), 1, "{block}: exactly one box item");
            assert_eq!(container_of(boxes[0]), [emeralds(40), sword]);
            assert!(
                ground.iter().all(|stack| !matches!(
                    stack.name(),
                    Some("minecraft:emerald" | "minecraft:diamond_sword")
                )),
                "{block}: no content spilled loose"
            );
            // The block entity went with the block: nothing is left to duplicate.
            assert!(source.block_entity(8, 65, 9).is_none(), "{block}");
        }
    }

    /// A box removed by a neighbour update (support lost, piston, gravity) goes through the same
    /// path as an explosion, and an empty box carries no container.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_shulker_box_removed_by_an_update_drops_one_item_with_its_contents() {
        let source = loaded_source().await;
        let air = u32::from(ferrite_data::blocks::default_state("minecraft:air").unwrap());
        for (block, filled) in [("lime_shulker_box", true), ("shulker_box", false)] {
            let mut region = Region::default();
            region.held_chunks.insert(ChunkPos(0, 0), 1);
            let contents: Vec<_> = filled.then(|| (13, stone_stack(5))).into_iter().collect();
            let state = fill_box(&source, [3, 65, 3], block, &contents);
            source.set_block(3, 65, 3, air).unwrap();
            region.spawn_block_drops(&source, [3, 65, 3], state as u16);
            let ground = ground_stacks(&region);
            assert_eq!(ground.len(), 1, "{block}");
            assert_eq!(
                ground[0].name(),
                Some(format!("minecraft:{block}").as_str())
            );
            if filled {
                assert_eq!(container_of(&ground[0]), [stone_stack(5)]);
            } else {
                assert!(
                    ground[0]
                        .components
                        .get(ferrite_item::component_ids::CONTAINER)
                        .is_none(),
                    "an empty box has no container component"
                );
            }
        }
    }

    /// Other containers destroyed by an explosion or an update spill their contents (they were
    /// lost before) and are not duplicated by the loot table.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_destroyed_chest_spills_its_contents_once() {
        let source = loaded_source().await;
        let air = u32::from(ferrite_data::blocks::default_state("minecraft:air").unwrap());
        let mut region = Region::default();
        region.held_chunks.insert(ChunkPos(0, 0), 1);
        let state = fill_box(
            &source,
            [3, 65, 3],
            "chest",
            &[(0, stone_stack(9)), (5, stone_stack(2))],
        );
        source.set_block(3, 65, 3, air).unwrap();
        region.spawn_block_drops(&source, [3, 65, 3], state as u16);
        let total: i32 = ground_stacks(&region)
            .iter()
            .filter(|stack| stack.name() == Some("minecraft:stone"))
            .map(|stack| stack.count)
            .sum();
        assert_eq!(total, 11, "every stone spilled, once");
        // A second removal at the same spot finds no block entity left to spill again.
        let before = ground_stacks(&region).len();
        region.spawn_block_drops(&source, [3, 65, 3], state as u16);
        let again: i32 = ground_stacks(&region)
            .iter()
            .filter(|stack| stack.name() == Some("minecraft:stone"))
            .map(|stack| stack.count)
            .sum();
        assert_eq!(again, 11, "no second spill");
        assert!(ground_stacks(&region).len() >= before);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn region_tick_assigns_a_villager_profession_and_schedule() {
        use ferrite_math::Vec3;

        let source = super::super::anvil::AnvilChunkSource::new_memory();
        let mut ready = false;
        for _ in 0..2_000 {
            if let Some(result) = source.get(0, 0, 0) {
                result.expect("chunk load");
                ready = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(ready, "chunk never loaded");
        let composter =
            u32::from(ferrite_data::blocks::default_state("minecraft:composter").unwrap());
        source.set_block(2, 66, 0, composter).unwrap();

        let mut region = Region::default();
        region.held_chunks.insert(ChunkPos(0, 0), 1);
        let key = region.entities.insert(mob::new_mob(
            mob::MobKind::Villager,
            Vec3::new(0.5, 66.0, 0.5),
            &mut ferrite_entity::Rng::new(1),
        ));
        let (signals, _rx) = tokio::sync::mpsc::unbounded_channel();
        let state = Arc::new(Mutex::new(super::super::commands::GameState::new(0, 0, 10)));
        {
            let mut state = state.lock().unwrap();
            state.clock_time = 2000;
            state.game_time = 2000;
        }
        let ctx = TickContext {
            signals,
            tracking: Default::default(),
            positions: Default::default(),
            source: Some(source),
            dimension: super::super::Dimension::Overworld,
            world_id: crate::game::WorldId::Vanilla(super::super::Dimension::Overworld),
            raining: false,
            rules: Default::default(),
            spawn: [0.0, 65.0, 0.0],
            command_entities: Default::default(),
            state,
            trader_spawner: Default::default(),
            profiler: None,
            tree: None,
            hooks: None,
        };
        region.tick(&ctx);
        let villager = region
            .entities
            .data(key)
            .unwrap()
            .villager
            .as_ref()
            .unwrap();
        assert_eq!(
            villager.profession,
            ferrite_entity::villager::Profession::Farmer
        );
        assert_eq!(villager.workstation, Some([2, 66, 0]));
        assert_eq!(villager.activity, ferrite_entity::villager::Activity::Work);
        assert_eq!(villager.destination, Some([2, 66, 0]));
    }

    #[test]
    fn a_riptide_spin_picks_each_mob_in_reach_once() {
        let mut region = Region::default();
        let near = mob::new_mob(
            mob::MobKind::Zombie,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut ferrite_entity::Rng::new(1),
        );
        let far = mob::new_mob(
            mob::MobKind::Zombie,
            ferrite_math::Vec3::new(40.5, 64.0, 0.5),
            &mut ferrite_entity::Rng::new(2),
        );
        let (near_uuid, far_uuid) = (near.data.uuid, far.data.uuid);
        region.add_entities(vec![near, far]);
        let bbox = ferrite_math::AABB::new(-0.3, 64.0, -0.3, 0.3, 65.8, 0.3).inflate(1.0, 0.0, 1.0);
        let mut hit = Vec::new();
        let first = spin_victims(&region.entities, bbox, &mut hit);
        assert_eq!(
            first.iter().map(|(_, uuid)| *uuid).collect::<Vec<_>>(),
            vec![near_uuid]
        );
        assert_eq!(hit, vec![near_uuid]);
        // The same spin does not pick the same mob again, and never the one out of reach.
        assert!(spin_victims(&region.entities, bbox, &mut hit).is_empty());
        assert!(!hit.contains(&far_uuid));
    }

    #[test]
    fn damage_hurts_a_mob_and_queues_its_hurt_animation() {
        let mut region = Region::default();
        let record = mob::new_mob(
            mob::MobKind::Zombie,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut ferrite_entity::Rng::new(1),
        );
        let uuid = record.data.uuid;
        region.add_entities(vec![record]);
        assert!(
            !region.damage_entity(Uuid::from_u128(99), 1.0, None, None, None),
            "unknown entity"
        );
        assert!(region.pending_hurts.is_empty());
        assert!(region.damage_entity(uuid, 3.0, Some([5.5, 64.0, 0.5]), None, None));
        assert_eq!(region.pending_hurts, vec![uuid]);
    }
    #[test]
    fn command_damage_credits_the_causing_player_with_the_kill() {
        let mut region = Region::default();
        let record = mob::new_mob(
            mob::MobKind::Zombie,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut ferrite_entity::Rng::new(1),
        );
        let uuid = record.data.uuid;
        region.add_entities(vec![record]);
        let player = Uuid::from_u128(7);
        assert!(region.damage_entity(uuid, 100.0, None, Some(player), Some(player)));
        let key = region
            .entities
            .keys()
            .iter()
            .copied()
            .find(|key| region.entities.data(*key).is_some_and(|d| d.uuid == uuid))
            .unwrap();
        // The credit is what makes the death drop `killed_by_player` loot and feed advancements.
        assert_eq!(
            mob::state(&region.entities, key).unwrap().last_killer,
            Some(player)
        );
        // `damage` without a causing player leaves any earlier credit alone.
        let other = Uuid::from_u128(8);
        assert!(region.damage_entity(uuid, 1.0, None, Some(other), None));
        assert_eq!(
            mob::state(&region.entities, key).unwrap().last_killer,
            Some(player)
        );
    }

    #[test]
    fn chunk_updates_only_reach_the_matching_dimension() {
        let (overworld, mut overworld_rx) = OutboundSender::channel(1024);
        let (nether, mut nether_rx) = OutboundSender::channel(1024);
        let mut players = HashMap::new();
        players.insert(
            Uuid::from_u128(1),
            (
                super::super::WorldId::Vanilla(super::super::Dimension::Overworld),
                HashSet::from([(0, 0)]),
                overworld,
            ),
        );
        players.insert(
            Uuid::from_u128(2),
            (
                super::super::WorldId::Vanilla(super::super::Dimension::Nether),
                HashSet::from([(0, 0)]),
                nether,
            ),
        );
        let update = Outbound::Packet(vec![42].into());
        broadcast_tracked_chunk(
            &players,
            super::super::Dimension::Overworld,
            (0, 0),
            &update,
        );
        assert!(overworld_rx.try_recv().is_some());
        assert!(nether_rx.try_recv().is_none());
        broadcast_tracked_chunk(
            &players,
            super::super::WorldId::Vanilla(super::super::Dimension::Nether),
            (0, 0),
            &update,
        );
        assert!(nether_rx.try_recv().is_some());
        assert!(overworld_rx.try_recv().is_none());
    }
    #[test]
    fn delayed_piston_update_replaces_the_same_position() {
        let mut region = Region::default();
        let update = |due| DelayedUpdate {
            pos: [1, 64, 2],
            due,
            chunk: (0, 0),
            expected_state: 1,
            packet: Outbound::Packet(vec![42].into()),
        };
        region.queue_delayed_update(update(5));
        region.queue_delayed_update(update(7));
        assert_eq!(region.delayed_updates.len(), 1);
        assert_eq!(region.delayed_updates[0].due, 7);
    }

    #[test]
    fn delayed_piston_updates_for_different_positions_are_kept() {
        let mut region = Region::default();
        let update = |pos, due| DelayedUpdate {
            pos,
            due,
            chunk: (0, 0),
            expected_state: 1,
            packet: Outbound::Packet(vec![42].into()),
        };
        region.queue_delayed_update(update([1, 64, 2], 5));
        region.queue_delayed_update(update([2, 64, 2], 6));
        assert_eq!(region.delayed_updates.len(), 2);
    }

    #[test]
    fn late_tracking_replays_only_moving_pistons_in_the_sent_chunks() {
        use ferrite_protocol::types::BlockPos;
        let stone = ferrite_data::blocks::default_state("stone").unwrap();
        let near = super::super::piston::MovingBlock::new(
            [1, 64, 0],
            [2, 64, 0],
            stone,
            ferrite_math::Direction::East,
            true,
            false,
        );
        let far = super::super::piston::MovingBlock::new(
            [20, 64, 0],
            [21, 64, 0],
            stone,
            ferrite_math::Direction::East,
            true,
            false,
        );
        let replay = moving_piston_replay(&[near.clone(), far], &[(0, 0)]);
        assert_eq!(replay.len(), 1);
        let (update, entity) = &replay[0];
        let destination = BlockPos::unpack(update.location);
        assert_eq!((destination.x, destination.y, destination.z), (2, 64, 0));
        assert_eq!(
            update.block_state.0,
            i32::from(near.moving_state().unwrap())
        );
        assert!(
            entity.is_some(),
            "the overlay needs its piston block entity"
        );
    }

    #[test]
    fn piston_animation_survives_a_region_split_and_merge() {
        let stone = ferrite_data::blocks::default_state("stone").unwrap();
        let mut block = super::super::piston::MovingBlock::new(
            [255, 64, 0],
            [256, 64, 0],
            stone,
            ferrite_math::Direction::East,
            true,
            false,
        );
        block.progress = 0.5;
        let mut region = Region::default();
        region
            .moving_pistons
            .start(vec![block], &HashSet::from([[255, 64, 0], [256, 64, 0]]));
        let sections = HashSet::from([SectionPos(1, 0)]);
        let split = RegionData::split(&mut region, &sections);
        assert!(region.moving_pistons.active().is_empty());
        assert_eq!(split.moving_pistons.active().len(), 1);
        assert_eq!(split.moving_pistons.active()[0].progress, 0.5);
        RegionData::merge(&mut region, split);
        assert_eq!(region.moving_pistons.active().len(), 1);
        assert_eq!(region.moving_pistons.active()[0].progress, 0.5);
    }

    #[test]
    fn experience_curve_matches_vanilla_levels() {
        assert_eq!(total_for_level(0), 0);
        assert_eq!(total_for_level(1), 7);
        assert_eq!(total_for_level(16), 352);
        assert_eq!(total_for_level(17), 394);
        assert_eq!(total_for_level(30), 1395);
        assert_eq!(total_for_level(31), 1507);
        assert_eq!(total_for_level(32), 1628);
        assert_eq!(level_and_progress(394), (17, 0.0));
        assert_eq!(level_and_progress(7), (1, 0.0));
        assert!((level_and_progress(15).1 - 8.0 / 9.0).abs() < 1e-6);
    }

    #[test]
    fn tick_records_a_region_scope_when_timings_are_attached() {
        let mut region = Region::default();
        let timings = Arc::new(Timings::new());
        region.timings = Some(timings.clone());

        let (signals, _rx) = tokio::sync::mpsc::unbounded_channel();
        region.tick(&TickContext {
            signals,
            tracking: Default::default(),
            positions: Default::default(),
            source: None,
            dimension: crate::game::Dimension::Overworld,
            world_id: crate::game::WorldId::Vanilla(crate::game::Dimension::Overworld),
            raining: false,
            rules: Default::default(),
            spawn: [0.0, 65.0, 0.0],
            command_entities: Default::default(),
            state: Default::default(),
            trader_spawner: Default::default(),
            profiler: None,
            tree: None,
            hooks: None,
        });

        let report = timings.report_all(50.0);
        assert!(
            report.rows.iter().any(|row| row.path == "region-tick"),
            "{report}"
        );
    }

    #[test]
    fn a_frozen_game_does_not_move_mobs() {
        let state: Arc<Mutex<super::super::commands::GameState>> = Default::default();
        let (signals, _rx) = tokio::sync::mpsc::unbounded_channel();
        let context = |state: &Arc<Mutex<super::super::commands::GameState>>| TickContext {
            signals: signals.clone(),
            tracking: Default::default(),
            positions: Default::default(),
            source: None,
            dimension: crate::game::Dimension::Overworld,
            world_id: crate::game::WorldId::Vanilla(crate::game::Dimension::Overworld),
            raining: false,
            rules: Default::default(),
            spawn: [0.0, 65.0, 0.0],
            command_entities: Default::default(),
            state: state.clone(),
            trader_spawner: Default::default(),
            profiler: None,
            tree: None,
            hooks: None,
        };
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let record = mob::new_mob(
            mob::MobKind::Zombie,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut ferrite_entity::Rng::new(1),
        );
        region.add_entities(vec![record]);
        let height = |region: &Region| {
            let key = region.entities.keys().first().copied()?;
            region.entities.pos(key).map(|p| p.y)
        };
        let control = state.lock().unwrap().more.tick.clone();
        control.set_frozen(true);
        for _ in 0..20 {
            control.begin_game_tick();
            region.tick(&context(&state));
        }
        assert_eq!(height(&region), Some(64.0), "frozen mobs stay put");
        control.set_frozen(false);
        for _ in 0..20 {
            control.begin_game_tick();
            region.tick(&context(&state));
        }
        assert_ne!(
            height(&region),
            Some(64.0),
            "unfrozen mobs fall, move or leave"
        );
    }

    #[test]
    fn kill_and_teleport_apply_to_region_entities_and_hand_off() {
        use ferrite_entity::{EntityData, EntityRecord, allocate_id, random_uuid};
        use ferrite_math::Vec3;
        let mut region = Region::default();
        let record = |x: f64| EntityRecord {
            pos: Vec3::new(x, 64.0, 0.5),
            vel: Vec3::ZERO,
            flags: Flags::default(),
            data: EntityData::new(allocate_id(), random_uuid(), 71, 0.25, 0.25),
        };

        let key = region.entities.insert(record(1.5));
        let uuid = region.entities.data(key).unwrap().uuid;
        assert!(region.kill_entity(uuid));
        assert!(region.entities.flags(key).unwrap().has(Flags::REMOVED));
        assert_eq!(region.entities.sweep_removed().len(), 1);

        // Teleporting across the default region section hands the record to the owner of the
        // destination chunk on the next crossing pass.
        let key = region.entities.insert(record(250.0));
        let uuid = region.entities.data(key).unwrap().uuid;
        assert!(region.teleport_entity(uuid, [300.5, 64.0, 0.5], 90.0, 10.0));
        assert_eq!(region.entities.data(key).unwrap().yaw, 90.0);
        assert!(region.entities.flags(key).unwrap().has(Flags::TELEPORTED));
        let out = region.entities.take_crossing();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].chunk(), (18, 0));
    }

    fn leash_context(entities: Arc<Mutex<super::super::entity_index::EntityIndex>>) -> TickContext {
        let (signals, _rx) = tokio::sync::mpsc::unbounded_channel();
        TickContext {
            signals,
            tracking: Default::default(),
            positions: Default::default(),
            source: None,
            dimension: super::super::Dimension::Overworld,
            world_id: crate::game::WorldId::Vanilla(super::super::Dimension::Overworld),
            raining: false,
            rules: Default::default(),
            spawn: [0.0, 65.0, 0.0],
            command_entities: entities,
            state: Default::default(),
            trader_spawner: Default::default(),
            profiler: None,
            tree: None,
            hooks: None,
        }
    }

    fn published_holder(
        index: &Arc<Mutex<super::super::entity_index::EntityIndex>>,
        uuid: Uuid,
        network_id: i32,
        pos: [f64; 3],
        dimension: super::super::Dimension,
    ) {
        index
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(super::super::entity_index::CommandEntity {
                uuid,
                network_id,
                kind: 71,
                pos,
                yaw: 0.0,
                pitch: 0.0,
                dimension,
                tags: Default::default(),
                custom_name: None,
                health: None,
                attacker: None,
                owner: None,
                leasher: None,
                target: None,
            });
    }

    /// A flat world for the leash goal tests: stone below y = 64, air above.
    struct Flat;

    impl BlockGetter for Flat {
        fn block_state(&self, _x: i32, y: i32, _z: i32) -> Option<u16> {
            let name = if y < 64 { "stone" } else { "air" };
            ferrite_data::blocks::default_state(name)
        }
    }

    fn dropped_leads(region: &Region) -> usize {
        region
            .entities
            .keys()
            .iter()
            .filter(|&&key| {
                region
                    .entities
                    .data(key)
                    .is_some_and(|data| data.item.is_some())
            })
            .count()
    }

    #[test]
    fn a_lead_resolves_a_holder_published_by_another_region() {
        let _turn = leash_test_turn();
        let holder = Uuid::from_u128(0x1ea5_1001);
        let index = Arc::new(Mutex::new(
            super::super::entity_index::EntityIndex::default(),
        ));
        published_holder(
            &index,
            holder,
            77,
            [0.5, 64.0, 0.5],
            super::super::Dimension::Overworld,
        );
        let ctx = leash_context(index);
        let mut region = Region::default();
        let cow = region.entities.insert(mob::new_mob(
            MobKind::Cow,
            ferrite_math::Vec3::new(9.5, 64.0, 0.5),
            &mut region.rng,
        ));
        mob::state_mut(&mut region.entities, cow).unwrap().leash = Some((holder, 0));
        let holders = region.leash_holders(&ctx);
        assert_eq!(
            holders.get(&holder).copied(),
            Some((ferrite_math::Vec3::new(0.5, 64.0, 0.5), 77))
        );
        region.refresh_leash_ids(&ctx, &holders);
        assert_eq!(
            mob::state(&region.entities, cow).unwrap().leash,
            Some((holder, 77)),
            "the link target comes from the other region's snapshot"
        );
    }

    #[test]
    fn a_lead_drags_its_animal_towards_a_holder_in_another_region() {
        let _turn = leash_test_turn();
        let holder = Uuid::from_u128(0x1ea5_1002);
        let index = Arc::new(Mutex::new(
            super::super::entity_index::EntityIndex::default(),
        ));
        published_holder(
            &index,
            holder,
            88,
            [0.5, 64.0, 0.5],
            super::super::Dimension::Overworld,
        );
        let ctx = leash_context(index);
        let mut region = Region::default();
        let cow = region.entities.insert(mob::new_mob(
            MobKind::Cow,
            ferrite_math::Vec3::new(9.5, 64.0, 0.5),
            &mut region.rng,
        ));
        mob::state_mut(&mut region.entities, cow).unwrap().leash = Some((holder, 88));
        let before = region.entities.pos(cow).unwrap().x;
        for _ in 0..40 {
            let positions: Vec<(Uuid, ferrite_math::Vec3)> = region
                .leash_holders(&ctx)
                .iter()
                .map(|(holder, (pos, _))| (*holder, *pos))
                .collect();
            ai::tick_with_holders(
                &mut region.entities,
                &Flat,
                &[],
                &positions,
                &mut region.rng,
            );
        }
        assert!(
            region.entities.pos(cow).unwrap().x < before,
            "the cross-region holder pulls the cow"
        );
        assert!(
            mob::state(&region.entities, cow).unwrap().leash.is_some(),
            "a reachable holder keeps the lead"
        );
    }

    #[test]
    fn a_lead_follows_a_player_owned_by_another_region() {
        let _turn = leash_test_turn();
        let holder = Uuid::from_u128(0x1ea5_1003);
        let ctx = leash_context(Default::default());
        {
            let mut state = lock_game_state(&ctx.state, ctx.profiler.as_deref());
            state.join(holder, "Holder".into(), 0, 20);
            state.set_position(holder, [0.5, 64.0, 0.5], 0.0, 0.0);
        }
        let mut region = Region::default();
        let cow = region.entities.insert(mob::new_mob(
            MobKind::Cow,
            ferrite_math::Vec3::new(9.5, 64.0, 0.5),
            &mut region.rng,
        ));
        mob::state_mut(&mut region.entities, cow).unwrap().leash = Some((holder, 123));
        let holders = region.leash_holders(&ctx);
        assert_eq!(
            holders.get(&holder).copied(),
            Some((ferrite_math::Vec3::new(0.5, 64.0, 0.5), 123))
        );
        let before = region.entities.pos(cow).unwrap().x;
        for _ in 0..40 {
            let positions: Vec<(Uuid, ferrite_math::Vec3)> = region
                .leash_holders(&ctx)
                .iter()
                .map(|(holder, (pos, _))| (*holder, *pos))
                .collect();
            ai::tick_with_holders(
                &mut region.entities,
                &Flat,
                &[],
                &positions,
                &mut region.rng,
            );
        }
        assert!(
            region.entities.pos(cow).unwrap().x < before,
            "the global player position pulls the cow"
        );
    }

    #[test]
    fn a_lead_refreshes_its_holders_position_during_a_region_hand_off() {
        let _turn = leash_test_turn();
        let holder = Uuid::from_u128(0x1ea5_1005);
        let index = Arc::new(Mutex::new(
            super::super::entity_index::EntityIndex::default(),
        ));
        published_holder(
            &index,
            holder,
            111,
            [20.5, 64.0, 0.5],
            super::super::Dimension::Overworld,
        );
        let ctx = leash_context(index.clone());
        let mut region = Region::default();
        let cow = region.entities.insert(mob::new_mob(
            MobKind::Cow,
            ferrite_math::Vec3::new(9.5, 64.0, 0.5),
            &mut region.rng,
        ));
        mob::state_mut(&mut region.entities, cow).unwrap().leash = Some((holder, 111));
        assert_eq!(
            region.leash_holders(&ctx).get(&holder).copied(),
            Some((ferrite_math::Vec3::new(20.5, 64.0, 0.5), 111))
        );

        // The old owner stops publishing, then the new owner reports a different position
        // while the hand-off is still inside the grace window.
        let mut owned = HashSet::from([holder]);
        index
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .sync(
                &mut owned,
                &EntityStore::default(),
                super::super::Dimension::Overworld,
            );
        ctx.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .game_time += 3;
        assert!(
            region.leash_holders(&ctx).contains_key(&holder),
            "the last sighting covers the gap"
        );
        published_holder(
            &index,
            holder,
            111,
            [7.5, 64.0, 0.5],
            super::super::Dimension::Overworld,
        );
        assert_eq!(
            region.leash_holders(&ctx).get(&holder).copied(),
            Some((ferrite_math::Vec3::new(7.5, 64.0, 0.5), 111)),
            "the new owner's position replaces the last sighting"
        );
    }

    #[test]
    fn a_lead_rejects_a_holder_that_crossed_to_another_dimension() {
        let _turn = leash_test_turn();
        let holder = Uuid::from_u128(0x1ea5_1006);
        let index = Arc::new(Mutex::new(
            super::super::entity_index::EntityIndex::default(),
        ));
        published_holder(
            &index,
            holder,
            222,
            [0.5, 64.0, 0.5],
            super::super::Dimension::Overworld,
        );
        let ctx = leash_context(index.clone());
        let mut region = Region::default();
        let cow = region.entities.insert(mob::new_mob(
            MobKind::Cow,
            ferrite_math::Vec3::new(9.5, 64.0, 0.5),
            &mut region.rng,
        ));
        mob::state_mut(&mut region.entities, cow).unwrap().leash = Some((holder, 222));
        assert!(region.leash_holders(&ctx).contains_key(&holder));

        // The holder moves to the Nether while the old sighting is still inside the grace
        // window; the stale Overworld entry must not keep the lead alive.
        published_holder(
            &index,
            holder,
            222,
            [0.5, 64.0, 0.5],
            super::super::Dimension::Nether,
        );
        ctx.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .game_time += 1;
        assert!(
            !region.leash_holders(&ctx).contains_key(&holder),
            "a Nether holder is not a local target"
        );

        let blocks = TickBlocks {
            source: None,
            seen: Default::default(),
        };
        region.tick_mobs_with_owner_attacks(&ctx, &blocks, &[]);
        assert!(mob::state(&region.entities, cow).unwrap().leash.is_none());
        assert_eq!(dropped_leads(&region), 1, "the lead drops once");
        region.tick_mobs_with_owner_attacks(&ctx, &blocks, &[]);
        assert_eq!(dropped_leads(&region), 1, "the break is not repeated");
    }

    #[test]
    fn a_lead_rejects_a_player_holder_that_moved_to_another_dimension() {
        let _turn = leash_test_turn();
        let holder = Uuid::from_u128(0x1ea5_1008);
        let ctx = leash_context(Default::default());
        {
            let mut state = lock_game_state(&ctx.state, ctx.profiler.as_deref());
            state.join(holder, "Holder".into(), 0, 20);
            state.set_position(holder, [0.5, 64.0, 0.5], 0.0, 0.0);
            state.set_dimension(holder, super::super::Dimension::Nether);
        }
        let mut region = Region::default();
        let cow = region.entities.insert(mob::new_mob(
            MobKind::Cow,
            ferrite_math::Vec3::new(9.5, 64.0, 0.5),
            &mut region.rng,
        ));
        mob::state_mut(&mut region.entities, cow).unwrap().leash = Some((holder, 123));
        assert!(
            !region.leash_holders(&ctx).contains_key(&holder),
            "a player in the Nether is not a local leash target"
        );

        let blocks = TickBlocks {
            source: None,
            seen: Default::default(),
        };
        region.tick_mobs(&ctx, &blocks);
        assert!(mob::state(&region.entities, cow).unwrap().leash.is_none());
        assert_eq!(dropped_leads(&region), 1, "the lead drops once");
        region.tick_mobs(&ctx, &blocks);
        assert_eq!(dropped_leads(&region), 1, "the break is not repeated");
    }

    #[test]
    fn a_lead_stops_following_a_logged_out_player_after_the_grace_window() {
        let _turn = leash_test_turn();
        let holder = Uuid::from_u128(0x1ea5_1007);
        let ctx = leash_context(Default::default());
        {
            let mut state = lock_game_state(&ctx.state, ctx.profiler.as_deref());
            state.join(holder, "Holder".into(), 0, 20);
            state.set_position(holder, [0.5, 64.0, 0.5], 0.0, 0.0);
        }
        let mut region = Region::default();
        let cow = region.entities.insert(mob::new_mob(
            MobKind::Cow,
            ferrite_math::Vec3::new(9.5, 64.0, 0.5),
            &mut region.rng,
        ));
        mob::state_mut(&mut region.entities, cow).unwrap().leash = Some((holder, 123));
        assert!(region.leash_holders(&ctx).contains_key(&holder));

        ctx.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .leave(holder);
        ctx.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .game_time += LEASH_HOLDER_GRACE;
        assert!(
            region.leash_holders(&ctx).contains_key(&holder),
            "the last sighting covers the hand-off"
        );
        ctx.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .game_time += 1;
        assert!(
            !region.leash_holders(&ctx).contains_key(&holder),
            "a logged-out player is not followed indefinitely"
        );

        let blocks = TickBlocks {
            source: None,
            seen: Default::default(),
        };
        region.tick_mobs_with_owner_attacks(&ctx, &blocks, &[]);
        assert!(mob::state(&region.entities, cow).unwrap().leash.is_none());
        assert_eq!(dropped_leads(&region), 1, "the lead drops once");
        region.tick_mobs_with_owner_attacks(&ctx, &blocks, &[]);
        assert_eq!(dropped_leads(&region), 1, "the break is not repeated");
    }

    #[test]
    fn a_lead_rejects_a_player_that_moved_to_another_dimension() {
        let _turn = leash_test_turn();
        let holder = Uuid::from_u128(0x1ea5_1008);
        let index = Arc::new(Mutex::new(
            super::super::entity_index::EntityIndex::default(),
        ));
        published_holder(
            &index,
            holder,
            333,
            [0.5, 64.0, 0.5],
            super::super::Dimension::Overworld,
        );
        let ctx = leash_context(index);
        {
            let mut state = lock_game_state(&ctx.state, ctx.profiler.as_deref());
            state.join(holder, "Holder".into(), 0, 20);
            state.set_position(holder, [0.5, 64.0, 0.5], 0.0, 0.0);
            state.set_dimension(holder, super::super::Dimension::Overworld);
        }
        let mut region = Region::default();
        let cow = region.entities.insert(mob::new_mob(
            MobKind::Cow,
            ferrite_math::Vec3::new(9.5, 64.0, 0.5),
            &mut region.rng,
        ));
        mob::state_mut(&mut region.entities, cow).unwrap().leash = Some((holder, 333));
        assert!(region.leash_holders(&ctx).contains_key(&holder));

        ctx.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .set_dimension(holder, super::super::Dimension::Nether);
        ctx.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .game_time += 1;
        assert!(
            !region.leash_holders(&ctx).contains_key(&holder),
            "the global player dimension wins over a stale entity snapshot"
        );

        let blocks = TickBlocks {
            source: None,
            seen: Default::default(),
        };
        region.tick_mobs_with_owner_attacks(&ctx, &blocks, &[]);
        assert!(mob::state(&region.entities, cow).unwrap().leash.is_none());
        assert_eq!(dropped_leads(&region), 1, "the lead drops once");
        region.tick_mobs_with_owner_attacks(&ctx, &blocks, &[]);
        assert_eq!(dropped_leads(&region), 1, "the break is not repeated");
    }

    #[test]
    fn a_lead_breaks_once_when_its_player_reconnects() {
        let _turn = leash_test_turn();
        let holder = Uuid::from_u128(0x1ea5_1009);
        let ctx = leash_context(Default::default());
        {
            let mut state = lock_game_state(&ctx.state, ctx.profiler.as_deref());
            state.join(holder, "Holder".into(), 0, 20);
            state.set_position(holder, [0.5, 64.0, 0.5], 0.0, 0.0);
        }
        let mut region = Region::default();
        let cow = region.entities.insert(mob::new_mob(
            MobKind::Cow,
            ferrite_math::Vec3::new(9.5, 64.0, 0.5),
            &mut region.rng,
        ));
        mob::state_mut(&mut region.entities, cow).unwrap().leash = Some((holder, 123));
        assert!(region.leash_holders(&ctx).contains_key(&holder));

        // A same-session region hand-off refreshes the position instead of breaking.
        ctx.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .set_position(holder, [7.5, 64.0, 0.5], 0.0, 0.0);
        assert_eq!(
            region.leash_holders(&ctx).get(&holder).copied(),
            Some((ferrite_math::Vec3::new(7.5, 64.0, 0.5), 123))
        );

        // Logging out and back in within the grace window is a new session: the cached
        // holder must not be revived even though the UUID and saved position match.
        {
            let mut state = lock_game_state(&ctx.state, ctx.profiler.as_deref());
            state.leave(holder);
            state.join(holder, "Holder".into(), 0, 20);
            state.set_position(holder, [7.5, 64.0, 0.5], 0.0, 0.0);
        }
        assert!(
            !region.leash_holders(&ctx).contains_key(&holder),
            "a new login session is not the old leash holder"
        );

        let blocks = TickBlocks {
            source: None,
            seen: Default::default(),
        };
        region.tick_mobs_with_owner_attacks(&ctx, &blocks, &[]);
        assert!(mob::state(&region.entities, cow).unwrap().leash.is_none());
        assert_eq!(dropped_leads(&region), 1, "the lead drops once");
        region.tick_mobs_with_owner_attacks(&ctx, &blocks, &[]);
        assert_eq!(dropped_leads(&region), 1, "the break is not repeated");

        // Once the stale lead is gone, the reconnected session can attach a new lead.
        mob::state_mut(&mut region.entities, cow).unwrap().leash = Some((holder, 456));
        assert_eq!(
            region.leash_holders(&ctx).get(&holder).copied(),
            Some((ferrite_math::Vec3::new(7.5, 64.0, 0.5), 456)),
            "the new session can be leashed again"
        );
        region.tick_mobs_with_owner_attacks(&ctx, &blocks, &[]);
        assert!(
            mob::state(&region.entities, cow).unwrap().leash.is_some(),
            "the new lead survives"
        );
        assert_eq!(
            dropped_leads(&region),
            1,
            "the old break is not repeated for the new lead"
        );
    }

    #[test]
    fn a_lead_outlives_a_holder_hand_off_and_breaks_once_on_removal() {
        let _turn = leash_test_turn();
        let holder = Uuid::from_u128(0x1ea5_1004);
        let index = Arc::new(Mutex::new(
            super::super::entity_index::EntityIndex::default(),
        ));
        published_holder(
            &index,
            holder,
            99,
            [0.5, 64.0, 0.5],
            super::super::Dimension::Overworld,
        );
        let ctx = leash_context(index.clone());
        let mut region = Region::default();
        let cow = region.entities.insert(mob::new_mob(
            MobKind::Cow,
            ferrite_math::Vec3::new(9.5, 64.0, 0.5),
            &mut region.rng,
        ));
        mob::state_mut(&mut region.entities, cow).unwrap().leash = Some((holder, 99));
        assert!(region.leash_holders(&ctx).contains_key(&holder));

        // The holder moved to another region: its old region stopped publishing it.
        let mut owned = HashSet::from([holder]);
        index
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .sync(
                &mut owned,
                &EntityStore::default(),
                super::super::Dimension::Overworld,
            );
        for _ in 0..LEASH_HOLDER_GRACE {
            ctx.state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .game_time += 1;
            assert!(
                region.leash_holders(&ctx).contains_key(&holder),
                "the last sighting covers the hand-off"
            );
            assert!(mob::state(&region.entities, cow).unwrap().leash.is_some());
        }

        // It never came back: the lead breaks once and drops exactly one lead item.
        ctx.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .game_time += LEASH_HOLDER_GRACE + 1;
        assert!(!region.leash_holders(&ctx).contains_key(&holder));
        let blocks = TickBlocks {
            source: None,
            seen: Default::default(),
        };
        region.tick_mobs_with_owner_attacks(&ctx, &blocks, &[]);
        assert!(mob::state(&region.entities, cow).unwrap().leash.is_none());
        assert_eq!(
            dropped_leads(&region),
            1,
            "the removed holder drops one lead"
        );
        region.tick_mobs_with_owner_attacks(&ctx, &blocks, &[]);
        assert_eq!(dropped_leads(&region), 1, "the break is not repeated");
    }

    #[test]
    fn window_is_sorted_and_unloads_outside() {
        let old = std::collections::HashSet::from([(0, 0), (1, 0), (-1, 0)]);
        let (forget, new) = chunk_window((3, 0), 2, &old);
        assert!(forget.contains(&(-1, 0)));
        assert_eq!(new.first(), Some(&(3, 0)));
        assert!(!new.contains(&(1, 0)));
    }

    #[test]
    fn daylight_rules_follow_overworld_time_boundaries_in_region_tick_mobs() {
        use super::super::Dimension::{Nether, Overworld};
        assert!(is_bright_outside(Overworld, 0));
        assert!(is_bright_outside(Overworld, 12_541));
        assert!(!is_bright_outside(Overworld, 12_542));
        assert!(!is_bright_outside(Overworld, 18_000));
        assert!(!is_bright_outside(Overworld, 23_459));
        assert!(is_bright_outside(Overworld, 23_460));
        assert!(!is_bright_outside(Nether, 6_000));

        // Exposed skeleton at midnight never catches fire through Region::tick_mobs.
        let ctx = tick_context();
        ctx.state.lock().unwrap().clock_time = 18_000;
        let mut region = Region::default();
        let skel = region.entities.insert(mob::new_mob(
            MobKind::Skeleton,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut region.rng,
        ));
        let blocks = TickBlocks {
            source: None,
            seen: Default::default(),
        };
        for _ in 0..200 {
            region.tick_mobs(&ctx, &blocks);
        }
        let state = mob::state(&region.entities, skel).unwrap();
        assert_eq!(state.fire, 0);
        assert_eq!(state.shade_pos, None);
    }

    async fn load_chunk_in(
        source: &std::sync::Arc<super::super::anvil::AnvilChunkSource>,
        cx: i32,
        cz: i32,
    ) {
        for _ in 0..2_000 {
            if let Some(result) = source.get(cx, cz, 0) {
                result.expect("chunk load");
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("chunk ({cx}, {cz}) never loaded");
    }

    fn build_door_corridor(
        source: &super::super::anvil::AnvilChunkSource,
        door_x: i32,
        door_z: i32,
        x_min: i32,
        x_max: i32,
    ) {
        let stone = u32::from(ferrite_data::blocks::default_state("minecraft:stone").unwrap());
        for x in x_min..=x_max {
            for z in (door_z - 3)..=(door_z + 3) {
                source.set_block(x, 64, z, stone).unwrap();
                for y in 65..=67 {
                    let state = if x == door_x && z != door_z { stone } else { 0 };
                    source.set_block(x, y, z, state).unwrap();
                }
            }
        }
        let lower = u32::from(
            ferrite_data::blocks::state_id(
                "minecraft:oak_door",
                &[
                    ("facing", "east"),
                    ("hinge", "left"),
                    ("half", "lower"),
                    ("open", "false"),
                ],
            )
            .unwrap(),
        );
        let upper = u32::from(
            ferrite_data::blocks::state_id(
                "minecraft:oak_door",
                &[
                    ("facing", "east"),
                    ("hinge", "left"),
                    ("half", "upper"),
                    ("open", "false"),
                ],
            )
            .unwrap(),
        );
        source.set_block(door_x, 65, door_z, lower).unwrap();
        source.set_block(door_x, 66, door_z, upper).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_villager_opens_and_closes_wooden_door_in_its_owning_region() {
        let source = loaded_source().await;
        let ctx = bare_context(&source);
        ctx.state.lock().unwrap().clock_time = 2_000;
        build_door_corridor(&source, 6, 4, 2, 12);
        let composter =
            u32::from(ferrite_data::blocks::default_state("minecraft:composter").unwrap());
        source.set_block(11, 65, 4, composter).unwrap();

        let mut region = Region::default();
        region.held_chunks.insert(ChunkPos(0, 0), 1);
        let mut villager = mob::new_mob(
            MobKind::Villager,
            ferrite_math::Vec3::new(3.5, 65.0, 4.5),
            &mut region.rng,
        );
        if let Some(v) = villager.data.villager.as_mut() {
            v.profession = ferrite_entity::villager::Profession::Farmer;
            v.workstation = Some([11, 65, 4]);
            v.home = [3.5, 65.0, 4.5];
            v.activity = ferrite_entity::villager::Activity::Work;
            v.destination = Some([11, 65, 4]);
        }
        let key = region.entities.insert(villager);

        let is_open_at = |y: i32| {
            source
                .block(6, y, 4)
                .and_then(|s| u16::try_from(s).ok())
                .is_some_and(ferrite_entity::path::is_open_door)
        };
        let mut saw_both_open = false;
        let mut saw_reclosed = false;
        for _ in 0..80 {
            let blocks = TickBlocks {
                source: Some(&source),
                seen: Default::default(),
            };
            region.tick_mobs(&ctx, &blocks);
            if is_open_at(65) && is_open_at(66) {
                saw_both_open = true;
            } else if saw_both_open && region.entities.pos(key).unwrap().x > 8.6 {
                assert!(!is_open_at(65) && !is_open_at(66));
                saw_reclosed = true;
                break;
            }
        }
        assert!(
            saw_both_open,
            "villager must open both door halves in region"
        );
        assert!(
            region.entities.pos(key).unwrap().x > 8.6,
            "villager must pass through the opened doorway: {:?}",
            region.entities.pos(key)
        );
        assert!(
            saw_reclosed,
            "villager must close both door halves after passing"
        );
        assert_eq!(mob::state(&region.entities, key).unwrap().opened_door, None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cross_region_door_guard_refuses_foreign_chunk_and_transfers_cleanly() {
        let source = loaded_source().await;
        load_chunk_in(&source, 15, 0).await;
        load_chunk_in(&source, 16, 0).await;
        let ctx = bare_context(&source);
        ctx.state.lock().unwrap().clock_time = 2_000;
        // Doorway sits at x=258 in ChunkPos(16, 0) (SectionPos(1, 0)), with open hallway across
        // the section boundary at x=256 so the villager crosses regions before reaching the door.
        build_door_corridor(&source, 258, 4, 252, 264);
        let composter =
            u32::from(ferrite_data::blocks::default_state("minecraft:composter").unwrap());
        source.set_block(263, 65, 4, composter).unwrap();

        let mut region_a = Region::default();
        region_a.held_chunks.insert(ChunkPos(15, 0), 1);
        let mut region_b = Region::default();
        region_b.held_chunks.insert(ChunkPos(16, 0), 1);

        // Region A does not hold ChunkPos(16, 0), so direct door open/break never mutates it.
        region_a.set_door_open(&ctx, [258, 65, 4], true);
        region_a.break_door(&ctx, [258, 65, 4]);
        let is_open_at = |y: i32| {
            source
                .block(258, y, 4)
                .and_then(|s| u16::try_from(s).ok())
                .is_some_and(ferrite_entity::path::is_open_door)
        };
        assert!(!is_open_at(65) && !is_open_at(66));

        // Villager starts in Region A (ChunkPos(15, 0) / SectionPos(0, 0)) walking east into
        // Region B's doorway at x=258 (ChunkPos(16, 0) / SectionPos(1, 0)).
        let mut villager = mob::new_mob(
            MobKind::Villager,
            ferrite_math::Vec3::new(254.5, 65.0, 4.5),
            &mut region_a.rng,
        );
        if let Some(v) = villager.data.villager.as_mut() {
            v.profession = ferrite_entity::villager::Profession::Farmer;
            v.workstation = Some([263, 65, 4]);
            v.home = [254.5, 65.0, 4.5];
            v.activity = ferrite_entity::villager::Activity::Work;
            v.destination = Some([263, 65, 4]);
        }
        region_a.entities.insert(villager);

        let mut transferred = None;
        for _ in 0..40 {
            let blocks = TickBlocks {
                source: Some(&source),
                seen: Default::default(),
            };
            region_a.tick_mobs(&ctx, &blocks);
            // Region A must never open Region B's door at x=258 even if called directly.
            assert!(
                !is_open_at(65),
                "Region A must not mutate ChunkPos(16, 0) door"
            );
            let leaving = region_a.entities.take_crossing();
            if !leaving.is_empty() {
                transferred = Some(leaving);
                break;
            }
        }
        let crossed = transferred.expect("villager must cross section boundary at x=256");
        assert!(region_a.entities.keys().is_empty());
        region_b.add_entities(crossed);

        let b_key = region_b.entities.keys()[0];
        let mut opened_in_b = false;
        let mut closed_in_b = false;
        for _ in 0..80 {
            let blocks = TickBlocks {
                source: Some(&source),
                seen: Default::default(),
            };
            region_b.tick_mobs(&ctx, &blocks);
            if is_open_at(65) && is_open_at(66) {
                opened_in_b = true;
            } else if opened_in_b && region_b.entities.pos(b_key).unwrap().x > 260.6 {
                assert!(!is_open_at(65) && !is_open_at(66));
                closed_in_b = true;
                break;
            }
        }
        assert!(
            opened_in_b,
            "owning Region B opens its own door after handoff"
        );
        assert!(
            closed_in_b,
            "owning Region B closes its own door after villager passes"
        );
    }

    #[test]
    fn mob_navigation_state_survives_region_split_and_merge() {
        let mut west = Region::default();
        west.hold_chunk(ChunkPos(0, 0));
        west.hold_chunk(ChunkPos(16, 0));

        let mut zom = mob::new_mob(
            MobKind::Zombie,
            ferrite_math::Vec3::new(4.5, 65.0, 4.5),
            &mut west.rng,
        );
        let z_uuid = zom.data.uuid;
        if let Some(st) = zom.data.mob.as_mut() {
            st.can_break_doors = true;
            st.door_pos = Some([6, 65, 4]);
            st.door_ticks = 42;
            st.path[0] = [5, 65, 4];
            st.path[1] = [6, 65, 4];
            st.path_len = 2;
        }

        let mut vil = mob::new_mob(
            MobKind::Villager,
            ferrite_math::Vec3::new(260.5, 65.0, 4.5),
            &mut west.rng,
        );
        let v_uuid = vil.data.uuid;
        if let Some(st) = vil.data.mob.as_mut() {
            st.opened_door = Some([258, 65, 4]);
            st.path[0] = [259, 65, 4];
            st.path[1] = [261, 65, 4];
            st.path_len = 2;
            st.path_index = 1;
            st.path_timer = 13;
        }

        let mut skel = mob::new_mob(
            MobKind::Skeleton,
            ferrite_math::Vec3::new(262.5, 65.0, 4.5),
            &mut west.rng,
        );
        let s_uuid = skel.data.uuid;
        if let Some(st) = skel.data.mob.as_mut() {
            st.shade_pos = Some([265, 65, 4]);
        }

        west.add_entities(vec![zom, vil, skel]);
        let east = west.split(&HashSet::from([SectionPos(1, 0)]));

        assert_eq!(west.entities.keys().len(), 1);
        assert_eq!(east.entities.keys().len(), 2);

        let ek_vil = east
            .entities
            .keys()
            .iter()
            .copied()
            .find(|&k| east.entities.data(k).unwrap().uuid == v_uuid)
            .unwrap();
        let vs = mob::state(&east.entities, ek_vil).unwrap();
        assert_eq!(vs.opened_door, Some([258, 65, 4]));
        assert_eq!(vs.path_len, 2);
        assert_eq!(vs.path_index, 1);
        assert_eq!(vs.path_timer, 13);

        let ek_skel = east
            .entities
            .keys()
            .iter()
            .copied()
            .find(|&k| east.entities.data(k).unwrap().uuid == s_uuid)
            .unwrap();
        assert_eq!(
            mob::state(&east.entities, ek_skel).unwrap().shade_pos,
            Some([265, 65, 4])
        );

        west.merge(east);
        assert_eq!(west.entities.keys().len(), 3);
        let wk_zom = west
            .entities
            .keys()
            .iter()
            .copied()
            .find(|&k| west.entities.data(k).unwrap().uuid == z_uuid)
            .unwrap();
        assert_eq!(
            mob::state(&west.entities, wk_zom).unwrap().door_pos,
            Some([6, 65, 4])
        );
        assert_eq!(mob::state(&west.entities, wk_zom).unwrap().door_ticks, 42);
        let wk_vil = west
            .entities
            .keys()
            .iter()
            .copied()
            .find(|&k| west.entities.data(k).unwrap().uuid == v_uuid)
            .unwrap();
        assert_eq!(
            mob::state(&west.entities, wk_vil).unwrap().opened_door,
            Some([258, 65, 4])
        );
    }

    fn test_ranged_player(ctx: &TickContext, uuid: Uuid, pos: [f64; 3]) -> Player {
        let mailbox = Arc::new(Mutex::new(VecDeque::new()));
        let (outbound, _rx) = OutboundSender::channel(64);
        let chunk = ChunkPos(
            (pos[0].floor() as i32).div_euclid(16),
            (pos[2].floor() as i32).div_euclid(16),
        );
        Player {
            uuid,
            name: "RangedTest".into(),
            permission: 0,
            plugins: None,
            plugin_event_errors: PluginEventPolicy::Allow,
            session: 1,
            entity: ferrite_entity::allocate_id(),
            broken: Vec::new(),
            ignited: Vec::new(),
            placed_pumpkins: Vec::new(),
            dimension: ctx.dimension,
            world_id: ctx.world_id,
            sleeping: None,
            spawn: None,
            explosions: Vec::new(),
            button_press: Vec::new(),
            editing_sign: None,
            changed_blocks: Vec::new(),
            primed_tnt: Vec::new(),
            firework_launches: Vec::new(),
            eye_throws: Vec::new(),
            bobber_cast: None,
            enchant_seed: 1,
            crystals: Vec::new(),
            tracking: Arc::clone(&ctx.tracking),
            positions: Arc::clone(&ctx.positions),
            source: ctx.source.clone(),
            playerdata: None,
            effects: Default::default(),
            pos,
            yaw: 0.0,
            pitch: 0.0,
            ground: true,
            gliding: false,
            chunk,
            mailbox,
            outbound,
            signals: ctx.signals.clone(),
            biome: 0,
            gamemode: 0,
            experience_level: 0,
            experience_progress: 0.0,
            experience_total: 0,
            inventory: Default::default(),
            menus: Default::default(),
            breaking: None,
            view: 8,
            pending_teleport: None,
            next_teleport: 1,
            loaded: FxSet::default(),
            queue: VecDeque::new(),
            prefetched: 0,
            prefetch_wait: 0,
            awaiting_batch: false,
            rate: 12.0,
            survival: Default::default(),
            dimension_type: ctx.dimension.dimension_type_id(),
            dimensions: super::super::DimensionRegistry::with_source(
                ctx.dimension,
                ctx.source
                    .clone()
                    .unwrap_or_else(super::super::anvil::AnvilChunkSource::new_memory),
                [0, 65, 0],
            ),
            portal_ticks: 0,
            portal_cooldown: 0,
            portal_pending: None,
            gateway_used: None,
            maps_sent: HashMap::new(),
            seen_credits: true,
            watching_credits: false,
            batch_chunk_holders: Arc::new(AtomicBool::new(true)),
            seed: 0,
            flat: false,
            respawn_requested: false,
            attacks: Vec::new(),
            uses: Vec::new(),
            pick_entities: Vec::new(),
            flying: false,
            attack_ticks: 0,
            blocking: false,
            shield_disabled: 0,
            bow_charge: 0,
            using_bow: false,
            trident_draw: false,
            vehicle_moves: Vec::new(),
            open_animal_chest: false,
            sneaking: false,
            spin_ticks: 0,
            spin_damage: 0.0,
            spin_hit: Vec::new(),
            bow_released: false,
            piglin_alerts: Vec::new(),
            hive_harvests: Vec::new(),
            broken_hives: Vec::new(),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn blaze_small_fireball_hurts_player_and_obeys_mob_griefing() {
        let source = loaded_source().await;
        let stone = u32::from(ferrite_data::blocks::default_state("stone").unwrap());
        let air = u32::from(ferrite_data::blocks::default_state("air").unwrap());
        let fire = u32::from(ferrite_data::blocks::default_state("fire").unwrap());
        for y in 64..=68 {
            for z in 0..=2 {
                source.set_block(10, y, z, stone).unwrap();
            }
        }
        source.set_block(9, 64, 0, stone).unwrap();
        source.set_block(9, 65, 0, air).unwrap();
        source.set_block(9, 65, 1, air).unwrap();

        let owner = Uuid::from_u128(0xFA01);
        for (griefing, expect_fire, pos_z) in [(true, true, 0.5), (false, false, 1.5)] {
            let ctx = eat_context(&source, griefing);
            let mut region = Region::default();
            region.held_chunks.insert(ChunkPos(0, 0), 1);
            region.entities.insert(arrow::new_thrown(
                arrow::Kind::SmallFireball,
                ferrite_math::Vec3::new(7.5, 65.5, pos_z),
                ferrite_math::Vec3::new(0.8, 0.0, 0.0),
                Some(owner),
            ));
            let blocks = TickBlocks {
                source: Some(&source),
                seen: Default::default(),
            };
            for _ in 0..8 {
                region.tick_arrows(&ctx);
                let _ = blocks;
            }
            let z_cell = pos_z.floor() as i32;
            assert_eq!(
                source.block(9, 65, z_cell) == Some(fire),
                expect_fire,
                "griefing={griefing} fire at [9, 65, {z_cell}]"
            );
        }

        // Player hit: 5 damage + 100 fire ticks, without lighting the floor under them.
        let ctx = eat_context(&source, true);
        let mut region = Region::default();
        region.held_chunks.insert(ChunkPos(0, 0), 1);
        let victim = Uuid::from_u128(0xFA02);
        let mut player = test_ranged_player(&ctx, victim, [5.5, 65.0, 0.5]);
        player.yaw = -90.0; // facing away from west fireball
        region.players.insert(victim, player);
        region.entities.insert(arrow::new_thrown(
            arrow::Kind::SmallFireball,
            ferrite_math::Vec3::new(2.5, 66.0, 0.5),
            ferrite_math::Vec3::new(0.8, 0.0, 0.0),
            Some(owner),
        ));
        for _ in 0..8 {
            region.tick_arrows(&ctx);
        }
        let p = region.players.get(&victim).unwrap();
        assert_eq!(p.survival.health, 15.0);
        assert_eq!(p.survival.fire_ticks, 100);
        assert_ne!(source.block(5, 65, 0), Some(fire));

        // Blocking shield stops fireball damage & ignition.
        region.players.remove(&victim);
        let blocked = Uuid::from_u128(0xFA03);
        let mut bp = test_ranged_player(&ctx, blocked, [5.5, 65.0, 0.5]);
        bp.yaw = 90.0; // facing west toward fireball at x=2.5
        bp.blocking = true;
        region.players.insert(blocked, bp);
        region.entities.insert(arrow::new_thrown(
            arrow::Kind::SmallFireball,
            ferrite_math::Vec3::new(2.5, 66.0, 0.5),
            ferrite_math::Vec3::new(0.8, 0.0, 0.0),
            Some(owner),
        ));
        for _ in 0..8 {
            region.tick_arrows(&ctx);
        }
        let bp = region.players.get(&blocked).unwrap();
        assert_eq!(bp.survival.health, 20.0);
        assert_eq!(bp.survival.fire_ticks, 0);
        assert_eq!(bp.shield_disabled, 5);
    }

    async fn loaded_chunks(
        chunks: &[(i32, i32)],
    ) -> std::sync::Arc<super::super::anvil::AnvilChunkSource> {
        let source = super::super::anvil::AnvilChunkSource::new_memory();
        for &(cx, cz) in chunks {
            for _ in 0..2_000 {
                if let Some(result) = source.get(cx, 0, cz) {
                    result.expect("chunk load");
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            }
        }
        source
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn witch_cross_region_snapshot_splash_self_immunity_and_potion_handoff() {
        let source = loaded_chunks(&[(15, 0), (16, 0)]).await;
        let mut ctx = tick_context();
        ctx.source = Some(Arc::clone(&source));
        let blocks = TickBlocks {
            source: Some(&source),
            seen: Default::default(),
        };
        let player_id = Uuid::from_u128(0xFA10);
        // Region section boundary sits at x = 256 (chunk 15 in Section(0,0), chunk 16 in Section(1,0)).
        let mut east = Region::default();
        east.hold_chunk(ChunkPos(16, 0));
        let mut target_player = test_ranged_player(&ctx, player_id, [259.5, 65.0, 0.5]);
        target_player
            .effects
            .add(ferrite_entity::effect::MobEffectInstance::new(
                Effect::Slowness.id(),
                200,
                0,
            ));
        east.players.insert(player_id, target_player);
        // Region B publishes snapshot & position for cross-region witch in Region A.
        super::super::targeting::publish_snapshot(
            player_id,
            Region::player_snapshot(east.players.get(&player_id).unwrap()),
        );
        ctx.positions
            .lock()
            .unwrap()
            .insert(player_id, [259.5, 65.0, 0.5]);
        ctx.tracking.lock().unwrap().insert(
            player_id,
            (ctx.world_id, HashSet::new(), OutboundSender::channel(16).0),
        );

        let mut west = Region {
            rng: Rng::new(10),
            ..Region::default()
        };
        west.hold_chunk(ChunkPos(15, 0));
        let witch = west.entities.insert(mob::new_mob(
            MobKind::Witch,
            ferrite_math::Vec3::new(255.5, 65.0, 0.5),
            &mut west.rng,
        ));
        let witch_uuid = west.entities.data(witch).unwrap().uuid;

        // Tick Region A: Witch sees Slowness+20HP cross-region player and throws Poison eastward.
        west.tick_mobs(&ctx, &blocks);
        let thrown: Vec<_> = west
            .entities
            .keys()
            .iter()
            .copied()
            .filter(|&k| {
                arrow::kind_of(west.entities.data(k).unwrap()) == Some(arrow::Kind::SplashPotion)
            })
            .collect();
        assert_eq!(
            thrown.len(),
            1,
            "witch threw splash potion toward cross-region player"
        );
        assert!(
            west.entities
                .data(thrown[0])
                .unwrap()
                .thrown_potion
                .is_some(),
            "thrown potion carries bottle item across transfers"
        );

        // Self-immunity: Witch's own harming splash does zero damage to her.
        let before_hp = mob::state(&west.entities, witch).unwrap().health;
        west.splash_potion_owned(
            [255.5, 65.0, 0.5],
            &mob::WitchPotion::Harming.stack("minecraft:splash_potion"),
            Some(witch_uuid),
        );
        assert_eq!(mob::state(&west.entities, witch).unwrap().health, before_hp);

        // Verify split/merge keeps the potion entity and bottle stack, then hand off across the seam.
        let split_out = west.split(&HashSet::from([SectionPos(0, 0)]));
        assert!(west.entities.keys().is_empty());
        west.merge(split_out);
        let mut landed_on_player = false;
        for _ in 0..40 {
            west.tick_arrows(&ctx);
            for rec in west.entities.take_crossing() {
                east.entities.insert(rec);
            }
            east.tick_arrows(&ctx);
            if east
                .players
                .get(&player_id)
                .unwrap()
                .effects
                .has(Effect::Poison)
            {
                landed_on_player = true;
                break;
            }
        }
        assert!(
            landed_on_player,
            "splash potion crossed region boundary and poisoned the player"
        );
        super::super::targeting::forget(player_id);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn piglin_cross_region_gold_armor_neutrality_crossbow_and_split_merge() {
        let source = loaded_chunks(&[(15, 0), (16, 0)]).await;
        let mut ctx = tick_context();
        ctx.source = Some(Arc::clone(&source));
        let blocks = TickBlocks {
            source: Some(&source),
            seen: Default::default(),
        };
        let player_id = Uuid::from_u128(0xFA20);
        let mut east = Region::default();
        east.hold_chunk(ChunkPos(16, 0));
        let mut player = test_ranged_player(&ctx, player_id, [259.5, 64.0, 0.5]);
        player.inventory.set(
            ferrite_inventory::HEAD,
            ferrite_item::ItemStack::from_name("minecraft:golden_helmet").unwrap(),
        );
        east.players.insert(player_id, player);
        super::super::targeting::publish_snapshot(
            player_id,
            Region::player_snapshot(east.players.get(&player_id).unwrap()),
        );
        ctx.positions
            .lock()
            .unwrap()
            .insert(player_id, [259.5, 64.0, 0.5]);
        ctx.tracking.lock().unwrap().insert(
            player_id,
            (ctx.world_id, HashSet::new(), OutboundSender::channel(16).0),
        );

        let mut west = Region::default();
        west.hold_chunk(ChunkPos(15, 0));
        let piglin = west.entities.insert(mob::new_mob(
            MobKind::Piglin,
            ferrite_math::Vec3::new(255.5, 64.0, 0.5),
            &mut west.rng,
        ));
        mob::state_mut(&mut west.entities, piglin).unwrap().age = 0;
        mob::set_equipment_stack(
            &mut west.entities,
            piglin,
            ferrite_entity::EquipmentSlot::MainHand,
            Some(ferrite_item::ItemStack::from_name("minecraft:crossbow").unwrap()),
        );
        let piglin_uuid = west.entities.data(piglin).unwrap().uuid;

        // With golden helmet published across regions, Piglin stays calm.
        west.tick_mobs(&ctx, &blocks);
        assert_eq!(mob::state(&west.entities, piglin).unwrap().target, None);

        // Take off gold helmet: Piglin locks on, charges crossbow, and keeps state across split/merge.
        east.players
            .get_mut(&player_id)
            .unwrap()
            .inventory
            .set(ferrite_inventory::HEAD, ferrite_item::ItemStack::empty());
        super::super::targeting::publish_snapshot(
            player_id,
            Region::player_snapshot(east.players.get(&player_id).unwrap()),
        );
        west.tick_mobs(&ctx, &blocks);
        assert_eq!(
            mob::state(&west.entities, piglin).unwrap().target,
            Some(player_id)
        );
        assert_eq!(
            mob::state(&west.entities, piglin).unwrap().crossbow_state,
            mob::CrossbowState::Charging
        );

        let split_piglin =
            |entities: &ferrite_entity::EntityStore| -> Option<(Option<Uuid>, mob::CrossbowState)> {
                entities.keys().iter().copied().find_map(|k| {
                    let d = entities.data(k)?;
                    (d.uuid == piglin_uuid).then(|| {
                        let s = d.mob.unwrap();
                        (s.target, s.crossbow_state)
                    })
                })
            };
        let taken = west.split(&HashSet::from([SectionPos(0, 0)]));
        assert_eq!(
            split_piglin(&taken.entities),
            Some((Some(player_id), mob::CrossbowState::Charging))
        );
        west.merge(taken);
        assert_eq!(
            split_piglin(&west.entities),
            Some((Some(player_id), mob::CrossbowState::Charging))
        );
        super::super::targeting::forget(player_id);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stray_and_bogged_arrows_apply_effects_to_region_players() {
        let source = loaded_source().await;
        let mut ctx = tick_context();
        ctx.source = Some(Arc::clone(&source));
        let blocks = TickBlocks {
            source: Some(&source),
            seen: Default::default(),
        };
        for (kind, expected_effect, expected_duration) in [
            (
                MobKind::Stray,
                Effect::Slowness,
                arrow::STRAY_SLOWNESS_TICKS,
            ),
            (MobKind::Bogged, Effect::Poison, arrow::BOGGED_POISON_TICKS),
        ] {
            let mut region = Region {
                rng: Rng::new(10),
                ..Default::default()
            };
            region.hold_chunk(ChunkPos(0, 0));
            let victim = Uuid::from_u128(0xFA30 + kind.entity_type() as u128);
            region
                .players
                .insert(victim, test_ranged_player(&ctx, victim, [3.5, 65.0, 0.5]));
            region.entities.insert(mob::new_mob(
                kind,
                ferrite_math::Vec3::new(0.5, 65.0, 0.5),
                &mut region.rng,
            ));
            region.tick_mobs(&ctx, &blocks);
            for _ in 0..10 {
                region.tick_arrows(&ctx);
            }
            let p = region.players.get(&victim).unwrap();
            let eff = p.effects.get(expected_effect).unwrap_or_else(|| {
                panic!("{kind:?} arrow did not apply {expected_effect:?} to player")
            });
            assert_eq!(eff.duration, expected_duration);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn piglin_barter_consumes_one_ingot_rejects_baby_and_cooldown_and_throws_table_result() {
        if super::super::drops::tables().is_none() {
            return;
        }
        let source = loaded_source().await;
        let ctx = bare_context(&source);
        let blocks = TickBlocks {
            source: Some(&source),
            seen: Default::default(),
        };
        let mut region = Region::default();
        let player_id = Uuid::from_u128(0x9161_0001);
        let mut player = test_player(player_id, 1, [2.5, 65.0, 0.5], 0, &ctx);
        let ingot = ferrite_item::ItemStack::from_name("minecraft:gold_ingot").unwrap();
        let slot = player.inventory.selected_slot();
        player.inventory.set(slot, ingot.copy_with_count(2));

        let adult = region.entities.insert(mob::new_mob(
            MobKind::Piglin,
            ferrite_math::Vec3::new(0.5, 65.0, 0.5),
            &mut region.rng,
        ));
        mob::state_mut(&mut region.entities, adult).unwrap().age = 0;
        let adult_id = region.entities.data(adult).unwrap().id;
        let adult_uuid = region.entities.data(adult).unwrap().uuid;

        let baby = region.entities.insert(mob::new_mob(
            MobKind::Piglin,
            ferrite_math::Vec3::new(1.5, 65.0, 0.5),
            &mut region.rng,
        ));
        mob::state_mut(&mut region.entities, baby).unwrap().age = mob::PIGLIN_BABY_AGE;
        let baby_id = region.entities.data(baby).unwrap().id;

        let dead = region.entities.insert(mob::new_mob(
            MobKind::Piglin,
            ferrite_math::Vec3::new(1.5, 65.0, 1.5),
            &mut region.rng,
        ));
        {
            let s = mob::state_mut(&mut region.entities, dead).unwrap();
            s.age = 0;
            s.health = 0.0;
        }
        let dead_id = region.entities.data(dead).unwrap().id;

        // First click on adult consumes 1 ingot and starts admiring.
        // Second click on same adult (cooldown/already admiring), baby, and dead must not consume.
        player
            .uses
            .extend([(adult_id, 0), (adult_id, 0), (baby_id, 0), (dead_id, 0)]);
        region.players.insert(player_id, player);
        region.resolve_uses(&ctx);

        let p = region.players.get(&player_id).unwrap();
        assert_eq!(p.inventory.get(slot).count, 1);
        assert_eq!(
            mob::state(&region.entities, adult).unwrap().admiring,
            ferrite_entity::piglin::ADMIRE_TICKS
        );
        assert_eq!(
            mob::equipment_stack(
                &region.entities,
                adult,
                ferrite_entity::EquipmentSlot::OffHand
            )
            .as_ref()
            .and_then(|s| s.name()),
            Some("minecraft:gold_ingot")
        );
        assert_eq!(mob::state(&region.entities, baby).unwrap().admiring, 0);
        assert_eq!(mob::state(&region.entities, dead).unwrap().admiring, 0);

        // Tick 119 times: barter finishes on the 119th tick and throws one table item toward the player.
        for _ in 0..ferrite_entity::piglin::ADMIRE_TICKS {
            region.tick_mobs(&ctx, &blocks);
        }
        assert_eq!(mob::state(&region.entities, adult).unwrap().admiring, 0);
        assert!(
            mob::equipment_stack(
                &region.entities,
                adult,
                ferrite_entity::EquipmentSlot::OffHand
            )
            .is_none()
        );
        let thrown: Vec<_> = region
            .entities
            .keys()
            .iter()
            .copied()
            .filter_map(|k| {
                let d = region.entities.data(k)?;
                let item_state = d.item.as_ref()?;
                let vel = region.entities.vel(k)?;
                Some((item::full_stack(d), item_state.clone(), vel))
            })
            .collect();
        assert_eq!(thrown.len(), 1);
        let (dropped_stack, item_state, vel) = &thrown[0];
        assert!(dropped_stack.count >= 1 && dropped_stack.name().is_some());
        assert_eq!(item_state.pickup_delay, item::DEFAULT_PICKUP_DELAY);
        assert_eq!(item_state.thrower, Some(adult_uuid));
        assert!(
            vel.x > 0.0,
            "thrown item should move toward player at x=2.5"
        );

        // Creative mode keeps the gold ingot stack while still triggering bartering.
        let p = region.players.get_mut(&player_id).unwrap();
        p.gamemode = 1;
        p.inventory.set(slot, ingot.copy_with_count(1));
        p.uses.push((adult_id, 0));
        region.resolve_uses(&ctx);
        assert_eq!(
            region
                .players
                .get(&player_id)
                .unwrap()
                .inventory
                .get(slot)
                .count,
            1
        );
        assert_eq!(
            mob::state(&region.entities, adult).unwrap().admiring,
            ferrite_entity::piglin::ADMIRE_TICKS
        );
        super::super::targeting::forget(player_id);
    }

    #[test]
    fn piglin_bartering_state_and_inventory_survive_region_split_merge_and_death() {
        if super::super::drops::tables().is_none() {
            return;
        }
        let ctx = tick_context();
        let blocks = TickBlocks {
            source: None,
            seen: Default::default(),
        };
        let mut region = Region::default();
        let piglin = region.entities.insert(mob::new_mob(
            MobKind::Piglin,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut region.rng,
        ));
        {
            let state = mob::state_mut(&mut region.entities, piglin).unwrap();
            state.age = 0;
            state.despawns = false;
        }
        let piglin_uuid = region.entities.data(piglin).unwrap().uuid;

        let pork = ferrite_item::ItemStack::from_name("minecraft:porkchop")
            .unwrap()
            .copy_with_count(2);
        ferrite_entity::piglin::inventory_mut(&mut region.entities, piglin)
            .unwrap()
            .set(0, pork.clone());
        assert!(ferrite_entity::piglin::start_admiring(&mut region.entities, piglin).is_some());

        // Advance partway through the admire timer, then hand off across a region split/merge.
        for _ in 0..50 {
            region.tick_mobs(&ctx, &blocks);
        }
        let expected_remaining = ferrite_entity::piglin::ADMIRE_TICKS - 50;
        let mut taken = region.split(&HashSet::from([SectionPos(0, 0)]));
        let split_key = taken
            .entities
            .keys()
            .iter()
            .copied()
            .find(|&k| {
                taken
                    .entities
                    .data(k)
                    .is_some_and(|d| d.uuid == piglin_uuid)
            })
            .expect("piglin moved with split region");
        assert_eq!(
            mob::state(&taken.entities, split_key).unwrap().admiring,
            expected_remaining
        );
        assert_eq!(
            ferrite_entity::piglin::inventory(&taken.entities, split_key)
                .unwrap()
                .get(0),
            Some(&pork)
        );
        // Tick the remaining ticks in the new region and confirm it completes and throws.
        for _ in 0..expected_remaining {
            taken.tick_mobs(&ctx, &blocks);
        }
        assert_eq!(mob::state(&taken.entities, split_key).unwrap().admiring, 0);
        assert!(
            taken
                .entities
                .keys()
                .iter()
                .any(|&k| taken.entities.data(k).is_some_and(|d| d.item.is_some())),
            "bartered item must spawn after split hand-off"
        );

        region.merge(taken);
        let merged_key = region
            .entities
            .keys()
            .iter()
            .copied()
            .find(|&k| {
                region
                    .entities
                    .data(k)
                    .is_some_and(|d| d.uuid == piglin_uuid)
            })
            .unwrap();
        // Killing the piglin drops its 8-slot inventory and clears it so nothing drops twice.
        mob::hurt(&mut region.entities, merged_key, 100.0, None);
        region.tick_mobs(&ctx, &blocks);
        assert!(
            region
                .entities
                .keys()
                .iter()
                .filter_map(|&k| {
                    let d = region.entities.data(k)?;
                    d.item.as_ref()?;
                    Some(item::full_stack(d))
                })
                .any(|s| s.name() == Some("minecraft:porkchop") && s.count == 2),
            "piglin death must drop its 8-slot inventory"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn piglin_guarded_chest_open_and_break_anger_adults_including_cross_region() {
        let source = loaded_source().await;
        let chest = u32::from(ferrite_data::blocks::default_state("minecraft:chest").unwrap());
        let stone = u32::from(ferrite_data::blocks::default_state("minecraft:stone").unwrap());
        let gold_block =
            u32::from(ferrite_data::blocks::default_state("minecraft:gold_block").unwrap());
        source.set_block(2, 65, 2, chest).unwrap();
        source.set_block(10, 65, 2, gold_block).unwrap();
        // Wall at x = 6 blocks line of sight between the chest (x=2) and `blocked_piglin` (x=8).
        for y in 64..=68 {
            for z in 0..=4 {
                source.set_block(6, y, z, stone).unwrap();
            }
        }

        let (signals, mut signal_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut ctx = bare_context(&source);
        ctx.signals = signals;
        let blocks = TickBlocks {
            source: Some(&source),
            seen: Default::default(),
        };

        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));

        let player_id = Uuid::from_u128(0xC4E5_7001);
        let mut player = test_player(player_id, 10, [1.5, 65.0, 2.5], 0, &ctx);
        player.source = Some(Arc::clone(&source));
        // Wearing golden armor protects from passive hostility, NOT from guarded-chest anger.
        player.inventory.set(
            ferrite_inventory::HEAD,
            ferrite_item::ItemStack::from_name("minecraft:golden_helmet").unwrap(),
        );

        let visible_adult = region.entities.insert(mob::new_mob(
            MobKind::Piglin,
            ferrite_math::Vec3::new(4.5, 65.0, 2.5),
            &mut region.rng,
        ));
        mob::state_mut(&mut region.entities, visible_adult)
            .unwrap()
            .age = 0;

        let blocked_adult = region.entities.insert(mob::new_mob(
            MobKind::Piglin,
            ferrite_math::Vec3::new(8.5, 65.0, 2.5),
            &mut region.rng,
        ));
        mob::state_mut(&mut region.entities, blocked_adult)
            .unwrap()
            .age = 0;

        let baby = region.entities.insert(mob::new_mob(
            MobKind::Piglin,
            ferrite_math::Vec3::new(3.5, 65.0, 2.5),
            &mut region.rng,
        ));
        mob::state_mut(&mut region.entities, baby).unwrap().age = mob::PIGLIN_BABY_AGE;

        // Creative player opening the chest does not anger piglins.
        player.gamemode = 1;
        let chest_loc = ferrite_protocol::types::BlockPos::new(2, 65, 2).pack();
        assert!(player.use_block(chest_loc, 0));
        assert!(player.piglin_alerts.is_empty());
        player.close_open_menu();

        // Survival player wearing gold armor opens the chest: queues LOS-checked guard alert.
        player.gamemode = 0;
        assert!(player.use_block(chest_loc, 0));
        for require_los in player.piglin_alerts.drain(..) {
            region.trigger_piglin_guard_alert(&ctx, &blocks, player_id, player.pos, require_los);
        }
        assert_eq!(
            mob::state(&region.entities, visible_adult).unwrap().target,
            Some(player_id)
        );
        assert_eq!(
            mob::state(&region.entities, visible_adult).unwrap().anger,
            mob::PIGLIN_ANGER_TICKS
        );
        assert_eq!(
            mob::state(&region.entities, blocked_adult).unwrap().target,
            None,
            "chest open requires line of sight"
        );
        assert_eq!(mob::state(&region.entities, baby).unwrap().target, None);

        // Unheld adjacent chunk within 16 blocks receives a cross-region PiglinGuardAlert signal.
        let mut saw_remote_alert = false;
        while let Ok(sig) = signal_rx.try_recv() {
            if let Signal::PiglinGuardAlert {
                player: target,
                require_los: true,
                chunks,
                ..
            } = sig
            {
                assert_eq!(target, player_id);
                assert!(chunks.contains(&ChunkPos(-1, 0)));
                saw_remote_alert = true;
            }
        }
        assert!(saw_remote_alert);

        // Breaking a guarded block (gold_block) angers even without line of sight.
        let gold_loc = ferrite_protocol::types::BlockPos::new(10, 65, 2).pack();
        assert!(player.break_block(gold_loc, gold_block));
        for require_los in player.piglin_alerts.drain(..) {
            region.trigger_piglin_guard_alert(&ctx, &blocks, player_id, player.pos, require_los);
        }
        assert_eq!(
            mob::state(&region.entities, blocked_adult).unwrap().target,
            Some(player_id)
        );
        super::super::targeting::forget(player_id);
    }
    fn hive_state(honey: u8) -> u32 {
        u32::from(
            ferrite_data::blocks::state_id(
                "minecraft:beehive",
                &[("facing", "north"), ("honey_level", &honey.to_string())],
            )
            .expect("a vanilla beehive"),
        )
    }

    fn hive_bee(rng: &mut Rng, at: ferrite_math::Vec3) -> ferrite_entity::EntityRecord {
        mob::new_mob(MobKind::Bee, at, rng)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_bee_enters_its_hive_at_night_and_leaves_when_day_returns() {
        let source = loaded_source().await;
        source.set_block(4, 65, 4, hive_state(0)).unwrap();
        super::super::beehive::ensure(&source, [4, 65, 4]);
        let ctx = eat_context(&source, true);
        ctx.state.lock().unwrap().clock_time = 18_000;
        let mut region = Region::default();
        region.held_chunks.insert(ChunkPos(0, 0), 1);
        let bee = region.entities.insert(hive_bee(
            &mut region.rng,
            ferrite_math::Vec3::new(4.5, 65.0, 4.5),
        ));
        mob::state_mut(&mut region.entities, bee).unwrap().hive_pos = Some([4, 65, 4]);

        region.tick_beehives(&ctx);
        assert!(
            region.entities.data(bee).is_none(),
            "the bee is an occupant at night"
        );
        assert_eq!(
            super::super::beehive::load(&source, [4, 65, 4])
                .occupants
                .len(),
            1
        );

        let mut data = super::super::beehive::load(&source, [4, 65, 4]);
        data.occupants[0].ticks_in_hive = data.occupants[0].min_ticks_in_hive + 1;
        super::super::beehive::store(&source, [4, 65, 4], &data);
        ctx.state.lock().unwrap().clock_time = 1_000;
        region.tick_beehives(&ctx);

        assert!(
            super::super::beehive::load(&source, [4, 65, 4])
                .occupants
                .is_empty(),
            "daylight releases the due occupant"
        );
        assert_eq!(region.entities.keys().len(), 1, "the bee is out again");
        let released = region.entities.keys()[0];
        assert_eq!(
            mob::state(&region.entities, released).unwrap().kind,
            MobKind::Bee
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_nectar_bee_delivers_honey_and_leaves_the_hive_empty_handed() {
        let source = loaded_source().await;
        source.set_block(4, 65, 4, hive_state(0)).unwrap();
        super::super::beehive::ensure(&source, [4, 65, 4]);
        let mut rng = Rng::new(4);
        let mut bee = hive_bee(&mut rng, ferrite_math::Vec3::new(4.5, 65.0, 4.5));
        bee.data.mob.as_mut().unwrap().has_nectar = true;
        bee.data.mob.as_mut().unwrap().hive_pos = Some([4, 65, 4]);
        let mut data = super::super::beehive::load(&source, [4, 65, 4]);
        assert!(data.add(&bee));
        data.occupants[0].ticks_in_hive = data.occupants[0].min_ticks_in_hive + 1;
        super::super::beehive::store(&source, [4, 65, 4], &data);

        let ctx = eat_context(&source, true);
        let mut region = Region::default();
        region.held_chunks.insert(ChunkPos(0, 0), 1);
        region.tick_beehives(&ctx);

        let state = u16::try_from(source.block(4, 65, 4).unwrap()).unwrap();
        let level = super::super::beehive::honey_level(state).expect("a hive");
        assert!(
            (1..=2).contains(&level),
            "one delivery adds one or two honey levels, got {level}"
        );
        assert!(
            super::super::beehive::load(&source, [4, 65, 4])
                .occupants
                .is_empty()
        );
        assert_eq!(region.entities.keys().len(), 1);
        let released = region.entities.keys()[0];
        let released = mob::state(&region.entities, released).unwrap();
        assert!(!released.has_nectar, "the nectar was delivered");
        assert_eq!(released.crops_grown, 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_nectar_bee_advances_a_crop_below_it() {
        let source = loaded_source().await;
        let wheat =
            u32::from(ferrite_data::blocks::default_state("minecraft:wheat").expect("wheat"));
        source.set_block(6, 64, 6, wheat).unwrap();
        let ctx = eat_context(&source, true);
        let mut region = Region::default();
        region.held_chunks.insert(ChunkPos(0, 0), 1);
        let bee = region.entities.insert(hive_bee(
            &mut region.rng,
            ferrite_math::Vec3::new(6.5, 65.0, 6.5),
        ));
        mob::state_mut(&mut region.entities, bee)
            .unwrap()
            .has_nectar = true;

        for _ in 0..900 {
            region.tick_beehives(&ctx);
        }

        let state = u16::try_from(source.block(6, 64, 6).unwrap()).unwrap();
        let (_, props) = ferrite_data::blocks::state(state).unwrap();
        let age = props
            .iter()
            .find(|(key, _)| *key == "age")
            .map(|(_, value)| *value)
            .expect("wheat has an age");
        assert_ne!(age, "0", "the bee grows the crop below it");
        assert!(mob::state(&region.entities, bee).unwrap().crops_grown > 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn harvesting_releases_bees_only_without_a_lit_campfire_below() {
        let source = loaded_source().await;
        source.set_block(4, 65, 4, hive_state(5)).unwrap();
        super::super::beehive::ensure(&source, [4, 65, 4]);
        let mut rng = Rng::new(5);
        let bee = hive_bee(&mut rng, ferrite_math::Vec3::new(4.5, 65.0, 4.5));
        let mut data = super::super::beehive::load(&source, [4, 65, 4]);
        assert!(data.add(&bee));
        super::super::beehive::store(&source, [4, 65, 4], &data);

        let ctx = eat_context(&source, true);
        let uuid = Uuid::from_u128(0xbee5);
        let mut player = test_player(uuid, 1, [4.5, 65.0, 4.5], 0, &ctx);
        player.source = Some(source.clone());
        player.hive_harvests.push(([4, 65, 4], uuid, false));
        let mut region = Region::default();
        region.held_chunks.insert(ChunkPos(0, 0), 1);
        region.players.insert(uuid, player);

        region.tick_beehives(&ctx);
        assert!(
            super::super::beehive::load(&source, [4, 65, 4])
                .occupants
                .is_empty()
        );
        assert_eq!(region.entities.keys().len(), 1, "the bee is released");

        let bee = hive_bee(&mut rng, ferrite_math::Vec3::new(4.5, 65.0, 4.5));
        let mut data = super::super::beehive::load(&source, [4, 65, 4]);
        assert!(data.add(&bee));
        super::super::beehive::store(&source, [4, 65, 4], &data);
        let campfire = ferrite_data::blocks::default_state("minecraft:campfire").unwrap();
        source.set_block(4, 63, 4, u32::from(campfire)).unwrap();
        assert!(super::super::beehive::is_sedated(&source, [4, 65, 4]));
        region
            .players
            .get_mut(&uuid)
            .unwrap()
            .hive_harvests
            .push(([4, 65, 4], uuid, false));

        region.tick_beehives(&ctx);
        assert_eq!(
            super::super::beehive::load(&source, [4, 65, 4])
                .occupants
                .len(),
            1,
            "a lit campfire keeps the bees inside"
        );
        assert_eq!(region.entities.keys().len(), 1, "no extra bee is released");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn breaking_a_hive_releases_its_bees_and_keeps_the_item_component() {
        let source = loaded_source().await;
        source.set_block(5, 65, 5, hive_state(3)).unwrap();
        super::super::beehive::ensure(&source, [5, 65, 5]);
        let mut rng = Rng::new(6);
        let bee = hive_bee(&mut rng, ferrite_math::Vec3::new(5.5, 65.0, 5.5));
        let mut data = super::super::beehive::load(&source, [5, 65, 5]);
        assert!(data.add(&bee));
        super::super::beehive::store(&source, [5, 65, 5], &data);

        let ctx = eat_context(&source, true);
        let uuid = Uuid::from_u128(0xbee6);
        let mut player = test_player(uuid, 1, [5.5, 65.0, 5.5], 0, &ctx);
        player.source = Some(source.clone());
        let location = ferrite_protocol::types::BlockPos::new(5, 65, 5).pack();
        assert!(player.break_block(location, hive_state(3)));
        let components = player
            .menus
            .broken_entities
            .get(&[5, 65, 5])
            .expect("the hive's item components");
        assert!(matches!(
            components.get("minecraft:bees"),
            Some(ferrite_nbt::Tag::List(ferrite_nbt::List::Compound(bees))) if bees.len() == 1
        ));

        let mut region = Region::default();
        region.held_chunks.insert(ChunkPos(0, 0), 1);
        region.players.insert(uuid, player);
        region.tick_beehives(&ctx);

        assert!(source.block_entity(5, 65, 5).is_none());
        assert_eq!(region.entities.keys().len(), 1, "the bee is released");
        let released = region.entities.keys()[0];
        assert_eq!(
            mob::state(&region.entities, released).unwrap().kind,
            MobKind::Bee
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_hive_at_a_region_seam_hands_the_released_bee_to_the_neighbour() {
        let source = super::super::anvil::AnvilChunkSource::new_memory();
        for x in [15, 16] {
            assert!(source.ensure_chunk_loaded(x, 0, std::time::Duration::from_secs(5)));
        }
        let hive = u32::from(
            ferrite_data::blocks::state_id(
                "minecraft:beehive",
                &[("facing", "east"), ("honey_level", "0")],
            )
            .unwrap(),
        );
        source.set_block(255, 65, 0, hive).unwrap();
        super::super::beehive::ensure(&source, [255, 65, 0]);
        let mut rng = Rng::new(7);
        let bee = hive_bee(&mut rng, ferrite_math::Vec3::new(255.5, 65.0, 0.5));
        let mut data = super::super::beehive::load(&source, [255, 65, 0]);
        assert!(data.add(&bee));
        data.occupants[0].ticks_in_hive = data.occupants[0].min_ticks_in_hive + 1;
        super::super::beehive::store(&source, [255, 65, 0], &data);

        let mut ctx = eat_context(&source, true);
        let (signals, mut received) = tokio::sync::mpsc::unbounded_channel();
        ctx.signals = signals;
        let mut region = Region::default();
        for x in [15, 16] {
            region.held_chunks.insert(ChunkPos(x, 0), 1);
        }
        region.tick_beehives(&ctx);

        match received.try_recv() {
            Ok(Signal::EntityTransfer { to, entity, .. }) => {
                assert_eq!(to, ChunkPos(16, 0));
                assert_eq!(
                    entity.data.mob.as_ref().map(|mob| mob.kind),
                    Some(MobKind::Bee)
                );
            }
            _ => panic!("expected a cross-region entity transfer"),
        }
        assert!(
            super::super::beehive::load(&source, [255, 65, 0])
                .occupants
                .is_empty()
        );
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_bee_forages_at_a_nearby_flower_before_carrying_nectar() {
        let source = loaded_source().await;
        let poppy =
            u32::from(ferrite_data::blocks::default_state("minecraft:poppy").expect("poppy"));
        source.set_block(6, 64, 6, poppy).unwrap();
        let ctx = eat_context(&source, true);
        let mut region = Region::default();
        region.held_chunks.insert(ChunkPos(0, 0), 1);
        let bee = region.entities.insert(hive_bee(
            &mut region.rng,
            ferrite_math::Vec3::new(5.5, 65.0, 5.5),
        ));

        region.tick_beehives(&ctx);
        let state = mob::state(&region.entities, bee).unwrap();
        assert!(!state.has_nectar, "pollination is not instant");
        assert_eq!(state.flower_pos, Some([6, 64, 6]));

        for _ in 0..40 {
            region.tick_beehives(&ctx);
        }
        let state = mob::state(&region.entities, bee).unwrap();
        assert!(
            state.has_nectar,
            "twenty ticks at the flower collect nectar"
        );
        assert_eq!(state.ticks_without_nectar, 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_bee_returns_home_after_an_hour_without_nectar() {
        let source = loaded_source().await;
        source.set_block(4, 65, 4, hive_state(0)).unwrap();
        super::super::beehive::ensure(&source, [4, 65, 4]);
        let ctx = eat_context(&source, true);
        let mut region = Region::default();
        region.held_chunks.insert(ChunkPos(0, 0), 1);
        let bee = region.entities.insert(hive_bee(
            &mut region.rng,
            ferrite_math::Vec3::new(4.5, 65.0, 4.5),
        ));
        mob::state_mut(&mut region.entities, bee).unwrap().hive_pos = Some([4, 65, 4]);

        region.tick_beehives(&ctx);
        assert!(
            super::super::beehive::load(&source, [4, 65, 4])
                .occupants
                .is_empty(),
            "a bee without nectar first looks for flowers"
        );
        assert_eq!(
            mob::state(&region.entities, bee)
                .unwrap()
                .ticks_without_nectar,
            1
        );
        mob::state_mut(&mut region.entities, bee)
            .unwrap()
            .ticks_without_nectar = 3599;
        region.tick_beehives(&ctx);
        assert!(
            super::super::beehive::load(&source, [4, 65, 4])
                .occupants
                .is_empty()
        );
        region.tick_beehives(&ctx);
        assert_eq!(
            super::super::beehive::load(&source, [4, 65, 4])
                .occupants
                .len(),
            1,
            "3600 ticks without nectar sends it home"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rain_keeps_a_bee_in_its_hive_during_the_day() {
        let source = loaded_source().await;
        source.set_block(4, 65, 4, hive_state(0)).unwrap();
        super::super::beehive::ensure(&source, [4, 65, 4]);
        let mut ctx = eat_context(&source, true);
        ctx.state.lock().unwrap().clock_time = 1_000;
        ctx.raining = true;
        let mut region = Region::default();
        region.held_chunks.insert(ChunkPos(0, 0), 1);
        let bee = region.entities.insert(hive_bee(
            &mut region.rng,
            ferrite_math::Vec3::new(4.5, 65.0, 4.5),
        ));
        mob::state_mut(&mut region.entities, bee).unwrap().hive_pos = Some([4, 65, 4]);

        region.tick_beehives(&ctx);
        assert_eq!(
            super::super::beehive::load(&source, [4, 65, 4])
                .occupants
                .len(),
            1
        );
    }

    #[test]
    fn hive_home_and_pending_breaks_survive_a_region_split_and_merge() {
        let mut region = Region::default();
        let bee = region.entities.insert(hive_bee(
            &mut region.rng,
            ferrite_math::Vec3::new(4.5, 65.0, 4.5),
        ));
        mob::state_mut(&mut region.entities, bee).unwrap().hive_pos = Some([4, 65, 4]);
        let state = u16::try_from(hive_state(0)).unwrap();
        region
            .pending_hive_breaks
            .push(([4, 65, 4], state, Default::default()));
        region
            .pending_hive_breaks
            .push(([400, 65, 4], state, Default::default()));

        let split = region.split(&HashSet::from([SectionPos(0, 0)]));
        assert!(region.entities.keys().is_empty());
        assert_eq!(split.entities.keys().len(), 1);
        assert_eq!(
            mob::state(&split.entities, split.entities.keys()[0])
                .unwrap()
                .hive_pos,
            Some([4, 65, 4])
        );
        assert_eq!(split.pending_hive_breaks.len(), 1);
        assert_eq!(split.pending_hive_breaks[0].0, [4, 65, 4]);
        assert_eq!(region.pending_hive_breaks.len(), 1);
        assert_eq!(region.pending_hive_breaks[0].0, [400, 65, 4]);

        region.merge(split);
        assert_eq!(region.entities.keys().len(), 1);
        assert_eq!(region.pending_hive_breaks.len(), 2);
        assert!(
            region
                .pending_hive_breaks
                .iter()
                .any(|(pos, _, _)| *pos == [4, 65, 4])
        );
        assert!(
            region
                .pending_hive_breaks
                .iter()
                .any(|(pos, _, _)| *pos == [400, 65, 4])
        );
    }
}

#[cfg(test)]
mod fluid_ownership_tests {
    use super::*;
    use ferrite_fluid::{FluidKind, FluidWorld, fluid_state};
    use std::collections::HashMap;

    struct Grid(HashMap<(i32, i32, i32), u32>);
    impl FluidWorld for Grid {
        fn block(&self, x: i32, y: i32, z: i32) -> u32 {
            *self.0.get(&(x, y, z)).unwrap_or(&0)
        }
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) {
            self.0.insert((x, y, z), state);
        }
        fn ultrawarm(&self) -> bool {
            false
        }
    }
    #[test]
    fn fluid_ticks_survive_merge_and_split() {
        let mut left = Region::default();
        let mut right = Region::default();
        left.fluid_ticks.schedule((0, 0, 0), 7);
        right.fluid_ticks.schedule((512, 0, 0), 9);
        left.merge(right);
        let mut right = left.split(&HashSet::from([SectionPos(2, 0)]));
        let water = u32::from(ferrite_data::blocks::default_state("water").unwrap());
        let stone = u32::from(ferrite_data::blocks::default_state("stone").unwrap());
        let mut grid = Grid(HashMap::from([
            ((0, 0, 0), water),
            ((512, 0, 0), water),
            ((0, -1, 0), stone),
            ((512, -1, 0), stone),
        ]));
        left.fluid_ticks.run_until(&mut grid, 6);
        right.fluid_ticks.run_until(&mut grid, 8);
        assert_eq!(fluid_state(grid.block(0, 0, 1)).kind, FluidKind::Empty);
        assert_eq!(fluid_state(grid.block(512, 0, 1)).kind, FluidKind::Empty);
        left.fluid_ticks.run_until(&mut grid, 7);
        right.fluid_ticks.run_until(&mut grid, 9);
        assert_eq!(fluid_state(grid.block(0, 0, 1)).kind, FluidKind::Water);
        assert_eq!(fluid_state(grid.block(512, 0, 1)).kind, FluidKind::Water);
    }
}

#[cfg(test)]
mod neighbor_ownership_tests {
    use super::*;
    use crate::game::{anvil::AnvilChunkSource, fluid::RegionFluidWorld};
    use std::sync::Mutex;

    async fn loaded(chunks: &[(i32, i32)]) -> Arc<AnvilChunkSource> {
        let source = AnvilChunkSource::new_memory();
        for &(x, z) in chunks {
            loop {
                if let Some(result) = source.get(x, z, 0) {
                    result.unwrap();
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
        source
    }
    fn id(name: &str) -> u32 {
        u32::from(ferrite_data::blocks::default_state(name).unwrap())
    }
    fn wall_torch() -> u32 {
        u32::from(ferrite_data::blocks::state_id("wall_torch", &[("facing", "east")]).unwrap())
    }

    fn gravity_context(
        source: &Arc<AnvilChunkSource>,
    ) -> (TickContext, tokio::sync::mpsc::UnboundedReceiver<Signal>) {
        let (signals, receiver) = tokio::sync::mpsc::unbounded_channel();
        let ctx = TickContext {
            signals,
            tracking: Default::default(),
            positions: Default::default(),
            source: Some(source.clone()),
            dimension: super::super::Dimension::Overworld,
            world_id: crate::game::WorldId::Vanilla(super::super::Dimension::Overworld),
            raining: false,
            rules: Default::default(),
            spawn: [0.0, 65.0, 0.0],
            command_entities: Default::default(),
            state: Default::default(),
            trader_spawner: Default::default(),
            profiler: None,
            tree: None,
            hooks: None,
        };
        (ctx, receiver)
    }

    /// The join-time footprint of the wake pass on the real fixture world: every chunk of a
    /// view-distance-5 square around the spawn is held at once, as when a player joins, and the
    /// pass runs until its queue is empty. Prints the total and the per-chunk cost
    /// (`cargo test -p ferrite-server wake_cost -- --nocapture`); it needs the extracted fixture
    /// world, so it is skipped when `vanilla/` is absent.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wake_cost_on_the_fixture_world_join_footprint() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = root.join("vanilla/26.3/fixture-world/world");
        let Ok(registry) =
            ferrite_world::RuntimeRegistry::load(root.join("vanilla/26.3/generated"))
        else {
            return;
        };
        let Ok(spawn) = super::super::anvil::read_spawn(&path) else {
            return;
        };
        let source = AnvilChunkSource::new(path, registry);
        let (cx, cz) = (spawn[0].div_euclid(16), spawn[2].div_euclid(16));
        let mut region = Region::default();
        let mut loaded_chunks = 0;
        for x in cx - 5..=cx + 5 {
            for z in cz - 5..=cz + 5 {
                for _ in 0..500 {
                    match source.get(x, z, 0) {
                        Some(Ok(_)) => {
                            loaded_chunks += 1;
                            region.hold_chunk(ChunkPos(x, z));
                            break;
                        }
                        Some(Err(_)) => break,
                        None => tokio::time::sleep(std::time::Duration::from_millis(5)).await,
                    }
                }
            }
        }
        let (signals, _rx) = tokio::sync::mpsc::unbounded_channel();
        let started = std::time::Instant::now();
        let mut passes = 0;
        while !region.pending_chunk_loads.is_empty() {
            region.wake_loaded_chunks(&source, signals.clone());
            passes += 1;
        }
        let elapsed = started.elapsed();
        eprintln!(
            "wake pass: {loaded_chunks} chunks, {passes} passes, {elapsed:?} total, {:?} per chunk",
            elapsed / loaded_chunks.max(1)
        );
    }

    /// Same region: the water reached the edge of its chunk while the east chunk was unloaded
    /// and stopped. Holding that chunk must schedule the border water again so it flows on.
    #[tokio::test]
    async fn a_chunk_that_loads_later_resumes_water_stopped_at_the_seam() {
        let source = loaded(&[(0, 0)]).await;
        let (water, stone, air) = (id("water"), id("stone"), id("air"));
        for x in 12..=15 {
            source.set_block(x, 64, 0, stone).unwrap();
            source.set_block(x, 65, 0, air).unwrap();
        }
        source.set_block(15, 65, 0, water).unwrap();
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let (signals, _rx) = tokio::sync::mpsc::unbounded_channel();
        region.wake_loaded_chunks(&source, signals);
        assert_eq!(source.block(16, 65, 0), None, "east chunk is unloaded");

        loop {
            if let Some(result) = source.get(1, 0, 0) {
                result.unwrap();
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        source.set_block(16, 64, 0, stone).unwrap();
        source.set_block(16, 65, 0, air).unwrap();
        region.hold_chunk(ChunkPos(1, 0));
        let (signals, _rx) = tokio::sync::mpsc::unbounded_channel();
        region.wake_loaded_chunks(&source, signals.clone());
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        for _ in 0..4 {
            region.tick_fluids(&source, &tracking, signals.clone(), true, false);
        }
        assert_eq!(
            ferrite_fluid::fluid_state(source.block(16, 65, 0).unwrap()).kind,
            ferrite_fluid::FluidKind::Water,
            "the resumed border water crossed into the newly loaded chunk"
        );
    }

    /// The wake pass runs from `Region::tick` itself: with no direct call, holding the east chunk
    /// after the water stopped at the seam is enough for the water to flow across.
    #[tokio::test]
    async fn the_tick_loop_wakes_water_at_a_seam_when_a_chunk_is_held_later() {
        let source = loaded(&[(0, 0)]).await;
        let (water, stone, air) = (id("water"), id("stone"), id("air"));
        for x in 12..=15 {
            source.set_block(x, 64, 0, stone).unwrap();
            source.set_block(x, 65, 0, air).unwrap();
        }
        source.set_block(15, 65, 0, water).unwrap();
        let (ctx, _rx) = gravity_context(&source);
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        region.tick(&ctx);
        loop {
            if let Some(result) = source.get(1, 0, 0) {
                result.unwrap();
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        source.set_block(16, 64, 0, stone).unwrap();
        source.set_block(16, 65, 0, air).unwrap();
        region.hold_chunk(ChunkPos(1, 0));
        for _ in 0..8 {
            // The world loop ticks fluids after the region tick (`game/mod.rs`).
            region.tick(&ctx);
            region.tick_fluids(&source, &ctx.tracking, ctx.signals.clone(), true, false);
        }
        assert_eq!(
            ferrite_fluid::fluid_state(source.block(16, 65, 0).unwrap()).kind,
            ferrite_fluid::FluidKind::Water,
            "the tick loop did not resume the border water"
        );
    }

    /// Same region: a wall torch in the chunk that loads had its support in the chunk that was
    /// unloaded while it was written. The unreadable side used to count as sturdy; loading the
    /// other chunk must re-validate the torch and remove it.
    #[tokio::test]
    async fn a_chunk_that_loads_later_breaks_a_torch_whose_support_is_air() {
        let source = loaded(&[(0, 0)]).await;
        loop {
            if let Some(result) = source.get(1, 0, 0) {
                result.unwrap();
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        source.set_block(15, 64, 0, id("air")).unwrap();
        source.set_block(16, 64, 0, wall_torch()).unwrap();
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let (signals, _rx) = tokio::sync::mpsc::unbounded_channel();
        region.wake_loaded_chunks(&source, signals);
        region.hold_chunk(ChunkPos(1, 0));
        let (signals, _rx) = tokio::sync::mpsc::unbounded_channel();
        region.wake_loaded_chunks(&source, signals.clone());

        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let mut world =
            RegionFluidWorld::new(&source, &region.held_chunks, &tracking).with_signals(signals);
        let mut removed = Vec::new();
        region.updates.apply(&mut world, &mut removed);
        assert!(
            removed.iter().any(|(pos, _)| *pos == [16, 64, 0]),
            "the torch is not re-validated: {removed:?}"
        );
        assert_eq!(source.block(16, 64, 0), Some(id("air")));
    }

    /// Cross region: the newly loaded chunk is the east region's, and the water that stopped at
    /// its edge belongs to the west region. The east load hands a fluid wake to the west owner.
    #[tokio::test]
    async fn a_late_chunk_wakes_water_owned_by_the_other_region_at_the_seam() {
        let source = loaded(&[(0, 0), (1, 0)]).await;
        source.set_block(15, 65, 0, id("water")).unwrap();
        let mut east = Region::default();
        east.hold_chunk(ChunkPos(1, 0));
        let (signals, mut received) = tokio::sync::mpsc::unbounded_channel();
        east.wake_loaded_chunks(&source, signals);
        let mut woken = false;
        while let Ok(signal) = received.try_recv() {
            if let Signal::FluidChange { x, y, z, .. } = signal
                && (x, y, z) == (15, 65, 0)
            {
                woken = true;
            }
        }
        assert!(woken, "the west owner was not woken for its border water");
    }

    /// A stack of gravity blocks falls one block per two ticks: the block above a vacated cell
    /// must be scheduled when the one below turns into an entity. Runs for a region holding the
    /// column and, with the column on the far side of a chunk border, for a second region.
    #[tokio::test]
    async fn a_gravity_stack_keeps_falling_and_wakes_the_fluid_around_each_vacated_cell() {
        for chunk_x in [0, 1] {
            let source = loaded(&[(0, 0), (1, 0)]).await;
            let x = chunk_x * 16;
            for y in 70..=72 {
                source.set_block(x, y, 0, id("sand")).unwrap();
            }
            source.set_block(x, 69, 0, id("air")).unwrap();
            let mut region = Region::default();
            region.hold_chunk(ChunkPos(chunk_x, 0));
            let (ctx, mut signals) = gravity_context(&source);
            region.schedule_gravity(&source, &[[x, 69, 0]]);
            for _ in 0..8 {
                region.tick_gravity(&ctx);
            }
            for y in 70..=72 {
                assert_eq!(source.block(x, y, 0), Some(id("air")), "x {x} y {y}");
            }
            assert_eq!(region.entities.len(), 3, "x {x}");
            let mut woken = Vec::new();
            while let Ok(signal) = signals.try_recv() {
                if let Signal::FluidChange { x, y, z, .. } = signal {
                    woken.push([x, y, z]);
                }
            }
            for y in 70..=72 {
                assert!(woken.contains(&[x, y, 0]), "fluid not woken at y {y}");
            }
        }
    }

    /// Cross-region: the west region breaks the support of a wall torch that lives in the east
    /// region. It must hand the update over through the scheduler and leave the torch alone; the
    /// east region then removes the torch and announces the removal back.
    #[tokio::test]
    async fn a_support_broken_across_a_region_boundary_removes_the_torch_in_the_owner() {
        let source = loaded(&[(0, 0), (1, 0)]).await;
        source.set_block(15, 64, 0, id("air")).unwrap();
        source.set_block(16, 64, 0, wall_torch()).unwrap();
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let (signals, mut received) = tokio::sync::mpsc::unbounded_channel();

        let mut west = Region::default();
        west.hold_chunk(ChunkPos(0, 0));
        let mut world = RegionFluidWorld::new(&source, &west.held_chunks, &tracking)
            .with_signals(signals.clone());
        west.updates.block_changed([15, 64, 0]);
        let mut removed = Vec::new();
        west.updates.apply(&mut world, &mut removed);
        assert!(removed.is_empty());
        assert_eq!(
            source.block(16, 64, 0),
            Some(wall_torch()),
            "west never writes east"
        );
        let Ok(Signal::NeighborUpdate { origin, target, .. }) = received.try_recv() else {
            panic!("the west region must hand the update to the east region");
        };
        assert_eq!((origin, target), ([15, 64, 0], [16, 64, 0]));

        let mut east = Region::default();
        east.hold_chunk(ChunkPos(1, 0));
        east.remote_block_changed(origin);
        assert_eq!(east.remote_changes, vec![[15, 64, 0]]);
        let mut world =
            RegionFluidWorld::new(&source, &east.held_chunks, &tracking).with_signals(signals);
        east.updates.apply(&mut world, &mut removed);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].0, [16, 64, 0]);
        assert_eq!(source.block(16, 64, 0), Some(id("air")));
    }

    /// Same-region baseline for the test above: one region holds both chunks and the torch
    /// breaks directly, without any signal.
    #[tokio::test]
    async fn a_support_broken_inside_one_region_removes_the_torch_without_a_signal() {
        let source = loaded(&[(0, 0), (1, 0)]).await;
        source.set_block(15, 64, 0, id("air")).unwrap();
        source.set_block(16, 64, 0, wall_torch()).unwrap();
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let (signals, mut received) = tokio::sync::mpsc::unbounded_channel();
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        region.hold_chunk(ChunkPos(1, 0));
        let mut world =
            RegionFluidWorld::new(&source, &region.held_chunks, &tracking).with_signals(signals);
        region.updates.block_changed([15, 64, 0]);
        let mut removed = Vec::new();
        region.updates.apply(&mut world, &mut removed);
        assert_eq!(removed.len(), 1);
        assert_eq!(source.block(16, 64, 0), Some(id("air")));
        assert!(received.try_recv().is_err());
    }

    /// An unloaded neighbour chunk has no owner to notify, so nothing is sent.
    #[tokio::test]
    async fn a_border_change_next_to_an_unloaded_chunk_sends_nothing() {
        let source = loaded(&[(0, 0)]).await;
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let (signals, mut received) = tokio::sync::mpsc::unbounded_channel();
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let mut world =
            RegionFluidWorld::new(&source, &region.held_chunks, &tracking).with_signals(signals);
        region.updates.block_changed([15, 64, 0]);
        region.updates.apply(&mut world, &mut Vec::new());
        assert!(received.try_recv().is_err());
    }

    /// Gravity across a boundary: sand in the east region becomes unsupported next to a change
    /// made by the west region. The east region schedules it from the remote origin.
    #[tokio::test]
    async fn a_remote_change_schedules_gravity_for_an_unsupported_block() {
        let source = loaded(&[(0, 0), (1, 0)]).await;
        source.set_block(16, 70, 0, id("sand")).unwrap();
        source.set_block(16, 69, 0, id("air")).unwrap();
        let mut east = Region::default();
        east.hold_chunk(ChunkPos(1, 0));
        east.schedule_gravity(&source, &[[15, 70, 0]]);
        assert!(east.gravity.scheduled.contains(&[16, 70, 0]));

        // A west region that does not hold the chunk never schedules the east block.
        let mut west = Region::default();
        west.hold_chunk(ChunkPos(0, 0));
        west.schedule_gravity(&source, &[[15, 70, 0]]);
        assert!(west.gravity.scheduled.is_empty());
    }
}

#[test]
fn chunk_holders_follow_region_merge_and_split() {
    let mut left = Region::default();
    let mut right = Region::default();
    left.hold_chunk(ChunkPos(0, 0));
    left.hold_chunk(ChunkPos(0, 0));
    right.hold_chunk(ChunkPos(32, 0));
    left.merge(right);
    let mut right = left.split(&HashSet::from([SectionPos(2, 0)]));
    assert_eq!(left.held_chunks.get(&ChunkPos(0, 0)), Some(&2));
    assert!(!left.held_chunks.contains_key(&ChunkPos(32, 0)));
    assert_eq!(right.held_chunks.get(&ChunkPos(32, 0)), Some(&1));
    right.release_chunk(ChunkPos(32, 0));
    assert!(right.held_chunks.is_empty());
}

#[cfg(test)]
mod worn_stack_tests {
    use super::{ItemMetaFields, with_meta, worn_stack, written_book};

    #[test]
    fn plugin_metadata_lands_as_components() {
        let sword = ferrite_item::item_id("diamond_sword").unwrap();
        let meta = ItemMetaFields {
            name: Some("{\"text\":\"Excalibur\"}".into()),
            lore: vec!["{\"text\":\"old\"}".into()],
            enchants: vec![("minecraft:sharpness".into(), 5)],
            unbreakable: true,
            model: Some(3),
            repair: 2,
            flags: vec![
                "HIDE_ENCHANTS".into(),
                "HIDE_DYE".into(),
                "HIDE_ADDITIONAL_TOOLTIP".into(),
            ],
            custom_data: Some({
                let mut data = ferrite_nbt::Compound::new();
                data.insert(
                    "ferrite:owner".into(),
                    ferrite_nbt::Tag::String("Steve".into()),
                );
                data.insert("ferrite:level".into(), ferrite_nbt::Tag::Int(12));
                data
            }),
            book: None,
            extra: None,
        };
        let stack = with_meta(ferrite_item::ItemStack::with_count(sword, 1), &meta);
        for id in [
            ferrite_item::component_ids::CUSTOM_NAME,
            ferrite_item::component_ids::LORE,
            ferrite_item::component_ids::ENCHANTMENTS,
            ferrite_item::component_ids::UNBREAKABLE,
            ferrite_item::component_ids::CUSTOM_MODEL_DATA,
            ferrite_item::component_ids::REPAIR_COST,
            ferrite_item::component_ids::TOOLTIP_DISPLAY,
            ferrite_item::component_ids::CUSTOM_DATA,
        ] {
            assert!(stack.components.get(id).is_some(), "component {id}");
        }
        assert_eq!(
            stack
                .components
                .get(ferrite_item::component_ids::TOOLTIP_DISPLAY),
            Some(&ferrite_item::Component::TooltipDisplay(
                ferrite_item::TooltipDisplay {
                    hide_tooltip: true,
                    hidden_components: vec![
                        ferrite_item::component_ids::ENCHANTMENTS,
                        ferrite_item::component_ids::DYED_COLOR,
                    ],
                }
            ))
        );
        let mut bytes = Vec::new();
        ferrite_protocol::Encode::encode(&stack, &mut bytes);
        let decoded = ferrite_item::ItemStack::decode_network(&bytes)
            .expect("plugin metadata stays in the network slot");
        assert_eq!(
            decoded
                .components
                .get(ferrite_item::component_ids::CUSTOM_DATA),
            stack
                .components
                .get(ferrite_item::component_ids::CUSTOM_DATA)
        );
        assert_eq!(
            decoded
                .components
                .get(ferrite_item::component_ids::TOOLTIP_DISPLAY),
            stack
                .components
                .get(ferrite_item::component_ids::TOOLTIP_DISPLAY)
        );
        let bare = ItemMetaFields {
            name: None,
            lore: vec![],
            enchants: vec![],
            unbreakable: false,
            model: None,
            repair: 0,
            flags: vec![],
            custom_data: None,
            book: None,
            extra: None,
        };
        let cleared = with_meta(stack, &bare);
        assert!(
            cleared
                .components
                .get(ferrite_item::component_ids::CUSTOM_NAME)
                .is_none()
        );
    }

    #[test]
    fn books_carry_their_pages_on_the_wire() {
        let book = written_book("T", "A", &["one".into(), "two".into()]).unwrap();
        let mut wire = Vec::new();
        ferrite_protocol::Encode::encode(&book, &mut wire);
        assert!(wire.windows(3).any(|w| w == b"one"));
        assert!(wire.windows(3).any(|w| w == b"two"));
    }

    fn book_meta(book: crate::game::plugin_types::BookFields) -> ItemMetaFields {
        ItemMetaFields {
            name: None,
            lore: Vec::new(),
            enchants: Vec::new(),
            unbreakable: false,
            model: None,
            repair: 0,
            flags: Vec::new(),
            custom_data: None,
            book: Some(book),
            extra: None,
        }
    }

    #[test]
    fn plugin_book_metadata_becomes_typed_components_that_survive_save_and_wire() {
        let signed = crate::game::plugin_types::BookFields {
            written: true,
            title: Some("Das Buch ✨".into()),
            author: Some("Álvaro".into()),
            generation: 2,
            pages: vec![
                r#"{"text":"Seite 1: Grüße 日本語 😀"}"#.into(),
                r#"{"text":"zweite","color":"red"}"#.into(),
            ],
        };
        let stack = with_meta(
            ferrite_item::ItemStack::from_name("written_book").unwrap(),
            &book_meta(signed),
        );
        let Some(ferrite_item::Component::WrittenBookContent(content)) = stack
            .components
            .get(ferrite_item::component_ids::WRITTEN_BOOK_CONTENT)
        else {
            panic!("a typed written book component");
        };
        assert_eq!(content.title.raw, "Das Buch ✨");
        assert_eq!(content.author, "Álvaro");
        assert_eq!(content.generation, 2);
        assert_eq!(content.pages.len(), 2);
        // Saved to the world and loaded again.
        let saved = ferrite_item::ItemStack::from_nbt(&stack.to_nbt().unwrap()).unwrap();
        assert_eq!(saved, stack);
        // Sent to a client and read back.
        let wire =
            ferrite_item::ItemStack::decode_network(&stack.encode_network().unwrap()).unwrap();
        assert_eq!(wire, stack);

        let quill = crate::game::plugin_types::BookFields {
            written: false,
            title: None,
            author: None,
            generation: 0,
            pages: vec!["§cred".into(), String::new(), "😀".into()],
        };
        let stack = with_meta(
            ferrite_item::ItemStack::from_name("writable_book").unwrap(),
            &book_meta(quill),
        );
        assert_eq!(
            ferrite_item::ItemStack::from_nbt(&stack.to_nbt().unwrap()).unwrap(),
            stack
        );
        let Some(ferrite_item::Component::WritableBookContent(content)) = stack
            .components
            .get(ferrite_item::component_ids::WRITABLE_BOOK_CONTENT)
        else {
            panic!("a typed writable book component");
        };
        assert_eq!(content.pages.len(), 3);
        assert_eq!(content.pages[1].raw, "");
    }

    #[test]
    fn tools_wear_but_never_break_through_this_path() {
        let pick = ferrite_item::item_id("diamond_pickaxe").unwrap();
        let worn = worn_stack(ferrite_item::ItemStack::with_count(pick, 1), 10).unwrap();
        let id = ferrite_item::Component::Damage(0).type_id().unwrap();
        assert_eq!(
            worn.components.get(id),
            Some(&ferrite_item::Component::Damage(10))
        );
        let clamped = worn_stack(ferrite_item::ItemStack::with_count(pick, 1), 99_999).unwrap();
        assert_eq!(
            clamped.components.get(id),
            Some(&ferrite_item::Component::Damage(1560))
        );
        let stone = ferrite_item::item_id("stone").unwrap();
        assert!(worn_stack(ferrite_item::ItemStack::with_count(stone, 1), 5).is_none());
        assert!(worn_stack(ferrite_item::ItemStack::empty(), 5).is_none());
    }
}

/// Item-frame strengths for comparators: `(block, facing, ItemFrame.getAnalogOutput)`. A frame
/// is a plain entity here (no placement logic yet), so its facing is the spawn data value.
pub(crate) fn item_frame_outputs(
    entities: &ferrite_entity::EntityStore,
) -> Vec<([i32; 3], ferrite_math::Direction, u8)> {
    use ferrite_protocol::packets::play::MetaValue;
    let kinds: Vec<u16> = ["minecraft:item_frame", "minecraft:glow_item_frame"]
        .iter()
        .filter_map(|name| ferrite_data::entity_props::by_name(name).map(|props| props.id))
        .collect();
    let mut out = Vec::new();
    for &key in entities.keys() {
        let (Some(at), Some(data)) = (entities.pos(key), entities.data(key)) else {
            continue;
        };
        if !kinds.contains(&data.kind) {
            continue;
        }
        let Some(&facing) = usize::try_from(data.spawn_data)
            .ok()
            .and_then(|index| ferrite_math::Direction::ALL.get(index))
        else {
            continue;
        };
        // `ItemFrame.getAnalogOutput`: nothing inside is 0, else rotation % 8 + 1.
        let strength = match data.metadata.get(8) {
            Some(MetaValue::ItemStack(stack)) if stack.count > 0 => {
                let rotation = match data.metadata.get(9) {
                    Some(MetaValue::Int(value)) => *value,
                    _ => 0,
                };
                (rotation.rem_euclid(8) + 1) as u8
            }
            _ => 0,
        };
        out.push((
            [
                at.x.floor() as i32,
                at.y.floor() as i32,
                at.z.floor() as i32,
            ],
            facing,
            strength,
        ));
    }
    out
}

/// How many players have each trapped chest open (`ContainerOpenersCounter`); both halves of
/// a double chest count the viewer, as vanilla counts them on either half's block entity.
pub(crate) fn trapped_chest_viewers<'a>(
    menus: impl Iterator<Item = &'a super::container::Containers>,
) -> std::collections::HashMap<[i32; 3], u8> {
    let mut out = std::collections::HashMap::new();
    for menu in menus {
        if let Some(super::container::OpenAt::Inventory(opened)) = &menu.open_at
            && opened.block == "minecraft:trapped_chest"
        {
            for &part in &opened.parts {
                let count = out.entry(part).or_insert(0u8);
                *count = count.saturating_add(1);
            }
        }
    }
    out
}

#[cfg(test)]
mod event_policy_tests {
    use super::*;
    use crate::game::plugin_types::{
        BlockPosition, CommandSender, PluginCommand, PluginCommandResult,
    };

    #[test]
    fn block_event_world_names_use_dimension_keys_and_runtime_uuids() {
        assert_eq!(
            plugin_world_name(super::super::WorldId::Vanilla(
                crate::game::Dimension::Nether
            )),
            "minecraft:the_nether"
        );
        assert_eq!(
            plugin_world_name(super::super::WorldId::Vanilla(crate::game::Dimension::End)),
            "minecraft:the_end"
        );
        let runtime = Uuid::from_u128(42);
        assert_eq!(
            plugin_world_name(super::super::WorldId::Runtime(runtime)),
            runtime.to_string()
        );
    }

    /// A bridge whose every call fails, standing in for a broken JVM call.
    struct Failing;
    impl crate::game::plugin_types::ServerHooks for Failing {
        fn player_joined(&self, _: Uuid, _: &str) -> Result<(), crate::game::plugin_types::Error> {
            Err(crate::game::plugin_types::Error::InvalidTask)
        }
        fn player_left(&self, _: Uuid, _: &str) -> Result<(), crate::game::plugin_types::Error> {
            Err(crate::game::plugin_types::Error::InvalidTask)
        }
        fn chat(
            &self,
            _: Uuid,
            _: &str,
            _: &str,
        ) -> Result<Option<String>, crate::game::plugin_types::Error> {
            Err(crate::game::plugin_types::Error::InvalidTask)
        }
        fn player_moved(
            &self,
            _: Uuid,
            _: &str,
            _: &crate::game::plugin_types::PlayerMove,
        ) -> Result<bool, crate::game::plugin_types::Error> {
            Err(crate::game::plugin_types::Error::InvalidTask)
        }
        fn player_interacted(
            &self,
            _: Uuid,
            _: &str,
            _: InteractAction,
            _: crate::game::plugin_types::BlockInteract<'_>,
        ) -> Result<bool, crate::game::plugin_types::Error> {
            Err(crate::game::plugin_types::Error::InvalidTask)
        }
        fn block_broken(
            &self,
            _: Uuid,
            _: &str,
            _: crate::game::plugin_types::BlockBreak<'_>,
        ) -> Result<bool, crate::game::plugin_types::Error> {
            Err(crate::game::plugin_types::Error::InvalidTask)
        }
        fn block_placed(
            &self,
            _: Uuid,
            _: &str,
            _: BlockPosition,
            _: u8,
            _: &str,
            _: &str,
        ) -> Result<bool, crate::game::plugin_types::Error> {
            Err(crate::game::plugin_types::Error::InvalidTask)
        }
        fn player_command(
            &self,
            _: Uuid,
            _: &str,
            _: &str,
        ) -> Result<bool, crate::game::plugin_types::Error> {
            Err(crate::game::plugin_types::Error::InvalidTask)
        }
        fn plugin_commands(&self) -> Result<Vec<PluginCommand>, crate::game::plugin_types::Error> {
            Err(crate::game::plugin_types::Error::InvalidTask)
        }
        fn run_plugin_command(
            &self,
            _: CommandSender,
            _: &str,
        ) -> Result<PluginCommandResult, crate::game::plugin_types::Error> {
            Err(crate::game::plugin_types::Error::InvalidTask)
        }
    }

    fn decide(
        policy: PluginEventPolicy,
        result: Result<bool, crate::game::plugin_types::Error>,
    ) -> bool {
        let failing: &dyn crate::game::plugin_types::ServerHooks = &Failing;
        event_allows(Some(failing), policy, |_| result)
    }

    #[test]
    fn failed_plugin_events_follow_the_configured_policy() {
        assert!(decide(
            PluginEventPolicy::Allow,
            Err(crate::game::plugin_types::Error::InvalidTask)
        ));
        assert!(!decide(
            PluginEventPolicy::Deny,
            Err(crate::game::plugin_types::Error::InvalidTask)
        ));
        assert!(!decide(PluginEventPolicy::Allow, Ok(false)));
        assert!(decide(PluginEventPolicy::Deny, Ok(true)));
    }

    #[test]
    fn a_cancelled_entity_change_block_stops_the_enderman() {
        use super::super::explosion::tests::Scripted;
        let mut region = Region::default();
        let key = region.entities.insert(mob::new_mob(
            mob::MobKind::Enderman,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut region.rng,
        ));
        let hooks = Arc::new(Scripted::canceling_changes());
        let shared = Arc::new(SharedPluginHooks::default());
        shared.publish(Some(hooks), PluginEventPolicy::Allow);
        let mut ctx = super::tests::tick_context();
        ctx.hooks = Some(shared);
        assert!(
            !region.entity_change_allowed(
                &ctx,
                key,
                [0, 64, 0],
                "minecraft:grass_block",
                "minecraft:air"
            ),
            "the listener's cancellation must reach the enderman"
        );
    }

    #[test]
    fn a_missing_plugin_bridge_always_allows_the_action() {
        assert!(event_allows(None, PluginEventPolicy::Deny, |_| Err(
            crate::game::plugin_types::Error::InvalidTask
        )));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn mob_hit_in_a_playerless_region_fires_a_cancellable_damage_event() {
        use super::super::explosion::tests::Scripted;
        let source = super::super::anvil::AnvilChunkSource::new_memory();
        let mut ready = false;
        for _ in 0..2_000 {
            if let Some(result) = source.get(0, 0, 0) {
                result.expect("chunk load");
                ready = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(ready, "chunk never loaded");
        let mut region = Region::default();
        region.held_chunks.insert(ChunkPos(0, 0), 1);
        let cow = region.entities.insert(mob::new_mob(
            mob::MobKind::Cow,
            ferrite_math::Vec3::new(8.5, 66.0, 8.5),
            &mut region.rng,
        ));
        assert!(region.players.is_empty());
        let full = mob::MobKind::Cow.max_health();
        let blast = |region: &mut Region, hooks: &Arc<Scripted>| {
            let shared = Arc::new(SharedPluginHooks::default());
            shared.publish(Some(hooks.clone()), PluginEventPolicy::Allow);
            let (signals, _rx) = tokio::sync::mpsc::unbounded_channel();
            let ctx = TickContext {
                signals,
                tracking: Default::default(),
                positions: Default::default(),
                source: Some(source.clone()),
                dimension: crate::game::Dimension::Overworld,
                world_id: crate::game::WorldId::Vanilla(crate::game::Dimension::Overworld),
                raining: false,
                rules: Default::default(),
                spawn: [0.0, 65.0, 0.0],
                command_entities: Default::default(),
                state: Default::default(),
                trader_spawner: Default::default(),
                profiler: None,
                tree: None,
                hooks: Some(shared),
            };
            region.explode([8.5, 66.5, 8.5], 4.0, false, &ctx);
        };

        let cancel = Arc::new(Scripted::new(Some(0.0)));
        blast(&mut region, &cancel);
        assert_eq!(cancel.seen.lock().unwrap().len(), 1, "the event fired");
        assert_eq!(cancel.seen.lock().unwrap()[0].2, "ENTITY_EXPLOSION");
        assert_eq!(mob::state(&region.entities, cow).unwrap().health, full);

        let pass = Arc::new(Scripted::new(None));
        blast(&mut region, &pass);
        assert_eq!(pass.seen.lock().unwrap().len(), 1);
        assert!(mob::state(&region.entities, cow).unwrap().health < full);
    }

    #[test]
    fn player_derived_hooks_still_fire_exactly_once() {
        use super::super::explosion::tests::Scripted;
        let mut store = ferrite_entity::EntityStore::default();
        let key = store.insert(mob::new_mob(
            mob::MobKind::Cow,
            ferrite_math::Vec3::new(0.5, 1.0, 0.5),
            &mut ferrite_entity::Rng::default(),
        ));
        let player = Arc::new(Scripted::new(None));
        let from_players = |hooks: &Arc<Scripted>| {
            vec![(
                hooks.clone() as Arc<dyn crate::game::plugin_types::ServerHooks>,
                PluginEventPolicy::Allow,
            )]
            .into_iter()
        };
        // No shared handle: the player's bridge is used.
        let hooks = resolve_damage_hooks(None, from_players(&player)).expect("player hooks");
        super::super::explosion::hurt_mob(&mut store, key, 3.0, Some((hooks.0.as_ref(), hooks.1)));
        assert_eq!(player.seen.lock().unwrap().len(), 1);
        // A shared handle wins, and the player's bridge is not asked as well.
        let shared = SharedPluginHooks::default();
        let live = Arc::new(Scripted::new(None));
        shared.publish(Some(live.clone()), PluginEventPolicy::Allow);
        let hooks = resolve_damage_hooks(Some(&shared), from_players(&player)).expect("hooks");
        super::super::explosion::hurt_mob(&mut store, key, 3.0, Some((hooks.0.as_ref(), hooks.1)));
        assert_eq!(live.seen.lock().unwrap().len(), 1);
        assert_eq!(player.seen.lock().unwrap().len(), 1, "no second event");
        // Nothing attached anywhere: no bridge.
        assert!(
            resolve_damage_hooks(Some(&SharedPluginHooks::default()), std::iter::empty()).is_none()
        );
    }

    #[test]
    fn damage_events_cancel_or_change_the_health_loss() {
        let mut survival = Survival::default();
        assert_eq!(damage_cause(DamageKind::Fall), "FALL");
        assert_eq!(damage_cause(DamageKind::Drown), "DROWNING");
        assert_eq!(
            plugin_damage(Some(&Failing), PluginEventPolicy::Allow, 5.0, |_| Ok(0.0)),
            None
        );
        assert_eq!(survival.health, 20.0);
        let modified = plugin_damage(Some(&Failing), PluginEventPolicy::Allow, 5.0, |_| Ok(2.5))
            .expect("modified damage");
        assert!(survival.damage(DamageKind::Fall, modified));
        assert_eq!(survival.health, 17.5);
        assert_eq!(
            plugin_damage(None, PluginEventPolicy::Deny, 5.0, |_| unreachable!()),
            Some(5.0)
        );
        assert_eq!(
            plugin_damage(Some(&Failing), PluginEventPolicy::Deny, 5.0, |_| Err(
                crate::game::plugin_types::Error::InvalidTask
            )),
            None
        );
        assert_eq!(
            plugin_damage(Some(&Failing), PluginEventPolicy::Allow, 5.0, |_| {
                Err(crate::game::plugin_types::Error::InvalidTask)
            }),
            Some(5.0)
        );
    }

    #[test]
    fn spawn_eggs_map_to_the_modelled_mobs() {
        assert_eq!(
            spawn_egg_kind(Some("minecraft:cow_spawn_egg")),
            Some(MobKind::Cow)
        );
        assert_eq!(
            spawn_egg_kind(Some("minecraft:trader_llama_spawn_egg")),
            Some(MobKind::TraderLlama)
        );
        assert_eq!(
            spawn_egg_kind(Some("minecraft:mooshroom_spawn_egg")),
            Some(MobKind::Mooshroom)
        );
        // An egg exists only for the entity types the server simulates.
        assert_eq!(spawn_egg_kind(Some("minecraft:unknown_spawn_egg")), None);
        assert_eq!(spawn_egg_kind(Some("minecraft:cow")), None);
        assert_eq!(spawn_egg_kind(None), None);
    }

    #[test]
    fn autosave_entity_snapshot_copies_only_its_chunk_without_unloading() {
        let mut region = Region::default();
        let first = region.entities.insert(mob::new_mob(
            MobKind::Cow,
            ferrite_math::Vec3::new(0.5, 64.0, 0.5),
            &mut region.rng,
        ));
        region.entities.insert(mob::new_mob(
            MobKind::Pig,
            ferrite_math::Vec3::new(16.5, 64.0, 0.5),
            &mut region.rng,
        ));

        let snapshot = region.snapshot_chunk_entities(ChunkPos(0, 0));

        assert_eq!(snapshot.len(), 1);
        assert_eq!(
            snapshot[0].data.uuid,
            region.entities.data(first).unwrap().uuid
        );
        assert_eq!(
            region.entities.len(),
            2,
            "autosave keeps live entities loaded"
        );
    }

    #[test]
    fn a_chunk_stays_held_until_the_last_viewer_releases_it() {
        let mut region = Region::default();
        let chunk = ChunkPos(2, 3);
        region.hold_chunk(chunk);
        region.hold_chunk(chunk);
        assert_eq!(region.held_chunks.get(&chunk), Some(&2));
        region.release_chunk(chunk);
        assert!(region.has_chunk_hold(chunk), "one viewer still sees it");
        region.release_chunk(chunk);
        assert!(!region.has_chunk_hold(chunk));
    }

    #[test]
    fn the_view_window_is_the_vanilla_square_and_the_trailing_edge_goes_at_once() {
        // `PlayerTicketTracker.haveTicketFor`: Chebyshev distance <= view distance.
        let (forget, new) = chunk_window((0, 0), 4, &HashSet::new());
        assert!(forget.is_empty());
        assert_eq!(new.len(), 81, "(2 * 4 + 1)^2 chunks");
        assert_eq!(
            new[0],
            (0, 0),
            "nearest first, as the vanilla sender orders them"
        );
        let loaded: HashSet<(i32, i32)> = new.into_iter().collect();

        // One chunk east: the west column leaves the square and the east column joins it, in
        // the same update (`processUnloads` has no grace period).
        let (mut forget, mut new) = chunk_window((1, 0), 4, &loaded);
        forget.sort_unstable();
        new.sort_unstable();
        assert_eq!(forget, (-4..=4).map(|z| (-4, z)).collect::<Vec<_>>());
        assert_eq!(new, (-4..=4).map(|z| (5, z)).collect::<Vec<_>>());

        // A far teleport forgets everything that was loaded in one go.
        let (forget, new) = chunk_window((100, 100), 4, &loaded);
        assert_eq!(forget.len(), 81);
        assert_eq!(new.len(), 81);
    }

    #[test]
    fn releasing_every_hold_drops_the_chunk_in_the_same_call() {
        let mut region = Region::default();
        let chunks: Vec<_> = (0..4).map(|x| ChunkPos(x, 0)).collect();
        for chunk in &chunks {
            region.hold_chunk(*chunk);
        }
        for chunk in &chunks {
            region.release_chunk(*chunk);
            assert!(
                !region.has_chunk_hold(*chunk),
                "no grace period for {chunk:?}"
            );
        }
        // Releasing a chunk nobody holds neither panics nor leaves an entry behind.
        region.release_chunk(ChunkPos(9, 9));
        assert!(region.held_chunk_counts().is_empty());
    }

    #[test]
    fn block_materials_come_from_the_state_table() {
        let stone = ferrite_data::blocks::default_state("stone").unwrap();
        let grass = ferrite_data::blocks::default_state("grass_block").unwrap();
        assert_eq!(material_name(u32::from(stone)), "minecraft:stone");
        assert_eq!(material_name(u32::from(grass)), "minecraft:grass_block");
        assert_eq!(material_name(u32::MAX), "minecraft:air");
    }

    /// A plugin attaches after the loop started and rebuilds the command tree; command blocks
    /// must see the rebuilt tree, not the one captured when the loop began.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn command_block_runs_a_plugin_command_registered_after_startup() {
        use super::super::command_block::Runner;
        let source = Arc::new(super::super::anvil::AnvilChunkSource::new_memory());
        let tracking: Tracking = Default::default();
        let state = Arc::new(Mutex::new(super::super::commands::GameState::new(0, 0, 10)));
        let startup_tree = Arc::new(super::super::commands::dispatcher());
        let shared = SharedCommandTree::new(Arc::clone(&startup_tree));
        let run = |shared: &SharedCommandTree| {
            BlockCommands {
                source: &source,
                tracking: &tracking,
                state: &state,
                dimension: super::super::Dimension::Overworld,
                tree: shared,
            }
            .run([0, 64, 0], "/fixtureping hello world")
        };
        let before = run(&shared);
        assert!(
            before.is_err(),
            "the plugin command does not exist yet: {before:?}"
        );
        let mut rebuilt = super::super::commands::dispatcher();
        super::super::commands::register_plugin_commands(
            &mut rebuilt,
            &[crate::game::plugin_types::PluginCommand {
                name: "fixtureping".to_owned(),
                aliases: Vec::new(),
                permission: None,
                usage: None,
            }],
        );
        shared.publish(Arc::new(rebuilt));
        let after = run(&shared);
        assert!(
            after.is_ok(),
            "command blocks must see the tree published after startup: {after:?}"
        );
    }
}

#[cfg(test)]
mod redstone_inputs_tests {
    use super::*;
    use ferrite_protocol::packets::play::{ItemStack as WireStack, MetaValue};

    fn frame(
        store: &mut ferrite_entity::EntityStore,
        at: [f64; 3],
        spawn_data: i32,
        rotation: i32,
        filled: bool,
    ) {
        let kind = ferrite_data::entity_props::by_name("minecraft:item_frame")
            .unwrap()
            .id;
        let mut data = ferrite_entity::EntityData::new(
            ferrite_entity::allocate_id(),
            uuid::Uuid::new_v4(),
            kind,
            0.5,
            0.5,
        );
        data.spawn_data = spawn_data;
        data.metadata.define(
            8,
            MetaValue::ItemStack(WireStack {
                count: i32::from(filled),
                item_id: 1,
            }),
        );
        data.metadata.define(9, MetaValue::Int(rotation));
        store.insert(ferrite_entity::EntityRecord {
            pos: ferrite_math::Vec3::new(at[0], at[1], at[2]),
            vel: ferrite_math::Vec3::ZERO,
            flags: Default::default(),
            data,
        });
    }

    /// `ItemFrame.getAnalogOutput`: empty is 0, otherwise `rotation % 8 + 1`.
    #[test]
    fn item_frames_output_rotation_plus_one_and_zero_when_empty() {
        let mut store = ferrite_entity::EntityStore::new(2);
        frame(&mut store, [0.5, 64.5, 0.5], 4, 3, true);
        frame(&mut store, [4.5, 64.5, 0.5], 5, 11, true);
        frame(&mut store, [8.5, 64.5, 0.5], 2, 5, false);
        let mut outputs = item_frame_outputs(&store);
        outputs.sort_by_key(|(pos, _, _)| pos[0]);
        assert_eq!(
            outputs,
            vec![
                ([0, 64, 0], ferrite_math::Direction::West, 4),
                ([4, 64, 0], ferrite_math::Direction::East, 4),
                ([8, 64, 0], ferrite_math::Direction::North, 0),
            ]
        );
    }

    /// `ContainerOpenersCounter`: every viewer counts once on each half of a double chest, and
    /// only trapped chests report.
    #[test]
    fn trapped_chest_viewers_count_each_open_menu_on_both_halves() {
        use super::super::block_container::Opened;
        use super::super::container::{Containers, OpenAt};
        let open = |block: &'static str| {
            let mut menus = Containers::default();
            menus.open_at = Some(OpenAt::Inventory(Opened {
                parts: vec![[0, 64, 0], [1, 64, 0]],
                block,
                rows: 6,
                title: "container.chestDouble",
            }));
            menus
        };
        let viewers = [
            open("minecraft:trapped_chest"),
            open("minecraft:trapped_chest"),
            open("minecraft:chest"),
        ];
        let counts = trapped_chest_viewers(viewers.iter());
        assert_eq!(counts[&[0, 64, 0]], 2);
        assert_eq!(counts[&[1, 64, 0]], 2);
        assert_eq!(counts.len(), 2);
    }

    #[test]
    fn bucket_max_stack_sizes_and_milk_remainder_match_vanilla() {
        let bucket = ferrite_item::ItemStack::from_name("minecraft:bucket").unwrap();
        assert_eq!(bucket.max_stack_size(), 16);
        for filled in [
            "minecraft:water_bucket",
            "minecraft:lava_bucket",
            "minecraft:milk_bucket",
            "minecraft:powder_snow_bucket",
        ] {
            let stack = ferrite_item::ItemStack::from_name(filled).unwrap();
            assert_eq!(stack.max_stack_size(), 1, "{filled} should stack to 1");
        }
        let milk = ferrite_item::ItemStack::from_name("minecraft:milk_bucket").unwrap();
        assert_eq!(
            milk.effective_component(ferrite_item::component_ids::USE_REMAINDER),
            Some(ferrite_item::Component::UseRemainder(Box::new(bucket)))
        );
    }
}

#[cfg(test)]
mod sapling_growth_tests {
    use super::*;
    use crate::game::{
        ChunkGenerator, Dimension, Generated, Placed, anvil::AnvilChunkSource,
        generator::TreeContext,
    };
    use ferrite_protocol::Decode;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct FeatureHost {
        calls: AtomicUsize,
        fail_all: AtomicBool,
        seen_features: Mutex<Vec<String>>,
        registry: Arc<ferrite_world::RuntimeRegistry>,
    }

    impl FeatureHost {
        fn new(registry: Arc<ferrite_world::RuntimeRegistry>) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                fail_all: AtomicBool::new(false),
                seen_features: Mutex::new(Vec::new()),
                registry,
            }
        }
    }

    fn state_u16(name: &str) -> u16 {
        ferrite_data::blocks::default_state(name).expect(name)
    }

    fn state_props(name: &str, props: &[(&str, &str)]) -> u16 {
        ferrite_data::blocks::state_id(name, props).expect(name)
    }

    impl ChunkGenerator for FeatureHost {
        fn generate(&self, dimension: Dimension, _x: i32, _z: i32) -> anyhow::Result<Generated> {
            let chunk = super::super::anvil::flat_chunk(&self.registry, 0, dimension)?;
            let light = ferrite_world::LightData {
                sky: vec![ferrite_world::LightSection::Empty; dimension.light_sections()],
                block: vec![ferrite_world::LightSection::Empty; dimension.light_sections()],
            };
            Ok(Generated { chunk, light })
        }

        fn spawn_height(&self, _dimension: Dimension, _x: i32, _z: i32) -> anyhow::Result<i32> {
            Ok(65)
        }

        fn locate_biome(
            &self,
            _dimension: Dimension,
            _target: &str,
            _x: i32,
            _z: i32,
            _radius: i32,
        ) -> anyhow::Result<Option<(i32, i32, i32)>> {
            Ok(None)
        }

        fn place(&self, _dimension: Dimension, argument: &str) -> anyhow::Result<Placed> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if self.fail_all.load(Ordering::Relaxed) {
                anyhow::bail!("nothing changed");
            }
            let parts: Vec<&str> = argument.split(';').collect();
            let feature = parts[1];
            let x: i32 = parts[2].parse()?;
            let y: i32 = parts[3].parse()?;
            let z: i32 = parts[4].parse()?;
            self.seen_features.lock().unwrap().push(feature.to_owned());

            let (log, leaves, two_by_two, beehive) = match feature {
                "minecraft:oak" | "minecraft:fancy_oak" => (
                    state_u16("oak_log"),
                    state_props(
                        "oak_leaves",
                        &[
                            ("distance", "1"),
                            ("persistent", "false"),
                            ("waterlogged", "false"),
                        ],
                    ),
                    false,
                    false,
                ),
                "minecraft:oak_bees_005" | "minecraft:fancy_oak_bees_005" => (
                    state_u16("oak_log"),
                    state_props(
                        "oak_leaves",
                        &[
                            ("distance", "1"),
                            ("persistent", "false"),
                            ("waterlogged", "false"),
                        ],
                    ),
                    false,
                    true,
                ),
                "minecraft:spruce" => (
                    state_u16("spruce_log"),
                    state_props(
                        "spruce_leaves",
                        &[
                            ("distance", "1"),
                            ("persistent", "false"),
                            ("waterlogged", "false"),
                        ],
                    ),
                    false,
                    false,
                ),
                "minecraft:mega_spruce" | "minecraft:mega_pine" => (
                    state_u16("spruce_log"),
                    state_props(
                        "spruce_leaves",
                        &[
                            ("distance", "1"),
                            ("persistent", "false"),
                            ("waterlogged", "false"),
                        ],
                    ),
                    true,
                    false,
                ),
                "minecraft:birch" => (
                    state_u16("birch_log"),
                    state_props(
                        "birch_leaves",
                        &[
                            ("distance", "1"),
                            ("persistent", "false"),
                            ("waterlogged", "false"),
                        ],
                    ),
                    false,
                    false,
                ),
                "minecraft:birch_bees_005" => (
                    state_u16("birch_log"),
                    state_props(
                        "birch_leaves",
                        &[
                            ("distance", "1"),
                            ("persistent", "false"),
                            ("waterlogged", "false"),
                        ],
                    ),
                    false,
                    true,
                ),
                "minecraft:jungle_tree_no_vine" => (
                    state_u16("jungle_log"),
                    state_props(
                        "jungle_leaves",
                        &[
                            ("distance", "1"),
                            ("persistent", "false"),
                            ("waterlogged", "false"),
                        ],
                    ),
                    false,
                    false,
                ),
                "minecraft:mega_jungle_tree" => (
                    state_u16("jungle_log"),
                    state_props(
                        "jungle_leaves",
                        &[
                            ("distance", "1"),
                            ("persistent", "false"),
                            ("waterlogged", "false"),
                        ],
                    ),
                    true,
                    false,
                ),
                "minecraft:acacia" => (
                    state_u16("acacia_log"),
                    state_props(
                        "acacia_leaves",
                        &[
                            ("distance", "1"),
                            ("persistent", "false"),
                            ("waterlogged", "false"),
                        ],
                    ),
                    false,
                    false,
                ),
                "minecraft:dark_oak" => (
                    state_u16("dark_oak_log"),
                    state_props(
                        "dark_oak_leaves",
                        &[
                            ("distance", "1"),
                            ("persistent", "false"),
                            ("waterlogged", "false"),
                        ],
                    ),
                    true,
                    false,
                ),
                "minecraft:cherry" => (
                    state_u16("cherry_log"),
                    state_props(
                        "cherry_leaves",
                        &[
                            ("distance", "1"),
                            ("persistent", "false"),
                            ("waterlogged", "false"),
                        ],
                    ),
                    false,
                    false,
                ),
                "minecraft:cherry_bees_005" => (
                    state_u16("cherry_log"),
                    state_props(
                        "cherry_leaves",
                        &[
                            ("distance", "1"),
                            ("persistent", "false"),
                            ("waterlogged", "false"),
                        ],
                    ),
                    false,
                    true,
                ),
                "minecraft:pale_oak_bonemeal" | "minecraft:pale_oak" => (
                    state_u16("pale_oak_log"),
                    state_props(
                        "pale_oak_leaves",
                        &[
                            ("distance", "1"),
                            ("persistent", "false"),
                            ("waterlogged", "false"),
                        ],
                    ),
                    true,
                    false,
                ),
                "minecraft:mangrove" | "minecraft:tall_mangrove" => (
                    state_u16("mangrove_log"),
                    state_props(
                        "mangrove_leaves",
                        &[
                            ("distance", "1"),
                            ("persistent", "false"),
                            ("waterlogged", "false"),
                        ],
                    ),
                    false,
                    false,
                ),
                "minecraft:orange_poplar" => (
                    state_u16("poplar_log"),
                    state_props(
                        "orange_poplar_leaves",
                        &[
                            ("distance", "1"),
                            ("persistent", "false"),
                            ("waterlogged", "false"),
                        ],
                    ),
                    false,
                    false,
                ),
                "minecraft:red_poplar" => (
                    state_u16("poplar_log"),
                    state_props(
                        "red_poplar_leaves",
                        &[
                            ("distance", "1"),
                            ("persistent", "false"),
                            ("waterlogged", "false"),
                        ],
                    ),
                    false,
                    false,
                ),
                "minecraft:yellow_poplar" => (
                    state_u16("poplar_log"),
                    state_props(
                        "yellow_poplar_leaves",
                        &[
                            ("distance", "1"),
                            ("persistent", "false"),
                            ("waterlogged", "false"),
                        ],
                    ),
                    false,
                    false,
                ),
                other => anyhow::bail!("unknown feature {other}"),
            };

            let mut blocks = vec![([x, y - 1, z], state_u16("dirt"))];
            if two_by_two {
                // Deliberately omit the SE corner log at (x+1, y, z+1) on y to exercise
                // clearing remaining 2x2 sapling cells when a trunk bends or skips a corner.
                for (dx, dz) in [(0, 0), (1, 0), (0, 1)] {
                    blocks.push(([x + dx, y, z + dz], log));
                }
                for dy in 1..=5 {
                    for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        blocks.push(([x + dx, y + dy, z + dz], log));
                    }
                }
                for dx in -1..=2 {
                    for dz in -1..=2 {
                        if (dx, dz) != (0, 0) {
                            blocks.push(([x + dx, y + 5, z + dz], leaves));
                        }
                        blocks.push(([x + dx, y + 6, z + dz], leaves));
                    }
                }
            } else {
                for dy in 0..=4 {
                    blocks.push(([x, y + dy, z], log));
                }
                for dx in -1..=1 {
                    for dz in -1..=1 {
                        if (dx, dz) != (0, 0) {
                            blocks.push(([x + dx, y + 3, z + dz], leaves));
                            blocks.push(([x + dx, y + 4, z + dz], leaves));
                        }
                        blocks.push(([x + dx, y + 5, z + dz], leaves));
                    }
                }
            }
            if matches!(feature, "minecraft:mangrove" | "minecraft:tall_mangrove") {
                blocks.push(([x + 1, y, z], state_u16("mangrove_roots")));
                blocks.push((
                    [x + 1, y + 2, z],
                    state_props(
                        "mangrove_propagule",
                        &[
                            ("age", "0"),
                            ("hanging", "true"),
                            ("stage", "0"),
                            ("waterlogged", "false"),
                        ],
                    ),
                ));
            }
            let mut entities = Vec::new();
            if beehive {
                let hive_pos = [x + 1, y + 2, z];
                blocks.push((hive_pos, state_u16("beehive")));
                let mut nbt = ferrite_nbt::Compound::new();
                nbt.insert(
                    "id".into(),
                    ferrite_nbt::Tag::String("minecraft:beehive".into()),
                );
                entities.push((hive_pos, nbt));
            }
            Ok(Placed { blocks, entities })
        }

        fn place_tree(
            &self,
            dimension: Dimension,
            feature: &str,
            pos: [i32; 3],
            _context: Option<&TreeContext>,
        ) -> anyhow::Result<Placed> {
            self.place(
                dimension,
                &format!("feature;{feature};{};{};{}", pos[0], pos[1], pos[2]),
            )
        }
    }

    async fn loaded_with_host(chunks: &[(i32, i32)]) -> (Arc<FeatureHost>, Arc<AnvilChunkSource>) {
        let base = AnvilChunkSource::new_memory();
        let host = Arc::new(FeatureHost::new(base.registry()));
        let source = AnvilChunkSource::new_memory_with_terrain(Some(host.clone()));
        for &(x, z) in chunks {
            loop {
                if let Some(result) = source.get(x, z, 0) {
                    result.unwrap();
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
        (host, source)
    }

    async fn loaded_flat(chunks: &[(i32, i32)]) -> Arc<AnvilChunkSource> {
        let source = AnvilChunkSource::new_memory();
        for &(x, z) in chunks {
            loop {
                if let Some(result) = source.get(x, z, 0) {
                    result.unwrap();
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
        source
    }

    fn stage1_sapling(name: &str) -> u32 {
        if name == "mangrove_propagule" {
            u32::from(state_props(
                name,
                &[
                    ("age", "0"),
                    ("hanging", "false"),
                    ("stage", "1"),
                    ("waterlogged", "false"),
                ],
            ))
        } else {
            u32::from(state_props(name, &[("stage", "1")]))
        }
    }

    #[tokio::test]
    async fn stage_one_saplings_grow_all_supported_species_through_worldgen_boundary() {
        let (host, source) = loaded_with_host(&[(0, 0)]).await;
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));

        // 1x1 species + flower next to oak so beehive entity path is exercised too.
        let air = u32::from(state_u16("air"));
        let grass = u32::from(state_u16("grass_block"));
        for (idx, (species, expected_log)) in [
            ("oak_sapling", "oak_log"),
            ("spruce_sapling", "spruce_log"),
            ("birch_sapling", "birch_log"),
            ("jungle_sapling", "jungle_log"),
            ("acacia_sapling", "acacia_log"),
            ("cherry_sapling", "cherry_log"),
            ("mangrove_propagule", "mangrove_log"),
            ("poplar_sapling", "poplar_log"),
        ]
        .into_iter()
        .enumerate()
        {
            for x in 1..=14 {
                for z in 1..=14 {
                    source.set_block(x, 64, z, grass).unwrap();
                    for y in 65..=75 {
                        source.set_block(x, y, z, air).unwrap();
                    }
                }
            }
            if species == "oak_sapling" {
                source
                    .set_block(9, 65, 8, u32::from(state_u16("poppy")))
                    .unwrap();
            }
            let sapling = stage1_sapling(species);
            source.set_block(8, 65, 8, sapling).unwrap();

            let (signals, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let mut grow_req = None;
            for _ in 0..40 {
                region.tick_random(&source, &tracking, signals.clone(), 4096, true, 0.0);
                while let Ok(sig) = rx.try_recv() {
                    if let Signal::GrowSapling {
                        dimension,
                        origin,
                        saplings,
                        feature,
                        context,
                        ..
                    } = sig
                    {
                        grow_req = Some((dimension, origin, saplings, feature, context));
                    }
                }
                if grow_req.is_some() {
                    break;
                }
            }
            let (dim, origin, saplings, feature, context) = grow_req
                .unwrap_or_else(|| panic!("{species} did not emit GrowSapling (idx {idx})"));
            assert!(
                region.random_ticks.pending_trees.contains(&origin),
                "in-flight origin must be tracked to avoid duplicate host calls"
            );
            // Ticking again while pending must not emit a duplicate request.
            region.tick_random(&source, &tracking, signals.clone(), 4096, true, 0.0);
            assert!(
                rx.try_recv().is_err(),
                "pending sapling must not emit duplicate GrowSapling"
            );

            let placed = host.place_tree(dim, feature, origin, Some(&context));
            region.apply_tree(
                &source,
                &tracking,
                &signals,
                origin,
                &saplings,
                placed.map_err(|e| e.to_string()),
            );
            assert!(!region.random_ticks.pending_trees.contains(&origin));
            assert_eq!(
                source.block(8, 65, 8),
                Some(u32::from(state_u16(expected_log))),
                "{species} did not place {expected_log}"
            );
            if species == "mangrove_propagule" {
                // Hanging propagule placed under leaves must survive updates.apply.
                assert_eq!(
                    ferrite_data::blocks::state(source.block(9, 67, 8).unwrap() as u16)
                        .map(|(n, _)| n),
                    Some("minecraft:mangrove_propagule")
                );
            }
        }

        // 2x2 species: dark_oak, pale_oak, mega spruce, mega jungle.
        for (species, expected_log) in [
            ("dark_oak_sapling", "dark_oak_log"),
            ("pale_oak_sapling", "pale_oak_log"),
            ("spruce_sapling", "spruce_log"),
            ("jungle_sapling", "jungle_log"),
        ] {
            for x in 1..=14 {
                for z in 1..=14 {
                    source.set_block(x, 64, z, grass).unwrap();
                    for y in 65..=75 {
                        source.set_block(x, y, z, air).unwrap();
                    }
                }
            }
            let sapling = stage1_sapling(species);
            for (x, z) in [(6, 6), (7, 6), (6, 7), (7, 7)] {
                source.set_block(x, 65, z, sapling).unwrap();
            }
            let (signals, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let mut grow_req = None;
            for _ in 0..40 {
                region.tick_random(&source, &tracking, signals.clone(), 4096, true, 0.0);
                while let Ok(sig) = rx.try_recv() {
                    if let Signal::GrowSapling {
                        dimension,
                        origin,
                        saplings,
                        feature,
                        context,
                        ..
                    } = sig
                    {
                        grow_req = Some((dimension, origin, saplings, feature, context));
                    }
                }
                if grow_req.is_some() {
                    break;
                }
            }
            let (dim, origin, saplings, feature, context) =
                grow_req.unwrap_or_else(|| panic!("2x2 {species} did not emit GrowSapling"));
            assert_eq!(origin, [6, 65, 6]);
            assert_eq!(saplings.len(), 4);
            let placed = host.place_tree(dim, feature, origin, Some(&context));
            region.apply_tree(
                &source,
                &tracking,
                &signals,
                origin,
                &saplings,
                placed.map_err(|e| e.to_string()),
            );
            assert_eq!(
                source.block(6, 65, 6),
                Some(u32::from(state_u16(expected_log)))
            );
            // Un-overwritten SE corner sapling (7, 65, 7) must be cleared to air.
            assert_eq!(source.block(7, 65, 7), Some(air));
        }

        // Without terrain (flat memory source), stage-1 saplings stay stage 1 and never place
        // hand-built trees.
        let flat_source = loaded_flat(&[(0, 0)]).await;
        flat_source
            .set_block(8, 65, 8, stage1_sapling("oak_sapling"))
            .unwrap();
        let mut flat_region = Region::default();
        flat_region.hold_chunk(ChunkPos(0, 0));
        let (signals, mut rx) = tokio::sync::mpsc::unbounded_channel();
        for _ in 0..20 {
            flat_region.tick_random(&flat_source, &tracking, signals.clone(), 4096, true, 0.0);
        }
        assert!(rx.try_recv().is_err());
        assert_eq!(
            flat_source.block(8, 65, 8),
            Some(stage1_sapling("oak_sapling"))
        );
    }

    #[tokio::test]
    async fn sapling_growth_fails_cleanly_on_insufficient_space_or_host_failure() {
        let (host, source) = loaded_with_host(&[(0, 0)]).await;
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let (signals, _rx) = tokio::sync::mpsc::unbounded_channel();

        let oak = stage1_sapling("oak_sapling");
        let stone = u32::from(state_u16("stone"));
        let air = u32::from(state_u16("air"));
        source.set_block(8, 65, 8, oak).unwrap();
        // Stone ceiling directly in the trunk path blocks growth.
        source.set_block(8, 68, 8, stone).unwrap();
        let placed = host
            .place_tree(Dimension::Overworld, "minecraft:oak", [8, 65, 8], None)
            .unwrap();
        region.random_ticks.pending_trees.insert([8, 65, 8]);
        region.apply_tree(
            &source,
            &tracking,
            &signals,
            [8, 65, 8],
            &[([8, 65, 8], oak)],
            Ok(placed.clone()),
        );
        assert_eq!(
            source.block(8, 65, 8),
            Some(oak),
            "stage-1 sapling stays after insufficient space"
        );
        assert_eq!(source.block(8, 68, 8), Some(stone));
        assert!(!region.random_ticks.pending_trees.contains(&[8, 65, 8]));

        // Host failure ("nothing changed") also leaves stage-1 sapling untouched.
        source.set_block(8, 68, 8, air).unwrap();
        region.random_ticks.pending_trees.insert([8, 65, 8]);
        region.apply_tree(
            &source,
            &tracking,
            &signals,
            [8, 65, 8],
            &[([8, 65, 8], oak)],
            Err("nothing changed".into()),
        );
        assert_eq!(source.block(8, 65, 8), Some(oak));
        assert!(!region.random_ticks.pending_trees.contains(&[8, 65, 8]));

        // If the sapling was broken before the host finished, apply_tree aborts.
        source.set_block(8, 65, 8, air).unwrap();
        region.apply_tree(
            &source,
            &tracking,
            &signals,
            [8, 65, 8],
            &[([8, 65, 8], oak)],
            Ok(placed.clone()),
        );
        assert_eq!(source.block(8, 65, 8), Some(air));

        // Non-replaceable stone wall at canopy edge is preserved while tree grows.
        source.set_block(8, 65, 8, oak).unwrap();
        source.set_block(9, 68, 8, stone).unwrap();
        region.apply_tree(
            &source,
            &tracking,
            &signals,
            [8, 65, 8],
            &[([8, 65, 8], oak)],
            Ok(placed),
        );
        assert_eq!(
            source.block(8, 65, 8),
            Some(u32::from(state_u16("oak_log")))
        );
        assert_eq!(source.block(9, 68, 8), Some(stone));
    }

    #[tokio::test]
    async fn tree_growth_broadcasts_block_updates_and_cascades_neighbor_and_fluid_updates() {
        let (host, source) = loaded_with_host(&[(0, 0), (1, 0)]).await;
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let (out_tx, mut out_rx) = OutboundSender::channel(64 * 1024);
        tracking.lock().unwrap().insert(
            Uuid::new_v4(),
            (
                crate::game::WorldId::Vanilla(Dimension::Overworld),
                HashSet::from([(0, 0)]),
                out_tx,
            ),
        );

        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let (signals, mut rx) = tokio::sync::mpsc::unbounded_channel();

        // Sapling on the chunk seam at x = 15.
        let oak = stage1_sapling("oak_sapling");
        source.set_block(15, 65, 8, oak).unwrap();
        let placed = host
            .place_tree(
                Dimension::Overworld,
                "minecraft:oak_bees_005",
                [15, 65, 8],
                None,
            )
            .unwrap();
        region.apply_tree(
            &source,
            &tracking,
            &signals,
            [15, 65, 8],
            &[([15, 65, 8], oak)],
            Ok(placed),
        );

        // Tracked viewer on chunk (0, 0) received BlockUpdate packets for placed logs/leaves.
        let mut updated_positions = Vec::new();
        while let Some(packet) = out_rx.try_recv() {
            if let Some(bytes) = packet.raw_bytes() {
                let mut slice = bytes;
                if let Ok(id) = VarInt::decode(&mut slice)
                    && id.0 == BlockUpdate::ID
                    && let Ok(update) = BlockUpdate::decode(&mut slice)
                {
                    let pos = ferrite_protocol::types::BlockPos::unpack(update.location);
                    updated_positions.push([pos.x, pos.y, pos.z]);
                }
            }
        }
        assert!(updated_positions.contains(&[15, 65, 8]));
        assert!(updated_positions.contains(&[15, 69, 8]));

        // FluidChange, RedstoneChange, NeighborUpdate, and TreePlaceChunk (for foreign chunk 1,0) emitted.
        let mut saw_fluid = false;
        let mut saw_redstone = false;
        let mut saw_neighbor = false;
        let mut saw_foreign_tree = false;
        while let Ok(sig) = rx.try_recv() {
            match sig {
                Signal::FluidChange { x, y, z, .. } if (x, y, z) == (15, 65, 8) => {
                    saw_fluid = true;
                }
                Signal::RedstoneChange { .. } => saw_redstone = true,
                Signal::NeighborUpdate { origin, target, .. }
                    if origin[0] == 15 && target[0] == 16 =>
                {
                    saw_neighbor = true;
                }
                Signal::TreePlaceChunk {
                    chunk: ChunkPos(1, 0),
                    ..
                } => {
                    saw_foreign_tree = true;
                }
                _ => {}
            }
        }
        assert!(saw_fluid);
        assert!(saw_redstone);
        assert!(saw_neighbor);
        assert!(saw_foreign_tree);
    }

    #[tokio::test]
    async fn cross_region_tree_growth_hands_foreign_chunks_to_owner() {
        let (host, source) = loaded_with_host(&[(0, 0), (1, 0)]).await;
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let (east_tx, mut east_rx) = OutboundSender::channel(64 * 1024);
        tracking.lock().unwrap().insert(
            Uuid::new_v4(),
            (
                crate::game::WorldId::Vanilla(Dimension::Overworld),
                HashSet::from([(1, 0)]),
                east_tx,
            ),
        );

        let oak = stage1_sapling("oak_sapling");
        source.set_block(15, 65, 8, oak).unwrap();
        let east_before = source.block(16, 68, 8).unwrap();

        let mut west = Region::default();
        west.hold_chunk(ChunkPos(0, 0));
        let (signals, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let placed = host
            .place_tree(
                Dimension::Overworld,
                "minecraft:oak_bees_005",
                [15, 65, 8],
                None,
            )
            .unwrap();
        west.apply_tree(
            &source,
            &tracking,
            &signals,
            [15, 65, 8],
            &[([15, 65, 8], oak)],
            Ok(placed),
        );

        // West region never writes to East's chunk directly.
        assert_eq!(
            source.block(15, 65, 8),
            Some(u32::from(state_u16("oak_log")))
        );
        assert_eq!(source.block(16, 68, 8), Some(east_before));

        let mut handed_off = None;
        while let Ok(sig) = rx.try_recv() {
            if let Signal::TreePlaceChunk {
                chunk,
                blocks,
                entities,
                ..
            } = sig
            {
                assert_eq!(chunk, ChunkPos(1, 0));
                handed_off = Some((blocks, entities));
            }
        }
        let (blocks, entities) = handed_off.expect("foreign chunk blocks handed to owner");
        assert!(blocks.iter().any(|(pos, _, _)| *pos == [16, 68, 8]));
        assert_eq!(entities.len(), 1, "beehive at [16, 67, 8] travels to owner");

        let mut east = Region::default();
        east.hold_chunk(ChunkPos(1, 0));
        east.apply_tree_chunk(
            &source,
            &tracking,
            &signals,
            ChunkPos(1, 0),
            &blocks,
            &entities,
        );
        assert_eq!(
            source.block(16, 68, 8),
            Some(u32::from(state_props(
                "oak_leaves",
                &[
                    ("distance", "1"),
                    ("persistent", "false"),
                    ("waterlogged", "false"),
                ]
            )))
        );
        assert_eq!(
            source.block(16, 67, 8),
            Some(u32::from(state_u16("beehive")))
        );
        assert!(
            source.block_entity(16, 67, 8).is_some(),
            "beehive block entity persisted on owning region"
        );
        assert!(
            east_rx.try_recv().is_some(),
            "East viewer received BlockUpdate for handed-off chunk"
        );
    }

    #[tokio::test]
    async fn failed_2x2_tree_growth_clears_all_four_saplings() {
        let (_host, source) = loaded_with_host(&[(0, 0)]).await;
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let (signals, _rx) = tokio::sync::mpsc::unbounded_channel();

        let sapling = stage1_sapling("dark_oak_sapling");
        let cells = [(8, 8), (9, 8), (8, 9), (9, 9)];
        let saplings: Vec<_> = cells
            .iter()
            .map(|&(x, z)| {
                source.set_block(x, 65, z, sapling).unwrap();
                ([x, 65, z], sapling)
            })
            .collect();
        region.random_ticks.pending_trees.insert([8, 65, 8]);
        region.apply_tree(
            &source,
            &tracking,
            &signals,
            [8, 65, 8],
            &saplings,
            Err("nothing changed".into()),
        );
        assert!(!region.random_ticks.pending_trees.contains(&[8, 65, 8]));
        for (x, z) in cells {
            assert_eq!(
                source.block(x, 65, z),
                Some(u32::from(state_u16("air"))),
                "a failed 2x2 attempt leaves ({x}, 65, {z}) cleared like vanilla"
            );
        }
    }

    #[test]
    fn pending_trees_follow_region_merge_and_split() {
        let mut left = Region::default();
        let mut right = Region::default();
        left.hold_chunk(ChunkPos(0, 0));
        left.random_ticks.pending_trees.insert([4, 65, 4]);
        right.hold_chunk(ChunkPos(32, 0));
        right.random_ticks.pending_trees.insert([516, 65, 4]);
        left.merge(right);
        assert!(left.random_ticks.pending_trees.contains(&[4, 65, 4]));
        assert!(left.random_ticks.pending_trees.contains(&[516, 65, 4]));

        let split = left.split(&HashSet::from([SectionPos(2, 0)]));
        assert!(left.random_ticks.pending_trees.contains(&[4, 65, 4]));
        assert!(!left.random_ticks.pending_trees.contains(&[516, 65, 4]));
        assert!(split.random_ticks.pending_trees.contains(&[516, 65, 4]));
        assert!(!split.random_ticks.pending_trees.contains(&[4, 65, 4]));
    }

    #[tokio::test]
    async fn world_grows_sapling_across_chunk_boundary_end_to_end() {
        let (_host, source) = loaded_with_host(&[(0, 0), (1, 0)]).await;
        let oak = stage1_sapling("oak_sapling");
        source.set_block(15, 65, 8, oak).unwrap();

        let world = crate::game::World::start_with(Some(source.clone()), [15, 65, 8]);
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let _ = world.sender.send(crate::game::GameMessage::Console {
            command: "forceload add 0 0 16 0".into(),
            complete: done_tx,
        });
        let _ = done_rx.await;
        world.set_random_tick_speed(512);

        let oak_log = u32::from(state_u16("oak_log"));
        let oak_leaves = u32::from(state_props(
            "oak_leaves",
            &[
                ("distance", "1"),
                ("persistent", "false"),
                ("waterlogged", "false"),
            ],
        ));
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if source.block(15, 65, 8) == Some(oak_log)
                && source.block(16, 68, 8) == Some(oak_leaves)
            {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                panic!(
                    "sapling did not grow through world loop: sapling={:?} leaf={:?}",
                    source.block(15, 65, 8),
                    source.block(16, 68, 8)
                );
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        world.shutdown().await.unwrap();
    }

    #[test]
    fn frog_eats_tiny_magma_cube_for_froglight_and_large_slime_splits_on_death() {
        let ctx = super::tests::tick_context();
        let mut region = Region::default();
        region.hold_chunk(ChunkPos(0, 0));
        let mut frog = mob::new_mob(
            MobKind::Frog,
            ferrite_math::Vec3::new(8.5, 64.0, 8.5),
            &mut region.rng,
        );
        if let Some(s) = frog.data.mob.as_mut() {
            s.variant = Some(0); // cold -> verdant_froglight
            s.long_jump_cooldown = 100;
        }
        mob::sync_state_metadata(&mut frog.data);
        region.entities.insert(frog);

        let mut tiny_cube = mob::new_mob(
            MobKind::MagmaCube,
            ferrite_math::Vec3::new(9.2, 64.0, 8.5),
            &mut region.rng,
        );
        if let Some(s) = tiny_cube.data.mob.as_mut() {
            mob::set_cube_size_state(s, 1);
            s.despawns = false;
        }
        mob::sync_state_metadata(&mut tiny_cube.data);
        region.entities.insert(tiny_cube);

        for _ in 0..25 {
            region.tick(&ctx);
        }

        let verdant_id = i32::from(ferrite_item::item_id("minecraft:verdant_froglight").unwrap());
        let dropped_froglight = region.entities.keys().iter().any(|&key| {
            region.entities.data(key).is_some_and(|d| {
                let s = ferrite_entity::item::stack(d);
                s.item_id == verdant_id && s.count == 1
            })
        });
        assert!(
            dropped_froglight,
            "cold frog eating tiny magma cube should drop verdant_froglight"
        );

        let mut big_slime = mob::new_mob(
            MobKind::Slime,
            ferrite_math::Vec3::new(4.5, 64.0, 4.5),
            &mut region.rng,
        );
        if let Some(s) = big_slime.data.mob.as_mut() {
            mob::set_cube_size_state(s, 2);
            s.despawns = false;
        }
        mob::sync_state_metadata(&mut big_slime.data);
        let big_key = region.entities.insert(big_slime);
        mob::kill(&mut region.entities, big_key);
        region.tick(&ctx);

        let small_slimes = region
            .entities
            .keys()
            .iter()
            .filter(|&&key| {
                mob::state(&region.entities, key)
                    .is_some_and(|s| s.kind == MobKind::Slime && s.cube_size == 1 && !s.is_dead())
            })
            .count();
        assert!(
            (2..=4).contains(&small_slimes),
            "size-2 slime should split into 2..=4 size-1 slimes, got {small_slimes}"
        );
    }

    #[test]
    fn shearing_a_mooshroom_drops_five_mushrooms_and_turns_into_a_cow() {
        let mut region = Region::default();
        let mut brown = mob::new_mob(
            MobKind::Mooshroom,
            ferrite_math::Vec3::new(3.5, 64.0, 3.5),
            &mut region.rng,
        );
        brown.data.mob.as_mut().unwrap().variant = Some(1);
        mob::sync_state_metadata(&mut brown.data);
        let key = region.entities.insert(brown);

        assert!(region.use_shears_on_mooshroom(Uuid::nil(), key, "minecraft:shears", 0, 0,));
        assert!(!mob::is_mob(&region.entities, key));
        let cows = region
            .entities
            .keys()
            .iter()
            .filter(|&&k| mob::state(&region.entities, k).is_some_and(|s| s.kind == MobKind::Cow))
            .count();
        assert_eq!(cows, 1, "sheared mooshroom should be replaced by one cow");
        let brown_id = i32::from(ferrite_item::item_id("minecraft:brown_mushroom").unwrap());
        let mushroom_count: i32 = region
            .entities
            .keys()
            .iter()
            .filter_map(|&k| region.entities.data(k))
            .map(ferrite_entity::item::stack)
            .filter(|s| s.item_id == brown_id)
            .map(|s| s.count)
            .sum();
        assert_eq!(mushroom_count, 5);
    }

    #[test]
    fn snow_golem_melts_in_nether_and_warm_biomes() {
        assert!(ferrite_entity::golem::melts_in_biome(Some(
            "minecraft:desert"
        )));
        assert!(ferrite_entity::golem::melts_in_biome(Some(
            "minecraft:badlands"
        )));
        assert!(!ferrite_entity::golem::melts_in_biome(Some(
            "minecraft:plains"
        )));

        let mut ctx = super::tests::tick_context();
        ctx.dimension = crate::game::Dimension::Nether;
        let mut region = Region::default();
        let golem = mob::new_mob(
            MobKind::SnowGolem,
            ferrite_math::Vec3::new(3.5, 64.0, 3.5),
            &mut region.rng,
        );
        let key = region.entities.insert(golem);
        let before = mob::state(&region.entities, key).unwrap().health;
        region.tick(&ctx);
        let after = mob::state(&region.entities, key).unwrap().health;
        assert!(
            after < before,
            "snow golem should take heat damage in the Nether ({before} -> {after})"
        );
    }
}
