//! Vanilla data (registries, tags, block states, entity types, packet ids) extracted from the
//! official server jar.
//!
//! The tables are generated at build time by `build.rs` from the data generator output in
//! `vanilla/26.3/generated/`. When that directory is missing, the build fails with a pointer to
//! `python tools/fetch_vanilla.py`, which produces it.
//!
//! All lookups are binary searches over tables sorted at generation time; nothing is built or
//! allocated at runtime, and names are accepted with or without the `minecraft:` namespace.

/// Strips an optional `minecraft:` namespace so lookups accept both spellings of a name.
fn key(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

pub mod blocks {
    //! Global block state ids (0..[`STATE_COUNT`]) and their properties.

    /// One block property: its name, its values (sorted) and the value the default state uses.
    struct Prop {
        name: &'static str,
        values: &'static [&'static str],
        /// Index into `values`, applied when a lookup omits the property.
        default: u16,
    }

    /// A block, with the state ids of the whole block indexed by property combination.
    struct Block {
        name: &'static str,
        props: &'static [Prop],
        /// Global state id per mixed-radix index of the `props` values (first property is the
        /// most significant digit, values in their order in [`Prop::values`]).
        map: &'static [u16],
    }

    include!(concat!(env!("OUT_DIR"), "/blocks.rs"));

    /// The block with the given name, if vanilla has one.
    fn locate(name: &str) -> Option<&'static Block> {
        let name = super::key(name);
        let index = BLOCKS
            .binary_search_by(|block| super::key(block.name).cmp(name))
            .ok()?;
        BLOCKS.get(index)
    }

    /// Global block state id for a block name (with or without `"minecraft:"`) and property
    /// values. Missing properties use the block's default.
    ///
    /// Returns `None` for an unknown block, an unknown or misspelled property, or a value that
    /// the property does not take.
    pub fn state_id(name: &str, props: &[(&str, &str)]) -> Option<u16> {
        let block = locate(name)?;
        // Unknown property names are an error rather than being silently dropped.
        for (name, _) in props {
            if !block.props.iter().any(|prop| prop.name == *name) {
                return None;
            }
        }

        let mut index = 0;
        let mut stride = block.map.len();
        for prop in block.props {
            stride /= prop.values.len();
            let position = match props.iter().find(|(name, _)| *name == prop.name) {
                Some((_, value)) => prop.values.binary_search(value).ok()?,
                None => prop.default as usize,
            };
            index += position * stride;
        }
        block.map.get(index).copied()
    }

    /// Global block state id of the default state of a block.
    pub fn default_state(name: &str) -> Option<u16> {
        state_id(name, &[])
    }

    /// Name and property list of a state id.
    pub fn state(id: u16) -> Option<(&'static str, &'static [(&'static str, &'static str)])> {
        let index = id as usize;
        let block = BLOCKS.get(*STATE_BLOCK.get(index)? as usize)?;
        let props = PROP_SETS.get(*STATE_PROP_SET.get(index)? as usize)?;
        Some((block.name, props))
    }
}

/// Vanilla piston reactions and block-entity flags, extracted for each block state.
pub mod piston_props {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum PushReaction {
        PushPull,
        PushOnly,
        Destroy,
        Immovable,
        IgnoreEntity,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Properties {
        pub reaction: PushReaction,
        pub has_block_entity: bool,
    }
    include!(concat!(env!("OUT_DIR"), "/piston_props.rs"));
    pub fn get(state: u16) -> Properties {
        let bits = PISTON_STATES.get(usize::from(state)).copied().unwrap_or(3);
        let reaction = match bits & 7 {
            0 => PushReaction::PushPull,
            1 => PushReaction::PushOnly,
            2 => PushReaction::Destroy,
            4 => PushReaction::IgnoreEntity,
            _ => PushReaction::Immovable,
        };
        Properties {
            reaction,
            has_block_entity: bits & 8 != 0,
        }
    }
}

#[cfg(test)]
mod piston_property_tests {
    #[test]
    fn vanilla_piston_properties_cover_block_entities_and_push_reactions() {
        use super::{
            blocks,
            piston_props::{self, PushReaction},
        };
        let get = |name| piston_props::get(blocks::default_state(name).unwrap());
        assert_eq!(get("stone").reaction, PushReaction::PushPull);
        assert_eq!(get("obsidian").reaction, PushReaction::Immovable);
        assert_eq!(get("redstone_wire").reaction, PushReaction::Destroy);
        assert!(get("chest").has_block_entity);
        assert!(!get("stone").has_block_entity);
    }
}

pub mod block_props {
    //! Physical properties of every block state, extracted from the vanilla server by
    //! `tools/extractor` into `generated/ferrite/block_states.json`.

