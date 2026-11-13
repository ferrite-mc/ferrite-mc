//! Structure templates for `/place template`: loads a `.nbt` template the user extracted from
//! their own server jar (`tools/fetch_vanilla.py`), applies rotation, mirror and integrity, and
//! yields the block states to write relative to the placement position.
//!
//! Block entity data and template entities are placed (the latter through
//! [`Template::place_entities`], spawned by the world loop); jigsaw blocks become their
//! `final_state`; `structure_void` and `structure_block` are skipped.

use std::path::{Path, PathBuf};

use ferrite_nbt::{Compound, List, Tag};

/// Which way a template is turned, clockwise quarter turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    None,
    Clockwise90,
    Clockwise180,
    CounterClockwise90,
}

impl Rotation {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "none" => Self::None,
            "clockwise_90" => Self::Clockwise90,
            "180" => Self::Clockwise180,
            "counterclockwise_90" => Self::CounterClockwise90,
            _ => return None,
        })
    }

    fn steps(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Clockwise90 => 1,
            Self::Clockwise180 => 2,
            Self::CounterClockwise90 => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirror {
    None,
    /// Mirrors across the Z axis (vanilla `left_right`).
    LeftRight,
    /// Mirrors across the X axis (vanilla `front_back`).
    FrontBack,
}

impl Mirror {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "none" => Self::None,
            "left_right" => Self::LeftRight,
            "front_back" => Self::FrontBack,
            _ => return None,
        })
    }
}

/// An entity of a template: its position relative to the placement origin and its saved NBT.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateEntity {
    pub pos: [f64; 3],
    pub nbt: Compound,
}

/// A parsed template: its palette as global state ids (`None` for unknown blocks).
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub size: [i32; 3],
    palette: Vec<Option<u16>>,
    blocks: Vec<([i32; 3], usize, Option<Compound>)>,
    entities: Vec<TemplateEntity>,
}

fn data_dir() -> Option<PathBuf> {
    let relative = Path::new("vanilla/26.3/generated/data/minecraft/structure");
    [
        relative.to_path_buf(),
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative),
    ]
    .into_iter()
    .find(|dir| dir.is_dir())
}

/// Whether the extracted templates are on this machine.
#[cfg(test)]
pub fn available() -> bool {
    data_dir().is_some()
}

fn ints(tag: Option<&Tag>) -> Option<Vec<i32>> {
    match tag? {
        Tag::List(List::Int(values)) => Some(values.clone()),
        _ => None,
    }
}

fn compounds(tag: Option<&Tag>) -> Vec<&Compound> {
    match tag {
        Some(Tag::List(List::Compound(values))) => values.iter().collect(),
        _ => Vec::new(),
    }
}

/// `minecraft:oak_stairs[facing=east,half=top]` to a state id.
pub fn parse_state(text: &str) -> Option<u16> {
    let (name, props) = match text.split_once('[') {
        Some((name, rest)) => (name, rest.strip_suffix(']')?),
        None => (text, ""),
    };
    let pairs: Vec<(&str, &str)> = props
        .split(',')
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.split_once('='))
        .collect();
    ferrite_data::blocks::state_id(name, &pairs)
}

fn palette_state(entry: &Compound) -> Option<u16> {
    // 26.x writes `id`/`properties`; older templates used `Name`/`Properties`.
    let Some(Tag::String(name)) = entry.get("id").or_else(|| entry.get("Name")) else {
        return None;
    };
    let mut pairs: Vec<(String, String)> = Vec::new();
    if let Some(Tag::Compound(props)) = entry.get("properties").or_else(|| entry.get("Properties"))
    {
        for (key, value) in props {
            if let Tag::String(value) = value {
                pairs.push((key.clone(), value.clone()));
            }
        }
    }
    let refs: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    ferrite_data::blocks::state_id(name, &refs)
}

