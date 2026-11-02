//! Beacon block entities (vanilla `BeaconBlockEntity`).
//!
//! A beacon stores its pyramid level, the chosen primary and secondary effects and the payment
//! in its block entity NBT (`Levels`, `primary_effect`, `secondary_effect`, `Payment`), like
//! vanilla. The pyramid scan, the effect validation and the effects one level grants live here;
//! the game loop refreshes them every 80 ticks for the players in range, and the menu owns the
//! payment slot.

use ferrite_entity::effect::MobEffectInstance;
use ferrite_nbt::{Compound, Tag};
use ferrite_world::BlockEntity;

use super::anvil::AnvilChunkSource;

/// The block entity type name of a beacon.
pub const BLOCK: &str = "minecraft:beacon";

/// Ticks between effect refreshes (vanilla `BeaconBlockEntity` applies its effects every 80
/// ticks, shorter than one beacon effect lasts).
pub const EFFECT_INTERVAL: u32 = 80;

/// Pyramid levels a beacon scans (vanilla `BeaconBlockEntity.updateBase`).
pub const MAX_LEVELS: i32 = 4;

/// Effect range per pyramid level. The Day 8 brief asks for eight blocks per level; vanilla uses
/// `10 * level + 10`, so this is the one deliberate divergence from the vanilla number.
pub const RANGE_PER_LEVEL: f64 = 8.0;

/// The lowest overworld build height, where the pyramid scan stops.
const MIN_Y: i32 = -64;

/// The items a beacon accepts as payment (vanilla `BeaconBlockEntity.PAYMENT_ITEMS`).
pub const PAYMENT_ITEMS: [&str; 5] = [
    "minecraft:iron_ingot",
    "minecraft:gold_ingot",
    "minecraft:emerald",
    "minecraft:diamond",
    "minecraft:netherite_ingot",
];

/// The saved state of a beacon.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BeaconData {
    /// `Levels`: pyramid levels (0 to 4).
    pub levels: i32,
    /// `primary_effect`, e.g. `minecraft:speed`.
    pub primary_effect: Option<String>,
    /// `secondary_effect`, the second effect at level 4.
    pub secondary_effect: Option<String>,
    /// `Payment`: the item id consumed to change the effects.
    pub payment: i32,
}

fn effect(entity: Option<&BlockEntity>, key: &str) -> Option<String> {
    match entity.and_then(|entity| entity.data.get(key)) {
        Some(Tag::String(name)) if !name.is_empty() => Some(name.clone()),
        _ => None,
    }
}

fn number(entity: Option<&BlockEntity>, key: &str) -> i32 {
    match entity.and_then(|entity| entity.data.get(key)) {
        Some(Tag::Int(value)) => *value,
        Some(Tag::Short(value)) => i32::from(*value),
        _ => 0,
    }
}