    /// Which fluid fills a block state.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum FluidKind {
        Water,
        FlowingWater,
        Lava,
        FlowingLava,
        /// A fluid without a dedicated variant.
        Other(&'static str),
    }

    /// Physical properties of one block state.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct BlockProps {
        /// Light level the state emits (0..=15).
        pub light_emission: u8,
        /// Light level the state dampens by (vanilla's `getLightDampening`, 0..=15).
        pub light_opacity: u8,
        pub can_occlude: bool,
        pub use_shape_for_light_occlusion: bool,
        pub propagates_skylight_down: bool,
        pub is_air: bool,
        /// Vanilla's `isSolid()`: the state blocks entity motion.
        pub blocks_motion: bool,
        pub is_solid_render: bool,
        pub is_redstone_conductor: bool,
        /// The state's fluid: kind, amount (8 is a full source block) and whether it is falling.
        pub fluid: Option<(FluidKind, u8, bool)>,
        /// Destroy speed in vanilla's units; `-1.0` for states that cannot be broken.
        pub hardness: f32,
        pub requires_correct_tool: bool,
        pub replaceable: bool,
        /// The collision shape is the unit cube.
        pub full_collision: bool,
        /// The collision shape is empty, so entities pass through.
        pub empty_collision: bool,
        /// The occlusion shape is the unit cube.
        pub full_occlusion: bool,
        /// The occlusion shape is empty.
        pub empty_occlusion: bool,
        /// Bit mask of the heightmaps the state counts for: [`WORLD_SURFACE`], [`OCEAN_FLOOR`],
        /// [`MOTION_BLOCKING`] and [`MOTION_BLOCKING_NO_LEAVES`].
        pub heightmaps: u8,
        pub randomly_ticking: bool,
        pub is_signal_source: bool,
        pub has_analog_output: bool,
        pub friction: f32,
        pub speed_factor: f32,
        pub jump_factor: f32,
    }

    /// Per-block metadata extracted alongside the states.
    #[derive(Clone, Copy, Debug)]
    pub struct BlockInfo {
        /// Vanilla block name, e.g. `"minecraft:stone"`.
        pub name: &'static str,
        /// Global state id of the block's default state.
        pub default_state: u16,
        pub explosion_resistance: f32,
        /// Item the block is mined with, `None` for blocks without an item (water, fire).
        pub item: Option<&'static str>,
    }

    /// The state counts for the `WORLD_SURFACE` heightmap.
    pub const WORLD_SURFACE: u8 = 1;
    /// The state counts for the `OCEAN_FLOOR` heightmap.
    pub const OCEAN_FLOOR: u8 = 2;
    /// The state counts for the `MOTION_BLOCKING` heightmap.
    pub const MOTION_BLOCKING: u8 = 4;
    /// The state counts for the `MOTION_BLOCKING_NO_LEAVES` heightmap.
    pub const MOTION_BLOCKING_NO_LEAVES: u8 = 8;

    include!(concat!(env!("OUT_DIR"), "/block_props.rs"));

