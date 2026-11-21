//! Clean-room loader and Jigsaw assembler for Mojang 26.3 structure `.nbt` files
//! and `worldgen/template_pool/*.json` definitions under `vanilla/<ver>/generated/`.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ferrite_nbt::{List, Tag, binary};
use ferrite_world::{BlockStateRegistry, RuntimeRegistry};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    North,
    East,
    South,
    West,
    Up,
    Down,
}

impl Dir {
    fn from_orientation(s: &str) -> Self {
        let head = s.split('_').next().unwrap_or(s);
        match head {
            "north" => Self::North,
            "east" => Self::East,
            "south" => Self::South,
            "west" => Self::West,
            "up" => Self::Up,
            "down" => Self::Down,
            _ => Self::North,
        }
    }

    fn rotate_cw(self, steps: u8) -> Self {
        let mut d = self;
        for _ in 0..(steps % 4) {
            d = match d {
                Self::North => Self::East,
                Self::East => Self::South,
                Self::South => Self::West,
                Self::West => Self::North,
                Self::Up => Self::Up,
                Self::Down => Self::Down,
            };
        }
        d
    }

    fn opposite(self) -> Self {
        match self {
            Self::North => Self::South,
            Self::South => Self::North,
            Self::East => Self::West,
            Self::West => Self::East,
            Self::Up => Self::Down,
            Self::Down => Self::Up,
        }
    }

    fn is_horizontal(self) -> bool {
        matches!(self, Self::North | Self::East | Self::South | Self::West)
    }

    fn offset(self) -> [i32; 3] {
        match self {
            Self::North => [0, 0, -1],
            Self::East => [1, 0, 0],
            Self::South => [0, 0, 1],
            Self::West => [-1, 0, 0],
            Self::Up => [0, 1, 0],
            Self::Down => [0, -1, 0],
        }
    }
}

#[derive(Debug, Clone)]
struct JigsawConnector {
    pool: String,
    target: String,
    name: String,
    final_state_id: u16,
    front: Dir,
}

#[derive(Debug, Clone)]
struct TemplateBlock {
    pos: [i32; 3],
    state_rot: [u16; 4],
    jigsaw: Option<JigsawConnector>,
}

#[derive(Debug, Clone)]
pub(super) struct StructureNbt {
    size: [i32; 3],
    blocks: Vec<TemplateBlock>,
}

#[derive(Debug, Clone)]
struct PoolElement {
    location: String,
    weight: u32,
}

#[derive(Debug, Clone)]
struct TemplatePool {
    elements: Vec<PoolElement>,
    total_weight: u32,
    fallback: Option<String>,
}

#[derive(Debug, Clone)]
struct StructureDef {
    start_pool: Option<String>,
    size: usize,
}

#[derive(Deserialize)]
struct RawPoolJson {
    #[serde(default)]
    elements: Vec<RawPoolEntry>,
    fallback: Option<String>,
}

#[derive(Deserialize)]
struct RawPoolEntry {
    element: RawPoolElement,
    #[serde(default = "default_weight")]
    weight: u32,
}

#[derive(Deserialize)]
struct RawPoolElement {
    location: Option<String>,
}

#[derive(Deserialize)]
struct RawStructureJson {
    start_pool: Option<String>,
    size: Option<usize>,
}

fn default_weight() -> u32 {
    1
}

/// Thread-safe lazy cache of vanilla 26.3 `.nbt` structures and `template_pool` JSONs.
pub(super) struct JigsawLibrary {
    data_root: PathBuf,
    registry: Arc<RuntimeRegistry>,
    templates: Mutex<HashMap<String, Option<Arc<StructureNbt>>>>,
    pools: Mutex<HashMap<String, Option<Arc<TemplatePool>>>>,
    structures: Mutex<HashMap<String, Option<StructureDef>>>,
}

impl JigsawLibrary {
    pub(super) fn new(vanilla_data: &Path, registry: Arc<RuntimeRegistry>) -> Self {
        Self {
            data_root: vanilla_data.join("data/minecraft"),
            registry,
            templates: Mutex::new(HashMap::new()),
            pools: Mutex::new(HashMap::new()),
            structures: Mutex::new(HashMap::new()),
        }
    }

