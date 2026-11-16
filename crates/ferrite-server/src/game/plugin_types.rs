//! Core-owned command and snapshot types used by the game loop.
//!
//! The Java compatibility layer and JVM host were removed; only the value types the
//! server still passes between its own systems remain here.

use std::path::PathBuf;

#[derive(Default)]
pub struct PluginConfig {
    pub bridge_jar: PathBuf,
    /// The extracted vanilla data pack, so the bridge can read recipe and tag tables.
    pub data_dir: PathBuf,
    pub java_home: Option<PathBuf>,
    pub command_handler: Option<CommandHandler>,
    pub region_context: Option<std::sync::Arc<dyn RegionContext>>,
    pub server_snapshot: ServerSnapshot,
    /// Published before plugins enable, so `onEnable` can query worlds.
    pub world: Option<WorldSnapshot>,
    /// Other dimensions published before plugins enable (nether, end).
    pub worlds: Vec<WorldSnapshot>,
    pub player_handler: Option<PlayerHandler>,
    pub world_handler: Option<WorldHandler>,
    /// Read-only block and biome access for Bukkit's block queries.
    pub world_view: Option<std::sync::Arc<dyn WorldView>>,
    pub server_handler: Option<ServerHandler>,
    /// Receives every Bukkit/Paper method that has no dedicated native (the generic route).
    pub host_call_handler: Option<HostCallHandler>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlockPosition {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}
/// A block break forwarded to Bukkit's `BlockBreakEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockBreak<'a> {
    /// World key: a vanilla dimension key (`minecraft:overworld`) or a runtime world UUID.
    pub world: &'a str,
    /// The block being broken.
    pub block: BlockPosition,
    /// Namespaced block id.
    pub material: &'a str,
}
/// A block click forwarded to Bukkit's `PlayerInteractEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockInteract<'a> {
    /// World key: a vanilla dimension key (`minecraft:overworld`) or a runtime world UUID.
    pub world: &'a str,
    /// The clicked block.
    pub block: BlockPosition,
    /// Clicked face, 0..=5.
    pub face: u8,
    /// Namespaced block id.
    pub material: &'a str,
}
/// Which mouse button touched a block; Bukkit's `PlayerInteractEvent.Action`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum InteractAction {
    LeftClickBlock = 0,
    RightClickBlock = 1,
}
/// Where a player was and where a movement packet takes it; Bukkit's `PlayerMoveEvent`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerMove {
    pub from: [f64; 3],
    pub to: [f64; 3],
    pub from_yaw: f32,
    pub from_pitch: f32,
    pub to_yaw: f32,
    pub to_pitch: f32,
}
#[derive(Clone, Debug)]
pub struct BlockSnapshot {
    pub position: BlockPosition,
    pub material: String,
}
/// Only explicitly supplied blocks are readable; absent positions are unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnHeight {
    pub x: i32,
    pub z: i32,
    pub y: i32,
}
#[derive(Clone, Debug)]
pub struct WorldSnapshot {
    pub id: uuid::Uuid,
    pub name: String,
    pub time: i64,
    pub spawn: BlockPosition,
    pub blocks: Vec<BlockSnapshot>,
    pub heights: Vec<ColumnHeight>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldCommand {
    SetBlock {
        world: uuid::Uuid,
        position: BlockPosition,
        material: String,
    },
}
pub type WorldHandler = std::sync::Arc<dyn Fn(WorldCommand) + Send + Sync>;
/// One block entity's world position and its binary NBT (a named root compound).
pub type BlockEntityData = ([i32; 3], Vec<u8>);
/// Why a block-entity box query could not be answered. A host reports every variant as a
/// failed query rather than as an empty list, so a plugin never mistakes unavailable data for
/// a structure that has no block entities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockEntityError {
    /// The server does not own a world with this UUID.
    UnknownWorld,
    /// At least one chunk covering the queried box is not loaded.
    ChunkNotLoaded,
    /// A block entity's NBT could not be encoded.
    Unreadable,
    /// The box covers more chunks than one host call will inspect.
    TooLarge,
}
/// The vanilla heightmaps a plugin can ask for, named like Bukkit's `HeightMap`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heightmap {
    MotionBlocking,
    MotionBlockingNoLeaves,
    OceanFloor,
    OceanFloorWg,
    WorldSurface,
    WorldSurfaceWg,
}

impl Heightmap {
    /// Parses the Bukkit enum constant name, for example `MOTION_BLOCKING_NO_LEAVES`.
    pub fn from_bukkit(name: &str) -> Option<Self> {
        Some(match name {
            "MOTION_BLOCKING" => Self::MotionBlocking,
            "MOTION_BLOCKING_NO_LEAVES" => Self::MotionBlockingNoLeaves,
            "OCEAN_FLOOR" => Self::OceanFloor,
            "OCEAN_FLOOR_WG" => Self::OceanFloorWg,
            "WORLD_SURFACE" => Self::WorldSurface,
            "WORLD_SURFACE_WG" => Self::WorldSurfaceWg,
            _ => return None,
        })
    }
}

/// A point-in-time copy of one chunk for `Chunk#getChunkSnapshot`.
///
/// Blocks and light are stored in the host's `y`, then `z`, then `x` order, so the index of
/// `(x, y, z)` is `((y - min_y) * 16 + z) * 16 + x`. Biome cells use the same ordering with
/// four-block steps on each axis. Palettes keep repeated state and biome names out of the
/// transfer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkSnapshot {
    pub min_y: i32,
    /// One past the highest block, like `World#getMaxHeight`.
    pub max_y: i32,
    pub block_palette: Vec<String>,
    pub blocks: Vec<u16>,
    /// Stored sky light (0..=15) at each block index.
    pub sky_light: Vec<u8>,
    /// Stored block light (0..=15) at each block index.
    pub block_light: Vec<u8>,
    pub biome_palette: Vec<String>,
    /// Biome palette indexes for the 4x4x4 cells, ordered `y`, `z`, `x`.
    pub biomes: Vec<u16>,
    /// Highest non-air block in each column, ordered `z * 16 + x`; `min_y` when empty.
    pub highest: Vec<i32>,
}