    /// Physical properties of a global block state id.
    ///
    /// Panics when `state` is not a valid state id; [`crate::blocks::STATE_COUNT`] is the
    /// number of valid ids.
    pub fn get(state: u16) -> &'static BlockProps {
        &PROPS[usize::from(STATE_PROPS[usize::from(state)])]
    }

    /// Collision boxes of a state, each `[min_x, min_y, min_z, max_x, max_y, max_z]`.
    pub fn collision_shape(state: u16) -> &'static [[f64; 6]] {
        SHAPES[usize::from(STATE_COLLISION[usize::from(state)])]
    }

    /// Occlusion boxes of a state, used for light propagation and face culling.
    pub fn occlusion_shape(state: u16) -> &'static [[f64; 6]] {
        SHAPES[usize::from(STATE_OCCLUSION[usize::from(state)])]
    }

    /// Face shape in west, east, down, up, north, south order.
    pub fn face_occlusion(state: u16, direction: usize) -> &'static [[f64; 6]] {
        SHAPES[usize::from(STATE_FACES[usize::from(state)][direction])]
    }

    /// Compact face shape identifier (zero is empty, one is full).
    pub fn face_occlusion_id(state: u16, direction: usize) -> u16 {
        STATE_FACES[usize::from(state)][direction]
    }

    /// Shape boxes for a compact face identifier.
    pub fn shape_by_id(id: u16) -> &'static [[f64; 6]] {
        SHAPES[usize::from(id)]
    }

    /// Per-block metadata for a block name, with or without `"minecraft:"`.
    pub fn block_info(name: &str) -> Option<&'static BlockInfo> {
        let name = super::key(name);
        let index = BLOCK_INFO
            .binary_search_by(|info| super::key(info.name).cmp(name))
            .ok()?;
        BLOCK_INFO.get(index)
    }
}

pub mod entity_props {
    //! Entity type properties, extracted from the vanilla server by `tools/extractor` into
    //! `generated/ferrite/entity_types.json`.

    /// Properties and default attributes of one entity type.
    #[derive(Clone, Copy, Debug)]
    pub struct EntityProps {
        /// Global protocol id, also the index of this entry in the generated table.
        pub id: u16,
        /// Vanilla entity type id, e.g. `"minecraft:zombie"`.
        pub key: &'static str,
        /// Spawn category (`MobCategory`), e.g. `"monster"` or `"creature"`.
        pub category: &'static str,
        /// Base width in blocks.
        pub width: f32,
        /// Base height in blocks.
        pub height: f32,
        pub eye_height: f32,
        /// Whether the dimensions ignore scale modifiers (vanilla's `EntityDimensions.fixed`).
        pub fixed: bool,
        /// Single-point attachments, sorted by name: passenger, vehicle, name tag, warden chest.
        pub attachments: &'static [(&'static str, [f32; 3])],
        /// Chunk tracking range in blocks.
        pub client_tracking_range: i32,
        /// Update interval in ticks; `i32::MAX` means the type never updates.
        pub update_interval: i32,
        pub fire_immune: bool,
        pub can_spawn_far_from_player: bool,
        pub is_summonable: bool,
        pub is_serializable: bool,
        /// Vanilla exposes one serialization flag; this mirrors [`Self::is_serializable`].
        pub can_be_serialized: bool,
        /// Default attribute values for living entity types, sorted by attribute name.
        pub attributes: &'static [(&'static str, f64)],
    }

    include!(concat!(env!("OUT_DIR"), "/entity_props.rs"));

    /// Properties of a global entity type protocol id.
    ///
    /// Panics when `id` is not a valid entity type id; [`ENTITY_TYPE_COUNT`] is the number of
    /// valid ids.
    pub fn get(id: u16) -> &'static EntityProps {
        &ENTITY_TYPES[usize::from(id)]
    }

    /// Properties of an entity type by name, with or without `"minecraft:"`.
    pub fn by_name(name: &str) -> Option<&'static EntityProps> {
        let name = super::key(name);
        let index = ENTITY_BY_NAME
            .binary_search_by(|(key, _)| super::key(key).cmp(name))
            .ok()?;
        ENTITY_BY_NAME.get(index).map(|(_, id)| get(*id))
    }

    /// Base value of one of an entity type's default attributes, e.g.
    /// `default_attribute(entity_props::by_name("zombie").unwrap(), "max_health")`.
    ///
    /// `attribute` accepts names with or without `"minecraft:"`; returns `None` when the type
    /// does not define that attribute (or is not a living entity type).
    pub fn default_attribute(id: u16, attribute: &str) -> Option<f64> {
        let attribute = super::key(attribute);
        let attributes = get(id).attributes;
        let index = attributes
            .binary_search_by(|(name, _)| super::key(name).cmp(attribute))
            .ok()?;
        attributes.get(index).map(|(_, value)| *value)
    }
}