    /// Assembles a multi-piece structure for `structure_id` (returning `None` if no vanilla NBT
    /// template exists for that structure so callers can fall back cleanly).
    pub(super) fn assemble_structure(
        &self,
        structure_id: &str,
        seed: u64,
        origin: [i32; 3],
        max_pieces: usize,
    ) -> Option<Vec<([i32; 3], u16)>> {
        let short = strip_mc(structure_id);
        if let Some(def) = self.load_structure_def(short)
            && let Some(start_pool) = def.start_pool
        {
            return self.assemble_jigsaw(
                &start_pool,
                None,
                def.size.clamp(1, 6),
                seed,
                origin,
                max_pieces,
            );
        }
        let template = match short {
            "igloo" => "minecraft:igloo/top",
            "pillager_outpost" => "minecraft:pillager_outpost/watchtower",
            "end_city" => "minecraft:end_city/base_floor",
            "woodland_mansion" => "minecraft:woodland_mansion/entrance",
            s if s.starts_with("ruined_portal") => "minecraft:ruined_portal/portal_1",
            s if s.starts_with("shipwreck") => "minecraft:shipwreck/with_mast",
            s if s.starts_with("ocean_ruin") => "minecraft:underwater_ruin/brick_1",
            "desert_well" => "minecraft:desert_well",
            _ => return None,
        };
        let nbt = self.load_template(template)?;
        let rot = (mix64(seed) % 4) as u8;
        let centered = [
            origin[0] - rotated_size(nbt.size, rot)[0] / 2,
            origin[1],
            origin[2] - rotated_size(nbt.size, rot)[2] / 2,
        ];
        Some(emit_piece(&nbt, centered, rot))
    }

    /// Assembles a Jigsaw structure starting from `pool_id` up to `max_depth` / `max_pieces`.
    pub(super) fn assemble_jigsaw(
        &self,
        pool_id: &str,
        target_filter: Option<&str>,
        max_depth: usize,
        seed: u64,
        origin: [i32; 3],
        max_pieces: usize,
    ) -> Option<Vec<([i32; 3], u16)>> {
        let mut rng = mix64(seed ^ 0x4A49_4753_4157);
        let root_pool = self.load_pool(pool_id)?;
        let root_loc = pick_pool_location(&root_pool, &mut rng)?;
        let root_nbt = self.load_template(&root_loc)?;
        let root_rot = (next_u32(&mut rng) % 4) as u8;
        let root_size = rotated_size(root_nbt.size, root_rot);
        let root_origin = [
            origin[0] - root_size[0] / 2,
            origin[1],
            origin[2] - root_size[2] / 2,
        ];

        let mut boxes = Vec::with_capacity(max_pieces);
        boxes.push(piece_aabb(root_origin, root_nbt.size, root_rot));
        let mut out_blocks = emit_piece(&root_nbt, root_origin, root_rot);

        let mut queue = VecDeque::new();
        for blk in &root_nbt.blocks {
            if let Some(jigsaw) = &blk.jigsaw {
                if let Some(filter) = target_filter
                    && jigsaw.name != filter
                {
                    continue;
                }
                let rpos = rotate_pos(blk.pos, root_nbt.size, root_rot);
                let front = jigsaw.front.rotate_cw(root_rot);
                if front.is_horizontal() {
                    queue.push_back(QueuedSocket {
                        world_pos: [
                            root_origin[0] + rpos[0],
                            root_origin[1] + rpos[1],
                            root_origin[2] + rpos[2],
                        ],
                        front,
                        pool: jigsaw.pool.clone(),
                        target: jigsaw.target.clone(),
                        depth: 0,
                    });
                }
            }
        }

        let max_depth = max_depth.clamp(1, 7);
        while let Some(sock) = queue.pop_front() {
            if boxes.len() >= max_pieces || sock.depth >= max_depth {
                continue;
            }
            let Some(pool) = self.load_pool(&sock.pool) else {
                continue;
            };
            let attach_pos = [
                sock.world_pos[0] + sock.front.offset()[0],
                sock.world_pos[1] + sock.front.offset()[1],
                sock.world_pos[2] + sock.front.offset()[2],
            ];
            if !self.try_attach_from_pool(
                &pool,
                &sock,
                attach_pos,
                &mut rng,
                &mut boxes,
                &mut out_blocks,
                &mut queue,
            ) && let Some(fb) = &pool.fallback
                && let Some(fb_pool) = self.load_pool(fb)
            {
                let _ = self.try_attach_from_pool(
                    &fb_pool,
                    &sock,
                    attach_pos,
                    &mut rng,
                    &mut boxes,
                    &mut out_blocks,
                    &mut queue,
                );
            }
        }

        Some(out_blocks)
    }