/// Read-only world access for `World#getBlockAt`, `Block#getLightFromSky`, `World#getBiome`
/// and `World#getHighestBlockYAt`.
///
/// Queries may arrive from any plugin thread, so implementations must be thread-safe and
/// return `None` rather than inventing data for an unloaded chunk or an unknown world.
pub trait WorldView: Send + Sync {
    /// The namespaced material key at a position, for example `minecraft:grass_block`.
    fn block(&self, world: uuid::Uuid, x: i32, y: i32, z: i32) -> Option<String>;
    /// Stored sky light at a position (0..=15).
    fn sky_light(&self, world: uuid::Uuid, x: i32, y: i32, z: i32) -> Option<u8>;
    /// The highest non-air block in a column.
    fn highest_block(&self, world: uuid::Uuid, x: i32, z: i32) -> Option<i32>;
    /// The biome key at a position, for example `minecraft:plains`.
    fn biome(&self, world: uuid::Uuid, x: i32, y: i32, z: i32) -> Option<String>;
    /// A point-in-time copy of chunk `(x, z)` (`Chunk#getChunkSnapshot`). A chunk that cannot
    /// be loaded or read answers `None`, never an all-air snapshot.
    fn chunk_snapshot(&self, _world: uuid::Uuid, _x: i32, _z: i32) -> Option<ChunkSnapshot> {
        None
    }
    /// The Y of the highest block counted by a vanilla heightmap in a column, read from the
    /// chunk's stored heightmap (Bukkit `World#getHighestBlockYAt(x, z, HeightMap)`). A column
    /// with no counted block answers one below the world's minimum height, like Paper.
    fn heightmap(&self, _world: uuid::Uuid, _x: i32, _z: i32, _kind: Heightmap) -> Option<i32> {
        None
    }
    /// The light level at a position, the brighter of sky and block light (`Block#getLightLevel`).
    fn light_level(&self, _world: uuid::Uuid, _x: i32, _y: i32, _z: i32) -> Option<u8> {
        None
    }
    /// Whether chunk `(x, z)` of a runtime world is resident right now (`World#isChunkLoaded`).
    /// `None` for a world the view does not know.
    fn chunk_loaded(&self, _world: uuid::Uuid, _x: i32, _z: i32) -> Option<bool> {
        None
    }
    /// Whether chunk `(x, z)` of a runtime world exists: resident or saved on disk
    /// (`World#isChunkGenerated`). Never creates the chunk. `None` for an unknown world.
    fn chunk_generated(&self, _world: uuid::Uuid, _x: i32, _z: i32) -> Option<bool> {
        None
    }
    /// Loads chunk `(x, z)` of a runtime world, generating it when it does not exist
    /// (`World#loadChunk`). `Some(true)` once it is readable; `None` for an unknown world.
    fn load_chunk(&self, _world: uuid::Uuid, _x: i32, _z: i32) -> Option<bool> {
        None
    }
    /// The chunk coordinates the world currently keeps resident (`World#getLoadedChunks`).
    /// `None` when the view cannot enumerate them; an empty answer is a world with no chunk.
    fn loaded_chunks(&self, _world: uuid::Uuid) -> Option<Vec<(i32, i32)>> {
        None
    }
    /// The block entities inside the box `min..=max` of the loaded chunks, each as its position
    /// and binary NBT (a named root compound). `Ok` with an empty vector means the world and
    /// every chunk in the box are loaded but the box has no block entities; an error means the
    /// query could not be answered and must not be treated as an empty result.
    fn block_entities(
        &self,
        _world: uuid::Uuid,
        _min: [i32; 3],
        _max: [i32; 3],
    ) -> Result<Vec<BlockEntityData>, BlockEntityError> {
        Err(BlockEntityError::UnknownWorld)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayerLocation {
    pub world: uuid::Uuid,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
}
/// The metadata a plugin writes onto one stack: name and lore are text JSON, `custom_data` is the
/// persistent data container.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemMetaSpec {
    pub name: Option<String>,
    pub lore: Vec<String>,
    pub enchants: Vec<(String, i32)>,
    pub unbreakable: bool,
    pub model: Option<i32>,
    pub repair: i32,
    pub flags: Vec<String>,
    pub custom_data: Option<ferrite_nbt::Compound>,
    pub book: Option<BookFields>,
    /// Components beyond the dedicated fields, keyed by full component name and carried in
    /// their 26.3 NBT shape (attributes, potion contents, glint override, limits, rarity, item
    /// model, tooltip style, use remainder, banner patterns, fireworks, profile, map colour).
    pub extra: Option<ferrite_nbt::Compound>,
}
/// The book a plugin writes onto a writable or written book (Bukkit `BookMeta`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookFields {
    /// A signed book (`written_book`) rather than a book and quill (`writable_book`).
    pub written: bool,
    /// The title of a signed book.
    pub title: Option<String>,
    /// The author of a signed book.
    pub author: Option<String>,
    /// Copy generation: 0 original, 1 copy of the original, 2 copy of a copy, 3 tattered.
    pub generation: i32,
    /// One entry per page: text component JSON for a signed book, plain text for a writable one.
    pub pages: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemSnapshot {
    pub material: String,
    pub amount: u8,
}
/// The death the server is about to apply, as sent to `PlayerDeathEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerDeath {
    pub drops: Vec<ItemSnapshot>,
    pub dropped_exp: i32,
    pub keep_inventory: bool,
    pub message: String,
}
/// What `PlayerDeathEvent` listeners left behind: the drops and experience to apply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerDeathOutcome {
    pub drops: Vec<ItemSnapshot>,
    pub dropped_exp: i32,
    pub keep_inventory: bool,
}
#[derive(Clone, Debug)]
pub struct PlayerSnapshot {
    pub id: uuid::Uuid,
    pub name: String,
    pub operator: bool,
    pub health: f64,
    pub food: u8,
    pub mode: GameMode,
    pub location: PlayerLocation,
    pub inventory: Vec<Option<ItemSnapshot>>,
    /// NBT save-format stacks aligned with `inventory`, base64 encoded. When present, the JVM
    /// builds the Bukkit stack from this instead of the material/amount pair so components
    /// (name, lore, enchantments, damage, map id, ...) survive the host boundary.
    pub inventory_data: Vec<Option<String>>,
}
/// What a plugin asks of one entity.
#[derive(Clone, Debug, PartialEq)]
pub enum EntityOperation {
    Remove,
    SetVelocity([f64; 3]),
    Teleport([f64; 3]),
    /// Hurts a mob by this many health points.
    Damage(f32),
    /// Replaces the stack of an item entity (`Item#setItemStack`).
    SetItem {
        material: String,
        amount: u8,
    },
    /// Sets the ticks before an item may be picked up (`Item#setPickupDelay`).
    SetPickupDelay(i32),
    /// Restricts pickup to one player, or clears the restriction (`Item#setOwner`).
    SetOwner(Option<uuid::Uuid>),
    /// Makes the item live forever, or lets it age again (`Item#setUnlimitedLifetime`).
    SetUnlimitedLifetime(bool),
    /// Records who threw the item (`Item#setThrower`).
    SetThrower(Option<uuid::Uuid>),
    SetCanMobPickup(bool),
    SetCanPlayerPickup(bool),
    SetWillAge(bool),
    /// Sets an item's remaining durability (`Item#setHealth`).
    SetItemHealth(i32),
    /// Sets a firework rocket's age in ticks (`Firework#setLife`; `detonate` sets it past its life).
    SetFireworkLife(i32),
    /// Sets the age at which a firework rocket bursts (`Firework#setMaxLife`).
    SetFireworkMaxLife(i32),
}

