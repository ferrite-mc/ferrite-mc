//! Every loot function the shipped 26.3 tables use, rolled through the real tables.
//!
//! Skipped when the generated vanilla data is not on this machine.

use ferrite_loot::{DamageFacts, EntityFacts, LootContext, LootHost, LootStack, LootTables, Rng};
use ferrite_nbt::Tag;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

struct Lcg(u64);

impl Rng for Lcg {
    fn next_int(&mut self, bound: i32) -> i32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) % bound.max(1) as u64) as i32
    }

    fn next_float(&mut self) -> f32 {
        self.next_int(1 << 24) as f32 / (1 << 24) as f32
    }
}

#[derive(Debug)]
struct Host(&'static str);

impl LootHost for Host {
    fn enchantability(&self, _item: &str) -> Option<i32> {
        Some(15)
    }
    fn max_damage(&self, _item: &str) -> Option<i32> {
        Some(200)
    }
    fn max_stack_size(&self, item: &str) -> i32 {
        if item.ends_with("_sword") { 1 } else { 64 }
    }
    fn biome_at(&self, _pos: [i32; 3]) -> Option<String> {
        Some(format!("minecraft:{}", self.0))
    }
    fn block_at(&self, _pos: [i32; 3]) -> Option<(String, BTreeMap<String, String>)> {
        None
    }
    fn exploration_map(
        &self,
        _origin: [f64; 3],
        destinations: &[String],
        decoration: &str,
        zoom: i32,
        _skip: bool,
    ) -> Option<Vec<(String, Tag)>> {
        assert!(
            !destinations.is_empty(),
            "destination tag resolved to structures"
        );
        Some(vec![
            ("minecraft:map_id".to_owned(), Tag::Int(7)),
            (
                "minecraft:custom_data".to_owned(),
                Tag::String(format!("{decoration}/{zoom}")),
            ),
        ])
    }
}

fn tables() -> Option<LootTables> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vanilla/26.3/generated/data/minecraft/loot_table");
    dir.is_dir().then(|| LootTables::load(dir).unwrap())
}

fn cow(on_fire: bool) -> EntityFacts {
    EntityFacts {
        entity_type: "minecraft:cow".to_owned(),
        on_fire,
        ..EntityFacts::default()
    }
}

type Weapon<'a> = Option<(&'a str, &'a [(&'a str, i32)])>;

fn player(weapon: Weapon) -> EntityFacts {
    EntityFacts {
        entity_type: "minecraft:player".to_owned(),
        mainhand: weapon.map(|(item, enchants)| {
            (
                item.to_owned(),
                enchants
                    .iter()
                    .map(|(name, level)| ((*name).to_owned(), *level))
                    .collect(),
            )
        }),
        ..EntityFacts::default()
    }
}

fn kill_context(looting: i32, on_fire: bool, weapon: Weapon) -> LootContext {
    LootContext {
        killed_by_player: true,
        looting,
        this_entity: Some(cow(on_fire)),
        damage_source: Some(DamageFacts {
            damage_type: "minecraft:player_attack".to_owned(),
            source_entity: Some(player(weapon)),
            direct_entity: Some(player(weapon)),
        }),
        host: Some(Arc::new(Host("plains"))),
        ..LootContext::default()
    }
}

fn totals(
    tables: &LootTables,
    id: &str,
    context: &LootContext,
    rolls: u64,
) -> BTreeMap<String, i32> {
    let table = tables.get(id).unwrap();
    let mut rng = Lcg(11);
    let mut out = BTreeMap::new();
    for _ in 0..rolls {
        for stack in table.roll_stacks(context, &mut rng) {
            *out.entry(stack.item).or_insert(0) += stack.count;
        }
    }
    out
}

fn most_of(tables: &LootTables, id: &str, item: &str, context: &LootContext) -> i32 {
    let table = tables.get(id).unwrap();
    let mut rng = Lcg(5);
    (0..400)
        .flat_map(|_| table.roll_stacks(context, &mut rng))
        .filter(|stack| stack.item == item)
        .map(|stack| stack.count)
        .max()
        .unwrap_or(0)
}

#[test]
fn looting_adds_enchanted_count_increase_to_mob_drops() {
    let Some(tables) = tables() else { return };
    // Cow leather: set_count 0..=2, then looting adds round(level * uniform(0, 1)).
    let plain = kill_context(0, false, None);
    let looted = kill_context(3, false, None);
    assert!(
        most_of(
            &tables,
            "minecraft:entities/cow",
            "minecraft:leather",
            &plain
        ) <= 2
    );
    let best = most_of(
        &tables,
        "minecraft:entities/cow",
        "minecraft:leather",
        &looted,
    );
    assert!(
        (3..=5).contains(&best),
        "looting III leather peaked at {best}"
    );
}

