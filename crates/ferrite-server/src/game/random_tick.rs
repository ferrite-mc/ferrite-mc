//! Region-local random ticking. Sampling follows ServerLevel.tickChunk; empty
//! sections are skipped as in Lithium's chunk-section random-tick counters.
use super::{anvil::AnvilChunkSource, fluid::RegionFluidWorld, region::Tracking};
use ferrite_entity::Rng;
use ferrite_fluid::FluidWorld;
use ferrite_region::ChunkPos;
use ferrite_world::Chunk;
use std::{
    collections::HashMap,
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

type Pos = (i32, i32, i32);
trait RandomWorld {
    fn block(&self, p: Pos) -> u32;
    fn set_block(&mut self, p: Pos, state: u32);
    fn brightness(&self, p: Pos) -> u8;
}
impl RandomWorld for RegionFluidWorld<'_> {
    fn block(&self, p: Pos) -> u32 {
        FluidWorld::block(self, p.0, p.1, p.2)
    }
    fn set_block(&mut self, p: Pos, state: u32) {
        FluidWorld::set_block(self, p.0, p.1, p.2, state);
    }
    fn brightness(&self, p: Pos) -> u8 {
        RegionFluidWorld::brightness(self, p.0, p.1, p.2)
    }
}
type Handler = fn(&mut dyn RandomWorld, Pos, u32, &mut Rng);
fn no_op(_: &mut dyn RandomWorld, _: Pos, _: u32, _: &mut Rng) {}
fn state_name(state: u32) -> Option<(&'static str, &'static [(&'static str, &'static str)])> {
    ferrite_data::blocks::state(u16::try_from(state).ok()?)
}
fn named(state: u32, name: &str) -> bool {
    state_name(state).is_some_and(|(n, _)| n.strip_prefix("minecraft:").unwrap_or(n) == name)
}
fn property(state: u32, key: &str, value: &str) -> bool {
    state_name(state).is_some_and(|(_, props)| props.iter().any(|&(k, v)| k == key && v == value))
}
fn default_block(name: &str) -> u32 {
    u32::from(ferrite_data::blocks::default_state(name).expect("vanilla block"))
}

/// Adapts the random-tick world to `fire::FireWorld` so `LavaFluid.randomTick` can light fire.
struct FireView<'a>(&'a mut dyn RandomWorld);

impl super::fire::FireWorld for FireView<'_> {
    fn block(&self, x: i32, y: i32, z: i32) -> Option<u16> {
        u16::try_from(self.0.block((x, y, z))).ok()
    }
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u16) {
        self.0.set_block((x, y, z), u32::from(state));
    }
    fn owns(&self, _: i32, _: i32) -> bool {
        true
    }
}