/// One step of showing a scoreboard to a player.
#[derive(Clone, Debug, PartialEq)]
pub enum ScoreboardStep {
    /// Adds (`method` 0), changes (2) or removes (1) an objective.
    Objective {
        name: String,
        method: u8,
        display: String,
        hearts: bool,
    },
    /// Puts an objective in a display slot (0 list, 1 sidebar, 2 below name); empty clears it.
    Display { slot: i32, name: String },
    Score {
        owner: String,
        objective: String,
        score: i32,
    },
    /// Forgets an entry's score in one objective (or all when `objective` is empty).
    Reset { owner: String, objective: String },
    /// A team (the client's `set_player_team` methods): 0 creates it from `params` and `players`,
    /// 1 removes it, 2 changes its `params`, 3 adds `players`, 4 removes them. `params` are the
    /// display name, options bits, name tag visibility id, collision rule id, color id (-1 none),
    /// prefix and suffix.
    Team {
        name: String,
        method: u8,
        params: Vec<String>,
        players: Vec<String>,
    },
}

/// What happens to one boss bar of a player; `id` names the bar.
#[derive(Clone, Debug, PartialEq)]
pub enum BossBarChange {
    Add {
        id: uuid::Uuid,
        name: String,
        progress: f32,
        color: i32,
        overlay: i32,
        flags: u8,
    },
    Remove {
        id: uuid::Uuid,
    },
    Progress {
        id: uuid::Uuid,
        progress: f32,
    },
    Name {
        id: uuid::Uuid,
        name: String,
    },
    Style {
        id: uuid::Uuid,
        color: i32,
        overlay: i32,
    },
    Flags {
        id: uuid::Uuid,
        flags: u8,
    },
}

