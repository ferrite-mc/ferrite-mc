//! Eye of ender (vanilla `EyeOfEnder`): flies toward a target for 80 ticks, then either drops as
//! an item (four times in five) or shatters.

use crate::store::{EntityData, EntityRecord, EntityStore, Flags};
use crate::{EntityKey, Rng, allocate_id, random_uuid};
use ferrite_math::Vec3;
use std::sync::OnceLock;

/// `EyeOfEnder` is a quarter-block entity.
pub const SIZE: f64 = 0.25;
/// Vanilla `EyeOfEnder` life before it drops or breaks.
pub const LIFETIME: i32 = 80;
/// How far (horizontally) the eye flies toward a farther target before it climbs.
pub const REACH: f64 = 12.0;

#[derive(Debug, Clone, PartialEq)]
pub struct EyeState {
    /// Where the eye is heading; `None` until the region has located one.
    pub target: Option<Vec3>,
    pub life: i32,
    /// Whether it drops as an item instead of shattering (`random.nextInt(5) > 0`).
    pub survives: bool,
}

pub fn eye_type() -> u16 {
    static KIND: OnceLock<u16> = OnceLock::new();
    *KIND.get_or_init(|| {
        ferrite_data::entity_props::by_name("eye_of_ender")
            .expect("vanilla has an eye_of_ender entity type")
            .id
    })
}

pub fn new_eye(pos: Vec3, rng: &mut Rng) -> EntityRecord {
    let mut data = EntityData::new(allocate_id(), random_uuid(), eye_type(), SIZE, SIZE);
    data.eye = Some(EyeState {
        target: None,
        life: 0,
        survives: rng.next_int(5) > 0,
    });
    EntityRecord {
        pos,
        vel: Vec3::default(),
        flags: Flags::default(),
        data,
    }
}

/// Vanilla `EyeOfEnder.signalTo`: a target beyond 12 blocks is replaced by the point 12 blocks
/// toward it, 8 blocks above the eye, so the eye arcs up while it travels.
pub fn aim(from: Vec3, target: Vec3) -> Vec3 {
    let (dx, dz) = (target.x - from.x, target.z - from.z);
    let distance = (dx * dx + dz * dz).sqrt();
    if distance > REACH {
        Vec3::new(
            from.x + dx / distance * REACH,
            from.y + 8.0,
            from.z + dz / distance * REACH,
        )
    } else {
        target
    }
}

/// One tick of the velocity (vanilla `EyeOfEnder.tick`).
pub fn steer(pos: Vec3, vel: Vec3, target: Vec3) -> Vec3 {
    let (dx, dz) = (target.x - pos.x, target.z - pos.z);
    let horizontal = (dx * dx + dz * dz).sqrt();
    let angle = dz.atan2(dx);
    let current = (vel.x * vel.x + vel.z * vel.z).sqrt();
    let mut speed = current + (horizontal - current) * 0.0025;
    let mut y = vel.y;
    if horizontal < 1.0 {
        speed *= 0.8;
        y *= 0.8;
    }
    let up = if pos.y < target.y { 1.0 } else { -1.0 };
    Vec3::new(
        angle.cos() * speed,
        y + (up - y) * 0.015,
        angle.sin() * speed,
    )
}

/// A finished eye.
#[derive(Debug, Clone, PartialEq)]
pub struct Ended {
    pub pos: Vec3,
    /// Dropped as an item (`true`) or shattered.
    pub drops: bool,
}

/// Eyes without a target, which the region must locate one for.
pub fn untargeted(store: &EntityStore) -> Vec<(EntityKey, Vec3)> {
    let kind = eye_type();
    store
        .keys()
        .iter()
        .filter_map(|&key| {
            let data = store.data(key)?;
            (data.kind == kind && data.eye.as_ref()?.target.is_none())
                .then(|| store.pos(key).map(|pos| (key, pos)))?
        })
        .collect()
}

/// Gives an eye its target (`None` shatters it at once, as vanilla does when nothing is found).
pub fn set_target(store: &mut EntityStore, key: EntityKey, target: Option<Vec3>) {
    let Some(pos) = store.pos(key) else {
        return;
    };
    if let Some(state) = store.data_mut(key).and_then(|d| d.eye.as_mut()) {
        match target {
            Some(target) => state.target = Some(aim(pos, target)),
            None => {
                state.target = Some(pos);
                state.life = LIFETIME;
                state.survives = false;
            }
        }
    }
}

pub fn tick_eyes(store: &mut EntityStore) -> Vec<Ended> {
    let kind = eye_type();
    let mut ended = Vec::new();
    for key in store.keys().to_vec() {
        let Some(data) = store.data(key) else {
            continue;
        };
        if data.kind != kind {
            continue;
        }
        let Some(state) = data.eye.clone() else {
            continue;
        };
        let Some(target) = state.target else {
            continue;
        };
        if store.flags(key).is_none_or(|f| f.has(Flags::REMOVED)) {
            continue;
        }
        let pos = store.pos(key).unwrap_or_default();
        let vel = steer(pos, store.vel(key).unwrap_or_default(), target);
        store.set_vel(key, vel);
        store.set_pos(key, Vec3::new(pos.x + vel.x, pos.y + vel.y, pos.z + vel.z));
        let data = store.data_mut(key).unwrap();
        data.tick_count += 1;
        let state = data.eye.as_mut().unwrap();
        state.life += 1;
        if state.life > LIFETIME {
            let drops = state.survives;
            store.flags_mut(key).unwrap().set(Flags::REMOVED, true);
            ended.push(Ended {
                pos: store.pos(key).unwrap_or(pos),
                drops,
            });
        }
    }
    ended
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_far_target_is_replaced_by_a_point_twelve_blocks_toward_it_and_eight_up() {
        let from = Vec3::new(0.0, 70.0, 0.0);
        let aimed = aim(from, Vec3::new(300.0, 40.0, 400.0));
        assert!((aimed.x - 7.2).abs() < 1e-9 && (aimed.z - 9.6).abs() < 1e-9);
        assert_eq!(aimed.y, 78.0);
        let near = Vec3::new(5.0, 60.0, 5.0);
        assert_eq!(aim(from, near), near);
    }

    #[test]
    fn an_eye_flies_toward_its_target_then_drops_or_shatters_after_eighty_ticks() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(11);
        let key = store.insert(new_eye(Vec3::new(0.0, 70.0, 0.0), &mut rng));
        assert_eq!(untargeted(&store).len(), 1);
        set_target(&mut store, key, Some(Vec3::new(100.0, 60.0, 0.0)));
        assert!(untargeted(&store).is_empty());
        let survives = store.data(key).unwrap().eye.as_ref().unwrap().survives;
        let mut end = None;
        for tick in 1..=LIFETIME + 2 {
            let ended = tick_eyes(&mut store);
            if let Some(found) = ended.first() {
                end = Some((tick, found.clone()));
                break;
            }
        }
        let (tick, found) = end.expect("it ends");
        assert_eq!(tick, LIFETIME + 1);
        assert_eq!(found.drops, survives);
        assert!(found.pos.x > 0.5, "it moved toward +x: {:?}", found.pos);
    }

    #[test]
    fn no_target_shatters_the_eye_on_the_next_tick() {
        let mut store = EntityStore::default();
        let key = store.insert(new_eye(Vec3::new(0.0, 70.0, 0.0), &mut Rng::new(2)));
        set_target(&mut store, key, None);
        let ended = tick_eyes(&mut store);
        assert_eq!(ended.len(), 1);
        assert!(!ended[0].drops);
    }
}