// Vanilla `LavaFluid.randomTick`: lava beside flammable blocks starts a fire and schedules it.
fn lava_tick(
    world: &mut dyn RandomWorld,
    pos: Pos,
    rng: &mut Rng,
    fire: &mut super::fire::FireTicks,
) {
    let mut view = FireView(world);
    if let Some(target) = super::fire::lava_ignites(&mut view, [pos.0, pos.1, pos.2], rng) {
        let delay = super::fire::tick_delay(rng);
        fire.schedule(target, delay);
    }
}
// SpreadingSnowyBlock.canStayAlive / canPropagate, including LightEngine.getLightDampeningInto.
fn can_stay(world: &dyn RandomWorld, p: Pos, grass: u32) -> bool {
    let above = world.block((p.0, p.1 + 1, p.2));
    if named(above, "snow") && property(above, "layers", "1") {
        return true;
    }
    let props = ferrite_data::block_props::get(above as u16);
    if props.fluid.is_some_and(|(_, amount, _)| amount == 8) || props.light_opacity >= 15 {
        return false;
    }
    let grass_props = ferrite_data::block_props::get(grass as u16);
    let face = |state: u32, prop: &ferrite_data::block_props::BlockProps, dir| {
        if prop.can_occlude && prop.use_shape_for_light_occlusion {
            ferrite_data::block_props::face_occlusion(state as u16, dir)
        } else {
            &[]
        }
    };
    let a = face(grass, grass_props, 3);
    let b = face(above, props, 2);
    let mut xs = vec![0.0, 1.0];
    let mut zs = vec![0.0, 1.0];
    for v in a.iter().chain(b) {
        xs.extend([v[0], v[3]]);
        zs.extend([v[2], v[5]]);
    }
    xs.sort_by(f64::total_cmp);
    xs.dedup();
    zs.sort_by(f64::total_cmp);
    zs.dedup();
    !xs.windows(2).all(|x| {
        zs.windows(2).all(|z| {
            let (x, z) = ((x[0] + x[1]) * 0.5, (z[0] + z[1]) * 0.5);
            a.iter()
                .chain(b)
                .any(|v| x >= v[0] && x < v[3] && z >= v[2] && z < v[5])
        })
    })
}
// Vanilla 26.3 SpreadingSnowyBlock.randomTick (older name: SpreadingSnowyDirtBlock).
fn grass_tick(world: &mut dyn RandomWorld, p: Pos, state: u32, rng: &mut Rng) {
    if !can_stay(world, p, state) {
        world.set_block(p, default_block("dirt"));
        return;
    }
    if world.brightness((p.0, p.1 + 1, p.2)) < 9 {
        return;
    }
    for _ in 0..4 {
        let target = (
            p.0 + rng.next_int(3) - 1,
            p.1 + rng.next_int(5) - 3,
            p.2 + rng.next_int(3) - 1,
        );
        let above = world.block((target.0, target.1 + 1, target.2));
        let water = matches!(
            ferrite_data::block_props::get(above as u16).fluid,
            Some((
                ferrite_data::block_props::FluidKind::Water
                    | ferrite_data::block_props::FluidKind::FlowingWater,
                _,
                _
            ))
        );
        if named(world.block(target), "dirt") && !water && can_stay(world, target, state) {
            let snowy = named(above, "snow") || named(above, "snow_block");
            let grass = ferrite_data::blocks::state_id(
                "grass_block",
                &[("snowy", if snowy { "true" } else { "false" })],
            )
            .expect("grass state");
            world.set_block(target, u32::from(grass));
        }
    }
}
// LeavesBlock.isRandomlyTicking / randomTick; item drops are not available yet.
fn leaves_tick(world: &mut dyn RandomWorld, p: Pos, state: u32, _: &mut Rng) {
    if property(state, "distance", "7") && property(state, "persistent", "false") {
        world.set_block(
            p,
            ferrite_fluid::block_for(ferrite_fluid::fluid_state(state)),
        );
    }
}
fn handlers() -> &'static HashMap<u32, Handler> {
    static TABLE: OnceLock<HashMap<u32, Handler>> = OnceLock::new();
    TABLE.get_or_init(|| {
        (0..ferrite_data::block_props::STATE_COUNT)
            .filter_map(|id| {
                let id = u16::try_from(id).ok()?;
                if !ferrite_data::block_props::get(id).randomly_ticking {
                    return None;
                }
                let state = u32::from(id);
                let handler: Handler = if named(state, "grass_block") {
                    grass_tick
                } else if state_name(state).is_some_and(|(n, _)| n.ends_with("_leaves")) {
                    leaves_tick
                } else {
                    no_op
                };
                Some((state, handler))
            })
            .collect()
    })
}
// Snapshot only the section mask while the cache is locked; handlers run after it is released.
fn ticking_sections(chunk: &Chunk) -> Vec<i32> {
    chunk
        .sections()
        .iter()
        .enumerate()
        .filter_map(|(i, section)| {
            section
                .is_randomly_ticking()
                .then_some(chunk.min_y() + i as i32 * 16)
        })
        .collect()
}
fn sample_positions(
    sections: &[i32],
    cx: i32,
    cz: i32,
    speed: u32,
    mut next: impl FnMut() -> usize,
    mut visit: impl FnMut(Pos),
) {
    for &base_y in sections {
        for _ in 0..speed {
            let index = next();
            visit((
                cx * 16 + (index & 15) as i32,
                base_y + ((index >> 8) & 15) as i32,
                cz * 16 + ((index >> 4) & 15) as i32,
            ));
        }
    }
}
pub(super) struct RandomTicks {
    rng: Rng,
}
impl Default for RandomTicks {
    fn default() -> Self {
        static SEED: AtomicU64 = AtomicU64::new(0x91e10da5c79e7b1d);
        Self {
            rng: Rng::new(SEED.fetch_add(0x9e3779b97f4a7c15, Ordering::Relaxed)),
        }
    }
}
impl RandomTicks {
    pub fn tick(
        &mut self,
        source: &AnvilChunkSource,
        held: &HashMap<ChunkPos, usize>,
        tracking: &Tracking,
        speed: u32,
        fire: &mut super::fire::FireTicks,
    ) {
        let mut world = RegionFluidWorld::new(source, held, tracking);
        for chunk in held.keys() {
            let Some(sections) = source.with_loaded_chunk(chunk.0, chunk.1, ticking_sections)
            else {
                continue;
            };
            // Sample and dispatch immediately: handler RNG draws precede the next position draw.
            let rng = std::cell::RefCell::new(&mut self.rng);
            sample_positions(
                &sections,
                chunk.0,
                chunk.1,
                speed,
                || rng.borrow_mut().next_int(4096) as usize,
                |pos| {
                    let state = FluidWorld::block(&world, pos.0, pos.1, pos.2);
                    let Ok(id) = u16::try_from(state) else {
                        return;
                    };
                    if usize::from(id) >= ferrite_data::block_props::STATE_COUNT {
                        return;
                    }
                    let props = ferrite_data::block_props::get(id);
                    if props.randomly_ticking
                        && let Some(handler) = handlers().get(&state)
                    {
                        handler(&mut world, pos, state, &mut rng.borrow_mut());
                    }
                    // Vanilla `LavaFluid.randomTick`: lava ignites nearby flammable blocks.
                    if matches!(
                        props.fluid,
                        Some((
                            ferrite_data::block_props::FluidKind::Lava
                                | ferrite_data::block_props::FluidKind::FlowingLava,
                            _,
                            _
                        ))
                    ) {
                        lava_tick(&mut world, pos, &mut rng.borrow_mut(), fire);
                    }
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_world::{BlockFlags, BlockInfo, Chunk};
    struct Info;
    impl BlockInfo for Info {
        fn flags(&self, id: u32) -> BlockFlags {
            BlockFlags {
                air: id == 0,
                ..Default::default()
            }
        }
        fn randomly_ticking(&self, id: u32) -> bool {
            id == u32::from(ferrite_data::blocks::default_state("grass_block").unwrap())
        }
    }
    #[test]
    fn non_ticking_sections_do_not_sample_and_ticking_sections_use_one_draw_per_pick() {
        let mut chunk = Chunk::new(-64, 32, 0, 0, 16, 6).unwrap();
        let grass = u32::from(ferrite_data::blocks::default_state("grass_block").unwrap());
        chunk.sections_mut()[1].set_block(0, grass, &Info).unwrap();
        let mut draws = 0;
        let mut events = Vec::new();
        let sections = ticking_sections(&chunk);
        sample_positions(
            &sections,
            2,
            -1,
            3,
            || {
                draws += 1;
                0
            },
            |pos| events.push(pos),
        );
        assert_eq!(draws, 3);
        assert_eq!(events.len(), 3);
        assert!(events.iter().all(|&pos| pos == (32, -48, -16)));
    }
    #[derive(Default)]
    struct Grid {
        blocks: HashMap<(i32, i32, i32), u32>,
        light: u8,
    }
    impl RandomWorld for Grid {
        fn block(&self, p: (i32, i32, i32)) -> u32 {
            *self.blocks.get(&p).unwrap_or(&state("air"))
        }
        fn set_block(&mut self, p: (i32, i32, i32), s: u32) {
            self.blocks.insert(p, s);
        }
        fn brightness(&self, _: (i32, i32, i32)) -> u8 {
            self.light
        }
    }
    fn state(name: &str) -> u32 {
        u32::from(ferrite_data::blocks::default_state(name).unwrap())
    }
    #[test]
    fn grass_decays_under_opaque_block_and_spreads_only_in_light() {
        let mut grid = Grid {
            light: 15,
            ..Grid::default()
        };
        let p = (0, 0, 0);
        let target = (1, 0, 0);
        let grass = state("grass_block");
        grid.set_block(p, grass);
        grid.set_block((0, 1, 0), state("stone"));
        grass_tick(&mut grid, p, grass, &mut Rng::new(1));
        assert_eq!(grid.block(p), state("dirt"));
        grid.set_block(p, grass);
        grid.set_block((0, 1, 0), state("air"));
        grid.set_block(target, state("dirt"));
        grid.light = 0;
        let mut rng = Rng::new(12);
        for _ in 0..1000 {
            grass_tick(&mut grid, p, grass, &mut rng);
        }
        assert_eq!(grid.block(target), state("dirt"));
        grid.light = 15;
        for _ in 0..1000 {
            grass_tick(&mut grid, p, grass, &mut rng);
        }
        assert_eq!(grid.block(target), grass);
    }
    #[test]
    fn leaves_decay_only_when_nonpersistent_and_distance_seven() {
        let mut grid = Grid::default();
        let p = (0, 0, 0);
        let leaf = |persistent, distance| {
            u32::from(
                ferrite_data::blocks::state_id(
                    "oak_leaves",
                    &[("persistent", persistent), ("distance", distance)],
                )
                .unwrap(),
            )
        };
        let mut rng = Rng::new(1);
        for state in [leaf("true", "7"), leaf("false", "6")] {
            grid.set_block(p, state);
            leaves_tick(&mut grid, p, state, &mut rng);
            assert_eq!(grid.block(p), state);
        }
        let state = leaf("false", "7");
        grid.set_block(p, state);
        leaves_tick(&mut grid, p, state, &mut rng);
        assert_eq!(grid.block(p), super::tests::state("air"));
    }
}
