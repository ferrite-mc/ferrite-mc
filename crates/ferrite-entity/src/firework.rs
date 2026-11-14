//! Firework rockets (vanilla `FireworkRocketEntity`): a rocket climbs with a little horizontal
//! drift for `10 * flight + rand(6) + rand(7)` ticks, then bursts. A rocket used while gliding is
//! attached to that player, who is pushed along their look direction until it bursts.

use crate::physics::{self, BlockGetter};
use crate::store::{EntityData, EntityRecord, EntityStore, Flags};
use crate::{EntityKey, Rng, allocate_id, random_uuid};
use ferrite_math::Vec3;
use std::sync::OnceLock;
use uuid::Uuid;

/// Vanilla `FireworkRocketEntity` dimensions.
pub const SIZE: f64 = 0.25;

/// What a rocket remembers (vanilla `life`, `lifetime`, the `Fireworks` component and `attachedToTarget`).
#[derive(Debug, Clone, PartialEq)]
pub struct FireworkState {
    pub life: i32,
    pub life_time: i32,
    /// `Fireworks.flightDuration`.
    pub flight: u8,
    /// How many explosion stars the item carries; each adds damage when the rocket bursts.
    pub explosions: u8,
    /// The gliding player the rocket pushes, if any.
    pub attached: Option<Uuid>,
}

/// Entity type id of `minecraft:firework_rocket`.
pub fn firework_type() -> u16 {
    static KIND: OnceLock<u16> = OnceLock::new();
    *KIND.get_or_init(|| {
        ferrite_data::entity_props::by_name("firework_rocket")
            .expect("vanilla has a firework_rocket entity type")
            .id
    })
}

/// A rocket at `pos`; `attached` is the gliding player it boosts. Vanilla launches a free rocket
/// straight up (`deltaMovement` of `(gauss * 0.001, 0.05, gauss * 0.001)`).
pub fn new_firework(
    pos: Vec3,
    flight: u8,
    explosions: u8,
    attached: Option<Uuid>,
    rng: &mut Rng,
) -> EntityRecord {
    let mut data = EntityData::new(allocate_id(), random_uuid(), firework_type(), SIZE, SIZE);
    let life_time = 10 * i32::from(flight) + rng.next_int(6) + rng.next_int(7);
    data.firework = Some(FireworkState {
        life: 0,
        life_time,
        flight,
        explosions,
        attached,
    });
    let drift = |rng: &mut Rng| rng.range(-0.001, 0.001);
    let vel = if attached.is_some() {
        Vec3::default()
    } else {
        Vec3::new(drift(rng), 0.05, drift(rng))
    };
    EntityRecord {
        pos,
        vel,
        flags: Flags::default(),
        data,
    }
}

/// A rocket that burst this tick.
#[derive(Debug, Clone, PartialEq)]
pub struct Burst {
    pub key: EntityKey,
    pub pos: Vec3,
    pub explosions: u8,
    pub attached: Option<Uuid>,
}

/// What one tick of every rocket asks of the region.
#[derive(Debug, Default, PartialEq)]
pub struct Tick {
    /// Rockets that burst (already marked removed); the region sends the burst event and damages.
    pub bursts: Vec<Burst>,
    /// `(rocket, player)` pairs: move the rocket to the player and push the player.
    pub attached: Vec<(EntityKey, Uuid)>,
}

/// Ticks every rocket. Free rockets climb (`vel * (1.15, 1, 1.15) + (0, 0.04, 0)`) and burst on
/// hitting a block; attached rockets are left to the region, which owns the player.
pub fn tick_fireworks(store: &mut EntityStore, blocks: &impl BlockGetter) -> Tick {
    let kind = firework_type();
    let mut tick = Tick::default();
    for key in store.keys().to_vec() {
        let Some(data) = store.data(key) else {
            continue;
        };
        if data.kind != kind || data.firework.is_none() {
            continue;
        }
        if store
            .flags(key)
            .is_none_or(|flags| flags.has(Flags::REMOVED))
        {
            continue;
        }
        let attached = data.firework.as_ref().and_then(|state| state.attached);
        let mut hit = false;
        if attached.is_none() {
            let mut vel = store.vel(key).unwrap_or_default();
            vel = Vec3::new(vel.x * 1.15, vel.y + 0.04, vel.z * 1.15);
            store.set_vel(key, vel);
            physics::move_entity(store, key, vel, blocks);
            let flags = store.flags(key).unwrap_or_default();
            hit = flags.has(Flags::HORIZONTAL_COLLISION) || flags.has(Flags::VERTICAL_COLLISION);
        }
        let data = store.data_mut(key).unwrap();
        data.tick_count += 1;
        let state = data.firework.as_mut().unwrap();
        state.life += 1;
        let done = hit || state.life > state.life_time;
        let (explosions, attached_to) = (state.explosions, state.attached);
        if done {
            let pos = store.pos(key).unwrap_or_default();
            store.flags_mut(key).unwrap().set(Flags::REMOVED, true);
            tick.bursts.push(Burst {
                key,
                pos,
                explosions,
                attached: attached_to,
            });
        } else if let Some(player) = attached_to {
            tick.attached.push((key, player));
        }
    }
    tick
}

