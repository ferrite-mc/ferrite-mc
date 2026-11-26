//! Vanilla dungeon/monster spawner (`minecraft:mob_spawner`, `BaseSpawner.serverTick`) runtime.
//!
//! Reads and mutates spawner NBT (`Delay`, `MinSpawnDelay`, `MaxSpawnDelay`, `SpawnCount`,
//! `MaxNearbyEntities`, `RequiredPlayerRange`, `SpawnRange`, `SpawnData.entity.id`) directly
//! inside loaded chunk block entities so state persists across restarts and region splits.

use std::collections::{HashMap, HashSet};

use ferrite_entity::mob::{self, MobKind};
use ferrite_entity::physics::BlockGetter;
use ferrite_entity::{EntityStore, Rng};
use ferrite_math::Vec3;
use ferrite_nbt::Tag;
use uuid::Uuid;

use super::region::Player;
use crate::game::AnvilChunkSource;

fn mob_kind_from_spawner_id(id: &str) -> MobKind {
    let short = id.strip_prefix("minecraft:").unwrap_or(id);
    match short {
        "skeleton" => MobKind::Skeleton,
        "spider" => MobKind::Spider,
        "cave_spider" => MobKind::CaveSpider,
        "blaze" => MobKind::Blaze,
        "magma_cube" => MobKind::MagmaCube,
        "slime" => MobKind::Slime,
        "wither_skeleton" => MobKind::WitherSkeleton,
        "drowned" => MobKind::Drowned,
        "husk" => MobKind::Husk,
        "zombie_villager" => MobKind::ZombieVillager,
        "stray" => MobKind::Stray,
        "bogged" => MobKind::Bogged,
        "creeper" => MobKind::Creeper,
        "pig" => MobKind::Pig,
        _ => MobKind::Zombie,
    }
}

fn tag_i16(nbt: &ferrite_nbt::Compound, key: &str, default: i16) -> i16 {
    match nbt.get(key) {
        Some(Tag::Short(v)) => *v,
        Some(Tag::Int(v)) => (*v).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
        _ => default,
    }
}

fn spawner_entity_id(nbt: &ferrite_nbt::Compound) -> &str {
    nbt.get("SpawnData")
        .and_then(|t| match t {
            Tag::Compound(c) => c.get("entity"),
            _ => None,
        })
        .and_then(|t| match t {
            Tag::Compound(c) => c.get("id"),
            _ => None,
        })
        .and_then(|t| match t {
            Tag::String(s) => Some(s.as_str()),
            _ => None,
        })
        .unwrap_or("minecraft:zombie")
}

struct SpawnerSnapshot {
    pos: [i32; 3],
    delay: i16,
    min_delay: i16,
    max_delay: i16,
    spawn_count: u8,
    max_nearby: usize,
    player_range: f64,
    spawn_range: i32,
    kind: MobKind,
}

/// Ticks vanilla `minecraft:mob_spawner` block entities in loaded chunks near active players.
pub(crate) fn tick_mob_spawners(
    entities: &mut EntityStore,
    players: &HashMap<Uuid, Player>,
    rng: &mut Rng,
    blocks: &impl BlockGetter,
    source: Option<&AnvilChunkSource>,
    peaceful: bool,
) {
    let active_players: Vec<Vec3> = players
        .values()
        .filter(|p| p.gamemode != 3 && !p.survival.dead)
        .map(|p| Vec3::new(p.pos[0], p.pos[1], p.pos[2]))
        .collect();
    tick_mob_spawners_for_positions(entities, &active_players, rng, blocks, source, peaceful);
}