impl Template {
    /// Parses the decompressed root compound of a template file.
    pub fn from_nbt(root: &Compound) -> Option<Self> {
        let size = ints(root.get("size"))?;
        let palette_entries: Vec<&Compound> = match root.get("palette") {
            Some(tag) => compounds(Some(tag)),
            None => match root.get("palettes") {
                Some(Tag::List(List::List(lists))) => match lists.first() {
                    Some(List::Compound(values)) => values.iter().collect(),
                    _ => Vec::new(),
                },
                _ => Vec::new(),
            },
        };
        let palette = palette_entries.iter().map(|e| palette_state(e)).collect();
        let mut blocks = Vec::new();
        for block in compounds(root.get("blocks")) {
            let pos = ints(block.get("pos"))?;
            let Some(Tag::Int(state)) = block.get("state") else {
                continue;
            };
            let mut index = usize::try_from(*state).ok()?;
            let pos = [*pos.first()?, *pos.get(1)?, *pos.get(2)?];
            // Jigsaw blocks turn into what they were meant to become.
            if let Some(Tag::Compound(nbt)) = block.get("nbt")
                && let Some(Tag::String(final_state)) = nbt.get("final_state")
                && let Some(id) = parse_state(final_state)
            {
                blocks.push((pos, usize::MAX - usize::from(id), None));
                continue;
            }
            if index >= palette_entries.len() {
                index = usize::MAX;
            }
            let nbt = match block.get("nbt") {
                Some(Tag::Compound(nbt)) if nbt.get("id").is_some() => Some(nbt.clone()),
                _ => None,
            };
            blocks.push((pos, index, nbt));
        }
        let mut entities = Vec::new();
        for entity in compounds(root.get("entities")) {
            let (Some(Tag::List(List::Double(pos))), Some(Tag::Compound(nbt))) =
                (entity.get("pos"), entity.get("nbt"))
            else {
                continue;
            };
            if pos.len() == 3 && nbt.get("id").is_some() {
                entities.push(TemplateEntity {
                    pos: [pos[0], pos[1], pos[2]],
                    nbt: nbt.clone(),
                });
            }
        }
        Some(Self {
            size: [*size.first()?, *size.get(1)?, *size.get(2)?],
            palette,
            blocks,
            entities,
        })
    }

    /// Loads `minecraft:igloo/top` (or `igloo/top`) from the extracted templates.
    pub fn load(id: &str) -> Option<Self> {
        let path = id.strip_prefix("minecraft:").unwrap_or(id);
        if path.contains("..") || path.contains(':') || path.starts_with('/') {
            return None;
        }
        let file = data_dir()?.join(format!("{path}.nbt"));
        let bytes = std::fs::read(file).ok()?;
        let (_, tag) =
            ferrite_nbt::binary::read_compressed(&bytes, ferrite_nbt::binary::Compression::Gzip)
                .ok()?;
        match tag {
            Tag::Compound(root) => Self::from_nbt(&root),
            _ => None,
        }
    }

    /// The blocks relative to the placement position, after mirror, rotation and integrity.
    /// `integrity` is the chance (0..=1) that each block is kept; `seed` drives that roll.
    #[cfg(test)]
    pub fn place(
        &self,
        rotation: Rotation,
        mirror: Mirror,
        integrity: f32,
        seed: u64,
    ) -> Vec<([i32; 3], u16)> {
        self.place_with_entities(rotation, mirror, integrity, seed)
            .0
    }