pub mod registries {
    //! Built-in registries (`reports/registries.json`) and data-driven registry entry names
    //! (`data/minecraft/**`).

    /// One built-in registry: entry names (sorted) with their protocol ids.
    struct Registry {
        name: &'static str,
        names: &'static [&'static str],
        ids: &'static [i32],
    }

    include!(concat!(env!("OUT_DIR"), "/registries.rs"));

    /// Built-in registries from `reports/registries.json`, e.g.
    /// `("minecraft:item", "minecraft:stone") -> protocol id`.
    pub fn id(registry: &str, entry: &str) -> Option<i32> {
        let registry = super::key(registry);
        let entry = super::key(entry);
        let index = REGISTRIES
            .binary_search_by(|candidate| super::key(candidate.name).cmp(registry))
            .ok()?;
        let registry = REGISTRIES.get(index)?;
        let index = registry
            .names
            .binary_search_by(|name| super::key(name).cmp(entry))
            .ok()?;
        registry.ids.get(index).copied()
    }

    /// Entry names of a built-in registry, sorted by name.
    pub fn entries(registry: &str) -> Option<&'static [&'static str]> {
        let registry = super::key(registry);
        let index = REGISTRIES
            .binary_search_by(|candidate| super::key(candidate.name).cmp(registry))
            .ok()?;
        REGISTRIES.get(index).map(|registry| registry.names)
    }

    /// Entry names of a data-driven registry: the entry names of
    /// `data/minecraft/<registry>/**/*.json`, sorted the way vanilla sorts them when sending a
    /// registry (by name).
    ///
    /// `registry` is the directory below `data/minecraft`, e.g. `"worldgen/biome"`,
    /// `"dimension_type"` or `"damage_type"`; `minecraft/` is accepted as a prefix.
    pub fn data_entries(registry: &str) -> Option<&'static [&'static str]> {
        let registry = super::key(registry);
        let index = DATA
            .binary_search_by(|(name, _)| name.cmp(&registry))
            .ok()?;
        DATA.get(index).map(|(_, entries)| *entries)
    }
}

pub mod tags {
    //! Built-in registry tags (`data/minecraft/tags/**`), with nested `#tag` references expanded
    //! inside their own registry.

