use super::{
    PluginEventPolicy,
    chunk::{ChunkSource, FlatGenerator},
    commands::{XpOperation, XpUnit},
    survival::{
        DamageKind, EAT_TICKS, Eating, FoodTick, MAX_FOOD, MAX_HEALTH, Survival, SurvivalRules,
    },
    valid_move,
};
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
use ferrite_plugin::InteractAction;
use ferrite_profiler::Timings;
use ferrite_protocol::{Encode, Packet, VarInt, packets::play::*};
use ferrite_region::{ChunkPos, RegionData, SectionPos};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

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
        ticket: u64,
    },
    /// A plugin put the player to bed; the verdict answers the waiting plugin call.
    PluginSleep {
        position: [i32; 3],
        force: bool,
        ticket: u64,
    },
    Input(ServerboundPlay),
    Leave,
    SetMode(i32),
    /// `Player.Spigot#respawn`: respawn the player if they are dead.
    Respawn,
    /// Plugin-chosen abilities; the game mode still decides instant build and creative flight.
    /// A client-only game event a plugin asked for.
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
        items: Vec<Option<ferrite_plugin::ItemSnapshot>>,
    },
    /// A plugin's change to one of this player's shoulders.
    PluginShoulder {
        left: bool,
        op: ferrite_plugin::ShoulderOp,
    },
    /// A packet a plugin sends to this player.
    PluginPacket(ferrite_plugin::PluginPacket),
    /// A plugin's direct change to a survival number.
    PluginSurvival {
        stat: ferrite_plugin::SurvivalStat,
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
    PluginAbilities {
        allow_flight: bool,
        flying: bool,
        invulnerable: bool,
        fly_speed: f32,
        walk_speed: f32,
    },
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
        item: Option<ferrite_plugin::ItemSnapshot>,
    },
    /// A plugin sends the colours and visible cursors of a map.
    MapData {
        id: i32,
        scale: u8,
        locked: bool,
        colors: Vec<u8>,
        decorations: Vec<ferrite_plugin::MapDecoration>,
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
        book: Option<ferrite_plugin::BookFields>,
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
        death_message: String,
    },
    /// A Paper `EntityScheduler` task for this player, run on the region that owns them.
    PluginEntityTask(ferrite_plugin::EntityTask),
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