/// One offer of a plugin-defined merchant (Bukkit `MerchantRecipe`).
#[derive(Clone, Debug, PartialEq)]
pub struct MerchantTrade {
    pub gives: ItemSnapshot,
    pub wants: ItemSnapshot,
    pub second_wants: Option<ItemSnapshot>,
    pub uses: i32,
    pub max_uses: i32,
    pub xp: i32,
    pub price_multiplier: f32,
}
/// A player the server knows but that is not connected.
#[derive(Clone, Debug)]
pub struct OfflinePlayerSnapshot {
    pub id: uuid::Uuid,
    pub name: Option<String>,
    pub operator: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageContent {
    Plain(String),
    AdventureJson(String),
}
/// What a plugin does to one of a player's shoulders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShoulderOp {
    /// Seats the parrot with this uuid there (lifting a live one out of the world).
    Seat(uuid::Uuid),
    /// Empties the shoulder without leaving a parrot behind (`setShoulderEntity*(null)`).
    Clear,
    /// Puts the parrot on that shoulder into the world beside the player.
    Release,
}
/// A packet a plugin sends to one player; the host builds and delivers it.
#[derive(Clone, Debug, PartialEq)]
pub enum PluginPacket {
    /// A fake block at a position, as the block state id the client should show.
    BlockChange {
        pos: [i32; 3],
        state: u32,
    },
    PluginMessage {
        channel: String,
        data: Vec<u8>,
    },
    Transfer {
        host: String,
        port: u16,
    },
    ResourcePackPush {
        id: uuid::Uuid,
        url: String,
        hash: String,
        required: bool,
        /// The prompt as component JSON.
        prompt: Option<String>,
    },
    ResourcePackPop {
        id: Option<uuid::Uuid>,
    },
    /// `Player#spawnParticle`: `ClientboundLevelParticlesPacket` with the particle's option
    /// bytes already encoded by the host.
    Particle {
        particle: i32,
        options: Vec<u8>,
        force: bool,
        pos: [f64; 3],
        spread: [f32; 3],
        speed: [f32; 3],
        count: i32,
        randomization: i32,
    },
    CompassTarget {
        pos: [i32; 3],
    },
    /// An animation of the player's own entity (swing, ...), shown to everyone who has the
    /// player's chunk loaded.
    Animation {
        action: u8,
    },
    /// Resends the open menu and the carried item (`Player#updateInventory`).
    ResyncInventory,
    /// `Player#dropItem(boolean)`: throws one or all of the selected stack.
    DropSelected {
        all: bool,
    },
    /// `Player#dropItem(ItemStack, ...)`: throws a stack that is not taken from the inventory.
    DropStack {
        item: ItemSnapshot,
    },
    /// `Player#openMerchant`: a trading screen with the given offers.
    OpenMerchant {
        title: String,
        trades: Vec<MerchantTrade>,
    },
    /// `Player#openSign`: the sign editor for the sign at `pos`.
    OpenSign {
        pos: [i32; 3],
        front: bool,
    },
    /// `Player#storeCookie`: the client keeps `payload` under `key` across transfers.
    StoreCookie {
        key: String,
        payload: Vec<u8>,
    },
    /// `Player#wakeup`: gets the player out of the bed they sleep in, if any.
    WakeUp,
    /// `Player#retrieveCookie`: asks the client for the cookie under `key`.
    RetrieveCookie {
        key: String,
    },
    /// A boss bar shown to, changed on or removed from this player (`ClientboundBossEventPacket`).
    BossBar(BossBarChange),
    /// One step of showing a scoreboard to this player.
    Scoreboard(ScoreboardStep),
    /// One property of the open menu (`Player#setWindowProperty`, `ClientboundContainerSetDataPacket`).
    WindowProperty {
        id: i16,
        value: i16,
    },
    /// `ClientboundServerLinksPacket`: `(known type index, label, url)` per link.
    ServerLinks(Vec<(Option<i32>, String, String)>),
    /// A sign's front text shown to one player only (`ClientboundBlockEntityDataPacket`).
    SignText {
        pos: [i32; 3],
        lines: [String; 4],
        color: String,
        glowing: bool,
    },
    /// `Player#sendBlockUpdate`: a container, furnace, banner or skull snapshot shown to one player only
    /// (`ClientboundBlockEntityDataPacket`). `items` are `(slot, material, count)`; `furnace` is
    /// `[BurnTime, BurnTimeTotal, CookTime, CookTimeTotal]`.
    TileUpdate {
        pos: [i32; 3],
        entity: String,
        items: Vec<(u8, String, i32)>,
        furnace: Option<[i16; 4]>,
        /// Banner layers `(pattern id, dye color)`.
        patterns: Vec<(String, String)>,
        /// Skull profile `(name, uuid as four ints)`; either part may be absent.
        profile: Option<(Option<String>, Option<[i32; 4]>)>,
        /// Skull profile properties `(name, value, signature)`, e.g. the `textures` property.
        properties: Vec<(String, String, Option<String>)>,
        /// Metadata of the stacks in `items`, by slot.
        metas: Vec<(u8, ItemMetaSpec)>,
        /// Keys of the block entity types without an item container (jukebox, lectern, command block,
        /// spawner, beehive, sculk sensor, structure block), already validated against the family.
        extra: ferrite_nbt::Compound,
    },
    /// `ClientboundSetEquipmentPacket` for one slot of an entity the client knows; `None` is empty.
    Equipment {
        entity_id: i32,
        slot: u8,
        item: Option<ItemSnapshot>,
    },
    /// `ClientboundUpdateMobEffectPacket` for an entity the client knows.
    PotionEffect {
        entity_id: i32,
        effect: i32,
        amplifier: i32,
        duration: i32,
        flags: u8,
    },
    PotionRemove {
        entity_id: i32,
        effect: i32,
    },
    /// An entity event (status byte) of the entity with this network id, shown to this player.
    EntityEvent {
        entity_id: i32,
        status: u8,
    },
    /// A world event (sound or particle effect, `Player#playEffect`) at a block position.
    LevelEvent {
        event: i32,
        pos: [i32; 3],
        data: i32,
    },
    /// The hurt tilt (`Player#playHurtAnimation`) from a yaw, shown to the same viewers.
    HurtAnimation {
        yaw: f32,
    },
    /// The hurt tilt of the entity with this network id, shown only to this player
    /// (`Player#broadcastHurtAnimation` sends one per viewer).
    EntityHurt {
        entity_id: i32,
        yaw: f32,
    },
    /// An item entity flying into the player (`Player#playPickupItemAnimation`), shown to the
    /// viewers of the player's chunk. `item` is the item entity's network id.
    PickupAnimation {
        item: i32,
        amount: i32,
    },
}
/// A survival number a plugin can set directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurvivalStat {
    Saturation,
    Exhaustion,
    Absorption,
    FallDistance,
    Air,
    /// The base `max_health`, before effects such as health boost.
    MaxHealth,
}
/// A map cursor caption: plain text, or a full text component (style, translation, children).
#[derive(Clone, Debug, PartialEq)]
pub enum MapCaption {
    Plain(String),
    Rich(ferrite_nbt::Tag),
}