fn tick_mob_spawners_for_positions(
    entities: &mut EntityStore,
    active_players: &[Vec3],
    rng: &mut Rng,
    blocks: &impl BlockGetter,
    source: Option<&AnvilChunkSource>,
    peaceful: bool,
) {
    let Some(source) = source else {
        return;
    };
    let Some(spawner_type_id) =
        ferrite_data::registries::id("minecraft:block_entity_type", "minecraft:mob_spawner")
            .map(|id| id as u32)
    else {
        return;
    };
    if active_players.is_empty() {
        return;
    }

    let mut chunks = HashSet::new();
    for p in active_players {
        let pcx = (p.x.floor() as i32).div_euclid(16);
        let pcz = (p.z.floor() as i32).div_euclid(16);
        for dz in -1..=1 {
            for dx in -1..=1 {
                chunks.insert((pcx + dx, pcz + dz));
            }
        }
    }

    let mut spawners = Vec::new();
    for (cx, cz) in chunks {
        source.with_loaded_chunk(cx, cz, |chunk| {
            for (&(lx, wy, lz), be) in &chunk.block_entities {
                if be.type_id != spawner_type_id {
                    continue;
                }
                let wx = cx * 16 + i32::from(lx);
                let wz = cz * 16 + i32::from(lz);
                let player_range = f64::from(tag_i16(&be.data, "RequiredPlayerRange", 16).max(1));
                let r2 = player_range * player_range;
                let center = Vec3::new(
                    f64::from(wx) + 0.5,
                    f64::from(wy) + 0.5,
                    f64::from(wz) + 0.5,
                );
                let near = active_players.iter().any(|p| {
                    let dx = p.x - center.x;
                    let dy = p.y - center.y;
                    let dz = p.z - center.z;
                    dx * dx + dy * dy + dz * dz <= r2
                });
                if !near {
                    continue;
                }
                let kind = mob_kind_from_spawner_id(spawner_entity_id(&be.data));
                if peaceful && kind.is_hostile() {
                    continue;
                }
                spawners.push(SpawnerSnapshot {
                    pos: [wx, wy, wz],
                    delay: tag_i16(&be.data, "Delay", 20),
                    min_delay: tag_i16(&be.data, "MinSpawnDelay", 200).max(1),
                    max_delay: tag_i16(&be.data, "MaxSpawnDelay", 800).max(1),
                    spawn_count: tag_i16(&be.data, "SpawnCount", 4).clamp(1, 16) as u8,
                    max_nearby: tag_i16(&be.data, "MaxNearbyEntities", 6).clamp(1, 32) as usize,
                    player_range,
                    spawn_range: tag_i16(&be.data, "SpawnRange", 4).clamp(1, 12) as i32,
                    kind,
                });
            }
        });
    }

    for sp in spawners {
        let _ = sp.player_range;
        if sp.delay > 0 {
            let next_delay = sp.delay - 1;
            let _ = source.update_block_entity(sp.pos[0], sp.pos[1], sp.pos[2], |slot| {
                if let Some(be) = slot.as_mut() {
                    be.data.insert("Delay".into(), Tag::Short(next_delay));
                    (true, ())
                } else {
                    (false, ())
                }
            });
            continue;
        }

        // Count nearby matching mobs within 9x9x9 box around spawner.
        let cx = f64::from(sp.pos[0]) + 0.5;
        let cy = f64::from(sp.pos[1]) + 0.5;
        let cz = f64::from(sp.pos[2]) + 0.5;
        let nearby = entities
            .keys()
            .iter()
            .filter(|&&k| {
                mob::state(entities, k).is_some_and(|s| s.kind == sp.kind && !s.is_dead())
                    && entities.pos(k).is_some_and(|p| {
                        (p.x - cx).abs() <= 4.5
                            && (p.y - cy).abs() <= 4.5
                            && (p.z - cz).abs() <= 4.5
                    })
            })
            .count();

        if nearby < sp.max_nearby {
            let budget = (sp.max_nearby - nearby).min(usize::from(sp.spawn_count));
            for _ in 0..budget {
                let ox = rng.next_int(sp.spawn_range * 2 + 1) - sp.spawn_range;
                let oz = rng.next_int(sp.spawn_range * 2 + 1) - sp.spawn_range;
                let oy = rng.next_int(3) - 1;
                let sx = sp.pos[0] + ox;
                let sy = sp.pos[1] + oy;
                let sz = sp.pos[2] + oz;
                if blocks.block_state(sx, sy, sz) == Some(0)
                    && blocks.block_state(sx, sy + 1, sz) == Some(0)
                {
                    let pos = Vec3::new(f64::from(sx) + 0.5, f64::from(sy), f64::from(sz) + 0.5);
                    entities.insert(mob::new_mob(sp.kind, pos, rng));
                }
            }
        }

        let lo = sp.min_delay.min(sp.max_delay);
        let hi = sp.min_delay.max(sp.max_delay);
        let span = i32::from(hi - lo) + 1;
        let next_delay = lo + rng.next_int(span) as i16;
        let _ = source.update_block_entity(sp.pos[0], sp.pos[1], sp.pos[2], |slot| {
            if let Some(be) = slot.as_mut() {
                be.data.insert("Delay".into(), Tag::Short(next_delay));
                (true, ())
            } else {
                (false, ())
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_nbt::Compound;
    use ferrite_world::BlockEntity;

    struct AllAir;
    impl BlockGetter for AllAir {
        fn block_state(&self, _x: i32, _y: i32, _z: i32) -> Option<u16> {
            Some(0)
        }
    }

    #[test]
    fn maps_vanilla_spawner_entity_ids_to_mob_kinds() {
        assert_eq!(
            mob_kind_from_spawner_id("minecraft:zombie"),
            MobKind::Zombie
        );
        assert_eq!(
            mob_kind_from_spawner_id("minecraft:skeleton"),
            MobKind::Skeleton
        );
        assert_eq!(
            mob_kind_from_spawner_id("minecraft:spider"),
            MobKind::Spider
        );
        assert_eq!(mob_kind_from_spawner_id("minecraft:blaze"), MobKind::Blaze);
        assert_eq!(
            mob_kind_from_spawner_id("minecraft:cave_spider"),
            MobKind::CaveSpider
        );
        assert_eq!(
            mob_kind_from_spawner_id("minecraft:zombie_villager"),
            MobKind::ZombieVillager
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn mob_spawner_ticks_delay_spawns_mobs_and_respects_cap_and_peaceful() {
        let source = AnvilChunkSource::new_memory();
        while source.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let spawner_type =
            ferrite_data::registries::id("minecraft:block_entity_type", "minecraft:mob_spawner")
                .unwrap() as u32;
        let mut entity_nbt = Compound::new();
        entity_nbt.insert("id".into(), Tag::String("minecraft:skeleton".into()));
        let mut spawn_data = Compound::new();
        spawn_data.insert("entity".into(), Tag::Compound(entity_nbt));
        let mut data = Compound::new();
        data.insert("Delay".into(), Tag::Short(2));
        data.insert("MinSpawnDelay".into(), Tag::Short(200));
        data.insert("MaxSpawnDelay".into(), Tag::Short(400));
        data.insert("SpawnCount".into(), Tag::Short(4));
        data.insert("MaxNearbyEntities".into(), Tag::Short(2));
        data.insert("RequiredPlayerRange".into(), Tag::Short(16));
        data.insert("SpawnRange".into(), Tag::Short(4));
        data.insert("SpawnData".into(), Tag::Compound(spawn_data));
        let _ = source.update_block_entity(8, 70, 8, |slot| {
            *slot = Some(BlockEntity {
                type_id: spawner_type,
                data,
            });
            (true, ())
        });

        let mut entities = EntityStore::default();
        let mut rng = Rng::new(42);
        let players = [Vec3::new(8.5, 70.0, 12.5)];

        // Tick 1 & 2: Delay counts down 2 -> 1 -> 0 without spawning yet.
        tick_mob_spawners_for_positions(
            &mut entities,
            &players,
            &mut rng,
            &AllAir,
            Some(&source),
            false,
        );
        assert_eq!(entities.keys().len(), 0);
        tick_mob_spawners_for_positions(
            &mut entities,
            &players,
            &mut rng,
            &AllAir,
            Some(&source),
            false,
        );
        assert_eq!(entities.keys().len(), 0);

        // Tick 3 in Peaceful mode: hostile skeleton spawner must NOT spawn.
        tick_mob_spawners_for_positions(
            &mut entities,
            &players,
            &mut rng,
            &AllAir,
            Some(&source),
            true,
        );
        assert_eq!(entities.keys().len(), 0);

        // Tick 4 in Normal mode: spawns skeletons up to MaxNearbyEntities = 2 and resets Delay.
        tick_mob_spawners_for_positions(
            &mut entities,
            &players,
            &mut rng,
            &AllAir,
            Some(&source),
            false,
        );
        assert_eq!(entities.keys().len(), 2);
        for &k in entities.keys() {
            assert_eq!(mob::state(&entities, k).unwrap().kind, MobKind::Skeleton);
        }
        let be = source.block_entity(8, 70, 8).unwrap();
        let reset_delay = tag_i16(&be.data, "Delay", 0);
        assert!(
            (200..=400).contains(&reset_delay),
            "Delay must reset into 200..=400, got {reset_delay}"
        );
    }
}