/// Reads a beacon's saved state; a missing block entity reads as a fresh beacon.
pub fn load(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> BeaconData {
    let entity = source.block_entity(x, y, z);
    let entity = entity.as_ref();
    BeaconData {
        levels: number(entity, "Levels"),
        primary_effect: effect(entity, "primary_effect"),
        secondary_effect: effect(entity, "secondary_effect"),
        payment: number(entity, "Payment"),
    }
}

/// Writes a beacon's state back, creating the block entity when one is missing.
pub fn store(source: &AnvilChunkSource, [x, y, z]: [i32; 3], data: &BeaconData) {
    let Some(type_id) = ferrite_data::registries::id("minecraft:block_entity_type", BLOCK)
        .and_then(|id| u32::try_from(id).ok())
    else {
        return;
    };
    let result = source.update_block_entity(x, y, z, |entity| {
        let current = entity.as_ref().map(|entity| BeaconData {
            levels: number(Some(entity), "Levels"),
            primary_effect: effect(Some(entity), "primary_effect"),
            secondary_effect: effect(Some(entity), "secondary_effect"),
            payment: number(Some(entity), "Payment"),
        });
        if current.as_ref() == Some(data) {
            return (false, ());
        }
        let entity = entity.get_or_insert_with(|| {
            let mut nbt = Compound::new();
            nbt.insert("id".into(), Tag::String(BLOCK.into()));
            nbt.insert("x".into(), Tag::Int(x));
            nbt.insert("y".into(), Tag::Int(y));
            nbt.insert("z".into(), Tag::Int(z));
            BlockEntity { type_id, data: nbt }
        });
        entity.data.insert("Levels".into(), Tag::Int(data.levels));
        match &data.primary_effect {
            Some(effect) => {
                entity
                    .data
                    .insert("primary_effect".into(), Tag::String(effect.clone()));
            }
            None => {
                entity.data.shift_remove("primary_effect");
            }
        }
        match &data.secondary_effect {
            Some(effect) => {
                entity
                    .data
                    .insert("secondary_effect".into(), Tag::String(effect.clone()));
            }
            None => {
                entity.data.shift_remove("secondary_effect");
            }
        }
        entity.data.insert("Payment".into(), Tag::Int(data.payment));
        (true, ())
    });
    if let Err(error) = result {
        tracing::warn!(x, y, z, %error, "could not store beacon state");
    }
}

/// Creates the block entity of a freshly placed beacon, so the menu, `/data merge block` and the
/// chunk save all have somewhere to keep its state.
pub fn ensure(source: &AnvilChunkSource, pos: [i32; 3]) {
    store(source, pos, &BeaconData::default());
}

/// Vanilla `BeaconBlockEntity.updateBase`: counts the complete square layers of
/// `minecraft:beacon_base_blocks` below the beacon (0 to 4), stopping at the first incomplete one.
pub fn scan_levels(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> i32 {
    let Some(base) = ferrite_data::tags::get("block", "beacon_base_blocks") else {
        return 0;
    };
    let mut levels = 0;
    for layer in 1..=MAX_LEVELS {
        let layer_y = y - layer;
        if layer_y < MIN_Y {
            break;
        }
        let mut full = true;
        'square: for dx in -layer..=layer {
            for dz in -layer..=layer {
                let is_base = source
                    .block(x + dx, layer_y, z + dz)
                    .and_then(|state| u16::try_from(state).ok())
                    .and_then(ferrite_data::blocks::state)
                    .is_some_and(|(name, _)| base.contains(name));
                if !is_base {
                    full = false;
                    break 'square;
                }
            }
        }
        if !full {
            break;
        }
        levels = layer;
    }
    levels
}

/// The registry name of a `minecraft:mob_effect` protocol id.
pub fn effect_name(id: i32) -> Option<&'static str> {
    ferrite_data::registries::entries("minecraft:mob_effect")?
        .iter()
        .copied()
        .find(|name| ferrite_data::registries::id("minecraft:mob_effect", name) == Some(id))
}

/// The protocol id of a `minecraft:mob_effect` registry name.
pub fn effect_id(name: &str) -> Option<i32> {
    ferrite_data::registries::id("minecraft:mob_effect", name)
}

