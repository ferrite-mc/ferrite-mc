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
    BlockGetter, EntityRecord, EntityStore, Flags, Rng, ai, arrow, chunk_of, combat,
    effect::{ActiveEffects, Effect, EffectEvent, MobEffectInstance},
    experience, item,
    mob::{self, MobKind},
    potion::PotionContents,
    spawn::{self, SpawnEnv},
    tracker::{self, Viewer, ViewerKey},
};
use ferrite_net::game::{Outbound, OutboundSender, ServerboundPlay, packet};
use ferrite_plugin::InteractAction;
use ferrite_profiler::Timings;
use ferrite_protocol::{VarInt, packets::play::*};
use ferrite_region::{ChunkPos, RegionData, SectionPos};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;
pub enum Command {
    Input(ServerboundPlay),
    Leave,
    SetMode(i32),
    Teleport {
        pos: [f64; 3],
        yaw: Option<f32>,
        pitch: Option<f32>,
    },
    /// Move to another dimension with the `Respawn` + position sequence.
    ChangeDimension {
        dimension: super::Dimension,
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
    /// `/kill`: `damageSources().genericKill()` applied on the owning region thread.
    Kill,
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
pub type Tracking = Arc<Mutex<HashMap<Uuid, (HashSet<(i32, i32)>, OutboundSender)>>>;
pub enum Signal {
    Transfer {
        dimension: super::Dimension,
        from: ChunkPos,
        to: ChunkPos,
        player: Box<Player>,
    },
    Hold {
        dimension: super::Dimension,
        chunk: ChunkPos,
        add: bool,
    },
    FluidChange {
        dimension: super::Dimension,
        x: i32,
        y: i32,
        z: i32,
    },
    FinishRelease {
        dimension: super::Dimension,
        chunk: ChunkPos,
        flush: bool,
    },
    /// The player left its old region for another dimension; the world loop inserts it into
    /// the target dimension's region scheduler.
    ChangeDimension {
        from_dimension: super::Dimension,
        from: ChunkPos,
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
        dimension: super::Dimension,
        uuid: Uuid,
        chunk: ChunkPos,
        held: Vec<ChunkPos>,
    },
    /// The entities a chunk held when its last holder released it, for the world loop to write
    /// to the chunk's sidecar before the chunk is unloaded.
    SaveEntities {
        dimension: super::Dimension,
        chunk: ChunkPos,
        entities: Vec<EntityRecord>,
    },
    /// An entity walked into another region section; the main loop hands it to the region
    /// owning `to` through that region's task queue (Folia: only the owner touches it).
    EntityTransfer {
        dimension: super::Dimension,
        to: ChunkPos,
        entity: Box<EntityRecord>,
    },
}
/// What a region tick needs from the world loop.
#[derive(Clone)]
pub struct TickContext {
    pub signals: UnboundedSender<Signal>,
    /// Every online player's sent chunks and connection; reaches players that left the region.
    pub tracking: Tracking,
    /// World blocks, for entity physics.
    pub source: Option<Arc<super::anvil::AnvilChunkSource>>,
    /// The dimension this world runs, for bed and fire rules.
    pub dimension: super::Dimension,
    /// Current weather, for fire and cauldron rules.
    pub raining: bool,
    /// Live game rules (natural regeneration, keepInventory, death messages, ...).
    pub rules: Arc<Mutex<ferrite_command::gamerule::GameRules>>,
    /// World spawn, where a player without a bed respawns (`ServerPlayer.respawn`).
    pub spawn: [f64; 3],
    /// Command-visible entity snapshot this region publishes at the end of each tick.
    pub command_entities: Arc<Mutex<super::entity_index::EntityIndex>>,
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
    fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
        let source = self.source?;
        *self
            .seen
            .borrow_mut()
            .entry((x, y, z))
            .or_insert_with(|| source.block(x, y, z).and_then(|s| u16::try_from(s).ok()))
    }
}
impl SpawnEnv for TickBlocks<'_> {
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
    /// Fire this player lit this tick; the region schedules `FireBlock` ticks for it.
    pub ignited: Vec<[i32; 3]>,
    /// The dimension this player is in; beds explode outside the overworld.
    pub dimension: super::Dimension,
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
}
#[derive(Default)]
pub struct Region {
    pub players: HashMap<Uuid, Player>,
    pub entities: EntityStore,
    generator: FlatGenerator,
    fluid_ticks: ferrite_fluid::FluidTicks,
    random_ticks: super::random_tick::RandomTicks,
    updates: super::neighbors::Updates,
    gravity: super::gravity::Gravity,
    fire: super::fire::FireTicks,
    buttons: super::blocks::ButtonTicks,
    dispensers: super::dispenser::Dispensers,
    hoppers: super::hopper::Hoppers,
    redstone: super::redstone::Redstone,
    held_chunks: HashMap<ChunkPos, usize>,
    /// Tick monitor slot, assigned by the game loop on this region's first tick.
    pub profile_slot: Option<u32>,
    /// Scope tree attached by the game loop; `None` until the first tick.
    pub timings: Option<Arc<Timings>>,
    rng: Rng,
    /// UUIDs this region last published into `TickContext::command_entities`.
    published_entities: HashSet<Uuid>,
}
impl RegionData for Region {
    fn merge(&mut self, mut other: Self) {
        other
            .fluid_ticks
            .transfer_to(&mut self.fluid_ticks, |_| true);
        for (chunk, count) in other.held_chunks {
            *self.held_chunks.entry(chunk).or_default() += count;
        }
        self.gravity.merge(other.gravity);
        self.fire.merge(other.fire);
        self.buttons.merge(other.buttons);
        self.dispensers.merge(other.dispensers);
        self.hoppers.merge(other.hoppers);
        self.redstone.merge(other.redstone);
        self.players.extend(other.players);
        self.entities.merge(other.entities);
        self.published_entities.extend(other.published_entities);
        self.generator.merge(other.generator);
    }
    fn split(&mut self, sections: &HashSet<SectionPos>) -> Self {
        let ids: Vec<_> = self
            .players
            .iter()
            .filter(|(_, p)| sections.contains(&SectionPos(p.chunk.0 >> 4, p.chunk.1 >> 4)))
            .map(|(id, _)| *id)
            .collect();
        let mut result = Self {
            players: HashMap::new(),
            rng: Rng::default(),
            entities: self
                .entities
                .split(|(x, z)| sections.contains(&SectionPos(x >> 4, z >> 4))),
            generator: self.generator.split(sections),
            fluid_ticks: ferrite_fluid::FluidTicks::default(),
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
            dispensers: self.dispensers.split(|x, z| {
                sections.contains(&SectionPos(x.div_euclid(16) >> 4, z.div_euclid(16) >> 4))
            }),
            hoppers: self.hoppers.split(|x, z| {
                sections.contains(&SectionPos(x.div_euclid(16) >> 4, z.div_euclid(16) >> 4))
            }),
            redstone: self.redstone.split(|x, z| {
                sections.contains(&SectionPos(x.div_euclid(16) >> 4, z.div_euclid(16) >> 4))
            }),
            held_chunks: HashMap::new(),
            profile_slot: None,
            timings: self.timings.clone(),
            published_entities: HashSet::new(),
        };
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
        .filter(|(x, z)| (x - center.0).abs() > radius || (z - center.1).abs() > radius)
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
impl Player {
    pub fn refresh_chunks(&mut self) {
        send(
            self,
            &CenterChunk {
                x: VarInt(self.chunk.0),
                z: VarInt(self.chunk.1),
            },
        );
        let (forget, new) = chunk_window((self.chunk.0, self.chunk.1), self.view, &self.loaded);
        for (x, z) in forget {
            send(self, &ForgetChunk { z, x });
            self.loaded.remove(&(x, z));
            if let Some(plugins) = self.plugins.clone() {
                let _ = plugins.chunk_unloaded(ferrite_plugin::default_world_id(), x, z);
            }
            if let Some((loaded, _)) = self.tracking.lock().unwrap().get_mut(&self.uuid) {
                loaded.remove(&(x, z));
            }
            let _ = self.signals.send(Signal::Hold {
                dimension: self.dimension,
                chunk: ChunkPos(x, z),
                add: false,
            });
        }
        self.queue = new.into();
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
    fn chunks_tick(&mut self, generator: &mut FlatGenerator) {
        if self.awaiting_batch || self.queue.is_empty() {
            return;
        }
        // Start loading the whole head of the queue at once; `get` hands back chunks that are ready
        // and spawns background loads for the rest. Without this, chunks became ready one per tick.
        if let Some(source) = &self.source {
            for &(x, z) in self.queue.iter().take(PREFETCH_CHUNKS) {
                source.prefetch(x, z, self.biome);
            }
        }
        let mut tracking = self.tracking.lock().unwrap();
        let count = (self.rate.clamp(1.0, 64.0) as usize).min(self.queue.len());
        let mut chunks = Vec::new();
        for _ in 0..count {
            let Some(&(x, z)) = self.queue.front() else {
                break;
            };
            let packet = if let Some(source) = &self.source {
                match source.get(x, z, self.biome) {
                    None => break,
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
            chunks.push((x, z, packet));
        }
        if chunks.is_empty() {
            return;
        }
        send(self, &ChunkBatchStart);
        let count = chunks.len();
        for (x, z, packet) in chunks {
            let _ = self.outbound.send(Outbound::Packet(packet.into()));
            self.loaded.insert((x, z));
            if let Some(plugins) = self.plugins.clone() {
                let _ = plugins.chunk_loaded(ferrite_plugin::default_world_id(), x, z, true);
            }
            if let Some((loaded, _)) = tracking.get_mut(&self.uuid) {
                loaded.insert((x, z));
            }
            let _ = self.signals.send(Signal::Hold {
                dimension: self.dimension,
                chunk: ChunkPos(x, z),
                add: true,
            });
        }
        send(
            self,
            &ChunkBatchFinished {
                size: VarInt(count as i32),
            },
        );
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
    /// Applies a block change and tells every player tracking the chunk. `expected` keeps the
    /// compare-and-set used by player interaction; commands pass `None`.
    pub(crate) fn change_block(&self, location: i64, expected: Option<u32>, state: u32) -> bool {
        let pos = ferrite_protocol::types::BlockPos::unpack(location);
        let tracking = self.tracking.lock().unwrap();
        if self.source.as_ref().is_some_and(|source| {
            source
                .compare_set_block(pos.x, pos.y, pos.z, expected, state)
                .is_ok()
        }) {
            let chunk = (pos.x.div_euclid(16), pos.z.div_euclid(16));
            // One encoding, shared by every observer.
            let update = packet(&BlockUpdate {
                location,
                block_state: VarInt(state as i32),
            });
            for (loaded, outbound) in tracking.values() {
                if loaded.contains(&chunk) {
                    let _ = outbound.send(update.clone());
                }
            }
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
            for stack in super::block_container::take_contents(source, pos, state) {
                self.menus.spilled.push((pos, stack));
            }
            let name = u16::try_from(state)
                .ok()
                .and_then(ferrite_data::blocks::state)
                .map(|(name, _)| name);
            if name.and_then(super::furnace::kind).is_some() {
                for stack in super::furnace::take_contents(source, pos) {
                    self.menus.spilled.push((pos, stack));
                }
            }
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
        // Vanilla `LeverBlock.use` / `ButtonBlock.use`: a right-click toggles the component
        // instead of placing the held item.
        if let Some(next) = super::redstone::use_block(clicked_state) {
            if self.change_block(p.location, Some(u32::from(clicked_state)), u32::from(next)) {
                self.changed_blocks.push([clicked.x, clicked.y, clicked.z]);
            }
            return;
        }
        // Vanilla `FlintAndSteelItem.useOn` / `FireChargeItem.useOn` on TNT: prime it.
        let held = self.inventory.get(self.hand_slot(p.hand.0)).clone();
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
        self.survival.set_max_health(MAX_HEALTH + bonus);
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
        self.survival.health = 1.0;
        self.survival.dead = false;
        self.survival.death_cause = None;
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
        if self.survival.dead || !(0..=1).contains(&p.hand.0) {
            return;
        }
        let slot = self.hand_slot(p.hand.0);
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
        // Vanilla `LivingEntity.startUsingItem`: a shield raises, a bow starts drawing.
        match stack.name() {
            Some("minecraft:shield") if self.shield_disabled == 0 => {
                self.blocking = true;
                return;
            }
            Some("minecraft:bow") => {
                self.using_bow = true;
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
    fn tick_portal(&mut self) {
        self.portal_cooldown = self.portal_cooldown.saturating_sub(1);
        if self.survival.dead {
            self.portal_ticks = 0;
            return;
        }
        let Some(target) = super::portal::counterpart(self.dimension) else {
            return;
        };
        let Some(source) = self.source.clone() else {
            return;
        };
        let feet = [
            self.pos[0].floor() as i32,
            self.pos[1].floor() as i32,
            self.pos[2].floor() as i32,
        ];
        let in_portal = source
            .block(feet[0], feet[1], feet[2])
            .and_then(|state| u16::try_from(state).ok())
            .is_some_and(super::portal::is_portal);
        if !in_portal {
            self.portal_ticks = 0;
            self.portal_pending = None;
            return;
        }
        if self.portal_cooldown > 0 {
            return;
        }
        if self.portal_pending.is_none() {
            self.portal_ticks += 1;
            if self.portal_ticks < super::portal::WAIT_TICKS {
                return;
            }
            self.portal_ticks = 0;
            let scaled = super::portal::scale(self.dimension, target, self.pos);
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
        let destination = super::portal::find_portal(
            target_source.as_ref(),
            scaled,
            super::portal::SEARCH_RADIUS,
        )
        .or_else(|| super::portal::create_portal(target_source.as_ref(), scaled));
        let Some(destination) = destination else {
            // The destination chunk is still loading; retry on the next tick.
            return;
        };
        self.portal_pending = None;
        self.portal_ticks = 0;
        self.change_dimension(target, destination, self.yaw, self.pitch);
        self.portal_cooldown = super::portal::COOLDOWN_TICKS;
        let sound =
            ferrite_data::registries::id("minecraft:sound_event", "minecraft:block.portal.travel")
                .unwrap_or(0);
        send(
            self,
            &SoundEffect {
                sound: VarInt(sound),
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
        let spawn = match self.plugins.clone() {
            Some(plugins) => plugins
                .player_respawned(self.uuid, &self.name, spawn, 0.0, 0.0)
                .unwrap_or(spawn),
            None => spawn,
        };
        let old_loaded = std::mem::take(&mut self.loaded);
        for (x, z) in old_loaded {
            let _ = self.signals.send(Signal::Hold {
                dimension: self.dimension,
                chunk: ChunkPos(x, z),
                add: false,
            });
        }
        if let Some((loaded, _)) = self.tracking.lock().unwrap().get_mut(&self.uuid) {
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
        let old = self.dimension;
        let old_loaded = std::mem::take(&mut self.loaded);
        for (x, z) in old_loaded {
            let _ = self.signals.send(Signal::Hold {
                dimension: old,
                chunk: ChunkPos(x, z),
                add: false,
            });
        }
        if let Some((loaded, _)) = self.tracking.lock().unwrap().get_mut(&self.uuid) {
            loaded.clear();
        }
        self.dimension = dimension;
        self.source = Some(self.dimensions.source(dimension).clone());
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
        self.awaiting_batch = false;
        self.portal_ticks = 0;
        self.portal_cooldown = 200;
        self.survival.last_pos = pos;
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

    fn update(&mut self) -> bool {
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
                    self.close_container();
                    if let Some(dir) = &self.playerdata
                        && let Err(error) = super::playerdata::save_to(
                            dir,
                            &self.uuid,
                            &super::playerdata::Saved::capture(
                                self.pos,
                                self.yaw,
                                self.pitch,
                                self.gamemode,
                                self.dimension,
                                &self.inventory,
                            ),
                        )
                    {
                        tracing::error!(%error, "failed to save player data");
                    }
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
                Command::SetInventorySlot { slot, item } => {
                    self.set_inventory_slot(slot, item);
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
                ServerboundPlay::Interact(p) => {
                    // The secondary action is a right-click; an attack is the primary hand.
                    if !p.using_secondary_action && p.hand.0 == 0 {
                        self.attacks.push(p.entity_id.0);
                    }
                }
                ServerboundPlay::ContainerClick(p) => self.handle_container_click(p),
                ServerboundPlay::ContainerButtonClick(p) => self.handle_container_button(p),
                ServerboundPlay::RenameItem(p) => self.rename_item(p.name),
                ServerboundPlay::ContainerClose(_) => self.close_container(),
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
                                    }) {
                                        self.break_block(p.location, state);
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
                ServerboundPlay::PlayerCommand(p) => {
                    // Vanilla `ServerboundPlayerCommandPacket.Action.STOP_SLEEPING` (2).
                    if p.action.0 == 2 {
                        super::blocks::wake(self);
                    }
                    // `ServerboundPlayerCommandPacket.Action.START_SPRINTING/STOP_SPRINTING`.
                    if p.entity_id.0 == self.entity {
                        match p.action.0 {
                            3 => self.survival.sprinting = true,
                            4 => self.survival.sprinting = false,
                            _ => {}
                        }
                    }
                }
                ServerboundPlay::SignUpdate(p) => super::blocks::edit_sign(self, &p),
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
                            self.ground = p.flags & 1 != 0;
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
                            self.ground = p.flags & 1 != 0;
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
                        self.ground = p.flags & 1 != 0;
                        changed = true;
                    }
                }
                ServerboundPlay::Flags(p) => {
                    self.ground = p.flags & 1 != 0;
                }
                ServerboundPlay::UseItem(p) => self.use_item(&p),
                // `ServerboundClientCommandPacket.Action.PERFORM_RESPAWN`.
                ServerboundPlay::ClientCommand(p) if p.action.0 == 0 && self.survival.dead => {
                    self.respawn_requested = true;
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
            if let Some((_, outbound)) = ctx.tracking.lock().unwrap().get(&uuid) {
                let _ = outbound.send(packet.clone());
            }
        }
    }
}
impl Region {
    pub fn hold_chunk(&mut self, chunk: ChunkPos) {
        *self.held_chunks.entry(chunk).or_default() += 1;
    }
    pub fn release_chunk(&mut self, chunk: ChunkPos) {
        if let Some(count) = self.held_chunks.get_mut(&chunk) {
            *count -= 1;
            if *count == 0 {
                self.held_chunks.remove(&chunk);
            }
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
        keys.into_iter()
            .filter_map(|key| self.entities.remove(key))
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
        keys.into_iter()
            .filter_map(|key| self.entities.remove(key))
            .collect()
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
        speed: u32,
    ) {
        self.random_ticks
            .tick(source, &self.held_chunks, tracking, speed, &mut self.fire);
    }
    pub fn tick_fluids(&mut self, source: &super::anvil::AnvilChunkSource, tracking: &Tracking) {
        let mut world = super::fluid::RegionFluidWorld::new(source, &self.held_chunks, tracking);
        self.fluid_ticks.advance(&mut world);
    }
    /// Creates a mob at an absolute position (`/summon`).
    pub fn summon(&mut self, kind: MobKind, pos: [f64; 3]) -> bool {
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
        self.entities.insert(record);
        true
    }
    /// Ticks the modelled mobs: goals, movement, fall damage, despawn, natural spawning and
    /// death loot.
    fn tick_mobs(&mut self, ctx: &TickContext, blocks: &TickBlocks<'_>) {
        let positions: Vec<_> = self
            .players
            .values()
            .map(|p| ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]))
            .collect();
        let targets: Vec<_> = self
            .players
            .values()
            .map(|p| ai::Target {
                uuid: p.uuid,
                pos: ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]),
            })
            .collect();
        for event in ai::tick_with(&mut self.entities, blocks, &targets, &mut self.rng) {
            match event {
                ai::MobEvent::Melee {
                    player,
                    damage,
                    from,
                    attacker,
                } => self.damage_player(player, damage, DamageKind::Mob, from, Some(attacker)),
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
                ai::MobEvent::Explode { center, power } => {
                    self.explode([center.x, center.y, center.z], power, ctx);
                    if let Some(source) = ctx.source.as_deref() {
                        let mut broken = Vec::new();
                        self.apply_neighbor_updates(source, ctx, &mut broken);
                        for (pos, state) in broken {
                            self.spawn_block_drops(pos, state);
                        }
                    }
                }
            }
        }
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
                Some((key, mob.kind, self.entities.pos(key)?))
            })
            .collect();
        for (key, kind, pos) in dead {
            if let Some(mob) = mob::state_mut(&mut self.entities, key) {
                mob.loot_dropped = true;
            }
            for stack in super::drops::mob_drops(kind, &mut self.rng) {
                let block = [
                    pos.x.floor() as i32,
                    pos.y.floor() as i32,
                    pos.z.floor() as i32,
                ];
                let record = item::pop_resource(block, stack, &mut self.rng);
                self.entities.insert(record);
            }
        }
    }
    /// Resolves this tick's `serverbound_attack` swings: reach, weapon damage and attack
    /// strength, critical hits, sweeping, knockback and the hurt animation. A drawn bow that
    /// was released this tick fires its arrow first.
    fn resolve_attacks(&mut self, ctx: &TickContext) {
        struct Pending {
            uuid: Uuid,
            attacks: Vec<i32>,
            ticks: i32,
            bow: Option<(f32, f32, f32)>,
        }
        let mut pending = Vec::new();
        for (&uuid, p) in self.players.iter_mut() {
            let attacks = std::mem::take(&mut p.attacks);
            let released = std::mem::take(&mut p.bow_released);
            if attacks.is_empty() && !released {
                continue;
            }
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
            });
        }
        for Pending {
            uuid,
            attacks,
            ticks,
            bow,
        } in pending
        {
            if let Some((charge, yaw, pitch)) = bow
                && charge >= 0.1
            {
                self.fire_bow(uuid, charge, yaw, pitch);
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
                let Some(hurt) = mob::hurt(&mut self.entities, key, damage as f32, Some(feet))
                else {
                    continue;
                };
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

    /// Advances arrow flight and applies arrow hits to the players of this region.
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
        for hit in arrow::tick_arrows(&mut self.entities, &blocks, &players) {
            if let Some(p) = self.players.get_mut(&hit.uuid) {
                p.hurt(DamageKind::Arrow, hit.damage);
            }
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
            self.spawn_block_drops(pos, state);
            changed.push(pos);
        }
        changed
    }

    /// Runs the queued shape updates against the region's blocks.
    fn apply_neighbor_updates(
        &mut self,
        source: &super::anvil::AnvilChunkSource,
        ctx: &TickContext,
        removed: &mut Vec<([i32; 3], u16)>,
    ) {
        let mut world =
            super::fluid::RegionFluidWorld::new(source, &self.held_chunks, &ctx.tracking);
        self.updates.apply(&mut world, removed);
    }

    /// Drops the loot table of a block broken by a neighbor or gravity update.
    fn spawn_block_drops(&mut self, pos: [i32; 3], state: u16) {
        let empty = ferrite_item::ItemStack::empty();
        for stack in super::drops::block_drops(state, &empty, &mut self.rng) {
            let record = item::pop_resource(pos, stack, &mut self.rng);
            self.entities.insert(record);
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
        }
        let mut broken = Vec::new();
        self.apply_neighbor_updates(source, ctx, &mut broken);
        for (pos, state) in broken {
            self.spawn_block_drops(pos, state);
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
            for stack in super::drops::block_drops(state, held, &mut self.rng) {
                let record = item::pop_resource([pos.x, pos.y, pos.z], stack, &mut self.rng);
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
    /// Spawns the TNT a player lit (vanilla `PrimedTnt` at the block position).
    fn spawn_primed_tnt(&mut self, p: &mut Player) {
        for (pos, fuse) in p.primed_tnt.drain(..) {
            self.spawn_tnt(pos, fuse);
        }
    }

    /// Ticks the region's redstone network after this tick's block changes, mirroring
    /// `Level.tickBlockEntities` + `Level.tickBlocks` + scheduled block ticks.
    fn tick_redstone(&mut self, ctx: &TickContext, changed: &[[i32; 3]]) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let players: Vec<[f64; 3]> = self.players.values().map(|p| p.pos).collect();
        let mut world =
            super::fluid::RegionFluidWorld::new(source, &self.held_chunks, &ctx.tracking);
        self.redstone.blocks_changed(&mut world, changed);
        self.redstone.tick(&mut world, &players);
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
                super::dispenser::Action::Fire { pos } => self.ignite_block(pos, ctx),
                super::dispenser::Action::Nothing => {}
            }
        }
    }

    /// Vanilla `ThrownPotion.applySplash`: every player within four blocks gains the potion's
    /// effects, with the duration falling off with distance.
    fn splash_potion(&mut self, center: [f64; 3], stack: &ferrite_item::ItemStack) {
        let effects = match stack.effective_component(ferrite_item::component_ids::POTION_CONTENTS)
        {
            Some(ferrite_item::Component::PotionContents(contents)) => {
                ferrite_entity::potion::effects_from_component(&contents)
            }
            Some(ferrite_item::Component::Opaque(tag)) => PotionContents::from_tag(&tag)
                .map(|contents| contents.all_effects())
                .unwrap_or_default(),
            _ => return,
        };
        if effects.is_empty() {
            return;
        }
        for p in self.players.values_mut() {
            let distance = ((p.pos[0] - center[0]).powi(2)
                + (p.pos[1] - center[1]).powi(2)
                + (p.pos[2] - center[2]).powi(2))
            .sqrt();
            if distance > 4.0 {
                continue;
            }
            let scale = (1.0 - distance / 4.0).max(0.25);
            for instance in &effects {
                let mut instance = *instance;
                if instance.duration > 0 {
                    instance.duration = (f64::from(instance.duration) * scale) as i32;
                }
                p.apply_effect(instance);
            }
        }
    }

    /// A fire charge: light the facing block when fire may be placed there.
    fn ignite_block(&mut self, pos: [i32; 3], ctx: &TickContext) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        let placed = {
            let mut world =
                super::fluid::RegionFluidWorld::new(source, &self.held_chunks, &ctx.tracking);
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
                ctx,
            );
        }
        // Vanilla `Explosion.finalizeExplosion` sets blocks with `Block.UPDATE_NEIGHBORS`.
        let mut broken = Vec::new();
        self.apply_neighbor_updates(source, ctx, &mut broken);
        for (pos, state) in broken {
            self.spawn_block_drops(pos, state);
        }
    }
    /// Vanilla `Explosion`: entity knockback and exposure first, then the ray cast blocks and
    /// their loot-table drops.
    fn explode(&mut self, center: [f64; 3], power: f32, ctx: &TickContext) {
        let Some(source) = ctx.source.as_deref() else {
            return;
        };
        // Players are not in the entity store; vanilla pushes them like any other entity.
        let hits: Vec<(i32, [f64; 3])> = self
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
                let impulse = super::explosion::impulse(
                    source,
                    center,
                    power,
                    ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]),
                    bbox,
                )?;
                Some((p.entity, impulse))
            })
            .collect();
        for (entity, impulse) in hits {
            if let Some(p) = self.players.values().find(|p| p.entity == entity) {
                send(
                    p,
                    &SetEntityMotion {
                        id: VarInt(entity),
                        velocity: ferrite_protocol::types::LpVec3(impulse),
                    },
                );
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
            let Some(impulse) = super::explosion::impulse(source, center, power, pos, bbox) else {
                continue;
            };
            let vel = self.entities.vel(key).unwrap_or_default();
            self.entities.set_vel(
                key,
                ferrite_math::Vec3::new(vel.x + impulse[0], vel.y + impulse[1], vel.z + impulse[2]),
            );
            if let Some(flags) = self.entities.flags_mut(key) {
                flags.set(ferrite_entity::Flags::NEEDS_SYNC, true);
            }
        }
        // Destroyed blocks, with the state read before it turns into air.
        let air = u32::from(ferrite_data::blocks::default_state("air").expect("vanilla air"));
        let destroyed;
        let mut removed = Vec::new();
        {
            let mut world =
                super::fluid::RegionFluidWorld::new(source, &self.held_chunks, &ctx.tracking);
            destroyed = super::explosion::explode_blocks(&world, center, power, &mut self.rng);
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
            self.spawn_block_drops(pos, state);
        }
        for pos in &destroyed {
            self.updates.block_changed(*pos);
        }
        self.schedule_gravity(source, &destroyed);
    }
    /// Spawns what a player threw (Q, clicks outside a screen, overflowing crafts) as item
    /// entities in front of it. Item entities hold plain stacks for now, so components are not
    /// carried over.
    fn spawn_thrown(&mut self, p: &mut Player) {
        for (block, stack) in p.menus.spilled.drain(..) {
            let plain = ItemStack {
                count: stack.count,
                item_id: i32::from(stack.item),
            };
            for record in item::drop_from_container(block, plain, &mut self.rng) {
                self.entities.insert(record);
            }
        }
        for stack in p.menus.thrown.drain(..) {
            let plain = ItemStack {
                count: stack.count,
                item_id: i32::from(stack.item),
            };
            let feet = ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]);
            let record = item::thrown_by_player(feet, 1.62, p.yaw, p.pitch, plain, &mut self.rng);
            self.entities.insert(record);
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
        p.survival.death_processed = true;
        p.survival.eating = None;
        p.breaking = None;
        p.close_container();
        let survival_mode = matches!(p.gamemode, 0 | 2);
        let dropped_exp = if !rules.keep_inventory && survival_mode {
            p.experience_level.saturating_mul(7).min(100)
        } else {
            0
        };
        if !rules.keep_inventory && survival_mode {
            let pos = [
                p.pos[0].floor() as i32,
                p.pos[1].floor() as i32,
                p.pos[2].floor() as i32,
            ];
            for slot in 0..ferrite_inventory::SIZE {
                let stack = p.inventory.get(slot).clone();
                if stack.is_empty() {
                    continue;
                }
                p.inventory.set(slot, ferrite_item::ItemStack::empty());
                let plain = ferrite_protocol::packets::play::ItemStack {
                    count: stack.count,
                    item_id: i32::from(stack.item),
                };
                self.entities
                    .insert(item::pop_resource(pos, plain, &mut self.rng));
            }
            // `ServerPlayer.dropExperience`: `min(level * 7, 100)` points, split by
            // `ExperienceOrb.award` and spawned where the player died.
            let dropped = p.experience_level.saturating_mul(7).min(100);
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
        let kind = p.survival.death_cause.unwrap_or(DamageKind::GenericKill);
        let message = kind.death_message(&p.name);
        if let Some(plugins) = p.plugins.clone() {
            let cause = format!("{kind:?}");
            let _ = plugins.player_died(p.uuid, &p.name, &cause, dropped_exp, &message);
        }
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
            for (_, outbound) in ctx.tracking.lock().unwrap().values() {
                let _ = outbound.send(update.clone());
            }
        }
        tracing::info!(player = %p.name, ?kind, "player died");
    }

    pub fn tick(&mut self, ctx: &TickContext) {
        let timings = self.timings.clone();
        let _tick = timings.as_ref().map(|t| t.scope("region-tick"));
        let rules = SurvivalRules::from(&ctx.rules.lock().unwrap());
        let blocks = TickBlocks {
            source: ctx.source.as_deref(),
            seen: Default::default(),
        };
        let ids: Vec<_> = self.players.keys().copied().collect();
        let mut explosions = Vec::new();
        for id in ids {
            let mut p = self.players.remove(&id).unwrap();
            let updated = match &timings {
                Some(timings) => {
                    let _scope = timings.scope("player-update");
                    p.update()
                }
                None => p.update(),
            };
            if updated {
                p.tick_portal();
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
            self.spawn_thrown(&mut p);
            self.spawn_primed_tnt(&mut p);
            explosions.append(&mut p.explosions);
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
                    dimension: p.dimension,
                    uuid: p.uuid,
                    chunk: p.chunk,
                    held,
                });
                continue;
            }
            if p.dimension != ctx.dimension {
                // The command/portal moved this player to another dimension; the source region
                // family owns the player only until the world loop hands it to the target one.
                let signals = p.signals.clone();
                let from = p.chunk;
                let _ = signals.send(Signal::ChangeDimension {
                    from_dimension: ctx.dimension,
                    from,
                    player: Box::new(p),
                });
                continue;
            }
            let next = ChunkPos(
                (p.pos[0].floor() as i32).div_euclid(16),
                (p.pos[2].floor() as i32).div_euclid(16),
            );
            if next != p.chunk {
                let from = p.chunk;
                p.chunk = next;
                p.refresh_chunks();
                let tx = p.signals.clone();
                let _ = tx.send(Signal::Transfer {
                    dimension: p.dimension,
                    from,
                    to: next,
                    player: Box::new(p),
                });
                continue;
            }
            match &timings {
                Some(timings) => {
                    let _scope = timings.scope("chunk-stream");
                    p.chunks_tick(&mut self.generator);
                }
                None => p.chunks_tick(&mut self.generator),
            }
            self.drop_broken_blocks(&mut p);
            self.players.insert(id, p);
        }
        // Vanilla `BedBlock.explodeBed` runs the ordinary `Explosion` and leaves fire behind.
        for (center, power, fire) in explosions {
            self.explode(center, power, ctx);
            if fire && let Some(source) = ctx.source.as_deref() {
                let pos = [
                    center[0].floor() as i32,
                    center[1].floor() as i32,
                    center[2].floor() as i32,
                ];
                let mut world =
                    super::fluid::RegionFluidWorld::new(source, &self.held_chunks, &ctx.tracking);
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
        // Vanilla `FireBlock.tick` runs on scheduled ticks and changes blocks through the same
        // neighbor-update path as any other block change.
        let mut fire_changed = Vec::new();
        if let Some(source) = ctx.source.as_deref() {
            let mut world =
                super::fluid::RegionFluidWorld::new(source, &self.held_chunks, &ctx.tracking);
            self.fire.tick(
                &mut world,
                ctx.dimension,
                ctx.raining,
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
                    self.spawn_block_drops(pos, state);
                }
            }
            changed.extend(fire_changed);
        }
        if let Some(source) = ctx.source.as_deref() {
            self.schedule_gravity(source, &changed);
        }
        self.tick_gravity(ctx);
        self.tick_redstone(ctx, &changed);
        self.tick_dispensers(ctx);
        let blocks = TickBlocks {
            source: ctx.source.as_deref(),
            seen: Default::default(),
        };
        item::tick_items(&mut self.entities, &blocks, &mut self.rng);
        self.resolve_attacks(ctx);
        self.tick_arrows(ctx);
        self.tick_mobs(ctx, &blocks);
        let alive: Vec<ferrite_math::Vec3> = self
            .players
            .values()
            .filter(|p| p.gamemode != 3 && !p.survival.dead)
            .map(|p| ferrite_math::Vec3::new(p.pos[0], p.pos[1], p.pos[2]))
            .collect();
        experience::tick_orbs(&mut self.entities, &blocks, &alive, &mut self.rng);
        self.pick_up_experience(ctx);
        self.pick_up_items(ctx);
        self.tick_falling(ctx);
        self.tick_tnt(ctx);
        for p in self.players.values_mut() {
            p.tick_containers();
        }
        self.track_entities(ctx);
        self.entities.sweep_removed();
        for entity in self.entities.take_crossing() {
            let (x, z) = entity.chunk();
            let _ = ctx.signals.send(Signal::EntityTransfer {
                dimension: ctx.dimension,
                to: ChunkPos(x, z),
                entity: Box::new(entity),
            });
        }
        self.publish_command_entities(&ctx.command_entities);
    }

    /// Publishes this region's entities for command selectors. Folia keeps the store region
    /// local; commands read the snapshot and route mutations back to this region.
    fn publish_command_entities(&mut self, index: &Arc<Mutex<super::entity_index::EntityIndex>>) {
        index
            .lock()
            .unwrap()
            .sync(&mut self.published_entities, &self.entities);
    }

    /// Marks one entity for removal (`/kill`). The tracker still sends the remove packet and
    /// the normal sweep drops it on this tick or the next.
    pub fn kill_entity(&mut self, uuid: Uuid) -> bool {
        let key = self.entities.keys().iter().copied().find(|key| {
            self.entities
                .data(*key)
                .is_some_and(|data| data.uuid == uuid)
        });
        let Some(key) = key else {
            return false;
        };
        if let Some(flags) = self.entities.flags_mut(key) {
            flags.set(Flags::REMOVED, true);
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
            source: None,
            dimension: crate::game::Dimension::Overworld,
            raining: false,
            rules: Default::default(),
            spawn: [0.0, 65.0, 0.0],
            command_entities: Default::default(),
        });

        let report = timings.report_all(50.0);
        assert!(
            report.rows.iter().any(|row| row.path == "region-tick"),
            "{report}"
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

    #[test]
    fn block_materials_come_from_the_state_table() {
        let stone = ferrite_data::blocks::default_state("stone").unwrap();
        let grass = ferrite_data::blocks::default_state("grass_block").unwrap();
        assert_eq!(material_name(u32::from(stone)), "minecraft:stone");
        assert_eq!(material_name(u32::from(grass)), "minecraft:grass_block");
        assert_eq!(material_name(u32::MAX), "minecraft:air");
    }
}