/// The velocity a boosted player has after one tick (vanilla `FireworkRocketEntity.tick`):
/// `delta + look * 0.1 + (look * 1.5 - delta) * 0.5`.
pub fn boost(delta: Vec3, look: Vec3) -> Vec3 {
    Vec3::new(
        delta.x + look.x * 0.1 + (look.x * 1.5 - delta.x) * 0.5,
        delta.y + look.y * 0.1 + (look.y * 1.5 - delta.y) * 0.5,
        delta.z + look.z * 0.1 + (look.z * 1.5 - delta.z) * 0.5,
    )
}

/// The damage a burst does to a living entity within five blocks: `5 + explosions * 2`, or none
/// for a rocket with no stars (vanilla `FireworkRocketEntity.dealExplosionDamage`).
pub fn burst_damage(explosions: u8) -> Option<f32> {
    (explosions > 0).then(|| 5.0 + f32::from(explosions) * 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Open;
    impl BlockGetter for Open {
        fn block_state(&self, _x: i32, y: i32, _z: i32) -> Option<u16> {
            ferrite_data::blocks::default_state(if y > 200 { "stone" } else { "air" })
        }
    }

    #[test]
    fn a_free_rocket_climbs_then_bursts_when_its_life_ends() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(7);
        let key = store.insert(new_firework(
            Vec3::new(0.5, 70.0, 0.5),
            1,
            2,
            None,
            &mut rng,
        ));
        let life_time = store
            .data(key)
            .unwrap()
            .firework
            .as_ref()
            .unwrap()
            .life_time;
        assert!((10..=21).contains(&life_time), "{life_time}");
        let mut burst = None;
        let mut last_y = 70.0;
        for tick in 1..=life_time + 1 {
            let result = tick_fireworks(&mut store, &Open);
            if let Some(found) = result.bursts.first() {
                burst = Some((tick, found.clone()));
                break;
            }
            let y = store.pos(key).unwrap().y;
            assert!(y > last_y, "it climbs every tick");
            last_y = y;
        }
        let (tick, found) = burst.expect("it bursts");
        assert_eq!(tick, life_time + 1);
        assert_eq!(found.explosions, 2);
        assert!(store.flags(key).unwrap().has(Flags::REMOVED));
        assert_eq!(burst_damage(2), Some(9.0));
        assert_eq!(burst_damage(0), None);
    }

    #[test]
    fn a_rocket_bursts_where_it_hits_a_block() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(1);
        let key = store.insert(new_firework(
            Vec3::new(0.5, 198.0, 0.5),
            3,
            0,
            None,
            &mut rng,
        ));
        let mut bursts = 0;
        for _ in 0..20 {
            bursts += tick_fireworks(&mut store, &Open).bursts.len();
            if bursts > 0 {
                break;
            }
        }
        assert_eq!(
            bursts, 1,
            "it hits the stone ceiling long before its life ends"
        );
        assert!(store.pos(key).unwrap().y < 201.0);
    }

    #[test]
    fn an_attached_rocket_is_reported_to_the_region_and_bursts_on_schedule() {
        let player = Uuid::from_u128(5);
        let mut store = EntityStore::default();
        let mut rng = Rng::new(3);
        let key = store.insert(new_firework(
            Vec3::new(0.0, 70.0, 0.0),
            1,
            0,
            Some(player),
            &mut rng,
        ));
        let life_time = store
            .data(key)
            .unwrap()
            .firework
            .as_ref()
            .unwrap()
            .life_time;
        let first = tick_fireworks(&mut store, &Open);
        assert_eq!(first.attached, vec![(key, player)]);
        assert!(first.bursts.is_empty());
        assert_eq!(
            store.pos(key).unwrap(),
            Vec3::new(0.0, 70.0, 0.0),
            "the store does not move it"
        );
        let mut last = Tick::default();
        for _ in 0..=life_time {
            last = tick_fireworks(&mut store, &Open);
            if !last.bursts.is_empty() {
                break;
            }
        }
        assert_eq!(last.bursts[0].attached, Some(player));
    }

    #[test]
    fn the_boost_pulls_the_player_toward_one_and_a_half_times_the_look() {
        let look = Vec3::new(0.0, 0.0, 1.0);
        let mut delta = Vec3::default();
        for _ in 0..20 {
            delta = boost(delta, look);
        }
        assert!((delta.z - 1.5 - 0.1 * 2.0).abs() < 0.01, "{delta:?}");
        assert!(delta.x.abs() < 1e-9 && delta.y.abs() < 1e-9);
    }
}