    /// A resolved tag: its registry, its namespaced name and its sorted entries.
    #[derive(Clone, Copy, Debug)]
    pub struct Tag {
        registry: &'static str,
        name: &'static str,
        entries: &'static [&'static str],
    }

    include!(concat!(env!("OUT_DIR"), "/tags.rs"));

    impl Tag {
        /// Registry of the tag, e.g. `"minecraft:block"`.
        pub const fn registry(&self) -> &'static str {
            self.registry
        }

        /// Namespaced tag name, e.g. `"minecraft:logs"`.
        pub const fn name(&self) -> &'static str {
            self.name
        }

        /// The entries the tag resolves to, sorted by name and without duplicates.
        pub const fn entries(&self) -> &'static [&'static str] {
            self.entries
        }

        /// Number of resolved entries.
        pub const fn len(&self) -> usize {
            self.entries.len()
        }

        /// Whether the tag resolves to no entries.
        pub const fn is_empty(&self) -> bool {
            self.entries.is_empty()
        }

        /// Whether the tag contains `entry`; names are accepted with or without `minecraft:`.
        pub fn contains(&self, entry: &str) -> bool {
            let entry = super::key(entry);
            self.entries
                .binary_search_by(|candidate| super::key(candidate).cmp(entry))
                .is_ok()
        }

        /// Whether the tag contains the entry with the bare name `name`, e.g. `"oak_log"`.
        pub fn contains_name(&self, name: &str) -> bool {
            self.contains(name)
        }
    }

    /// Every built-in tag, sorted by registry and then name.
    pub fn all() -> &'static [Tag] {
        TAGS
    }

    /// Registries with built-in tags, sorted by name.
    pub fn registries() -> &'static [&'static str] {
        REGISTRIES
    }

    /// The tag in `registry` named `name`, e.g. `get("block", "logs")` or
    /// `get("minecraft:block", "minecraft:logs")`. The `minecraft:` prefix is optional for both.
    pub fn get(registry: &str, name: &str) -> Option<&'static Tag> {
        let registry = super::key(registry);
        let name = super::key(name);
        let index = TAGS
            .binary_search_by(|tag| {
                (super::key(tag.registry), super::key(tag.name)).cmp(&(registry, name))
            })
            .ok()?;
        TAGS.get(index)
    }

    /// Whether the tag `name` in `registry` contains `entry`; names may omit `minecraft:`.
    pub fn contains(registry: &str, name: &str, entry: &str) -> bool {
        get(registry, name).is_some_and(|tag| tag.contains(entry))
    }

    /// Whether the tag `name` in `registry` contains the entry with the bare name `entry`, e.g.
    /// `contains_name("block", "logs", "oak_log")`.
    pub fn contains_name(registry: &str, name: &str, entry: &str) -> bool {
        get(registry, name).is_some_and(|tag| tag.contains_name(entry))
    }
}

pub mod villager_trades {
    //! Villager trade offers from the 26.3 data generator: `data/minecraft/villager_trade` for
    //! the offers, `trade_set` for how many a profession level draws and the `villager_trade`
    //! tags for the pools.
    //!
    //! Offers whose `merchant_predicate` restricts them to a villager variant are skipped, and
    //! an offer's result keeps its base item when the data attaches `given_item_modifier`
    //! effects (enchantment rolls, discards) that the item component model cannot apply.

    /// One offer definition, with item protocol ids and the counts the data asks for.
    #[derive(Clone, Copy, Debug)]
    pub struct TradeDef {
        /// Namespaced trade name, e.g. `minecraft:farmer/1/wheat_emerald`.
        pub name: &'static str,
        pub wants: u16,
        pub wants_count: i32,
        pub gives: u16,
        pub gives_count: i32,
        /// The optional second cost (`additional_wants`).
        pub second_wants: Option<(u16, i32)>,
        pub max_uses: i32,
        pub xp: i32,
        /// Vanilla `reputation_discount`, used as the price multiplier.
        pub price_multiplier: f32,
    }

    /// The offer pool of one profession level.
    #[derive(Clone, Copy, Debug)]
    pub struct TradeSet {
        /// Namespaced profession, e.g. `minecraft:farmer`.
        pub profession: &'static str,
        pub level: u8,
        /// How many distinct offers the level draws (vanilla `TradeSet.amount`).
        pub amount: usize,
        /// Trade names in the pool, sorted.
        pub trades: &'static [&'static str],
    }

    include!(concat!(env!("OUT_DIR"), "/villager_trades.rs"));

    /// The pool for a profession level, falling back to the closest lower level the data has
    /// (the vanilla smith family only defines some levels).
    pub fn set(profession: &str, level: u8) -> Option<&'static TradeSet> {
        let profession = super::key(profession);
        (1..=level.clamp(1, 5)).rev().find_map(|level| {
            TRADE_SETS
                .iter()
                .find(|set| super::key(set.profession) == profession && set.level == level)
        })
    }

    /// A trade definition by namespaced name.
    pub fn offer(name: &str) -> Option<&'static TradeDef> {
        let name = super::key(name);
        TRADES
            .binary_search_by(|def| super::key(def.name).cmp(name))
            .ok()
            .map(|index| &TRADES[index])
    }

    /// Number of compiled trade definitions.
    pub const fn count() -> usize {
        TRADES.len()
    }

    /// Number of profession level pools.
    pub const fn set_count() -> usize {
        TRADE_SETS.len()
    }
}