/// The stack worn down to `damage` (never past its last point of durability); `None` for an
/// empty stack or an item that cannot wear.
/// The filled map `stack` pointing at saved map `id`; `None` for any other item.
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
    pub book: Option<ferrite_plugin::BookFields>,
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
            // 26.3 no longer has one component for this deprecated flag; hiding the whole
            // tooltip is the closest explicit behaviour.
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
pub(super) fn with_meta_spec(
    stack: ferrite_item::ItemStack,
    meta: &ferrite_plugin::ItemMetaSpec,
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
    // A book's pages belong to its own item only: book metadata sent for any other item has
    // nothing to attach to and is left out rather than put on the wrong stack.
    if let Some(book) = &meta.book
        && let Some(content) = book_content(item.as_deref(), book)
    {
        put(content);
    }
    // Components the Java side had no dedicated field for travel in their 26.3 NBT shape. They
    // are applied last so an explicit tooltip_display from the meta wins over the flags above.
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
    book: &ferrite_plugin::BookFields,
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

/// The Bukkit UUID a region's world is published under, for world events.
fn plugin_world_id(ctx: &TickContext) -> Uuid {
    match ctx.world_id {
        super::WorldId::Vanilla(dimension) => crate::plugins::world_id(dimension),
        super::WorldId::Runtime(id) => id,
    }
}

/// Picks the bridge for an event about a non-player entity: the live shared handle first, else
/// the first player's own handle. Exactly one bridge is returned, so the event fires once.
pub(super) fn resolve_damage_hooks(
    shared: Option<&SharedPluginHooks>,
    mut players: impl Iterator<Item = (Arc<dyn ferrite_plugin::ServerHooks>, PluginEventPolicy)>,
) -> Option<(Arc<dyn ferrite_plugin::ServerHooks>, PluginEventPolicy)> {
    shared
        .and_then(SharedPluginHooks::load)
        .or_else(|| players.next())
}

/// The plugin hooks and the failure policy that goes with them. The bridge attaches after the
/// world loop starts, so regions read the current value when an event fires instead of
/// capturing it at startup.
#[derive(Default)]
pub struct SharedPluginHooks {
    current: Mutex<Option<(Arc<dyn ferrite_plugin::ServerHooks>, PluginEventPolicy)>>,
}

impl SharedPluginHooks {
    /// Replaces the bridge every later event sees.
    pub fn publish(
        &self,
        hooks: Option<Arc<dyn ferrite_plugin::ServerHooks>>,
        policy: PluginEventPolicy,
    ) {
        *self
            .current
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = hooks.map(|hooks| (hooks, policy));
    }

    pub fn load(&self) -> Option<(Arc<dyn ferrite_plugin::ServerHooks>, PluginEventPolicy)> {
        self.current
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

/// The command tree as the world loop currently sees it. Plugin hooks attach after the loop
/// starts and rebuild the tree, so region-driven commands must read the current tree per
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
/// Block access for entity physics, memoized for one tick (each lookup locks the chunk cache).
pub(super) struct TickBlocks<'a> {
    source: Option<&'a super::anvil::AnvilChunkSource>,
    seen: std::cell::RefCell<HashMap<(i32, i32, i32), Option<u16>>>,
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
        *self
            .seen
            .borrow_mut()
            .entry((x, y, z))
            .or_insert_with(|| source.block(x, y, z).and_then(|s| u16::try_from(s).ok()))
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
        // `AnvilChunkSource::block` only reads an already-loaded column; it never starts a
        // generation task. Asking for the bottom block is a cheap loaded-chunk probe.
        let Some(source) = self.source else {
            return false;
        };
        source.block(x, source.dimension().min_y(), z).is_some()
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
const PREFETCH_CHUNKS: usize = 96;

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
    pub plugins: Option<Arc<dyn ferrite_plugin::ServerHooks>>,
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
    pub loaded: HashSet<(i32, i32)>,
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
    /// Explosions plugins asked for: center, power, whether fire is left behind and whether
    /// blocks break.
    pub plugin_explosions: Vec<([f64; 3], f32, bool, bool)>,
    /// Dragon breath clouds: vanilla growth, wait and damage schedule.
    pub breath_clouds: Vec<ferrite_entity::dragon::BreathCloud>,
    /// Final block updates held back until the client finished the push animation. Keyed by
    /// position through [`Region::queue_delayed_update`] so a reversal replaces stale updates.
    pub delayed_updates: Vec<DelayedUpdate>,
    /// Client-side moving-piston blocks that have not reached the two-tick completion yet.
    pub moving_pistons: super::piston::Animations,
    /// The stack of every trident in flight, by entity uuid, so it can drop as it was thrown.
    pub thrown_tridents: HashMap<Uuid, (Uuid, ferrite_item::ItemStack)>,
    /// The bottle each splash potion in flight was thrown from, keyed by projectile uuid.
    pub thrown_potions: HashMap<Uuid, ferrite_item::ItemStack>,
    /// Clouds left by landed lingering potions. Each has an `area_effect_cloud` entity in
    /// `entities` (same uuid) that clients see; the cloud state itself is not saved.
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
    pub players: HashMap<Uuid, Player>,
    pub entities: EntityStore,
    generator: FlatGenerator,
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
    /// What this region last told plugins about its entities.
    published_facts: HashSet<Uuid>,
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
        self.published_entities.extend(other.published_entities);
        self.published_facts.extend(other.published_facts);
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
        let mut result = Self {
            plugin_explosions: Vec::new(),
            breath_clouds: Vec::new(),
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
            players: HashMap::new(),
            rng: Rng::default(),
            beacon_clock: 0,
            entities: self
                .entities
                .split(|(x, z)| sections.contains(&SectionPos(x >> 4, z >> 4))),
            generator: self.generator.split(sections),
            fluid_ticks: ferrite_fluid::FluidTicks::default(),
            remote_changes: Vec::new(),
            random_ticks: super::random_tick::RandomTicks::default(),
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
fn chunk_window(
    center: (i32, i32),
    radius: i32,
    loaded: &HashSet<(i32, i32)>,
) -> (ChunkList, ChunkList) {
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
    new.sort_by_key(|(x, z)| {
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
/// The mob a spawn egg places (`SpawnEggItem.getType`), e.g. `minecraft:cow_spawn_egg`.
fn spawn_egg_kind(item: Option<&str>) -> Option<MobKind> {
    let name = item?;
    MobKind::from_name(name.strip_suffix("_spawn_egg")?)
}

fn material_name(state: u32) -> &'static str {
    u16::try_from(state)
        .ok()
        .and_then(ferrite_data::blocks::state)
        .map_or("minecraft:air", |(name, _)| name)
}
/// Runs an event hook. A missing bridge allows the action; a failed call is
/// decided by `plugin-event-errors` (Bukkit-compatible default: allow).
fn event_allows(
    plugins: Option<&dyn ferrite_plugin::ServerHooks>,
    policy: PluginEventPolicy,
    event: impl FnOnce(&dyn ferrite_plugin::ServerHooks) -> Result<bool, ferrite_plugin::Error>,
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
    plugins: Option<&dyn ferrite_plugin::ServerHooks>,
    policy: PluginEventPolicy,
    amount: f32,
    event: impl FnOnce(&dyn ferrite_plugin::ServerHooks) -> Result<f64, ferrite_plugin::Error>,
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
        DamageKind::Arrow => "PROJECTILE",
        DamageKind::GenericKill => "KILL",
        DamageKind::Generic => "CUSTOM",
    }
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
            crate::plugins::facts::set_gliding(self.uuid, false);
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
        let mut released = Vec::with_capacity(forget.len());
        for (x, z) in forget {
            send(self, &ForgetChunk { z, x });
            self.loaded.remove(&(x, z));
            crate::plugins::facts::set_chunk_sent(self.uuid, x, z, false);
            if let Some(plugins) = self.plugins.clone() {
                let _ = plugins.chunk_unloaded(ferrite_plugin::default_world_id(), x, z);
            }
            if let Some((_, loaded, _)) = self.tracking.lock().unwrap().get_mut(&self.uuid) {
                loaded.remove(&(x, z));
            }
            released.push(ChunkPos(x, z));
        }
        if !released.is_empty() {
            let _ = self.signals.send(Signal::HoldBatch {
                world: self.world_id,
                chunks: released,
                add: false,
            });
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
    fn chunks_tick(
        &mut self,
        generator: &mut FlatGenerator,
        moving_pistons: &[super::piston::MovingBlock],
    ) {
        if self.awaiting_batch || self.queue.is_empty() {
            return;
        }
        // Start loading the whole head of the queue at once; `get` hands back chunks that are ready
        // and spawns background loads for the rest. Without this, chunks became ready one per tick.
        if let Some(source) = &self.source {
            for &(x, z) in self
                .queue
                .iter()
                .take(PREFETCH_CHUNKS)
                .skip(self.prefetched)
            {
                source.prefetch(x, z, self.biome);
            }
            self.prefetched = self.queue.len().min(PREFETCH_CHUNKS);
        }
        let count = (self.rate.clamp(1.0, 64.0) as usize).min(self.queue.len());
        let mut chunks = Vec::new();
        for _ in 0..count {
            let Some(&(x, z)) = self.queue.front() else {
                break;
            };
            let packet = if let Some(source) = &self.source {
                match source.get(x, z, self.biome) {
                    None => {
                        // The source may have refused deeper chunks while busy; ask again, but
                        // not every tick: each ask takes the shared cache lock.
                        self.prefetch_wait += 1;
                        if self.prefetch_wait >= 20 {
                            self.prefetch_wait = 0;
                            self.prefetched = 0;
                        }
                        break;
                    }
                    // The cache shares one encoded body; the connection layer
                    // needs an owned buffer, so this is the only copy per send.
                    Some(Ok(packet)) => packet.to_vec(),
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
                packet
            };
            self.queue.pop_front();
            self.prefetched = self.prefetched.saturating_sub(1);
            chunks.push((x, z, packet));
        }
        if chunks.is_empty() {
            return;
        }
        send(self, &ChunkBatchStart);
        let count = chunks.len();
        let sent: Vec<_> = chunks.iter().map(|&(x, z, _)| (x, z)).collect();
        for (x, z, packet) in chunks {
            let _ = self.outbound.send(Outbound::Packet(packet.into()));
            self.loaded.insert((x, z));
            crate::plugins::facts::set_chunk_sent(self.uuid, x, z, true);
            if let Some(plugins) = self.plugins.clone() {
                let _ = plugins.chunk_loaded(ferrite_plugin::default_world_id(), x, z, true);
            }
        }
        let held: Vec<_> = sent.iter().map(|&(x, z)| ChunkPos(x, z)).collect();
        if !held.is_empty() {
            let _ = self.signals.send(Signal::HoldBatch {
                world: self.world_id,
                chunks: held,
                add: true,
            });
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
        use ferrite_math::{AABB, Vec3, look_from_yaw_pitch, raycast};
        let source = self.source.as_ref()?;
        let eye = Vec3::new(self.pos[0], self.pos[1] + 1.62, self.pos[2]);
        let mut shapes = HashMap::new();
        let look = look_from_yaw_pitch(self.yaw, self.pitch);
        let distance = if self.gamemode == 1 { 5.0 } else { 4.5 };
        let end = eye + look * distance;
        let start = ferrite_math::BlockPos::containing(eye);
        let end = ferrite_math::BlockPos::containing(end);
        for x in start.x.min(end.x)..=start.x.max(end.x) {
            for y in start.y.min(end.y)..=start.y.max(end.y) {
                for z in start.z.min(end.z)..=start.z.max(end.z) {
                    let boxes: Vec<_> = match source
                        .block(x, y, z)
                        .and_then(|v| u16::try_from(v).ok())
                        .filter(|v| usize::from(*v) < ferrite_data::blocks::STATE_COUNT)
                    {
                        Some(state) => {
                            let props = ferrite_data::block_props::get(state);
                            let shapes: Vec<_> = ferrite_data::block_props::collision_shape(state)
                                .iter()
                                .map(|b| AABB::new(b[0], b[1], b[2], b[3], b[4], b[5]))
                                .collect();
                            // Vanilla `ClipContext.Fluid.SOURCE_ONLY`: fluids are
                            // clickable even without a collision shape (buckets).
                            if shapes.is_empty() && !props.is_air {
                                // Fluids and interaction blocks without a collision shape
                                // (doors, signs) still have to be clickable.
                                vec![AABB::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0)]
                            } else {
                                shapes
                            }
                        }
                        None => vec![AABB::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0)],
                    };
                    if !boxes.is_empty() {
                        shapes.insert((x, y, z), boxes);
                    }
                }
            }
        }
        // Vanilla gives levers and buttons an outline shape; the generated block-state report
        // only models collision, so hand-activated components would be unselectable. Give them
        // a full-cell outline, but still let a nearer collision block occlude the ray.
        let outline = source
            .block(target.x, target.y, target.z)
            .and_then(|v| u16::try_from(v).ok())
            .is_some_and(|state| super::redstone::use_block(state).is_some());
        let outline_box = [AABB::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0)];
        raycast(eye, look, distance, |p| {
            if outline && (p.x, p.y, p.z) == (target.x, target.y, target.z) {
                Some(&outline_box[..])
            } else {
                shapes.get(&(p.x, p.y, p.z)).map(Vec::as_slice)
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
            if name.is_some_and(super::block_container::is_shulker_box) {
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
        let clicked = ferrite_plugin::BlockPosition {
            x: position[0],
            y: position[1],
            z: position[2],
        };
        if !self.plugin_allows(|plugins| {
            plugins.block_broken(self.uuid, &self.name, clicked, material_name(state))
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
        // Vanilla `BucketItem.place`: water and lava buckets place their fluid block.
        let bucket_name = match item.name() {
            Some("minecraft:water_bucket") => Some("water"),
            Some("minecraft:lava_bucket") => Some("lava"),
            _ => None,
        };
        let state = if let Some(fluid) = bucket_name {
            ferrite_data::blocks::default_state(fluid).expect("vanilla fluid")
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
                ferrite_plugin::BlockPosition {
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
        event: impl FnOnce(&dyn ferrite_plugin::ServerHooks) -> Result<bool, ferrite_plugin::Error>,
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
    /// A plugin's `PlayerInventory#setItem`: replaces one slot and syncs it to the client.
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

    fn set_inventory_slot(&mut self, slot: u8, item: Option<ferrite_plugin::ItemSnapshot>) {
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
    /// Sets the wear of the stack in a slot; an empty slot or an item that cannot wear is ignored.
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
        if self.gamemode == 3 || (self.gamemode == 1 && kind.ignores_creative()) {
            return false;
        }
        // `FireResistanceMobEffect`: immune to fire and lava, and the fire goes out.
        if self.effects.fire_resistance() && matches!(kind, DamageKind::Lava | DamageKind::Fire) {
            self.survival.fire_ticks = 0;
            return false;
        }
        // `LivingEntity.getDamageAfterArmorAbsorb` includes the resistance attribute.
        let reduction = self.effects.attributes().damage_reduction as f32;
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
        self.survival.food.add_exhaustion(0.1);
        self.breaking = None;
        self.survival.eating = None;
        // `Player.checkTotemDeathProtection`: the totem revives before `ServerPlayer.die`.
        if self.survival.dead && self.try_totem() {
            self.sync_health();
            return true;
        }
        // Vanilla `ServerPlayer.die` sends the combat-kill packet before the zero-health update,
        // so fatal damage is synced by the region's death processing instead.
        if !self.survival.dead {
            self.sync_health();
        }
        true
    }

    /// Applies one effect instance and, when it is not instantaneous, sends
    /// `ClientboundUpdateMobEffectPacket` (unless a stronger instance kept the slot).
    fn apply_effect(&mut self, instance: MobEffectInstance) {
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
        let void = crate::plugins::facts::void_damage(super::plugin_world_id(self.world_id));
        self.survival.void_y = f64::from(self.dimension.min_y()) + void.offset;
        self.survival.void_damage = void.enabled.then_some(void.amount);
        let damage = self.survival.environment_tick(
            self.pos,
            self.ground,
            eye_fluid == ferrite_fluid::FluidKind::Water,
            feet_fluid == ferrite_fluid::FluidKind::Lava,
            fire,
            self.gamemode,
            self.survival.sprinting,
        );
        crate::plugins::facts::set_fall_distance(self.uuid, self.survival.fall_distance as f32);
        crate::plugins::facts::set_use_state(self.uuid, self.use_state());
        crate::plugins::facts::set_vitals(
            self.uuid,
            crate::plugins::facts::Vitals {
                saturation: self.survival.food.saturation,
                exhaustion: self.survival.food.exhaustion,
                absorption: self.survival.absorption,
                air: self.survival.air,
                fire_ticks: self.survival.fire_ticks,
            },
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

    /// What the player is using right now as `(hand, ticks left, ticks used)`; hand 0 is the
    /// main hand, 1 the off hand. A bow or a raised shield has no end (vanilla 72000 ticks).
    fn use_state(&self) -> Option<(u8, i32, i32)> {
        if self.survival.dead {
            return None;
        }
        if let Some(eating) = self.survival.eating {
            let hand = u8::from(eating.slot == ferrite_inventory::OFFHAND);
            return Some((hand, eating.remaining, EAT_TICKS - eating.remaining));
        }
        if self.using_bow {
            return Some((0, 72_000 - self.bow_charge, self.bow_charge));
        }
        if self.blocking {
            let main = self.inventory.get(self.hand_slot(0));
            let hand = u8::from(!matches!(main.name(), Some("minecraft:shield")));
            return Some((hand, 72_000, 0));
        }
        None
    }

    /// Vanilla `LivingEntity.startUsingItem` for the stack in `hand` (0 main, 1 off).
    fn start_use(&mut self, hand: i32) {
        if self.survival.dead || !(0..=1).contains(&hand) {
            return;
        }
        let slot = self.hand_slot(hand);
        let stack = self.inventory.get(slot).clone();
        // Milk bucket: `MilkBucketItem.finishUsingItem` removes every effect and leaves a bucket.
        if matches!(stack.name(), Some("minecraft:milk_bucket")) {
            self.clear_effects();
            if !matches!(self.gamemode, 1 | 3)
                && let Some(bucket) = ferrite_item::ItemStack::from_name("bucket")
            {
                self.inventory.set(slot, bucket.clone());
                send(
                    self,
                    &SetPlayerInventory {
                        slot: VarInt(slot as i32),
                        item: ItemStack {
                            count: bucket.count,
                            item_id: i32::from(bucket.item),
                        },
                    },
                );
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
    /// then apply the food component (`FoodData.eat`) and shrink the stack in survival.
    fn tick_eating(&mut self) {
        let Some(mut eating) = self.survival.eating else {
            return;
        };
        if self.survival.dead {
            self.survival.eating = None;
            return;
        }
        let stack = self.inventory.get(eating.slot).clone();
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
        if food.is_none() && potion.is_none() {
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
        if let Some(food) = food {
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
            if self.inventory.get(eating.slot).is_empty()
                && let Some(ferrite_item::Component::Opaque(ferrite_nbt::Tag::Compound(compound))) =
                    remainder
                && let Some(ferrite_nbt::Tag::String(name)) = compound.get("id")
                && let Some(item) = ferrite_item::ItemStack::from_name(name)
            {
                self.inventory.set(eating.slot, item);
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
        crate::plugins::facts::clear_sent_chunks(self.uuid);
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
        crate::plugins::facts::clear_sent_chunks(self.uuid);
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
                    send(
                        self,
                        &GameEvent {
                            event: 3,
                            value: mode as f32,
                        },
                    );
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
                    use ferrite_plugin::SurvivalStat;
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
                    if crate::plugins::facts::is_gliding(self.uuid) {
                        self.firework_launches.push((flight, explosions, entity));
                    }
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
                    // Bukkit's scale is twice the protocol's: 0.1 fly and 0.2 walk are the defaults.
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
                            let y = if self.ground { 0.4 } else { 0.0 };
                            send(
                                self,
                                &SetEntityMotion {
                                    id: VarInt(self.entity),
                                    velocity: ferrite_protocol::types::LpVec3([
                                        dx / length * 0.4,
                                        y,
                                        dz / length * 0.4,
                                    ]),
                                },
                            );
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
                Command::PluginSleep {
                    position,
                    force,
                    ticket,
                } => {
                    let slept = super::blocks::plugin_sleep(self, position, force);
                    crate::plugins::playerops::complete(ticket, slept);
                    continue;
                }
                Command::PluginBreakBlock { position, ticket } => {
                    let broke = self.plugin_break_block(position);
                    crate::plugins::playerops::complete(ticket, broke);
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
                                                    ferrite_plugin::MapCaption::Plain(text) => {
                                                        ferrite_protocol::types::NetworkText::Plain(
                                                            text,
                                                        )
                                                    }
                                                    ferrite_plugin::MapCaption::Rich(tag) => {
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
                        // Finish a meal or potion at the next use tick; fire a drawn bow.
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
                    death_message,
                } => {
                    self.survival.death_message = Some(death_message);
                    self.hurt(kind, amount);
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
                    match p.status.0 {
                        0 if self.hit(pos).is_some() => {
                            let clicked = ferrite_plugin::BlockPosition {
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
                                    clicked,
                                    u8::try_from(p.face.clamp(0, 5)).unwrap_or(0),
                                    material_name(state.unwrap_or(0)),
                                )
                            });
                            if allowed && let Some(state) = state {
                                if self.gamemode == 1 {
                                    if self.plugin_allows(|plugins| {
                                        plugins.block_broken(
                                            self.uuid,
                                            &self.name,
                                            clicked,
                                            material_name(state),
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
                                                clicked,
                                                material_name(state),
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
                                        ferrite_plugin::BlockPosition {
                                            x: pos.x,
                                            y: pos.y,
                                            z: pos.z,
                                        },
                                        material_name(state),
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
                    let state = self
                        .source
                        .as_ref()
                        .and_then(|s| s.block(clicked.x, clicked.y, clicked.z));
                    let allowed = self.plugin_allows(|plugins| {
                        plugins.player_interacted(
                            self.uuid,
                            &self.name,
                            InteractAction::RightClickBlock,
                            ferrite_plugin::BlockPosition {
                                x: clicked.x,
                                y: clicked.y,
                                z: clicked.z,
                            },
                            u8::try_from(p.face.0.clamp(0, 5)).unwrap_or(0),
                            material_name(state.unwrap_or(0)),
                        )
                    });
                    if allowed {
                        let hit = self.hit(clicked);
                        let used = (0..=1).contains(&p.hand.0)
                            && hit.is_some()
                            && self.use_block(p.location);
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
                ServerboundPlay::CookieResponse(p) => {
                    crate::plugins::facts::set_cookie(self.uuid, &p.key, p.payload);
                }
                ServerboundPlay::ResourcePackResponse(p) => {
                    crate::plugins::facts::set_pack_status(self.uuid, p.action.0);
                    if let Some(plugins) = &self.plugins {
                        let _ =
                            plugins.resource_pack_status(self.uuid, &self.name, p.id, p.action.0);
                    }
                }
                ServerboundPlay::PlayerInput(p) => {
                    crate::plugins::facts::set_input(self.uuid, p.flags);
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
                                crate::plugins::facts::set_sprinting(self.uuid, true);
                            }
                            2 => {
                                self.survival.sprinting = false;
                                crate::plugins::facts::set_sprinting(self.uuid, false);
                            }
                            // START_FALL_FLYING; vanilla refuses it on the ground.
                            6 if !self.ground => {
                                crate::plugins::facts::set_gliding(self.uuid, true)
                            }
                            _ => {}
                        }
                    }
                }
                ServerboundPlay::SignUpdate(p) => super::blocks::edit_sign(self, &p),
                ServerboundPlay::SetCommandBlock(p) => self.edit_command_block(&p),
                ServerboundPlay::SetHeldItem(p) => {
                    // Vanilla `handleSetCarriedItem` ignores slots outside the hotbar.
                    if (0..=8).contains(&p.slot) {
                        self.using_bow = false;
                        self.bow_charge = 0;
                        self.blocking = false;
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
                            // Bukkit fires `PlayerMoveEvent` only when the position changes.
                            let movement = ferrite_plugin::PlayerMove {
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
                            let movement = ferrite_plugin::PlayerMove {
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

fn deliver(
    players: &HashMap<Uuid, Player>,
    ctx: &TickContext,
    (uuid, session): ViewerKey,
    bytes: &[u8],
) {
    let packet = Outbound::Packet(bytes.to_vec().into());
    match players.get(&uuid) {
        Some(p) if p.session == session => {
            let _ = p.outbound.send(packet.clone());
        }
        Some(_) => {}
        None => {
            if let Some((_, _, outbound)) = ctx.tracking.lock().unwrap().get(&uuid) {
                let _ = outbound.send(packet.clone());
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
    /// Currently test-only: the tick-loop call is disabled until the join-time scan is made
    /// chunk-direct (see `docs/world-interaction-audit.md`).
    #[cfg(test)]
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
        let dimension = source.dimension();
        let air = u32::from(ferrite_data::blocks::default_state("air").expect("vanilla air"));
        // A section made only of full opaque blocks cannot hold a fluid, a support-dependent
        // block or a gravity block, so its 16 layers cannot need a border wake. This keeps the
        // pass proportional to the surface, not to the full build height.
        let interesting = |state: u32| {
            u16::try_from(state).ok().is_some_and(|state| {
                let name = ferrite_data::blocks::state(state).map(|(name, _)| name);
                // Cave and void air are separate states and are not full blocks; without this
                // check every underground cave section would look interesting.
                !matches!(
                    name,
                    Some("minecraft:air" | "minecraft:cave_air" | "minecraft:void_air")
                ) && (ferrite_data::block_props::get(state).fluid.is_some()
                    || !ferrite_data::block_props::get(state).can_occlude
                    || super::gravity::is_gravity(state))
            })
        };
        let _ = air;
        for chunk in pending {
            if !self.held_chunks.contains_key(&chunk) {
                continue;
            }
            // Only sections that hold a fluid, a non-full block or a gravity block can need a
            // wake. Check the inside chunk and each neighbour separately instead of building one
            // bounding box: deep water plus a surface plant would otherwise make the scan cover
            // the whole build height.
            let mask = |chunk: &ferrite_world::Chunk| {
                let min = chunk.min_y();
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
                (min, bits)
            };
            let Some((min_y, inside_mask)) = source.with_loaded_chunk(chunk.0, chunk.1, mask)
            else {
                continue;
            };
            if inside_mask.is_empty() {
                continue;
            }
            let (x0, z0) = (chunk.0 * 16, chunk.1 * 16);
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                for t in 0..16 {
                    let (x, z) = if dx != 0 {
                        (if dx > 0 { x0 + 15 } else { x0 }, z0 + t)
                    } else {
                        (x0 + t, if dz > 0 { z0 + 15 } else { z0 })
                    };
                    let (ox, oz) = (x + dx, z + dz);
                    let outside_chunk = ChunkPos(ox.div_euclid(16), oz.div_euclid(16));
                    if outside_chunk == chunk {
                        continue;
                    }
                    let outside_mask = source
                        .with_loaded_chunk(outside_chunk.0, outside_chunk.1, mask)
                        .map(|(_, bits)| bits)
                        .unwrap_or_default();
                    for (index, inside_interesting) in inside_mask.iter().enumerate() {
                        if !*inside_interesting
                            && !outside_mask.get(index).copied().unwrap_or(false)
                        {
                            continue;
                        }
                        let base = min_y + index as i32 * 16;
                        for y in base..base + 16 {
                            let Some(inside_state) = source.block(x, y, z) else {
                                continue;
                            };
                            let Some(outside_state) = source.block(ox, y, oz) else {
                                // Still unloaded: nothing has become readable on that side.
                                continue;
                            };
                            if inside_state == air && outside_state == air {
                                continue;
                            }
                            // Only fluid that could not move because the other side was unloaded
                            // needs a wake. An ocean border with water on both sides does not.
                            if ferrite_fluid::is_liquid_block(inside_state)
                                && ferrite_fluid::can_hold_fluid(
                                    outside_state,
                                    ferrite_fluid::fluid_state(inside_state).kind,
                                )
                            {
                                self.schedule_fluid((x, y, z));
                            }
                            if ferrite_fluid::is_liquid_block(outside_state)
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
                            if inside_u16.is_some_and(|state| {
                                !super::neighbors::can_survive(state, [x, y, z], source)
                            }) {
                                // The block is ours; the unreadable side is now readable and failed
                                // its support test. Re-validate it without forwarding the origin.
                                self.updates.remote_changed([ox, y, oz]);
                            }
                            if outside_u16.is_some_and(|state| {
                                !super::neighbors::can_survive(state, [ox, y, oz], source)
                            }) {
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
                            if inside_u16.is_some_and(super::gravity::is_gravity)
                                || outside_u16.is_some_and(super::gravity::is_gravity)
                            {
                                self.schedule_gravity(source, &[[x, y, z], [ox, y, oz]]);
                            }
                        }
                    }
                }
            }
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
        crate::plugins::facts::forget_entities(records.iter().map(|record| record.data.uuid));
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
        crate::plugins::facts::forget_entities(self.published_facts.drain());
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
    ) {
        self.random_ticks.tick(
            source,
            &self.held_chunks,
            tracking,
            signals,
            speed,
            &mut self.fire,
        );
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
        .with_signals(signals.clone());
        self.fluid_ticks.advance(&mut world);
        let washed = std::mem::take(&mut world.washed);
        // Only blocks water actually destroyed need a shape pass. Queueing a neighbour pass for
        // every water level change would turn an ocean border into an update storm on every
        // flowing-water tick.
        for (pos, _) in &washed {
            self.updates.block_changed(*pos);
        }
        let mut removed = Vec::new();
        self.updates.apply(&mut world, &mut removed);
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
    /// Creates a mob at an absolute position (`/summon`).
    pub fn summon(&mut self, kind: MobKind, pos: [f64; 3], no_ai: bool) -> bool {
        if !pos
            .iter()
            .all(|value| value.is_finite() && value.abs() <= 30_000_000.0)
        {
            return false;
        }
        let record = mob::new_mob(
            kind,
            ferrite_math::Vec3::new(pos[0], pos[1], pos[2]),
            &mut self.rng,
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
    fn refresh_leash_ids(
        &mut self,
        ctx: &TickContext,
        holders: &HashMap<Uuid, (ferrite_math::Vec3, i32)>,
    ) {
        for key in self.entities.keys().to_vec() {
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
        let (day_time, game_time) = {
            let state = lock_game_state(&ctx.state, ctx.profiler.as_deref());
            (state.clock_time, state.game_time)
        };
        ferrite_entity::villager::tick_all(&mut self.entities, blocks, day_time, game_time);
        // Read immutable player snapshots across regions without taking another region's state.
        // Read the two shared maps separately so no locks are held together.
        let same_dimension: HashSet<Uuid> = ctx
            .tracking
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(&uuid, (world, _, _))| (*world == ctx.world_id).then_some(uuid))
            .collect();
        for p in self.players.values() {
            super::targeting::publish(p.uuid, p.gamemode);
        }
        let mut players: HashMap<Uuid, ferrite_math::Vec3> = ctx
            .positions
            .lock()
            .unwrap()
            .iter()
            .filter(|(uuid, _)| same_dimension.contains(uuid))
            .map(|(&uuid, pos)| (uuid, ferrite_math::Vec3::new(pos[0], pos[1], pos[2])))
            .collect();
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
        let rules = ai::MobRules {
            untargetable: super::targeting::untargetable(),
            // Vanilla `Difficulty.HARD` is 3.
            hard: ctx.state.lock().unwrap().difficulty == 3,
        };
        let targets: Vec<_> = players
            .iter()
            .map(|(&uuid, &pos)| ai::Target { uuid, pos })
            .collect();
        let positions: Vec<_> = players.into_values().collect();
        let trading = self.trading_traders();
        self.tick_trader_lifetimes(&trading, ctx);
        let holders = {
            // A test world's clock must not prune another test's last sighting.
            #[cfg(test)]
            let _turn = leash_test_turn();
            self.leash_holders(ctx)
        };
        self.refresh_leash_ids(ctx, &holders);
        let holder_positions: Vec<(Uuid, ferrite_math::Vec3)> = holders
            .iter()
            .map(|(holder, (pos, _))| (*holder, *pos))
            .collect();
        for event in ai::tick_full(
            &mut self.entities,
            blocks,
            &targets,
            &rules,
            owner_attacks,
            &holder_positions,
            &mut self.rng,
        ) {
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
                        });
                    }
                }
                ai::MobEvent::Arrow {
                    from,
                    direction,
                    damage,
                } => {
                    let speed = arrow::SPEED;
                    let vel = ferrite_math::Vec3::new(
                        direction.x * speed,
                        direction.y * speed,
                        direction.z * speed,
                    );
                    let record = arrow::new_arrow(from, vel, damage, None, &mut self.rng);
                    self.entities.insert(record);
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
                ai::MobEvent::Breath { pos } => {
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
                    self.breath_clouds
                        .push(ferrite_entity::dragon::BreathCloud::perched(pos));
                }
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
            }
        }
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
        for record in spawn::natural_spawn(&self.entities, blocks, &positions, &mut self.rng, 1) {
            self.entities.insert(record);
        }
        for record in
            spawn::natural_spawn_monsters(&self.entities, blocks, &positions, &mut self.rng, 1)
        {
            self.entities.insert(record);
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
                ))
            })
            .collect();
        for (key, kind, pos, baby, killer, burning) in dead {
            if let Some(killer) = killer {
                super::recipe_unlock::KILLS
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push((killer, kind.name()));
            }
            if mob::state(&self.entities, key).is_some_and(|s| s.leash.is_some()) {
                self.drop_lead(key);
                if let Some(state) = mob::state_mut(&mut self.entities, key) {
                    state.leash = None;
                }
            }
            // A horse drops its saddle and body armour; a donkey, mule or llama drops its
            // chest, what is in it, and its saddle. A baby equine drops no gear at all.
            if let Some(mob) = mob::state_mut(&mut self.entities, key) {
                mob.loot_dropped = true;
            }
            let gear = if baby {
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
            // Nothing may stay behind to be saved, reopened or dropped a second time.
            if let Some(data) = self.entities.data(key) {
                super::equine_chest::clear(data.uuid);
            }
            if kind == ferrite_entity::mob::MobKind::EnderDragon {
                self.dragon_died(ctx, pos);
            }
            // A baby animal drops nothing.
            let drops = if baby {
                Vec::new()
            } else {
                super::drops::mob_drops_by(kind, &mut self.rng, killer, burning)
            };
            for stack in drops
                .into_iter()
                .chain(gear.into_iter().filter(|s| !s.is_empty()).map(|s| {
                    ferrite_protocol::packets::play::ItemStack {
                        count: s.count,
                        item_id: i32::from(s.item),
                    }
                }))
            {
                let block = [
                    pos.x.floor() as i32,
                    pos.y.floor() as i32,
                    pos.z.floor() as i32,
                ];
                let record = item::pop_resource(block, stack, &mut self.rng);
                self.entities.insert(record);
            }
        }
        self.tick_end_fight(ctx);
        ferrite_entity::dragon::heal_from_crystals(&mut self.entities);
        self.update_dragon_bar();
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
        self.apply_plugin_fight_requests(ctx);
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
        crate::plugins::dragon::publish(fight.snapshot());
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

    /// Applies what plugins asked of the fight through `World#getEnderDragonBattle`.
    fn apply_plugin_fight_requests(&mut self, ctx: &TickContext) {
        let requests = crate::plugins::dragon::take_requests();
        if requests.is_empty() {
            return;
        }
        let floor = ctx
            .source
            .as_deref()
            .and_then(|source| super::end_fight::podium_floor(|y| source.block(0, y, 0)));
        let mut podiums = Vec::new();
        let mut gateways = Vec::new();
        let mut reset_crystals = false;
        {
            let mut state = lock_game_state(&ctx.state, ctx.profiler.as_deref());
            for request in requests {
                match request {
                    super::end_fight_view::Request::SpawnGatewayAt(at) => gateways.push(at),
                    super::end_fight_view::Request::ResetCrystals => reset_crystals = true,
                    _ => {}
                }
                podiums.extend(state.more.end_fight.apply(request, floor));
            }
        }
        if reset_crystals {
            // Only the crystals standing on a spike go; the four respawn cells stay.
            for index in 0..super::end_fight::SPIKE_COUNT {
                let [x, z] = super::end_fight::spike_column(index);
                let column = ferrite_math::AABB::new(
                    f64::from(x) - 2.0,
                    -64.0,
                    f64::from(z) - 2.0,
                    f64::from(x) + 3.0,
                    320.0,
                    f64::from(z) + 3.0,
                );
                for key in self.entities.query_vec(column) {
                    if ferrite_entity::dragon::is_crystal(&self.entities, key)
                        && let Some(flags) = self.entities.flags_mut(key)
                    {
                        flags.set(ferrite_entity::Flags::REMOVED, true);
                    }
                }
            }
        }
        for at in gateways {
            self.place_blocks(ctx, &super::end_fight::gateway_blocks(at));
            self.set_gateway_age(ctx, at, 0);
        }
        for (level, active) in podiums {
            self.place_blocks(ctx, &super::end_fight::podium_blocks([0, level, 0], active));
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
        super::end_fight::guarded_spike_indices(&tops)
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
                    lock_game_state(&ctx.state, ctx.profiler.as_deref())
                        .more
                        .end_fight
                        .finish_respawn();
                }
            }
        }
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
        let Some(state) = ferrite_entity::mob::state(&self.entities, key) else {
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
        let Some(state) = ferrite_entity::mob::state(&self.entities, key) else {
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
            if !crate::plugins::facts::is_gliding(uuid) {
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
        op: ferrite_plugin::ShoulderOp,
    ) -> bool {
        use ferrite_plugin::ShoulderOp;
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
            ferrite_plugin::ShoulderOp::Release,
            ferrite_plugin::ShoulderOp::Seat,
        );
        let changed = self.apply_shoulder(&p, side, op);
        self.players.insert(player, p);
        changed
    }

    /// The game-state half of [`Self::apply_shoulder`]: moves parrots between the entity store and
    /// the shoulders. Returns what to announce, as `(side, variant)` with `None` for an empty
    /// shoulder.
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

    /// Releases both shoulders (death, sneak-jump or a plugin request).
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
            if ferrite_entity::horse::is_steerable(&self.entities, key) {
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
                if self
                    .entities
                    .data(key)
                    .and_then(|data| data.mob)
                    .is_some_and(|mob| mob.kind == mob::MobKind::EnderDragon)
                {
                    let yaw = self.entities.data(key).map_or(0.0, |data| data.yaw);
                    let Some(part) = ferrite_entity::dragon::part_boxes(target, yaw)
                        .iter()
                        .find(|(_, part_box)| {
                            combat::within_reach(eye, *part_box, range)
                                && combat::facing(eye, look, part_box.center())
                        })
                        .map(|(part, _)| *part)
                    else {
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
                owner_attacks.push((uuid, key));
                mob::credit(&mut self.entities, key, uuid);
                mob::retaliate(&mut self.entities, key, uuid);
                hurt_keys.push((key, critical));
                // Sweeping: a sword at full charge on the ground hits everything around the
                // target for 1.0 (`Player.attack`'s `sweepAttack`).
                if matches!(hurt, mob::Hurt::Survived { .. } | mob::Hurt::Died)
                    && combat::can_sweep(sword, strength, p.ground, p.survival.sprinting)
                {
                    for other in self.entities.query_vec(bbox.inflate(1.0, 0.25, 1.0)) {
                        if other == key || !mob::is_mob(&self.entities, other) {
                            continue;
                        }
                        mob::hurt(&mut self.entities, other, combat::SWEEP_DAMAGE, Some(feet));
                        mob::credit(&mut self.entities, other, uuid);
                    }
                }
            }
            for (key, critical) in hurt_keys {
                let Some(data) = self.entities.data(key) else {
                    continue;
                };
                let id = VarInt(data.id);
                let yaw = data.yaw;
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
        let Some(p) = self.players.get_mut(&uuid) else {
            return;
        };
        if p.blocks_damage_from([from.x, from.y, from.z]) {
            // A blocked hit costs no health: the shield is disabled for its cooldown and the
            // attacker is knocked back (`LivingEntity.blockUsingShield`).
            p.shield_disabled = 5;
            if let Some(attacker) = attacker {
                let source = ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]);
                mob::knockback(&mut self.entities, attacker, source);
            }
            return;
        }
        if p.hurt(kind, amount) {
            // Vanilla `Mob.doHurtTarget` knocks the hurt player away from the attacker.
            let dx = p.pos[0] - from.x;
            let dz = p.pos[2] - from.z;
            let length = (dx * dx + dz * dz).sqrt().max(1.0e-4);
            let y = if p.ground { 0.4 } else { 0.0 };
            send(
                p,
                &SetEntityMotion {
                    id: VarInt(p.entity),
                    velocity: ferrite_protocol::types::LpVec3([
                        dx / length * 0.4,
                        y,
                        dz / length * 0.4,
                    ]),
                },
            );
        }
    }

    /// What the dragon does to the world: it smashes the blocks inside its body and its breath
    /// clouds burn the players standing in them.
    fn tick_dragon_effects(&mut self, ctx: &TickContext) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        if source.dimension() != super::Dimension::End {
            return;
        }
        {
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
                            let Some(state) =
                                source.block(x, y, z).and_then(|s| u16::try_from(s).ok())
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
        for cloud in &mut self.breath_clouds {
            let Some(damage) = cloud.advance() else {
                continue;
            };
            if damage <= 0.0 {
                continue;
            }
            let centre = cloud.center;
            let radius = cloud.radius();
            let hit: Vec<Uuid> = self
                .players
                .values()
                .filter(|p| {
                    (p.pos[0] - centre.x).hypot(p.pos[2] - centre.z) <= radius
                        && (0.0..=3.0).contains(&(p.pos[1] - centre.y))
                })
                .map(|p| p.uuid)
                .collect();
            for uuid in hit {
                if let Some(p) = self.players.get_mut(&uuid) {
                    p.hurt(DamageKind::Magic, damage);
                }
            }
        }
        self.breath_clouds
            .retain(|cloud| cloud.age < cloud.duration);
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
                    ferrite_entity::dragon::part_boxes(pos, data.yaw).to_vec(),
                ))
            })
            .collect();
        for hit in
            arrow::tick_arrows_with_parts(&mut self.entities, &blocks, &players, &dragon_parts)
        {
            if let Some(p) = self.players.get_mut(&hit.uuid) {
                p.hurt(DamageKind::Arrow, hit.damage);
            }
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
                self.plugin_explosions.push((
                    [landing.pos.x, landing.pos.y, landing.pos.z],
                    1.0,
                    fire,
                    true,
                ));
            }
            // Vanilla ender pearl impact: the thrower arrives where the pearl landed and takes
            // 5 fall damage.
            if landing.kind == arrow::Kind::DragonFireball {
                self.breath_clouds
                    .push(ferrite_entity::dragon::BreathCloud::fireball(landing.pos));
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
            // Vanilla small fireball: 5 damage to what it hits.
            if landing.kind == arrow::Kind::SmallFireball {
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
                    if mob::hurt(&mut self.entities, key, 5.0, Some(at)).is_some() {
                        mob::ignite(&mut self.entities, key, 100);
                    }
                }
                fires.push([
                    at.x.floor() as i32,
                    at.y.floor() as i32,
                    at.z.floor() as i32,
                ]);
            }
            if landing.kind == arrow::Kind::SplashPotion
                && let Some(bottle) = self.thrown_potions.remove(&landing.uuid)
            {
                self.splash_potion([landing.pos.x, landing.pos.y, landing.pos.z], &bottle);
            }
            if landing.kind == arrow::Kind::LingeringPotion
                && let Some(bottle) = self.thrown_potions.remove(&landing.uuid)
            {
                self.linger_potion(landing.pos, &bottle);
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
            // `Player#launchProjectile(Trident)`: vanilla gives the thrown trident a plain trident
            // item, which `land_tridents` drops (or returns, with Loyalty) when the flight ends.
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
    /// Returns whether something was spawned.
    pub fn plugin_spawn_snapshot(
        &mut self,
        snbt: &str,
        uuid: Uuid,
        pos: [f64; 3],
        yaw: f32,
    ) -> bool {
        let Some(record) = crate::plugins::snapshot::record_from_snbt(snbt, uuid, pos, yaw) else {
            return false;
        };
        self.entities.insert(record);
        true
    }

    /// `World#dropItem`: an item entity at `center`; the natural variant scatters it a little
    /// and pops it up like a block drop. An unknown item is ignored.
    pub fn plugin_drop_item(
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
        let entity = if is_shulker {
            super::shulker::take_components(source, pos)
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
    fn drop_broken_blocks(&mut self, p: &mut Player) {
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
        }
        // Like vanilla: the take animation first, the inventory sync at the end of the tick.
        for (viewer, bytes) in sent {
            deliver(&self.players, ctx, viewer, &bytes);
        }
    }
    /// Spawns primed TNT at a block position (vanilla `PrimedTnt` entity).
    fn spawn_tnt(&mut self, pos: [i32; 3], fuse: i32) {
        let center = ferrite_math::Vec3::new(
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]),
            f64::from(pos[2]) + 0.5,
        );
        self.entities
            .insert(ferrite_entity::tnt::new_primed_tnt(center, fuse));
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
        for (pos, fuse) in p.primed_tnt.drain(..) {
            self.spawn_tnt(pos, fuse);
        }
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
                super::dispenser::Action::Nothing => {}
            }
        }
    }

    /// Vanilla `ThrownSplashPotion.onHitAsPotion`: every living entity whose box (grown by the
    /// projectile margin) is within four blocks of the potion's box, found in the box inflated by
    /// 4 sideways and 2 up and down, gets the effects scaled by `1 - distance / 4`. Lasting effects
    /// last `scale * duration + 0.5` ticks (and are skipped when that is a second or less);
    /// instant ones act at `scale` of their strength.
    fn splash_potion(&mut self, center: [f64; 3], stack: &ferrite_item::ItemStack) {
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
        let keys: Vec<_> = self
            .entities
            .keys()
            .iter()
            .copied()
            .filter(|key| {
                self.entities.data(*key).is_some_and(|data| {
                    !data.effects.is_empty()
                        && data
                            .mob
                            .as_ref()
                            .is_some_and(|mob| mob.kind != MobKind::WanderingTrader)
                })
            })
            .collect();
        for key in keys {
            let Some(uuid) = self.entities.data(key).map(|data| data.uuid) else {
                continue;
            };
            for expired in mob::tick_effects(&mut self.entities, key) {
                self.pending_mob_effects
                    .push((uuid, MobEffectNotice::Removed(expired.effect)));
            }
        }
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
    fn linger_potion(&mut self, center: ferrite_math::Vec3, stack: &ferrite_item::ItemStack) {
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
        let cloud =
            ferrite_entity::potion::PotionCloud::lingering(center, &effects).with_color(color);
        self.entities.insert(cloud.spawn_record());
        self.potion_clouds.push(cloud);
    }

    /// Ticks every lingering cloud and applies what it reaches to the players of this region.
    fn tick_potion_clouds(&mut self) {
        if self.potion_clouds.is_empty() {
            return;
        }
        let mut candidates: Vec<(Uuid, [f64; 3], f64, f64)> = self
            .players
            .values()
            .filter(|p| p.gamemode != 3 && !p.survival.dead)
            .map(|p| (p.uuid, p.pos, 0.6, if p.sneaking { 1.5 } else { 1.8 }))
            .collect();
        candidates.extend(self.entities.keys().iter().filter_map(|&key| {
            let data = self.entities.data(key)?;
            let live = self
                .entities
                .flags(key)
                .is_some_and(|f| !f.has(Flags::REMOVED))
                && data.mob.as_ref().is_some_and(|mob| !mob.is_dead());
            let pos = self.entities.pos(key)?;
            live.then_some((data.uuid, [pos.x, pos.y, pos.z], data.width, data.height))
        }));
        let mut clouds = std::mem::take(&mut self.potion_clouds);
        let entities = &mut self.entities;
        let mut pending = Vec::new();
        clouds.retain_mut(|cloud| {
            let Some(applied) = cloud.tick_boxes(candidates.iter().copied()) else {
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
    /// The plugin bridge for events about non-player entities: the live handle on the tick
    /// context first (it works in a region without players), else a player's own handle.
    fn damage_hooks(
        &self,
        ctx: &TickContext,
    ) -> Option<(Arc<dyn ferrite_plugin::ServerHooks>, PluginEventPolicy)> {
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
                let positions: Vec<ferrite_plugin::BlockPosition> = destroyed
                    .iter()
                    .map(|[x, y, z]| ferrite_plugin::BlockPosition {
                        x: *x,
                        y: *y,
                        z: *z,
                    })
                    .collect();
                let center_pos = ferrite_plugin::BlockPosition {
                    x: center[0].floor() as i32,
                    y: center[1].floor() as i32,
                    z: center[2].floor() as i32,
                };
                match hooks.blocks_exploded(plugin_world_id(ctx), center_pos, &positions, power) {
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
            let record = arrow::new_thrown(kind, eye, vel, Some(p.uuid));
            if let Some(contents) = contents {
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
        let viewers: Vec<_> = self
            .players
            .values()
            .map(|p| Viewer {
                key: (p.uuid, p.session),
                pos: ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]),
                view_distance: p.view,
                chunks: &p.loaded,
            })
            .collect();
        let players = &self.players;
        tracker::track(&mut self.entities, &viewers, &mut |viewer, bytes| {
            deliver(players, ctx, viewer, bytes)
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
        let mut initial_drops = Vec::new();
        // The stacks behind `initial_drops`, so drops a plugin leaves alone keep their components.
        let mut dropped_stacks = Vec::new();
        if drop_allowed {
            for slot in 0..ferrite_inventory::SIZE {
                let stack = p.inventory.get(slot);
                if stack.is_empty() {
                    continue;
                }
                let Some(material) = ferrite_item::item_name(stack.item) else {
                    continue;
                };
                dropped_stacks.push(stack.clone());
                initial_drops.push(ferrite_plugin::ItemSnapshot {
                    material: material.to_owned(),
                    amount: u8::try_from(stack.count).unwrap_or(u8::MAX),
                });
            }
        }
        let kind = p.survival.death_cause.unwrap_or(DamageKind::GenericKill);
        // `Entity.awardKillScore` fires `entity_killed_player` for the entity that dealt the
        // killing blow: here a mob's melee hit or an arrow. Explosions are not counted because
        // the damage model does not say whether an entity caused them.
        if matches!(kind, DamageKind::Mob | DamageKind::Arrow) {
            super::recipe_unlock::push_event(p.uuid, super::recipe_unlock::Event::KilledByEntity);
        }
        let message = p
            .survival
            .death_message
            .take()
            .unwrap_or_else(|| kind.death_message(&p.name));
        let initial = ferrite_plugin::PlayerDeath {
            drops: initial_drops,
            dropped_exp: initial_exp,
            keep_inventory: !drop_allowed,
            message: message.clone(),
        };
        let mut outcome = ferrite_plugin::PlayerDeathOutcome {
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
                            .insert(item::pop_resource_stack(pos, &stack, &mut self.rng))
                    }
                    Err(material) => {
                        tracing::warn!(%material, "death drop material is unknown");
                        continue;
                    }
                };
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
        let ids: Vec<_> = self.players.keys().copied().collect();
        let mut explosions: Vec<PendingExplosion> = std::mem::take(&mut self.plugin_explosions)
            .into_iter()
            .map(|(center, power, fire, breaks)| (center, power, fire, breaks, None))
            .collect();
        // Snapshot the live overlay so chunk streaming can replay it without borrowing the
        // region's animation state while players are removed for their turn.
        let moving_pistons = self.moving_pistons.active().to_vec();
        for id in ids {
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
            self.drop_broken_blocks(&mut p);
            self.players.insert(id, p);
        }
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
        let mut changed = self.tick_block_updates(ctx);
        changed.extend(button_changed);
        stage!("vaults");
        // Vanilla `VaultBlockEntity.Server.tick`: a vault wakes up near players and runs its
        // unlocking/ejecting timers. Only vaults in chunks this region holds are ticked.
        let mut vault_changed = Vec::new();
        if let Some(source) = ctx.source.as_deref() {
            self.vaults.discover(source, &self.held_chunks);
            self.vaults
                .blocks_changed(source, &changed, &self.held_chunks);
            let players: Vec<super::vault::VaultPlayer> = self
                .players
                .values()
                .map(|p| super::vault::VaultPlayer {
                    pos: p.pos,
                    spectator: p.gamemode == 3,
                })
                .collect();
            let vault_updates =
                self.vaults
                    .tick_with_delivered(source, &players, &self.held_chunks, |token| {
                        self.entities.keys().iter().any(|&key| {
                            self.entities
                                .data(key)
                                .is_some_and(|data| data.uuid == token)
                        })
                    });
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
            self.hoppers.blocks_changed(source, &changed);
            self.dispensers.blocks_changed(source, &changed);
            let Self {
                hoppers,
                entities,
                rng,
                ..
            } = self;
            hoppers.tick(source, entities, rng);
        }
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
                ctx.dimension == super::Dimension::Overworld
                    && ctx
                        .state
                        .lock()
                        .is_ok_and(|s| s.weather != super::commands::Weather::Clear),
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
        let entities_run = lock_game_state(&ctx.state, ctx.profiler.as_deref())
            .more
            .tick
            .simulating();
        if entities_run {
            item::tick_items(&mut self.entities, &blocks, &mut self.rng);
        }
        stage!("entities");
        let owner_attacks = self.resolve_attacks(ctx);
        self.resolve_uses(ctx);
        if entities_run {
            self.tick_arrows(ctx);
            self.tick_bobbers(&blocks);
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
        }
        let alive: Vec<ferrite_math::Vec3> = self
            .players
            .values()
            .filter(|p| p.gamemode != 3 && !p.survival.dead)
            .map(|p| ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]))
            .collect();
        if entities_run {
            experience::tick_orbs(&mut self.entities, &blocks, &alive, &mut self.rng);
            self.pick_up_experience(ctx);
            self.pick_up_items(ctx);
            self.tick_falling(ctx);
            self.tick_tnt(ctx);
            self.tick_fireworks(ctx);
            self.tick_eyes(ctx);
        }
        stage!("containers");
        self.tick_beacons(ctx);
        self.tick_campfires(ctx);
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
        let campfires: Vec<[i32; 3]> = source
            .block_entities()
            .into_iter()
            .filter(|(pos, entity)| {
                self.held_chunks
                    .contains_key(&ChunkPos(pos[0].div_euclid(16), pos[2].div_euclid(16)))
                    && entity.data.get("id")
                        == Some(&ferrite_nbt::Tag::String("minecraft:campfire".into()))
            })
            .map(|(pos, _)| pos)
            .collect();
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
        let beacons: Vec<[i32; 3]> = source
            .block_entities()
            .into_iter()
            .filter(|(pos, entity)| {
                self.held_chunks
                    .contains_key(&ChunkPos(pos[0].div_euclid(16), pos[2].div_euclid(16)))
                    && entity.data.get("id")
                        == Some(&ferrite_nbt::Tag::String("minecraft:beacon".into()))
            })
            .map(|(pos, _)| pos)
            .collect();
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
        world_id: super::WorldId,
    ) {
        index
            .lock()
            .unwrap()
            .sync(&mut self.published_entities, &self.entities, dimension);
        crate::plugins::facts::publish_entities(
            &mut self.published_facts,
            &self.entities,
            world_id,
        );
    }

    /// A plugin's operation on one entity of this region. Returns whether the entity was here.
    pub fn plugin_entity_op(&mut self, uuid: Uuid, op: &ferrite_plugin::EntityOperation) -> bool {
        use ferrite_math::Vec3;
        use ferrite_plugin::EntityOperation as Op;
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

    /// Marks one entity for removal (`/kill`). The tracker still sends the remove packet and
    /// the normal sweep drops it on this tick or the next.
    /// Hurts one mob (`/damage`) through the same path player attacks use. Returns whether the
    /// entity was found in this region.
    pub fn damage_entity(
        &mut self,
        uuid: Uuid,
        amount: f32,
        origin: Option<[f64; 3]>,
        aggro: Option<Uuid>,
        credit: Option<Uuid>,
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
        if mob::hurt(&mut self.entities, key, amount, source).is_some() {
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
            crate::plugins::facts::forget_entities([data.uuid]);
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
#[cfg(test)]
mod tests {
    use super::*;

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

    fn tick_context() -> TickContext {
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
        use ferrite_plugin::EntityOperation as Op;
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
        // Clear is the bare removal `apply_shoulder` does; Release is `change_shoulder(None)`.
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
            if let Outbound::Packet(bytes) = message {
                let mut body = &bytes[..];
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
    fn a_snapshot_spawns_a_mob_with_its_nbt_at_the_requested_spot() {
        let mut source = Region::default();
        let original = mob::new_mob(
            MobKind::Zombie,
            ferrite_math::Vec3::new(1.5, 64.0, 1.5),
            &mut source.rng,
        );
        let snbt = crate::plugins::snapshot::entity_snbt(&original).expect("serializes");
        let mut region = Region::default();
        let uuid = Uuid::from_u128(0x5a47);
        assert!(!region.plugin_spawn_snapshot("{id:\"minecraft:nope\"}", uuid, [0.0; 3], 0.0));
        assert!(region.plugin_spawn_snapshot(&snbt, uuid, [40.5, 70.0, -9.5], 180.0));
        let key = *region.entities.keys().first().expect("spawned");
        let data = region.entities.data(key).unwrap();
        assert_eq!(data.uuid, uuid);
        assert_eq!(data.yaw, 180.0);
        assert_eq!(region.entities.pos(key).unwrap().x, 40.5);
        assert_eq!(region.entities.keys().len(), 1);
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
            .map(|out| match out {
                Outbound::Packet(bytes) => bytes.to_vec(),
                Outbound::Disconnect(reason) => panic!("unexpected disconnect: {reason}"),
            })
            .collect()
    }

    fn spin_packet(entity: i32, spin: bool) -> Vec<u8> {
        match packet(&spin_metadata(entity, spin)) {
            Outbound::Packet(bytes) => bytes.to_vec(),
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
    fn extra_components_are_applied_and_cleared() {
        let sword = ferrite_item::item_id("diamond_sword").unwrap();
        let mut extra = ferrite_nbt::Compound::new();
        extra.insert("minecraft:max_stack_size".into(), ferrite_nbt::Tag::Int(16));
        extra.insert(
            "minecraft:enchantment_glint_override".into(),
            ferrite_nbt::Tag::Byte(1),
        );
        let meta = ItemMetaFields {
            name: None,
            lore: Vec::new(),
            enchants: Vec::new(),
            unbreakable: false,
            model: None,
            repair: 0,
            flags: Vec::new(),
            custom_data: None,
            book: None,
            extra: Some(extra),
        };
        let stack = with_meta(ferrite_item::ItemStack::with_count(sword, 1), &meta);
        assert_eq!(
            stack
                .components
                .get(ferrite_item::component_ids::MAX_STACK_SIZE),
            Some(&ferrite_item::Component::MaxStackSize(16))
        );
        assert_eq!(
            stack
                .components
                .get(ferrite_item::component_ids::ENCHANTMENT_GLINT_OVERRIDE),
            Some(&ferrite_item::Component::Opaque(ferrite_nbt::Tag::Byte(1)))
        );
        // A later call without the components removes them, so clearing a glint override or a
        // stack limit in a plugin actually clears it on the stack.
        let bare = ItemMetaFields {
            name: None,
            lore: Vec::new(),
            enchants: Vec::new(),
            unbreakable: false,
            model: None,
            repair: 0,
            flags: Vec::new(),
            custom_data: None,
            book: None,
            extra: None,
        };
        let cleared = with_meta(stack, &bare);
        assert!(
            cleared
                .components
                .get(ferrite_item::component_ids::MAX_STACK_SIZE)
                .is_none()
        );
        assert!(
            cleared
                .components
                .get(ferrite_item::component_ids::ENCHANTMENT_GLINT_OVERRIDE)
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

    fn book_meta(book: ferrite_plugin::BookFields) -> ItemMetaFields {
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
        let signed = ferrite_plugin::BookFields {
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

        let quill = ferrite_plugin::BookFields {
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
    fn book_metadata_on_another_item_is_left_off_and_replaces_older_pages() {
        let signed = ferrite_plugin::BookFields {
            written: true,
            title: Some("T".into()),
            author: Some("A".into()),
            generation: 0,
            pages: vec![r#"{"text":"p"}"#.into()],
        };
        // A book and quill cannot take a signed book's pages, and a stone takes none.
        for item in ["writable_book", "stone"] {
            let stack = with_meta(
                ferrite_item::ItemStack::from_name(item).unwrap(),
                &book_meta(signed.clone()),
            );
            assert!(
                stack
                    .components
                    .get(ferrite_item::component_ids::WRITTEN_BOOK_CONTENT)
                    .is_none()
            );
            assert!(
                stack
                    .components
                    .get(ferrite_item::component_ids::WRITABLE_BOOK_CONTENT)
                    .is_none()
            );
        }
        // Writing metadata again replaces the pages instead of appending to them.
        let first = with_meta(
            ferrite_item::ItemStack::from_name("written_book").unwrap(),
            &book_meta(signed.clone()),
        );
        let mut second = signed;
        second.pages = vec![r#"{"text":"a"}"#.into(), r#"{"text":"b"}"#.into()];
        let replaced = with_meta(first, &book_meta(second));
        let Some(ferrite_item::Component::WrittenBookContent(content)) = replaced
            .components
            .get(ferrite_item::component_ids::WRITTEN_BOOK_CONTENT)
        else {
            panic!("a written book component");
        };
        assert_eq!(content.pages.len(), 2);
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
    use ferrite_plugin::{BlockPosition, CommandSender, PluginCommand, PluginCommandResult};

    /// A bridge whose every call fails, standing in for a broken JVM call.
    struct Failing;
    impl ferrite_plugin::ServerHooks for Failing {
        fn player_joined(&self, _: Uuid, _: &str) -> Result<(), ferrite_plugin::Error> {
            Err(ferrite_plugin::Error::InvalidTask)
        }
        fn player_left(&self, _: Uuid, _: &str) -> Result<(), ferrite_plugin::Error> {
            Err(ferrite_plugin::Error::InvalidTask)
        }
        fn chat(&self, _: Uuid, _: &str, _: &str) -> Result<Option<String>, ferrite_plugin::Error> {
            Err(ferrite_plugin::Error::InvalidTask)
        }
        fn player_moved(
            &self,
            _: Uuid,
            _: &str,
            _: &ferrite_plugin::PlayerMove,
        ) -> Result<bool, ferrite_plugin::Error> {
            Err(ferrite_plugin::Error::InvalidTask)
        }
        fn player_interacted(
            &self,
            _: Uuid,
            _: &str,
            _: InteractAction,
            _: BlockPosition,
            _: u8,
            _: &str,
        ) -> Result<bool, ferrite_plugin::Error> {
            Err(ferrite_plugin::Error::InvalidTask)
        }
        fn block_broken(
            &self,
            _: Uuid,
            _: &str,
            _: BlockPosition,
            _: &str,
        ) -> Result<bool, ferrite_plugin::Error> {
            Err(ferrite_plugin::Error::InvalidTask)
        }
        fn block_placed(
            &self,
            _: Uuid,
            _: &str,
            _: BlockPosition,
            _: u8,
            _: &str,
            _: &str,
        ) -> Result<bool, ferrite_plugin::Error> {
            Err(ferrite_plugin::Error::InvalidTask)
        }
        fn player_command(&self, _: Uuid, _: &str, _: &str) -> Result<bool, ferrite_plugin::Error> {
            Err(ferrite_plugin::Error::InvalidTask)
        }
        fn plugin_commands(&self) -> Result<Vec<PluginCommand>, ferrite_plugin::Error> {
            Err(ferrite_plugin::Error::InvalidTask)
        }
        fn run_plugin_command(
            &self,
            _: CommandSender,
            _: &str,
        ) -> Result<PluginCommandResult, ferrite_plugin::Error> {
            Err(ferrite_plugin::Error::InvalidTask)
        }
    }

    fn decide(policy: PluginEventPolicy, result: Result<bool, ferrite_plugin::Error>) -> bool {
        let failing: &dyn ferrite_plugin::ServerHooks = &Failing;
        event_allows(Some(failing), policy, |_| result)
    }

    #[test]
    fn failed_plugin_events_follow_the_configured_policy() {
        assert!(decide(
            PluginEventPolicy::Allow,
            Err(ferrite_plugin::Error::InvalidTask)
        ));
        assert!(!decide(
            PluginEventPolicy::Deny,
            Err(ferrite_plugin::Error::InvalidTask)
        ));
        assert!(!decide(PluginEventPolicy::Allow, Ok(false)));
        assert!(decide(PluginEventPolicy::Deny, Ok(true)));
    }

    #[test]
    fn a_missing_plugin_bridge_always_allows_the_action() {
        assert!(event_allows(None, PluginEventPolicy::Deny, |_| Err(
            ferrite_plugin::Error::InvalidTask
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
                hooks.clone() as Arc<dyn ferrite_plugin::ServerHooks>,
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
                ferrite_plugin::Error::InvalidTask
            )),
            None
        );
        assert_eq!(
            plugin_damage(Some(&Failing), PluginEventPolicy::Allow, 5.0, |_| {
                Err(ferrite_plugin::Error::InvalidTask)
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
        // An egg exists only for the entity types the server simulates.
        assert_eq!(spawn_egg_kind(Some("minecraft:mooshroom_spawn_egg")), None);
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
            &[ferrite_plugin::PluginCommand {
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
}