#[test]
fn burning_mobs_and_fire_aspect_weapons_drop_cooked_meat() {
    let Some(tables) = tables() else { return };
    let cow_table = "minecraft:entities/cow";
    let raw = totals(&tables, cow_table, &kill_context(0, false, None), 60);
    assert!(raw.contains_key("minecraft:beef") && !raw.contains_key("minecraft:cooked_beef"));
    let burning = totals(&tables, cow_table, &kill_context(0, true, None), 60);
    assert!(
        burning.contains_key("minecraft:cooked_beef") && !burning.contains_key("minecraft:beef")
    );
    let fire_aspect = kill_context(
        0,
        false,
        Some(("minecraft:iron_sword", &[("minecraft:fire_aspect", 1)])),
    );
    let smelted = totals(&tables, cow_table, &fire_aspect, 60);
    assert!(smelted.contains_key("minecraft:cooked_beef"), "{smelted:?}");
    // A sharpness sword does not smelt.
    let sharp = kill_context(
        0,
        false,
        Some(("minecraft:iron_sword", &[("minecraft:sharpness", 5)])),
    );
    assert!(!totals(&tables, cow_table, &sharp, 60).contains_key("minecraft:cooked_beef"));
}

#[test]
fn smelting_multiplies_by_the_input_count() {
    // use_input_count defaults to true: 1..=3 beef becomes as many cooked beef.
    let Some(tables) = tables() else { return };
    let burning = kill_context(0, true, None);
    let beef = most_of(
        &tables,
        "minecraft:entities/cow",
        "minecraft:cooked_beef",
        &burning,
    );
    assert!(beef >= 2, "cooked beef peaked at {beef}");
}

#[test]
fn luck_raises_fishing_treasure_and_lowers_junk() {
    let Some(tables) = tables() else { return };
    let base = LootContext {
        host: Some(Arc::new(Host("plains"))),
        ..LootContext::default()
    };
    let lucky = LootContext {
        luck: 3.0,
        ..base.clone()
    };
    let count = |context: &LootContext| -> (i32, i32) {
        let table = tables.get("minecraft:gameplay/fishing").unwrap();
        let mut rng = Lcg(3);
        let (mut treasure, mut junk) = (0, 0);
        for _ in 0..3000 {
            for stack in table.roll_stacks(context, &mut rng) {
                match stack.item.as_str() {
                    "minecraft:nautilus_shell" | "minecraft:saddle" | "minecraft:name_tag" => {
                        treasure += 1;
                    }
                    "minecraft:stick"
                    | "minecraft:string"
                    | "minecraft:bone"
                    | "minecraft:rotten_flesh" => junk += 1,
                    _ => {}
                }
            }
        }
        (treasure, junk)
    };
    let (treasure0, junk0) = count(&base);
    let (treasure3, junk3) = count(&lucky);
    assert!(treasure3 > treasure0, "treasure {treasure0} -> {treasure3}");
    assert!(junk3 < junk0, "junk {junk0} -> {junk3}");
}

/// Every shipped table rolls under a rich context, and the functions that attach components
/// leave exactly the components vanilla would.
#[test]
fn every_shipped_table_rolls_and_the_function_components_appear() {
    let Some(tables) = tables() else { return };
    assert_eq!(tables.unsupported(), Vec::<String>::new());
    let contexts: Vec<LootContext> = ["plains", "bamboo_jungle", "cherry_grove", "swamp"]
        .into_iter()
        .map(|biome| LootContext {
            killed_by_player: true,
            looting: 2,
            luck: 1.0,
            this_entity: Some(cow(false)),
            damage_source: Some(DamageFacts {
                damage_type: "minecraft:player_attack".to_owned(),
                source_entity: Some(player(None)),
                direct_entity: Some(player(None)),
            }),
            origin: Some([0.0, 64.0, 0.0]),
            host: Some(Arc::new(Host(biome))),
            ..LootContext::default()
        })
        .collect();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut maps = 0;
    let mut stacks: Vec<LootStack> = Vec::new();
    for id in tables.ids().map(str::to_owned).collect::<Vec<_>>() {
        let table = tables.get(&id).unwrap();
        let mut rng = Lcg(id.len() as u64 + 1);
        for round in 0..25 {
            for stack in table.roll_stacks(&contexts[round % contexts.len()], &mut rng) {
                assert!(stack.count > 0 && stack.count <= 127, "{id}: {stack:?}");
                seen.extend(stack.components.keys().cloned());
                if stack.components.contains_key("minecraft:map_id") {
                    maps += 1;
                }
                stacks.push(stack);
            }
        }
    }
    for component in [
        "minecraft:potion_contents",
        "minecraft:enchantments",
        "minecraft:stored_enchantments",
        "minecraft:damage",
        "minecraft:suspicious_stew_effects",
        "minecraft:instrument",
        "minecraft:ominous_bottle_amplifier",
        "minecraft:item_name",
        "minecraft:trim",
        "minecraft:map_id",
    ] {
        assert!(
            seen.contains(component),
            "no table produced {component}: {seen:?}"
        );
    }
    assert!(maps > 0, "exploration maps were not produced");
    // Enchanted stacks hold real levels.
    for stack in &stacks {
        for name in ["minecraft:enchantments", "minecraft:stored_enchantments"] {
            if let Some(Tag::Compound(levels)) = stack.components.get(name) {
                assert!(!levels.is_empty());
                assert!(
                    levels
                        .values()
                        .all(|level| matches!(level, Tag::Int(l) if (1..=255).contains(l)))
                );
            }
        }
    }
}