/// One visible map cursor after the Java bridge has encoded it for the host.
#[derive(Clone, Debug, PartialEq)]
pub struct MapDecoration {
    /// Protocol id in the `minecraft:map_decoration_type` registry.
    pub kind: i32,
    pub x: i8,
    pub y: i8,
    /// Rotation in 0..=15.
    pub direction: u8,
    pub caption: Option<MapCaption>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlayerCommand {
    Message {
        player: uuid::Uuid,
        content: MessageContent,
    },
    Teleport {
        player: uuid::Uuid,
        location: PlayerLocation,
    },
    /// `Player.Spigot#respawn`: respawn a dead player at the world spawn.
    Respawn {
        player: uuid::Uuid,
    },
    SetInventorySlot {
        player: uuid::Uuid,
        slot: u8,
        item: Option<ItemSnapshot>,
    },
    /// How worn the stack in an inventory slot is (the `minecraft:damage` component).
    /// `startUsingItem` (action 0, with the hand 0 main / 1 off), `completeUsingActiveItem`
    /// (1) or `clearActiveItem` (2).
    ItemUse {
        player: uuid::Uuid,
        action: u8,
        hand: u8,
    },
    /// Writes the name, lore, enchantments, unbreakable flag, custom model data, repair cost,
    /// item flags and persistent data of the stack in a slot (`name` and `lore` are text JSON).
    ItemMeta {
        player: uuid::Uuid,
        slot: u8,
        name: Option<String>,
        lore: Vec<String>,
        enchants: Vec<(String, i32)>,
        unbreakable: bool,
        model: Option<i32>,
        repair: i32,
        flags: Vec<String>,
        custom_data: Option<ferrite_nbt::Compound>,
        book: Option<BookFields>,
        extra: Option<ferrite_nbt::Compound>,
    },
    /// Points the filled map in a slot at saved map `id` (Bukkit `MapMeta#setMapView`).
    ItemMapId {
        player: uuid::Uuid,
        slot: u8,
        id: i32,
    },
    ItemDamage {
        player: uuid::Uuid,
        slot: u8,
        damage: i32,
    },
    /// `Player#sleep(Location, boolean)`: the owning region answers the waiting plugin call
    /// through `ticket` with whether the player lay down.
    Sleep {
        player: uuid::Uuid,
        position: [i32; 3],
        force: bool,
        ticket: u64,
    },
    /// `Player#startRiptideAttack`: a riptide spin of `ticks` ticks that hurts each mob it touches
    /// for `damage` once (`LivingEntity#startAutoSpinAttack`).
    RiptideAttack {
        player: uuid::Uuid,
        ticks: u8,
        damage: f32,
    },
    SetOperator {
        player: uuid::Uuid,
        operator: bool,
    },
    DisplayName {
        player: uuid::Uuid,
        component_json: Option<String>,
    },
    /// `Player#chat` with a slash command: runs it as the player, with the permission the player
    /// has, exactly like a command typed in the chat box.
    Chat {
        player: uuid::Uuid,
        message: String,
    },
    /// `Player#breakBlock`: the owning region applies the player's own break rules and reports
    /// the verdict to the waiting caller through `ticket`.
    BreakBlock {
        player: uuid::Uuid,
        position: [i32; 3],
        ticket: u64,
    },
    /// `Player#updateCommands`: sends the player the command tree again, filtered by their
    /// current permission.
    UpdateCommands {
        player: uuid::Uuid,
    },
    /// `World#spawnEntity` / `spawnFallingBlock` in the world with this UUID: the default world
    /// or a runtime world.
    SpawnEntity {
        world: uuid::Uuid,
        /// The shooter of a projectile.
        owner: Option<uuid::Uuid>,
        center: [f64; 3],
        /// The `minecraft:` entity type key.
        kind: String,
        /// The block of a falling block, as a `minecraft:` block key.
        block: Option<String>,
        /// The speed it starts with, in blocks per tick.
        velocity: [f64; 3],
        /// The id the plugin handle carries, so later calls find this entity again.
        uuid: uuid::Uuid,
    },
    /// `Block#setType` in a world the console cannot address (a runtime world): the default
    /// state of `material`, a `minecraft:` block key, at `position`.
    SetBlock {
        world: uuid::Uuid,
        position: [i32; 3],
        material: String,
    },
    /// `World#ferriteSpawnSnapshot`: the entity an SNBT snapshot describes, placed in a runtime
    /// world at `pos` facing `yaw` under `uuid`.
    SpawnSnapshot {
        world: uuid::Uuid,
        snbt: String,
        uuid: uuid::Uuid,
        pos: [f64; 3],
        yaw: f32,
    },
    /// An operation on a spawned or known entity, run by the region that owns it.
    EntityOp {
        uuid: uuid::Uuid,
        op: EntityOperation,
    },
    /// `World#dropItem` / `dropItemNaturally` on the default world.
    DropItem {
        center: [f64; 3],
        /// The `minecraft:` item key.
        item: String,
        amount: u8,
        natural: bool,
        uuid: uuid::Uuid,
    },
    /// `Player#openBook`: shows a written book with this content in the main hand.
    OpenBook {
        player: uuid::Uuid,
        title: String,
        author: String,
        pages: Vec<String>,
    },
    /// `Player#sendMap`: the full 128x128 colour data and visible cursors of a map.
    MapData {
        player: uuid::Uuid,
        id: i32,
        scale: u8,
        locked: bool,
        colors: Vec<u8>,
        decorations: Vec<MapDecoration>,
    },
    /// `World#refreshChunk` on the default world: players that have the chunk get it again.
    RefreshChunk {
        x: i32,
        z: i32,
    },
    /// The passengers of `vehicle` changed (the ride table already holds the new state):
    /// resend its passenger list to the players.
    RideChanged {
        vehicle: uuid::Uuid,
    },
    /// `World#strikeLightning(Effect)` on the default world: the bolt and its thunder, shown
    /// to the players around it. Nothing is burnt or hurt.
    Lightning {
        center: [f64; 3],
    },
    /// `World#createExplosion` in the world with this id (the default world or a runtime world).
    Explosion {
        world: uuid::Uuid,
        center: [f64; 3],
        power: f32,
        fire: bool,
        /// Whether the blast breaks blocks.
        breaks: bool,
    },
    /// Bukkit `hidePlayer`/`showPlayer`: whether `viewer` sees `target` (entity and tab list).
    Visibility {
        viewer: uuid::Uuid,
        target: uuid::Uuid,
        hidden: bool,
    },
    Kick {
        player: uuid::Uuid,
        component_json: Option<String>,
    },
    Title {
        player: uuid::Uuid,
        title: Option<String>,
        subtitle: Option<String>,
        fade_in: i32,
        stay: i32,
        fade_out: i32,
    },
    PlaySound {
        player: uuid::Uuid,
        world: uuid::Uuid,
        sound: String,
        category: SoundCategory,
        x: f64,
        y: f64,
        z: f64,
        volume: f32,
        pitch: f32,
        seed: Option<i64>,
    },
    /// `Player#sendActionBar`, as Adventure JSON.
    ActionBar {
        player: uuid::Uuid,
        component_json: String,
    },
    /// `Player#clearTitle` (`reset: false`) or `Player#resetTitle` (`reset: true`).
    ClearTitle {
        player: uuid::Uuid,
        reset: bool,
    },
    /// `Player#sendPlayerListHeaderAndFooter`, both as Adventure JSON.
    ListHeaderFooter {
        player: uuid::Uuid,
        header_json: String,
        footer_json: String,
    },
    /// Flight and speed abilities. Speeds use Bukkit's scale (`0.1` fly, `0.2` walk).
    Abilities {
        player: uuid::Uuid,
        allow_flight: bool,
        flying: bool,
        invulnerable: bool,
        fly_speed: f32,
        walk_speed: f32,
    },
    /// One `ClientboundGameEventPacket` for this player (demo screen, credits, weather level...).
    GameEvent {
        player: uuid::Uuid,
        event: u8,
        value: f32,
    },
    /// `Player#setShoulderEntityLeft/Right` and `releaseLeft/RightShoulderEntity`.
    Shoulder {
        player: uuid::Uuid,
        left: bool,
        op: ShoulderOp,
    },
    /// Opens a plugin-created chest-style inventory (`Player#openInventory`).
    OpenInventory {
        player: uuid::Uuid,
        title: String,
        rows: u8,
        /// A shulker box inventory (`InventoryType.SHULKER_BOX`, always three rows), which uses
        /// the `shulker_box` menu and refuses shulker boxes.
        shulker: bool,
        items: Vec<Option<ItemSnapshot>>,
    },
    /// Sends one packet to the player (`sendBlockChange`, `sendPluginMessage`, `transfer`, ...).
    Packet {
        player: uuid::Uuid,
        packet: PluginPacket,
    },
    /// Sets one survival number (`Player#setSaturation`, `setFallDistance`, ...).
    Survival {
        player: uuid::Uuid,
        stat: SurvivalStat,
        value: f32,
    },
    /// Starts (`ticks > 0`) or ends (`ticks == 0`) an item cooldown for one item group.
    Cooldown {
        player: uuid::Uuid,
        group: String,
        ticks: i32,
    },
    /// Entity data the player's own client shows (`setGlowing`, `setInvisible`, `setVisualFire`,
    /// `setSilent`, `setGravity`, `setFreezeTicks`); `None` leaves a value as it was.
    EntityData {
        player: uuid::Uuid,
        flags: Option<u8>,
        silent: Option<bool>,
        no_gravity: Option<bool>,
        freeze_ticks: Option<i32>,
    },
    /// Pushes the player (`Player#setVelocity`, `knockback`): the client applies the motion.
    Velocity {
        player: uuid::Uuid,
        velocity: [f64; 3],
    },
    /// `Player#launchProjectile(FishHook)`: casts the player's fishing bobber.
    CastBobber {
        player: uuid::Uuid,
        /// The id the plugin's handle already carries; the bobber takes it when it spawns.
        entity: Option<uuid::Uuid>,
    },
    /// `Player#saveData` (`load: false`) and `Player#loadData` (`load: true`): write the player's
    /// data file, or read it back over the live player.
    PlayerData {
        player: uuid::Uuid,
        load: bool,
    },
    /// `Player#fireworkBoost`: launches a rocket attached to the gliding player.
    FireworkBoost {
        player: uuid::Uuid,
        flight: u8,
        explosions: u8,
        /// The id the plugin's handle already carries; the rocket takes it when it spawns.
        entity: Option<uuid::Uuid>,
    },
    /// Opens a crafting station without a block (`Player#openWorkbench`, `openAnvil`, ...).
    OpenStation {
        player: uuid::Uuid,
        station: String,
    },
    /// Closes whatever menu the player has open (`Player#closeInventory`).
    CloseInventory {
        player: uuid::Uuid,
    },
    /// Generic damage (`Player#damage`); armor and invulnerability apply.
    Damage {
        player: uuid::Uuid,
        amount: f32,
    },
    VisualExperienceCurrentLevel {
        player: uuid::Uuid,
        progress: f32,
    },
    VisualExperience {
        player: uuid::Uuid,
        progress: f32,
        level: i32,
    },
    VisualBlockDamage {
        player: uuid::Uuid,
        position: i64,
        source: i32,
        progress: i8,
    },
    CustomChatCompletions {
        player: uuid::Uuid,
        action: i32,
        entries: Vec<String>,
    },
    OpenVirtualSign {
        player: uuid::Uuid,
        position: i64,
        front: bool,
    },
    VisualHealth {
        player: uuid::Uuid,
        health: f32,
        food: i32,
        saturation: f32,
    },
    SetHealth {
        player: uuid::Uuid,
        health: f64,
    },
    SetFood {
        player: uuid::Uuid,
        food: u8,
    },
    SetGameMode {
        player: uuid::Uuid,
        mode: GameMode,
    },
    SetExperience {
        player: uuid::Uuid,
        level: Option<i32>,
        progress: Option<f32>,
        total: Option<i32>,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundCategory {
    Master,
    Music,
    Records,
    Weather,
    Blocks,
    Hostile,
    Neutral,
    Players,
    Ambient,
    Voice,
    Ui,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameMode {
    Survival,
    Creative,
    Adventure,
    Spectator,
}
pub type PlayerHandler = std::sync::Arc<dyn Fn(PlayerCommand) + Send + Sync>;
/// Stable identity for the main world published to plugins.
///
/// The Rust game addresses chunks by coordinate, so the Java side needs one id that both
/// `register_world` and every player location agree on.
pub fn default_world_id() -> uuid::Uuid {
    uuid::Uuid::from_u128(1)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerCommand {
    BanIp {
        address: std::net::IpAddr,
        reason: Option<String>,
        expires_unix_millis: Option<i64>,
        source: Option<String>,
    },
    PardonIp {
        address: std::net::IpAddr,
    },
    SetWhitelist {
        enabled: bool,
    },
}
pub type ServerHandler = std::sync::Arc<dyn Fn(ServerCommand) + Send + Sync>;
/// Separates arguments in `HostCall::args` on the Java side.
pub const HOST_ARG_SEPARATOR: char = '\u{1f}';
/// Separates the items of a list, map or compound argument (a location, an item stack).
pub const HOST_ITEM_SEPARATOR: char = '\u{1d}';
/// What Java sends for a null argument or reply.
pub const HOST_NULL: &str = "\u{1b}null";
/// One Bukkit/Paper method call forwarded from the embedded JVM.
///
/// `owner` is the API the method belongs to (`Player`, `Server`, ...), `subject` identifies the
/// receiver (a player UUID, or `server`), `method` is the signature key such as
/// `addAttachment(Plugin,String,boolean)` and `args` are the encoded arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostCall {
    pub owner: String,
    pub subject: String,
    pub method: String,
    pub args: Vec<String>,
}
impl HostCall {
    /// The method name without its parameter list.
    pub fn name(&self) -> &str {
        self.method.split('(').next().unwrap_or(&self.method)
    }
}
/// Answers a [`HostCall`]: `Some(text)` is the encoded return value (empty for `void`), `None`
/// means the host has no handler, and Java then throws `UnsupportedOperationException`.
pub type HostCallHandler = std::sync::Arc<dyn Fn(&HostCall) -> Option<String> + Send + Sync>;
/// Server settings made visible to plugins during startup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerSnapshot {
    pub motd: String,
    pub max_players: u16,
    pub port: u16,
    pub view_distance: u8,
    pub whitelist: bool,
    pub online_mode: bool,
    pub banned_ips: Vec<std::net::IpAddr>,
    /// Host part of the bind address, so `Server#getIp` matches the listener.
    pub bind_address: String,
}
impl Default for ServerSnapshot {
    fn default() -> Self {
        Self {
            motd: "Ferrite".into(),
            max_players: 20,
            port: 25565,
            view_distance: 10,
            whitelist: false,
            online_mode: true,
            banned_ips: Vec::new(),
            bind_address: String::new(),
        }
    }
}
/// Server-owned routing and thread ownership information for region-aware plugins.
pub trait RegionContext: Send + Sync {
    fn is_owned_by_current_region(&self, world: uuid::Uuid, chunk_x: i32, chunk_z: i32) -> bool;
    fn schedule_on_region(&self, task: RegionTask);
    fn schedule_on_entity(&self, task: EntityTask);
    fn retire_entity(&self, entity: uuid::Uuid);
}
pub type CommandHandler = std::sync::Arc<dyn Fn(CommandSender, &str) -> bool + Send + Sync>;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("plugin metadata rejected: {0}")]
    Metadata(String),
    #[error("task was not issued or has already been consumed")]
    InvalidTask,
    #[error("region context is not configured")]
    MissingRegionContext,
    #[error("invalid player snapshot")]
    InvalidPlayerSnapshot,
    #[error("invalid offline player snapshot")]
    InvalidOfflinePlayerSnapshot,
    #[error("plugin event returned invalid data: {0}")]
    InvalidEvent(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandSender {
    Console,
    Player(uuid::Uuid),
}
/// A command declared by a plugin's `plugin.yml` or `paper-plugin.yml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginCommand {
    pub name: String,
    pub aliases: Vec<String>,
    pub permission: Option<String>,
    pub usage: Option<String>,
}
/// A message a plugin sent to one player during a callback.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginMessage {
    pub player: uuid::Uuid,
    pub content: MessageContent,
}
/// Result of dispatching a line to a plugin `CommandExecutor`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PluginCommandResult {
    pub handled: bool,
    /// Messages captured from `CommandSender.sendMessage` during the call.
    pub messages: Vec<PluginMessage>,
}
/// Server hooks the game loop forwards to plugins.
pub trait ServerHooks: Send + Sync {
    /// Fires Bukkit's `PlayerJoinEvent` for a player that just joined.
    fn player_joined(&self, id: uuid::Uuid, name: &str) -> Result<(), Error>;
    /// Fires Bukkit's `PlayerQuitEvent` before the player is removed.
    fn player_left(&self, id: uuid::Uuid, name: &str) -> Result<(), Error>;
    /// Fires Paper's `AsyncChatEvent` and Bukkit's `AsyncPlayerChatEvent`.
    ///
    /// Returns the message after listeners ran, or `None` when one cancelled chat.
    fn chat(&self, id: uuid::Uuid, name: &str, message: &str) -> Result<Option<String>, Error>;
    /// Fires Bukkit's `PlayerMoveEvent`; `false` means a listener cancelled the move.
    fn player_moved(
        &self,
        id: uuid::Uuid,
        name: &str,
        movement: &PlayerMove,
    ) -> Result<bool, Error>;
    /// Fires Bukkit's `PlayerInteractEvent` for a block click; `false` means cancelled.
    fn player_interacted(
        &self,
        id: uuid::Uuid,
        name: &str,
        action: InteractAction,
        event: BlockInteract<'_>,
    ) -> Result<bool, Error>;
    /// Fires Bukkit's `BlockBreakEvent` before a block is removed; `false` means cancelled.
    fn block_broken(
        &self,
        id: uuid::Uuid,
        name: &str,
        event: BlockBreak<'_>,
    ) -> Result<bool, Error>;
    /// Fires Bukkit's `BlockPlaceEvent` before a block is placed; `false` means cancelled.
    fn block_placed(
        &self,
        id: uuid::Uuid,
        name: &str,
        block: BlockPosition,
        face: u8,
        placed: &str,
        replaced: &str,
    ) -> Result<bool, Error>;
    /// Fires Bukkit's `EntityChangeBlockEvent` before a non-player entity changes a block;
    /// `false` means cancelled. The default allows the change.
    fn entity_changed_block(
        &self,
        _id: uuid::Uuid,
        _kind: &str,
        _world: uuid::Uuid,
        _block: BlockPosition,
        _from: &str,
        _to: &str,
    ) -> Result<bool, Error> {
        Ok(true)
    }
    /// Fires Bukkit's `BlockExplodeEvent` for the blocks an explosion would destroy. `None`
    /// leaves the host's list untouched; `Some(list)` is what listeners left (empty = cancelled).
    fn blocks_exploded(
        &self,
        _world: uuid::Uuid,
        _center: BlockPosition,
        _blocks: &[BlockPosition],
        _yield: f32,
    ) -> Result<Option<Vec<BlockPosition>>, Error> {
        Ok(None)
    }
    /// Fires Bukkit's `PlayerCommandPreprocessEvent`; `false` means cancelled.
    fn player_command(&self, id: uuid::Uuid, name: &str, message: &str) -> Result<bool, Error>;
    /// Fires Bukkit's `ChunkLoadEvent` after the chunk's packet was queued.
    fn chunk_loaded(&self, _world: uuid::Uuid, _x: i32, _z: i32, _new: bool) -> Result<(), Error> {
        Ok(())
    }
    /// Fires Bukkit's `ChunkUnloadEvent` before the client forgets the chunk.
    fn chunk_unloaded(&self, _world: uuid::Uuid, _x: i32, _z: i32) -> Result<(), Error> {
        Ok(())
    }
    /// Fires Bukkit's `PlayerDeathEvent` once per death and returns the drops and experience to
    /// apply. The default keeps the server's vanilla values.
    fn player_died(
        &self,
        _id: uuid::Uuid,
        _name: &str,
        _cause: &str,
        death: &PlayerDeath,
    ) -> Result<PlayerDeathOutcome, Error> {
        Ok(PlayerDeathOutcome {
            drops: death.drops.clone(),
            dropped_exp: death.dropped_exp,
            keep_inventory: death.keep_inventory,
        })
    }
    /// Fires Bukkit's `PlayerRespawnEvent`; returns the spawn a listener selected.
    fn player_respawned(
        &self,
        _id: uuid::Uuid,
        _name: &str,
        spawn: [f64; 3],
        _yaw: f32,
        _pitch: f32,
    ) -> Result<[f64; 3], Error> {
        Ok(spawn)
    }
    /// Fires Bukkit's `EntityDamageEvent`; returns the damage after listeners.
    fn entity_damaged(
        &self,
        _id: uuid::Uuid,
        _name: &str,
        _cause: &str,
        amount: f64,
    ) -> Result<f64, Error> {
        Ok(amount)
    }
    /// Fires Bukkit's `EntityDeathEvent` for a non-player entity.
    fn entity_died(
        &self,
        _id: uuid::Uuid,
        _kind: &str,
        _position: [f64; 3],
        _dropped_exp: i32,
    ) -> Result<(), Error> {
        Ok(())
    }
    /// Fires Bukkit's `WeatherChangeEvent`; `false` cancels the change.
    fn weather_changing(&self, _world: uuid::Uuid, _to_weather_state: bool) -> Result<bool, Error> {
        Ok(true)
    }
    /// Fires Bukkit's `WorldLoadEvent` for a newly published world.
    fn world_loaded(&self, _world: uuid::Uuid) -> Result<(), Error> {
        Ok(())
    }
    /// Fires Bukkit's `PlayerLevelChangeEvent`; returns the level to use.
    fn player_level_changed(
        &self,
        _id: uuid::Uuid,
        _name: &str,
        _old_level: i32,
        new_level: i32,
    ) -> Result<i32, Error> {
        Ok(new_level)
    }
    /// Fires Bukkit's `PlayerExpChangeEvent`; returns the amount to grant.
    fn player_exp_changed(&self, _id: uuid::Uuid, _name: &str, amount: i32) -> Result<i32, Error> {
        Ok(amount)
    }
    /// Fires Bukkit's `InventoryClickEvent`; `false` cancels the click.
    fn inventory_clicked(
        &self,
        _id: uuid::Uuid,
        _name: &str,
        _slot: i32,
        _click: &str,
        _action: &str,
    ) -> Result<bool, Error> {
        Ok(true)
    }
    /// Fires Bukkit's `InventoryDragEvent` once a drag finishes; `false` cancels it and the
    /// menu is restored to its state when the drag began. The default keeps hosts that do not
    /// care about drags working unchanged.
    fn inventory_dragged(
        &self,
        _id: uuid::Uuid,
        _name: &str,
        _old_cursor: &ItemSnapshot,
        _new_cursor: &ItemSnapshot,
        _items: &[(i32, ItemSnapshot)],
    ) -> Result<bool, Error> {
        Ok(true)
    }
    /// Fires Bukkit's `InventoryCloseEvent` when a menu closes.
    fn inventory_closed(&self, _id: uuid::Uuid, _name: &str) -> Result<(), Error> {
        Ok(())
    }
    /// Fires `PlayerResourcePackStatusEvent` when the client answers a pushed resource pack;
    /// `action` is `ServerboundResourcePackPacket.Action`'s ordinal.
    fn resource_pack_status(
        &self,
        _player: uuid::Uuid,
        _name: &str,
        _pack: uuid::Uuid,
        _action: i32,
    ) -> Result<(), Error> {
        Ok(())
    }
    /// Commands declared by enabled plugins.
    fn plugin_commands(&self) -> Result<Vec<PluginCommand>, Error>;
    /// Runs plugin scheduler tasks and flushes plugin logs for one server tick.
    fn tick(&self) -> Result<(), Error> {
        Ok(())
    }
    /// Publishes a world's identity, spawn and known blocks before plugins query it.
    fn register_world(&self, _world: &WorldSnapshot) -> Result<(), Error> {
        Ok(())
    }
    /// Fires the cancellable Bukkit unload event and removes a published runtime world.
    fn unregister_world(&self, _id: uuid::Uuid) -> Result<bool, Error> {
        Ok(true)
    }
    /// Publishes an online player's location and state for plugin lookups.
    fn register_player(&self, _player: &PlayerSnapshot) -> Result<(), Error> {
        Ok(())
    }
    /// Pushes one inventory slot changed by gameplay to the JVM snapshot that
    /// `Player#getInventory` reads. `data` is the 26.3 item save NBT; `item` is the
    /// material/amount fallback used when the NBT cannot be encoded.
    fn inventory_slot_changed(
        &self,
        _id: uuid::Uuid,
        _slot: u8,
        _item: Option<&ItemSnapshot>,
        _data: Option<&str>,
    ) -> Result<(), Error> {
        Ok(())
    }
    /// Runs a plugin command for `sender`, returning captured `sendMessage` output.
    fn run_plugin_command(
        &self,
        sender: CommandSender,
        line: &str,
    ) -> Result<PluginCommandResult, Error>;
    /// Region-scheduler tasks that became due this tick; the server routes them to the chunk.
    fn take_region_tasks(&self) -> Vec<RegionTask> {
        Vec::new()
    }
    /// Runs one region-scheduler task on the thread that owns its chunk.
    fn run_region_task(&self, _task: RegionTask) -> Result<(), Error> {
        Ok(())
    }
    /// Entity-scheduler tasks that became due this tick; the server routes them to the region
    /// that owns the entity.
    fn take_entity_tasks(&self) -> Vec<EntityTask> {
        Vec::new()
    }
    /// Runs one entity-scheduler task on the thread that owns its entity.
    fn run_entity_task(&self, _task: EntityTask) -> Result<(), Error> {
        Ok(())
    }
    /// Moves the host-side player snapshot with the live player.
    fn update_player_location(
        &self,
        _id: uuid::Uuid,
        _location: &PlayerLocation,
    ) -> Result<(), Error> {
        Ok(())
    }
}

/// A region task ready for the server's region scheduler.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionTask {
    pub id: i64,
    pub world: String,
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub region_aware: bool,
}

/// An entity task to be routed by the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityTask {
    pub id: i64,
    pub entity: String,
    pub region_aware: bool,
}