fn short(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

/// Whether a primary effect is available at a pyramid level (vanilla
/// `BeaconBlockEntity.BEACON_EFFECTS`: speed and haste at level 1, resistance and jump boost at
/// level 2, strength at level 3).
pub fn valid_primary(levels: i32, effect: i32) -> bool {
    match effect_name(effect).map(short) {
        Some("speed" | "haste") => levels >= 1,
        Some("resistance" | "jump_boost") => levels >= 2,
        Some("strength") => levels >= 3,
        _ => false,
    }
}

/// Whether a secondary effect is available: only a full four-level pyramid grants one, and only
/// regeneration or one of the primary effects (vanilla `BeaconScreen`).
pub fn valid_secondary(levels: i32, effect: i32) -> bool {
    levels >= 4
        && effect_name(effect).is_some_and(|name| {
            matches!(
                short(name),
                "regeneration" | "speed" | "haste" | "resistance" | "jump_boost" | "strength"
            )
        })
}

/// The radius inside which a beacon grants its effects.
pub fn range(levels: i32) -> f64 {
    f64::from(levels.clamp(0, MAX_LEVELS)) * RANGE_PER_LEVEL
}

/// Whether a player at `pos` is inside a beacon's effect range, measured from the block's
/// centre like vanilla's `AABB.inflate(range)`.
pub fn in_range(beacon: [i32; 3], levels: i32, pos: [f64; 3]) -> bool {
    if levels <= 0 {
        return false;
    }
    let center = [
        f64::from(beacon[0]) + 0.5,
        f64::from(beacon[1]) + 0.5,
        f64::from(beacon[2]) + 0.5,
    ];
    let distance_sq: f64 = pos
        .iter()
        .zip(center)
        .map(|(coordinate, center)| (coordinate - center).powi(2))
        .sum();
    distance_sq <= range(levels).powi(2)
}

/// Vanilla beacon effect duration: `(9 + 2 * level) * 20` ticks, renewed every
/// [`EFFECT_INTERVAL`] ticks while the player stays in range.
pub fn effect_duration(levels: i32) -> i32 {
    (9 + levels * 2) * 20
}

/// The effect instances a beacon grants: its primary effect, plus the secondary one at level 4.
/// A level-4 beacon that repeats its primary effect amplifies it instead (vanilla
/// `BeaconBlockEntity.applyEffects`).
pub fn effects(data: &BeaconData) -> Vec<MobEffectInstance> {
    if data.levels <= 0 {
        return Vec::new();
    }
    let Some(primary) = data.primary_effect.as_deref().and_then(effect_id) else {
        return Vec::new();
    };
    let secondary = data.secondary_effect.as_deref().and_then(effect_id);
    let duration = effect_duration(data.levels);
    let amplifier = i32::from(data.levels >= 4 && secondary == Some(primary));
    let mut out = vec![beacon_effect(primary, duration, amplifier)];
    if data.levels >= 4
        && let Some(secondary) = secondary
        && secondary != primary
    {
        out.push(beacon_effect(secondary, duration, 0));
    }
    out
}

/// A beacon effect has no particles of its own rights and counts as ambient, like vanilla's
/// `new MobEffectInstance(effect, duration, amplifier, true, true)`.
fn beacon_effect(effect: i32, duration: i32, amplifier: i32) -> MobEffectInstance {
    MobEffectInstance {
        ambient: true,
        ..MobEffectInstance::new(effect, duration, amplifier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn loaded() -> std::sync::Arc<AnvilChunkSource> {
        let source = AnvilChunkSource::new_memory();
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        source
    }

    fn base(name: &str) -> u32 {
        u32::from(ferrite_data::blocks::default_state(name).expect("vanilla block"))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn beacon_state_round_trips_through_the_block_entity() {
        let source = loaded().await;
        let state =
            u32::from(ferrite_data::blocks::state_id("beacon", &[]).expect("vanilla has a beacon"));
        source.set_block(6, 70, 6, state).unwrap();
        assert_eq!(load(&source, [6, 70, 6]), BeaconData::default());
        let data = BeaconData {
            levels: 3,
            primary_effect: Some("minecraft:speed".into()),
            secondary_effect: None,
            payment: 1,
        };
        store(&source, [6, 70, 6], &data);
        assert_eq!(load(&source, [6, 70, 6]), data);
        let entity = source.block_entity(6, 70, 6).expect("beacon block entity");
        assert_eq!(
            entity.data.get("id"),
            Some(&Tag::String("minecraft:beacon".into()))
        );
        assert_eq!(entity.data.get("Levels"), Some(&Tag::Int(3)));
        // Clearing an effect removes the key instead of writing an empty one.
        let cleared = BeaconData {
            levels: 1,
            primary_effect: None,
            secondary_effect: None,
            payment: 0,
        };
        store(&source, [6, 70, 6], &cleared);
        assert_eq!(load(&source, [6, 70, 6]), cleared);
        assert!(
            source
                .block_entity(6, 70, 6)
                .unwrap()
                .data
                .get("primary_effect")
                .is_none()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pyramid_layers_are_detected_and_a_wrong_base_block_rejects_the_beacon() {
        let source = loaded().await;
        let beacon = u32::from(ferrite_data::blocks::state_id("beacon", &[]).unwrap());
        let [x, y, z] = [4, 70, 4];
        source.set_block(x, y, z, beacon).unwrap();
        for (layer, block) in [
            (1, "iron_block"),
            (2, "gold_block"),
            (3, "diamond_block"),
            (4, "emerald_block"),
        ] {
            let state = base(block);
            for dx in -layer..=layer {
                for dz in -layer..=layer {
                    source.set_block(x + dx, y - layer, z + dz, state).unwrap();
                }
            }
            assert_eq!(scan_levels(&source, [x, y, z]), layer);
        }
        // Every layer of a 4-level pyramid is made of the beacon base tag.
        assert_eq!(scan_levels(&source, [x, y, z]), 4);

        // A single wrong block breaks the bottom layer, so no layer counts.
        source.set_block(x + 1, y - 1, z, base("stone")).unwrap();
        assert_eq!(scan_levels(&source, [x, y, z]), 0);
        source
            .set_block(x + 1, y - 1, z, base("iron_block"))
            .unwrap();

        // A hole in the third layer still leaves the two complete layers below it.
        source.set_block(x - 2, y - 3, z, base("stone")).unwrap();
        assert_eq!(scan_levels(&source, [x, y, z]), 2);
    }

    #[test]
    fn effects_follow_the_pyramid_level_and_the_chosen_effects() {
        let speed = effect_id("minecraft:speed").expect("speed is a registered effect");
        let regeneration = effect_id("minecraft:regeneration").expect("regeneration exists");
        assert_eq!(effect_name(speed), Some("minecraft:speed"));

        let data = BeaconData {
            levels: 3,
            primary_effect: Some("minecraft:speed".into()),
            secondary_effect: Some("minecraft:regeneration".into()),
            payment: 1,
        };
        let applied = effects(&data);
        assert_eq!(applied.len(), 1, "a secondary effect needs level 4");
        assert_eq!(applied[0].effect, speed);
        assert_eq!(applied[0].amplifier, 0);
        assert_eq!(applied[0].duration, effect_duration(3));
        assert!(applied[0].ambient);

        let doubled = BeaconData {
            levels: 4,
            primary_effect: Some("minecraft:regeneration".into()),
            secondary_effect: Some("minecraft:regeneration".into()),
            ..Default::default()
        };
        let applied = effects(&doubled);
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].effect, regeneration);
        assert_eq!(applied[0].amplifier, 1, "a repeated effect is amplified");

        let both = BeaconData {
            levels: 4,
            primary_effect: Some("minecraft:speed".into()),
            secondary_effect: Some("minecraft:regeneration".into()),
            ..Default::default()
        };
        assert_eq!(effects(&both).len(), 2);
        assert_eq!(effects(&BeaconData::default()).len(), 0);

        assert!(valid_primary(1, speed));
        assert!(!valid_primary(1, effect_id("minecraft:strength").unwrap()));
        assert!(valid_primary(3, effect_id("minecraft:strength").unwrap()));
        assert!(!valid_secondary(3, regeneration));
        assert!(valid_secondary(4, regeneration));
        assert!(
            !valid_primary(4, regeneration),
            "regeneration is secondary only"
        );
        assert_eq!(range(1), 8.0);
        assert_eq!(range(4), 32.0);
        assert_eq!(range(0), 0.0);
        let beacon = [4, 70, 4];
        assert!(in_range(beacon, 1, [4.5, 70.5, 11.0]));
        assert!(!in_range(beacon, 1, [4.5, 70.5, 13.0]));
        assert!(!in_range(beacon, 0, [4.5, 70.5, 4.5]));
        assert!(in_range(beacon, 4, [4.5, 90.5, 4.5]));
    }
}