    #[allow(clippy::too_many_arguments)]
    fn try_attach_from_pool(
        &self,
        pool: &TemplatePool,
        sock: &QueuedSocketRef<'_>,
        attach_pos: [i32; 3],
        rng: &mut u64,
        boxes: &mut Vec<[i32; 6]>,
        out_blocks: &mut Vec<([i32; 3], u16)>,
        queue: &mut VecDeque<QueuedSocketOwned>,
    ) -> bool {
        let desired_front = sock.front.opposite();
        for _ in 0..4 {
            let Some(loc) = pick_pool_location(pool, rng) else {
                continue;
            };
            let Some(child_nbt) = self.load_template(&loc) else {
                continue;
            };
            for blk in &child_nbt.blocks {
                let Some(cj) = &blk.jigsaw else {
                    continue;
                };
                if cj.name != sock.target || !cj.front.is_horizontal() {
                    continue;
                }
                let child_rot = rotation_to_match(cj.front, desired_front);
                let rpos = rotate_pos(blk.pos, child_nbt.size, child_rot);
                let child_origin = [
                    attach_pos[0] - rpos[0],
                    attach_pos[1] - rpos[1],
                    attach_pos[2] - rpos[2],
                ];
                let aabb = piece_aabb(child_origin, child_nbt.size, child_rot);
                if boxes.iter().any(|b| aabbs_overlap(*b, aabb)) {
                    continue;
                }
                boxes.push(aabb);
                out_blocks.extend(emit_piece(&child_nbt, child_origin, child_rot));
                for next_blk in &child_nbt.blocks {
                    if let Some(nj) = &next_blk.jigsaw {
                        if next_blk.pos == blk.pos {
                            continue;
                        }
                        let nf = nj.front.rotate_cw(child_rot);
                        if nf.is_horizontal() {
                            let np = rotate_pos(next_blk.pos, child_nbt.size, child_rot);
                            queue.push_back(QueuedSocketOwned {
                                world_pos: [
                                    child_origin[0] + np[0],
                                    child_origin[1] + np[1],
                                    child_origin[2] + np[2],
                                ],
                                front: nf,
                                pool: nj.pool.clone(),
                                target: nj.target.clone(),
                                depth: sock.depth + 1,
                            });
                        }
                    }
                }
                return true;
            }
        }
        false
    }

    fn load_structure_def(&self, short: &str) -> Option<StructureDef> {
        let mut cache = self.structures.lock().unwrap();
        if let Some(cached) = cache.get(short) {
            return cached.clone();
        }
        let path = self
            .data_root
            .join(format!("worldgen/structure/{short}.json"));
        let parsed = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<RawStructureJson>(&s).ok())
            .map(|r| StructureDef {
                start_pool: r.start_pool,
                size: r.size.unwrap_or(4),
            });
        cache.insert(short.to_owned(), parsed.clone());
        parsed
    }

    fn load_pool(&self, pool_id: &str) -> Option<Arc<TemplatePool>> {
        let short = strip_mc(pool_id);
        let mut cache = self.pools.lock().unwrap();
        if let Some(cached) = cache.get(short) {
            return cached.clone();
        }
        let path = self
            .data_root
            .join(format!("worldgen/template_pool/{short}.json"));
        let parsed = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<RawPoolJson>(&s).ok())
            .map(|raw| {
                let mut elements = Vec::new();
                let mut total_weight = 0u32;
                for entry in raw.elements {
                    if let Some(loc) = entry.element.location
                        && entry.weight > 0
                    {
                        total_weight = total_weight.saturating_add(entry.weight);
                        elements.push(PoolElement {
                            location: loc,
                            weight: entry.weight,
                        });
                    }
                }
                Arc::new(TemplatePool {
                    elements,
                    total_weight,
                    fallback: raw.fallback,
                })
            });
        cache.insert(short.to_owned(), parsed.clone());
        parsed
    }

    fn load_template(&self, template_id: &str) -> Option<Arc<StructureNbt>> {
        let short = strip_mc(template_id);
        let mut cache = self.templates.lock().unwrap();
        if let Some(cached) = cache.get(short) {
            return cached.clone();
        }
        let path = self.data_root.join(format!("structure/{short}.nbt"));
        let parsed = std::fs::read(path)
            .ok()
            .and_then(|bytes| binary::read_compressed(&bytes, binary::Compression::Gzip).ok())
            .and_then(|(_, root)| parse_structure_nbt(&root, &self.registry))
            .map(Arc::new);
        cache.insert(short.to_owned(), parsed.clone());
        parsed
    }
}

