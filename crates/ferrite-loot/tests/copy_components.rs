//! `copy_components` from a block entity: the loot context carries the components the block
//! entity would write onto its item, and the function copies the ones the table names.

use ferrite_loot::{LootContext, LootStack, LootTables, Rng};
use ferrite_nbt::Tag;
use std::collections::BTreeMap;
use std::path::PathBuf;

struct Zero;

impl Rng for Zero {
    fn next_int(&mut self, _bound: i32) -> i32 {
        0
    }

    fn next_float(&mut self) -> f32 {
        0.0
    }
}

/// A loot directory written to a scratch folder, removed again on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str, tables: &[(&str, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!("ferrite-loot-{name}-{}", std::process::id()));
        let dir = root.join("data/minecraft/loot_table");
        for (id, json) in tables {
            let path = dir.join(format!("{id}.json"));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, json).unwrap();
        }
        Self(root)
    }

    fn load(&self) -> LootTables {
        LootTables::load(self.0.join("data/minecraft/loot_table")).unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn components(entries: &[(&str, Tag)]) -> BTreeMap<String, Tag> {
    entries
        .iter()
        .map(|(name, tag)| ((*name).to_owned(), tag.clone()))
        .collect()
}

fn context(entries: &[(&str, Tag)]) -> LootContext {
    LootContext {
        block_entity_components: components(entries),
        ..LootContext::default()
    }
}

fn empty_list() -> Tag {
    Tag::List(ferrite_nbt::List::Compound(Vec::new()))
}

const COPY: &str = r#"{
  "type": "minecraft:block",
  "pools": [{"rolls": 1, "entries": [{
    "type": "minecraft:item", "name": "minecraft:white_banner",
    "functions": [{"type": "minecraft:copy_components", "source": "block_entity",
      "include": ["minecraft:custom_name", "minecraft:lock"]}]
  }]}]
}"#;

#[test]
fn copy_components_copies_only_the_included_components() {
    let scratch = Scratch::new("include", &[("blocks/banner", COPY)]);
    let tables = scratch.load();
    let table = tables.get("minecraft:blocks/banner").unwrap();
    let ctx = context(&[
        ("minecraft:custom_name", Tag::String("\"Box\"".into())),
        ("minecraft:lock", Tag::String("key".into())),
        ("minecraft:container", empty_list()),
    ]);
    let stacks = table.roll_stacks(&ctx, &mut Zero);
    assert_eq!(stacks.len(), 1);
    assert_eq!(stacks[0].item, "minecraft:white_banner");
    assert_eq!(
        stacks[0].components,
        components(&[
            ("minecraft:custom_name", Tag::String("\"Box\"".into())),
            ("minecraft:lock", Tag::String("key".into())),
        ]),
        "`container` is not in the include list and must stay behind"
    );
}

#[test]
fn copy_components_without_include_copies_everything_but_the_excluded() {
    let json = COPY.replace(
        r#""include": ["minecraft:custom_name", "minecraft:lock"]"#,
        r#""exclude": ["minecraft:lock"]"#,
    );
    let scratch = Scratch::new("exclude", &[("blocks/banner", &json)]);
    let tables = scratch.load();
    let table = tables.get("minecraft:blocks/banner").unwrap();
    let ctx = context(&[
        ("minecraft:custom_name", Tag::String("\"Box\"".into())),
        ("minecraft:lock", Tag::String("key".into())),
        ("minecraft:rarity", Tag::String("epic".into())),
    ]);
    let stacks = table.roll_stacks(&ctx, &mut Zero);
    assert_eq!(
        stacks[0]
            .components
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["minecraft:custom_name", "minecraft:rarity"]
    );
}

#[test]
fn copy_components_without_a_block_entity_copies_nothing() {
    let scratch = Scratch::new("none", &[("blocks/banner", COPY)]);
    let tables = scratch.load();
    let table = tables.get("minecraft:blocks/banner").unwrap();
    let stacks = table.roll_stacks(&LootContext::default(), &mut Zero);
    assert_eq!(
        stacks,
        [LootStack {
            item: "minecraft:white_banner".into(),
            count: 1,
            components: BTreeMap::new(),
        }]
    );
}

#[test]
fn the_tuple_roll_still_reports_item_and_count() {
    let scratch = Scratch::new("tuple", &[("blocks/banner", COPY)]);
    let tables = scratch.load();
    let table = tables.get("minecraft:blocks/banner").unwrap();
    let ctx = context(&[("minecraft:lock", Tag::String("key".into()))]);
    assert_eq!(
        table.roll(&ctx, &mut Zero),
        [("minecraft:white_banner".to_owned(), 1)]
    );
}

#[test]
fn an_unsupported_source_copies_nothing_and_keeps_the_item() {
    let json = COPY.replace("block_entity", "this");
    let scratch = Scratch::new("source", &[("blocks/banner", &json)]);
    let tables = scratch.load();
    let table = tables.get("minecraft:blocks/banner").unwrap();
    let ctx = context(&[("minecraft:lock", Tag::String("key".into()))]);
    let stacks = table.roll_stacks(&ctx, &mut Zero);
    assert_eq!(stacks.len(), 1);
    assert!(stacks[0].components.is_empty());
}

/// Every vanilla shulker box table includes the same four components, so each of the 17 tables
/// must carry exactly those and leave unrelated block entity components behind.
#[test]
fn every_vanilla_shulker_box_table_copies_its_four_components() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../vanilla/26.3/generated/data/minecraft/loot_table");
    assert!(dir.is_dir(), "vanilla data is missing at {}", dir.display());
    let tables = LootTables::load(&dir).unwrap();
    let colors = [
        "white",
        "orange",
        "magenta",
        "light_blue",
        "yellow",
        "lime",
        "pink",
        "gray",
        "light_gray",
        "cyan",
        "purple",
        "blue",
        "brown",
        "green",
        "red",
        "black",
    ];
    let mut ids = vec!["shulker_box".to_owned()];
    ids.extend(colors.iter().map(|color| format!("{color}_shulker_box")));
    assert_eq!(ids.len(), 17);
    for id in ids {
        let table = tables.get(&format!("minecraft:blocks/{id}")).unwrap();
        let ctx = context(&[
            ("minecraft:custom_name", Tag::String("\"N\"".into())),
            ("minecraft:container", empty_list()),
            ("minecraft:lock", Tag::String("k".into())),
            (
                "minecraft:container_loot",
                Tag::Compound(Default::default()),
            ),
            ("minecraft:banner_patterns", empty_list()),
        ]);
        let stacks = table.roll_stacks(&ctx, &mut Zero);
        assert_eq!(stacks.len(), 1, "{id}");
        assert_eq!(stacks[0].item, format!("minecraft:{id}"));
        assert_eq!(
            stacks[0]
                .components
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [
                "minecraft:container",
                "minecraft:container_loot",
                "minecraft:custom_name",
                "minecraft:lock"
            ],
            "{id}: only the included components are copied"
        );
    }
}