    /// Like [`Self::place`], plus the block entity NBT (chest contents, signs, spawners...) of the
    /// blocks that were kept, at their transformed positions.
    #[allow(clippy::type_complexity)]
    pub fn place_with_entities(
        &self,
        rotation: Rotation,
        mirror: Mirror,
        integrity: f32,
        seed: u64,
    ) -> (Vec<([i32; 3], u16)>, Vec<([i32; 3], Compound)>) {
        let mut rng = seed ^ 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            (rng >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut out = Vec::new();
        let mut entities = Vec::new();
        for (pos, index, nbt) in &self.blocks {
            let state = if *index > usize::MAX / 2 {
                u16::try_from(usize::MAX - index).ok()
            } else {
                self.palette.get(*index).copied().flatten()
            };
            let Some(state) = state else { continue };
            let Some((name, _)) = ferrite_data::blocks::state(state) else {
                continue;
            };
            if matches!(
                name.trim_start_matches("minecraft:"),
                "structure_void" | "structure_block"
            ) {
                continue;
            }
            if integrity < 1.0 && next() >= f64::from(integrity) {
                continue;
            }
            let [mut x, y, mut z] = *pos;
            match mirror {
                Mirror::None => {}
                Mirror::LeftRight => z = -z,
                Mirror::FrontBack => x = -x,
            }
            let (x, z) = match rotation {
                Rotation::None => (x, z),
                Rotation::Clockwise90 => (-z, x),
                Rotation::Clockwise180 => (-x, -z),
                Rotation::CounterClockwise90 => (z, -x),
            };
            out.push(([x, y, z], transform_state(state, rotation, mirror)));
            if let Some(nbt) = nbt {
                entities.push(([x, y, z], nbt.clone()));
            }
        }
        (out, entities)
    }
}

/// Vanilla `Mth.wrapDegrees`.
fn wrap_degrees(angle: f32) -> f32 {
    let wrapped = angle % 360.0;
    if wrapped >= 180.0 {
        wrapped - 360.0
    } else if wrapped < -180.0 {
        wrapped + 360.0
    } else {
        wrapped
    }
}

impl Template {
    /// The entities after mirror and rotation, positions relative to the placement origin.
    ///
    /// `StructureTemplate.transform(Vec3, ...)` with the pivot at the origin: a mirror flips
    /// `z`/`x` to `1 - z`/`1 - x`, a quarter turn clockwise maps `(x, z)` to `(1 - z, x)`, a half
    /// turn to `(1 - x, 1 - z)` and a quarter turn counter-clockwise to `(z, 1 - x)`. The yaw is
    /// `Entity.rotate(rotation) + (Entity.mirror(mirror) - yaw)` (`+90/+180/+270`; `-yaw` for a
    /// front-back mirror, `180 - yaw` for a left-right one). `UUID` is dropped so the world
    /// gives each spawned entity a new one. Integrity does not thin entities. The `Facing` of
    /// hanging entities (item frames, paintings) is not turned.
    pub fn place_entities(&self, rotation: Rotation, mirror: Mirror) -> Vec<TemplateEntity> {
        self.entities
            .iter()
            .map(|entity| {
                let [mut x, y, mut z] = entity.pos;
                match mirror {
                    Mirror::None => {}
                    Mirror::LeftRight => z = 1.0 - z,
                    Mirror::FrontBack => x = 1.0 - x,
                }
                let (x, z) = match rotation {
                    Rotation::None => (x, z),
                    Rotation::Clockwise90 => (1.0 - z, x),
                    Rotation::Clockwise180 => (1.0 - x, 1.0 - z),
                    Rotation::CounterClockwise90 => (z, 1.0 - x),
                };
                let mut nbt = entity.nbt.clone();
                nbt.shift_remove("UUID");
                let (yaw, pitch) = match nbt.get("Rotation") {
                    Some(Tag::List(List::Float(angles))) if angles.len() == 2 => {
                        (angles[0], angles[1])
                    }
                    _ => (0.0, 0.0),
                };
                let wrapped = wrap_degrees(yaw);
                let turned = wrapped
                    + match rotation {
                        Rotation::None => 0.0,
                        Rotation::Clockwise90 => 90.0,
                        Rotation::Clockwise180 => 180.0,
                        Rotation::CounterClockwise90 => 270.0,
                    };
                let mirrored = match mirror {
                    Mirror::None => wrapped,
                    Mirror::FrontBack => -wrapped,
                    Mirror::LeftRight => 180.0 - wrapped,
                };
                nbt.insert(
                    "Rotation".into(),
                    Tag::List(List::Float(vec![turned + (mirrored - yaw), pitch])),
                );
                TemplateEntity {
                    pos: [x, y, z],
                    nbt,
                }
            })
            .collect()
    }
}

const HORIZONTAL: [&str; 4] = ["north", "east", "south", "west"];

fn mirror_direction(value: &str, mirror: Mirror) -> String {
    let swap = |a: &str, b: &str| {
        if value == a {
            b.to_owned()
        } else if value == b {
            a.to_owned()
        } else {
            value.to_owned()
        }
    };
    match mirror {
        Mirror::None => value.to_owned(),
        Mirror::LeftRight => swap("north", "south"),
        Mirror::FrontBack => swap("east", "west"),
    }
}

fn rotate_direction(value: &str, steps: u8) -> String {
    match HORIZONTAL.iter().position(|d| *d == value) {
        Some(i) => HORIZONTAL[(i + usize::from(steps)) % 4].to_owned(),
        None => value.to_owned(),
    }
}

/// Rotates and mirrors the directional properties of a state (`facing`, `axis`, `rotation`
/// and the `north/east/south/west` connections). Stair shapes and similar are left as they are.
pub fn transform_state(state: u16, rotation: Rotation, mirror: Mirror) -> u16 {
    let Some((name, props)) = ferrite_data::blocks::state(state) else {
        return state;
    };
    let steps = rotation.steps();
    let mut mapped: Vec<(String, String)> = props
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    let old = mapped.clone();
    let get = |key: &str| old.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    for (key, value) in &mut mapped {
        match key.as_str() {
            "facing" => {
                *value = rotate_direction(&mirror_direction(value, mirror), steps);
            }
            "axis" if steps % 2 == 1 => {
                *value = match value.as_str() {
                    "x" => "z".to_owned(),
                    "z" => "x".to_owned(),
                    other => other.to_owned(),
                };
            }
            "rotation" => {
                if let Ok(r) = value.parse::<i32>() {
                    let mirrored = match mirror {
                        Mirror::None => r,
                        Mirror::LeftRight => (16 - r).rem_euclid(16),
                        Mirror::FrontBack => (8 - r).rem_euclid(16),
                    };
                    *value = ((mirrored + 4 * i32::from(steps)).rem_euclid(16)).to_string();
                }
            }
            "north" | "east" | "south" | "west" => {
                // The new side takes the value its source side had before the turn.
                let from = HORIZONTAL
                    .iter()
                    .position(|d| d == key)
                    .map(|i| HORIZONTAL[(i + 4 - usize::from(steps)) % 4])
                    .map(|d| mirror_direction(d, mirror));
                if let Some(value_from) = from.and_then(|d| get(&d)) {
                    *value = value_from;
                }
            }
            _ => {}
        }
    }
    let refs: Vec<(&str, &str)> = mapped
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    ferrite_data::blocks::state_id(name, &refs).unwrap_or(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(text: &str) -> u16 {
        parse_state(text).unwrap()
    }

    #[test]
    fn facing_turns_with_rotation_and_flips_with_mirror() {
        let north = id("minecraft:furnace[facing=north,lit=false]");
        assert_eq!(
            transform_state(north, Rotation::Clockwise90, Mirror::None),
            id("minecraft:furnace[facing=east,lit=false]")
        );
        assert_eq!(
            transform_state(north, Rotation::None, Mirror::LeftRight),
            id("minecraft:furnace[facing=south,lit=false]")
        );
        assert_eq!(
            transform_state(north, Rotation::Clockwise180, Mirror::None),
            id("minecraft:furnace[facing=south,lit=false]")
        );
    }

    #[test]
    fn axis_and_connections_follow_the_turn() {
        let log = id("minecraft:oak_log[axis=x]");
        assert_eq!(
            transform_state(log, Rotation::Clockwise90, Mirror::None),
            id("minecraft:oak_log[axis=z]")
        );
        let fence = id("minecraft:oak_fence[north=true]");
        assert_eq!(
            transform_state(fence, Rotation::Clockwise90, Mirror::None),
            id("minecraft:oak_fence[east=true]")
        );
    }

    #[test]
    fn small_template_places_rotated_and_respects_integrity() {
        let mut palette = Compound::new();
        palette.insert("id".into(), Tag::String("minecraft:stone".into()));
        let mut a = Compound::new();
        a.insert("pos".into(), Tag::List(List::Int(vec![1, 0, 0])));
        a.insert("state".into(), Tag::Int(0));
        let mut b = Compound::new();
        b.insert("pos".into(), Tag::List(List::Int(vec![0, 0, 1])));
        b.insert("state".into(), Tag::Int(0));
        let mut root = Compound::new();
        root.insert("size".into(), Tag::List(List::Int(vec![2, 1, 2])));
        root.insert("palette".into(), Tag::List(List::Compound(vec![palette])));
        root.insert("blocks".into(), Tag::List(List::Compound(vec![a, b])));
        let template = Template::from_nbt(&root).unwrap();
        let stone = id("minecraft:stone");
        let placed = template.place(Rotation::Clockwise90, Mirror::None, 1.0, 1);
        assert_eq!(placed, vec![([0, 0, 1], stone), ([-1, 0, 0], stone)]);
        assert!(
            template
                .place(Rotation::None, Mirror::None, 0.0, 1)
                .is_empty()
        );
        assert_eq!(
            template.place(Rotation::None, Mirror::None, 1.0, 1).len(),
            2
        );
    }

    #[test]
    fn block_entity_nbt_follows_its_block_through_rotation() {
        let mut palette = Compound::new();
        palette.insert("id".into(), Tag::String("minecraft:stone".into()));
        let mut chest = Compound::new();
        chest.insert("id".into(), Tag::String("minecraft:chest".into()));
        let mut a = Compound::new();
        a.insert("pos".into(), Tag::List(List::Int(vec![1, 0, 0])));
        a.insert("state".into(), Tag::Int(0));
        a.insert("nbt".into(), Tag::Compound(chest.clone()));
        let mut b = Compound::new();
        b.insert("pos".into(), Tag::List(List::Int(vec![0, 0, 1])));
        b.insert("state".into(), Tag::Int(0));
        let mut root = Compound::new();
        root.insert("size".into(), Tag::List(List::Int(vec![2, 1, 2])));
        root.insert("palette".into(), Tag::List(List::Compound(vec![palette])));
        root.insert("blocks".into(), Tag::List(List::Compound(vec![a, b])));
        let template = Template::from_nbt(&root).unwrap();
        let (blocks, entities) =
            template.place_with_entities(Rotation::Clockwise90, Mirror::None, 1.0, 1);
        assert_eq!(blocks.len(), 2);
        assert_eq!(entities, vec![([0, 0, 1], chest)]);
        let (_, none) = template.place_with_entities(Rotation::None, Mirror::None, 0.0, 1);
        assert!(none.is_empty());
    }

    fn entity_template() -> Template {
        let mut palette = Compound::new();
        palette.insert("id".into(), Tag::String("minecraft:stone".into()));
        let mut block = Compound::new();
        block.insert("pos".into(), Tag::List(List::Int(vec![0, 0, 0])));
        block.insert("state".into(), Tag::Int(0));
        let mut nbt = Compound::new();
        nbt.insert("id".into(), Tag::String("minecraft:pig".into()));
        nbt.insert("UUID".into(), Tag::IntArray(vec![1, 2, 3, 4]));
        nbt.insert("Rotation".into(), Tag::List(List::Float(vec![10.0, 5.0])));
        let mut entity = Compound::new();
        entity.insert("pos".into(), Tag::List(List::Double(vec![0.25, 1.0, 0.75])));
        entity.insert("blockPos".into(), Tag::List(List::Int(vec![0, 1, 0])));
        entity.insert("nbt".into(), Tag::Compound(nbt));
        let mut root = Compound::new();
        root.insert("size".into(), Tag::List(List::Int(vec![1, 2, 1])));
        root.insert("palette".into(), Tag::List(List::Compound(vec![palette])));
        root.insert("blocks".into(), Tag::List(List::Compound(vec![block])));
        root.insert("entities".into(), Tag::List(List::Compound(vec![entity])));
        Template::from_nbt(&root).unwrap()
    }

    fn yaw_of(entity: &TemplateEntity) -> f32 {
        match entity.nbt.get("Rotation") {
            Some(Tag::List(List::Float(angles))) => angles[0],
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn template_entities_are_parsed_and_lose_their_uuid() {
        let template = entity_template();
        let placed = template.place_entities(Rotation::None, Mirror::None);
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].pos, [0.25, 1.0, 0.75]);
        assert!(placed[0].nbt.get("UUID").is_none());
        assert_eq!(yaw_of(&placed[0]), 10.0);
    }

    #[test]
    fn template_entities_follow_vanillas_vec3_transform() {
        let template = entity_template();
        let at = |rotation, mirror| {
            let placed = template.place_entities(rotation, mirror);
            (placed[0].pos, yaw_of(&placed[0]))
        };
        // (x, z) = (0.25, 0.75): clockwise turn gives (1 - z, x), counter-clockwise (z, 1 - x).
        assert_eq!(
            at(Rotation::Clockwise90, Mirror::None),
            ([0.25, 1.0, 0.25], 100.0)
        );
        assert_eq!(
            at(Rotation::Clockwise180, Mirror::None),
            ([0.75, 1.0, 0.25], 190.0)
        );
        assert_eq!(
            at(Rotation::CounterClockwise90, Mirror::None),
            ([0.75, 1.0, 0.75], 280.0)
        );
        // Mirrors flip one axis around the block and mirror the yaw.
        assert_eq!(
            at(Rotation::None, Mirror::LeftRight),
            ([0.25, 1.0, 0.25], 170.0)
        );
        assert_eq!(
            at(Rotation::None, Mirror::FrontBack),
            ([0.75, 1.0, 0.75], -10.0)
        );
    }

    #[test]
    fn real_templates_with_entities_keep_them() {
        if !available() {
            return;
        }
        // igloo/bottom holds two villagers in the 26.3 data.
        let igloo = Template::load("minecraft:igloo/bottom").expect("igloo/bottom");
        let placed = igloo.place_entities(Rotation::None, Mirror::None);
        assert_eq!(placed.len(), 2);
        assert!(placed.iter().all(|e| e.nbt.get("id").is_some()));
        let top = Template::load("minecraft:igloo/top").expect("igloo/top");
        assert!(top.place_entities(Rotation::None, Mirror::None).is_empty());
    }

    #[test]
    fn real_templates_load_when_extracted() {
        if !available() {
            return;
        }
        let igloo = Template::load("minecraft:igloo/top").expect("igloo/top");
        assert_eq!(igloo.size, [7, 5, 8]);
        assert!(!igloo.place(Rotation::None, Mirror::None, 1.0, 0).is_empty());
        assert!(Template::load("minecraft:nope/nothing").is_none());
        assert!(Template::load("../secret").is_none());
    }
}