pub mod packets {
    //! Packet ids per connection state and direction (`reports/packets.json`).

    /// One `(state, bound)` table: packet names (sorted) with their protocol ids.
    struct Table {
        state: &'static str,
        bound: &'static str,
        names: &'static [&'static str],
        ids: &'static [i32],
    }

    include!(concat!(env!("OUT_DIR"), "/packets.rs"));

    /// Packet id from `reports/packets.json`: `(state, bound, "minecraft:name") -> id`, with
    /// `state` in `{"handshake", "status", "login", "configuration", "play"}` and `bound` in
    /// `{"clientbound", "serverbound"}`. The `minecraft:` prefix is optional.
    pub fn id(state: &str, bound: &str, name: &str) -> Option<i32> {
        let name = super::key(name);
        let table = TABLES
            .iter()
            .find(|table| table.state == state && table.bound == bound)?;
        let index = table
            .names
            .binary_search_by(|candidate| super::key(candidate).cmp(name))
            .ok()?;
        table.ids.get(index).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::{blocks, packets, registries, villager_trades};

    #[test]
    fn stone_state_matches_its_default() {
        assert_eq!(
            blocks::state_id("stone", &[]),
            blocks::default_state("minecraft:stone")
        );
        assert!(blocks::default_state("stone").is_some());
        assert_eq!(
            blocks::default_state("minecraft:stone"),
            Some(1),
            "stone is not state 1 in this report"
        );
    }

    #[test]
    fn air_is_state_zero() {
        assert_eq!(blocks::state_id("air", &[]), Some(0));
        assert_eq!(blocks::default_state("minecraft:air"), Some(0));
    }

    #[test]
    fn multi_property_state_round_trips() {
        let expected = [
            ("facing", "east"),
            ("half", "top"),
            ("shape", "straight"),
            ("waterlogged", "false"),
        ];
        let id = blocks::state_id("oak_stairs", &expected).expect("oak_stairs state exists");
        let (name, props) = blocks::state(id).expect("state id is in range");
        assert_eq!(name, "minecraft:oak_stairs");
        assert_eq!(props.len(), expected.len(), "unexpected props {props:?}");
        for (key, value) in expected {
            assert!(
                props.contains(&(key, value)),
                "{key}={value} missing from {props:?}"
            );
        }

        // Property order does not matter, and omitted properties fall back to the default.
        let reordered = blocks::state_id(
            "minecraft:oak_stairs",
            &[
                ("waterlogged", "false"),
                ("shape", "straight"),
                ("half", "top"),
                ("facing", "east"),
            ],
        );
        assert_eq!(reordered, Some(id));

        let partial = blocks::state_id("oak_stairs", &[("facing", "east")]).expect("state exists");
        let (_, props) = blocks::state(partial).expect("state id is in range");
        for defaulted in [
            ("half", "bottom"),
            ("shape", "straight"),
            ("waterlogged", "false"),
        ] {
            assert!(
                props.contains(&defaulted),
                "{defaulted:?} missing from {props:?}"
            );
        }
    }

    #[test]
    fn villager_trade_data_is_compiled_in() {
        assert!(
            villager_trades::count() > 250,
            "{} trades",
            villager_trades::count()
        );
        assert!(
            villager_trades::set_count() >= 60,
            "{} sets",
            villager_trades::set_count()
        );
        let set = villager_trades::set("farmer", 1).expect("a farmer level-1 pool");
        assert_eq!(set.amount, 2);
        assert!(!set.trades.is_empty());
        let def = villager_trades::offer(set.trades[0]).expect("the pool names real trades");
        assert!(def.wants_count > 0);
        assert!(def.gives_count > 0);
        assert!(def.max_uses > 0);
        assert!(villager_trades::set("farmer", 5).is_some());
        // The smith family has no level 2 pool; the closest lower level is used.
        assert_eq!(villager_trades::set("toolsmith", 2).unwrap().level, 1);
        assert!(villager_trades::set("not_a_profession", 1).is_none());
        assert!(villager_trades::offer("minecraft:not/a/trade").is_none());
    }

    #[test]
    fn unknown_lookups_are_none() {
        assert_eq!(blocks::state_id("not_a_block", &[]), None);
        assert_eq!(blocks::state_id("stone", &[("facing", "north")]), None);
        assert_eq!(blocks::state_id("stone", &[("facing", "sideways")]), None);
        assert_eq!(blocks::state_id("oak_stairs", &[("facing", "up")]), None);
        assert_eq!(blocks::default_state("oak_stairs_typo"), None);
        assert_eq!(blocks::state(u16::MAX), None);
    }

    #[test]
    fn every_state_round_trips() {
        assert_eq!(blocks::STATE_COUNT, 35_723, "unexpected state count");
        for id in 0..blocks::STATE_COUNT as u16 {
            let (name, props) =
                blocks::state(id).unwrap_or_else(|| panic!("state {id} is missing"));
            assert_eq!(
                blocks::state_id(name, props),
                Some(id),
                "{name} {props:?} does not round-trip"
            );
        }
    }

    #[test]
    fn built_in_registries() {
        assert_eq!(registries::id("minecraft:item", "minecraft:air"), Some(0));
        assert_eq!(registries::id("item", "air"), Some(0));
        assert_eq!(
            registries::id("minecraft:block", "minecraft:stone"),
            Some(1)
        );
        assert_eq!(
            registries::id("minecraft:item", "minecraft:not_an_item"),
            None
        );
        assert_eq!(registries::id("minecraft:not_a_registry", "air"), None);

        let items = registries::entries("minecraft:item").expect("item registry exists");
        assert!(items.contains(&"minecraft:air"));
        assert!(items.windows(2).all(|pair| pair[0] < pair[1]), "sorted");
        assert_eq!(registries::entries("minecraft:not_a_registry"), None);
    }

    #[test]
    fn data_driven_registries() {
        let biomes = registries::data_entries("worldgen/biome").expect("biomes are generated");
        assert!(biomes.contains(&"minecraft:plains"), "{biomes:?}");
        assert!(biomes.windows(2).all(|pair| pair[0] < pair[1]), "sorted");

        let dimensions = registries::data_entries("dimension_type").expect("dimension types");
        assert!(dimensions.contains(&"minecraft:overworld"));
        assert!(registries::data_entries("damage_type").is_some());
        assert_eq!(registries::data_entries("not_a_registry"), None);
    }

    #[test]
    fn packet_ids() {
        assert_eq!(
            packets::id("handshake", "serverbound", "minecraft:intention"),
            Some(0)
        );
        assert_eq!(
            packets::id("status", "clientbound", "minecraft:status_response"),
            Some(0)
        );
        assert_eq!(
            packets::id("status", "serverbound", "minecraft:status_request"),
            Some(0)
        );
        assert_eq!(packets::id("status", "serverbound", "intention"), None);
        assert_eq!(
            packets::id("handshake", "clientbound", "minecraft:intention"),
            None
        );
        assert_eq!(packets::id("play", "serverbound", "not_a_packet"), None);
    }
}