#[test]
fn enchant_with_levels_respects_exclusive_sets() {
    let Some(tables) = tables() else { return };
    let context = LootContext {
        host: Some(Arc::new(Host("plains"))),
        ..LootContext::default()
    };
    let table = tables.get("minecraft:chests/end_city_treasure").unwrap();
    let mut rng = Lcg(21);
    let mut enchanted = 0;
    for _ in 0..200 {
        for stack in table.roll_stacks(&context, &mut rng) {
            if let Some(Tag::Compound(levels)) = stack.components.get("minecraft:enchantments") {
                enchanted += 1;
                // Sharpness, smite and bane of arthropods exclude each other.
                let damage = [
                    "minecraft:sharpness",
                    "minecraft:smite",
                    "minecraft:bane_of_arthropods",
                ]
                .iter()
                .filter(|name| levels.contains_key(**name))
                .count();
                assert!(damage <= 1, "{levels:?}");
            }
        }
    }
    assert!(enchanted > 0);
}

#[test]
fn without_a_host_enchanting_leaves_the_stack_alone() {
    let Some(tables) = tables() else { return };
    let table = tables.get("minecraft:chests/end_city_treasure").unwrap();
    let mut rng = Lcg(1);
    for _ in 0..50 {
        for stack in table.roll_stacks(&LootContext::default(), &mut rng) {
            assert!(!stack.components.contains_key("minecraft:enchantments"));
        }
    }
}

#[test]
fn location_checks_use_the_hosts_biome() {
    let Some(tables) = tables() else { return };
    // Plains: the jungle-only bamboo junk entry never drops.
    let context = LootContext {
        origin: Some([0.0, 64.0, 0.0]),
        host: Some(Arc::new(Host("plains"))),
        ..LootContext::default()
    };
    let drops = totals(&tables, "minecraft:gameplay/fishing/junk", &context, 600);
    assert!(!drops.contains_key("minecraft:bamboo"), "{drops:?}");
    assert!(!drops.is_empty());
}

fn run(tables: &LootTables, json: &str, stack: &mut LootStack, context: &LootContext) {
    let function = tables
        .parse_function(&serde_json::from_str(json).unwrap())
        .unwrap();
    function.apply_to(stack, context, &mut Lcg(5));
}

#[test]
fn simple_item_functions_write_their_components() {
    let Some(tables) = tables() else { return };
    let context = LootContext::default();

    let mut sword = LootStack::new("minecraft:iron_sword");
    run(
        &tables,
        r#"{"type":"minecraft:set_attributes","replace":true,"modifiers":[{"attribute":"minecraft:attack_damage","id":"minecraft:test","amount":3,"operation":"add_value","slot":"mainhand"}]}"#,
        &mut sword,
        &context,
    );
    let Some(Tag::List(ferrite_nbt::List::Compound(mods))) =
        sword.components.get("minecraft:attribute_modifiers")
    else {
        panic!("no attribute modifiers: {:?}", sword.components);
    };
    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].get("amount"), Some(&Tag::Double(3.0)));

    let mut item = LootStack::new("minecraft:stone");
    run(
        &tables,
        r#"{"type":"minecraft:toggle_tooltips","toggles":{"minecraft:enchantments":false}}"#,
        &mut item,
        &context,
    );
    assert!(item.components.contains_key("minecraft:tooltip_display"));

    let mut chest = LootStack::new("minecraft:chest");
    run(
        &tables,
        r#"{"type":"minecraft:set_loot_table","loot_table_id":"minecraft:chests/simple_dungeon","seed":9}"#,
        &mut chest,
        &context,
    );
    assert!(chest.components.contains_key("minecraft:container_loot"));

    let mut book = LootStack::new("minecraft:written_book");
    run(
        &tables,
        r#"{"type":"minecraft:set_book_cover","title":"Hi","author":"Me","generation":1}"#,
        &mut book,
        &context,
    );
    assert!(
        book.components
            .contains_key("minecraft:written_book_content")
    );

    let mut star = LootStack::new("minecraft:firework_star");
    run(
        &tables,
        r#"{"type":"minecraft:set_firework_explosion","shape":"star","colors":[255],"trail":true}"#,
        &mut star,
        &context,
    );
    assert!(star.components.contains_key("minecraft:firework_explosion"));

    let mut banner = LootStack::new("minecraft:white_banner");
    run(
        &tables,
        r#"{"type":"minecraft:set_banner_pattern","append":false,"patterns":[{"pattern":"minecraft:creeper","color":"lime"}]}"#,
        &mut banner,
        &context,
    );
    assert!(banner.components.contains_key("minecraft:banner_patterns"));

    let mut potion = LootStack::new("minecraft:potion");
    run(
        &tables,
        r#"{"type":"minecraft:set_random_potion","options":["minecraft:swiftness"]}"#,
        &mut potion,
        &context,
    );
    assert!(potion.components.contains_key("minecraft:potion_contents"));
}