// Helper types so `try_attach_from_pool` shares the exact queued socket shape cleanly.
type QueuedSocketOwned = QueuedSocket;
type QueuedSocketRef<'a> = QueuedSocket;

struct QueuedSocket {
    world_pos: [i32; 3],
    front: Dir,
    pool: String,
    target: String,
    depth: usize,
}

fn strip_mc(id: &str) -> &str {
    let s = id.strip_prefix("minecraft:").unwrap_or(id);
    s.trim_start_matches('/')
}

fn mix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn next_u32(state: &mut u64) -> u32 {
    *state = mix64(*state);
    (*state >> 32) as u32
}

fn pick_pool_location(pool: &TemplatePool, rng: &mut u64) -> Option<String> {
    if pool.elements.is_empty() || pool.total_weight == 0 {
        return None;
    }
    let mut pick = next_u32(rng) % pool.total_weight;
    for elem in &pool.elements {
        if pick < elem.weight {
            return Some(elem.location.clone());
        }
        pick -= elem.weight;
    }
    pool.elements.first().map(|e| e.location.clone())
}

fn parse_structure_nbt(tag: &Tag, registry: &RuntimeRegistry) -> Option<StructureNbt> {
    let Tag::Compound(root) = tag else {
        return None;
    };
    let size = match root.get("size") {
        Some(Tag::List(List::Int(v))) if v.len() == 3 => [v[0], v[1], v[2]],
        _ => return None,
    };
    let Tag::List(List::Compound(raw_palette)) = root.get("palette")? else {
        return None;
    };

    #[derive(Clone)]
    struct ParsedPalette {
        is_void: bool,
        is_jigsaw: bool,
        orientation: String,
        rot_states: [u16; 4],
    }

    let mut palette = Vec::with_capacity(raw_palette.len());
    for entry in raw_palette {
        let raw_name = match entry.get("name").or_else(|| entry.get("Name")) {
            Some(Tag::String(s)) => s.clone(),
            _ => "minecraft:air".to_owned(),
        };
        let full_name = if raw_name.contains(':') {
            raw_name
        } else {
            format!("minecraft:{raw_name}")
        };
        let mut props = BTreeMap::new();
        if let Some(Tag::Compound(p)) = entry.get("properties").or_else(|| entry.get("Properties"))
        {
            for (k, v) in p {
                if let Tag::String(s) = v {
                    props.insert(k.clone(), s.clone());
                }
            }
        }
        let is_void = full_name == "minecraft:structure_void";
        let is_jigsaw = full_name == "minecraft:jigsaw";
        let orientation = props
            .get("orientation")
            .cloned()
            .unwrap_or_else(|| "north_up".to_owned());
        let rot_states = if is_void {
            [0; 4]
        } else {
            let mut arr = [0u16; 4];
            for rot in 0..4u8 {
                let rprops = rotate_properties(&props, rot);
                let id = registry
                    .block_id(&full_name, &rprops)
                    .or_else(|| registry.block_id(&full_name, &BTreeMap::new()))
                    .or_else(|| ferrite_data::blocks::default_state(&full_name).map(u32::from))
                    .unwrap_or(0);
                arr[rot as usize] = u16::try_from(id).unwrap_or(0);
            }
            arr
        };
        palette.push(ParsedPalette {
            is_void,
            is_jigsaw,
            orientation,
            rot_states,
        });
    }

    let Tag::List(List::Compound(raw_blocks)) = root.get("blocks")? else {
        return None;
    };
    let mut blocks = Vec::new();
    for b in raw_blocks {
        let pos = match b.get("pos") {
            Some(Tag::List(List::Int(v))) if v.len() == 3 => [v[0], v[1], v[2]],
            _ => continue,
        };
        let state_idx = match b.get("state") {
            Some(Tag::Int(i)) if *i >= 0 => *i as usize,
            _ => continue,
        };
        let Some(pal) = palette.get(state_idx) else {
            continue;
        };
        if pal.is_void {
            continue;
        }
        if pal.is_jigsaw {
            let nbt = match b.get("nbt") {
                Some(Tag::Compound(c)) => Some(c),
                _ => None,
            };
            let get_str = |k: &str, default: &str| -> String {
                nbt.and_then(|c| c.get(k))
                    .and_then(|t| match t {
                        Tag::String(s) => Some(s.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| default.to_owned())
            };
            let final_name = get_str("final_state", "minecraft:air");
            let final_full = if final_name.contains(':') {
                final_name
            } else {
                format!("minecraft:{final_name}")
            };
            let final_state_id = registry
                .block_id(&final_full, &BTreeMap::new())
                .or_else(|| ferrite_data::blocks::default_state(&final_full).map(u32::from))
                .unwrap_or(0) as u16;
            blocks.push(TemplateBlock {
                pos,
                state_rot: [final_state_id; 4],
                jigsaw: Some(JigsawConnector {
                    pool: get_str("pool", "minecraft:empty"),
                    target: get_str("target", "minecraft:empty"),
                    name: get_str("name", "minecraft:empty"),
                    final_state_id,
                    front: Dir::from_orientation(&pal.orientation),
                }),
            });
        } else {
            blocks.push(TemplateBlock {
                pos,
                state_rot: pal.rot_states,
                jigsaw: None,
            });
        }
    }

    Some(StructureNbt { size, blocks })
}

fn rotate_properties(props: &BTreeMap<String, String>, steps: u8) -> BTreeMap<String, String> {
    let steps = steps % 4;
    if steps == 0 || props.is_empty() {
        return props.clone();
    }
    let mut out = props.clone();
    if let Some(facing) = out.get_mut("facing") {
        let mut d = Dir::from_orientation(facing);
        if d.is_horizontal() {
            d = d.rotate_cw(steps);
            *facing = match d {
                Dir::North => "north",
                Dir::East => "east",
                Dir::South => "south",
                Dir::West => "west",
                _ => "north",
            }
            .to_owned();
        }
    }
    if !steps.is_multiple_of(2)
        && let Some(axis) = out.get_mut("axis")
    {
        if axis == "x" {
            *axis = "z".to_owned();
        } else if axis == "z" {
            *axis = "x".to_owned();
        }
    }
    if let Some(rot_str) = out.get_mut("rotation")
        && let Ok(r) = rot_str.parse::<u8>()
    {
        *rot_str = ((r + steps * 4) % 16).to_string();
    }
    out
}

fn rotated_size(size: [i32; 3], rot: u8) -> [i32; 3] {
    if rot.is_multiple_of(2) {
        size
    } else {
        [size[2], size[1], size[0]]
    }
}

fn rotate_pos(pos: [i32; 3], size: [i32; 3], rot: u8) -> [i32; 3] {
    let [x, y, z] = pos;
    let [sx, _, sz] = size;
    match rot % 4 {
        0 => [x, y, z],
        1 => [sz - 1 - z, y, x],
        2 => [sx - 1 - x, y, sz - 1 - z],
        3 => [z, y, sx - 1 - x],
        _ => unreachable!(),
    }
}

fn rotation_to_match(from: Dir, to: Dir) -> u8 {
    for rot in 0..4u8 {
        if from.rotate_cw(rot) == to {
            return rot;
        }
    }
    0
}

fn piece_aabb(origin: [i32; 3], size: [i32; 3], rot: u8) -> [i32; 6] {
    let rs = rotated_size(size, rot);
    [
        origin[0],
        origin[1],
        origin[2],
        origin[0] + rs[0] - 1,
        origin[1] + rs[1] - 1,
        origin[2] + rs[2] - 1,
    ]
}

fn aabbs_overlap(a: [i32; 6], b: [i32; 6]) -> bool {
    a[0] <= b[3] && a[3] >= b[0] && a[1] <= b[4] && a[4] >= b[1] && a[2] <= b[5] && a[5] >= b[2]
}

fn emit_piece(nbt: &StructureNbt, origin: [i32; 3], rot: u8) -> Vec<([i32; 3], u16)> {
    let mut out = Vec::with_capacity(nbt.blocks.len());
    for blk in &nbt.blocks {
        let p = rotate_pos(blk.pos, nbt.size, rot);
        let state = blk
            .jigsaw
            .as_ref()
            .map_or(blk.state_rot[(rot % 4) as usize], |j| j.final_state_id);
        out.push((
            [origin[0] + p[0], origin[1] + p[1], origin[2] + p[2]],
            state,
        ));
    }
    out
}
