use ferrite_item::{Component, Enchantments, HashedSlot, ItemStack, component_ids};

use crate::{
    ClickInput::{self, *},
    Clicker, CookingOutput, Crafter, FurnaceKind, Menu, NoCrafting, PlayerInventory, SLOT_OUTSIDE,
    Sync,
};

fn item(name: &str, count: i32) -> ItemStack {
    ItemStack::from_name(name)
        .unwrap_or_else(|| panic!("unknown item {name}"))
        .copy_with_count(count)
}

fn empty() -> ItemStack {
    ItemStack::empty()
}

/// One oak log makes four oak planks; a lone milk bucket makes a cake (to exercise remainders).
struct TestCrafter;

impl Crafter for TestCrafter {
    fn craft(&self, grid: &[ItemStack], _: usize) -> ItemStack {
        let present: Vec<&ItemStack> = grid.iter().filter(|s| !s.is_empty()).collect();
        match present.as_slice() {
            [one] if one.name() == Some("minecraft:oak_log") => item("oak_planks", 4),
            [one] if one.name() == Some("minecraft:milk_bucket") => item("cake", 1),
            _ => empty(),
        }
    }
}

struct Fixture {
    inv: PlayerInventory,
    menu: Menu,
    creative: bool,
}

impl Fixture {
    fn new(menu: Menu) -> Self {
        Self {
            inv: PlayerInventory::new(),
            menu,
            creative: false,
        }
    }

    fn inventory() -> Self {
        Self::new(Menu::inventory())
    }

    /// Writes a menu slot directly, like a creative inventory action.
    fn set(&mut self, slot: usize, stack: ItemStack) {
        self.menu.set_slot(&mut self.inv, &TestCrafter, slot, stack);
    }

    fn click(&mut self, slot: i16, button: i8, input: ClickInput) -> Vec<ItemStack> {
        let mut p = Clicker::new(&mut self.inv, self.creative, &TestCrafter);
        self.menu.clicked(&mut p, slot, button, input);
        p.drops
    }

    fn slot(&self, slot: usize) -> ItemStack {
        self.menu.slot(&self.inv, slot).clone()
    }

    fn carried(&self) -> ItemStack {
        self.menu.carried().clone()
    }

    /// A full drag: start, one add per slot, end.
    fn drag(&mut self, kind: i8, slots: &[i16]) {
        self.click(SLOT_OUTSIDE, kind << 2, QuickCraft);
        for &slot in slots {
            self.click(slot, (kind << 2) | 1, QuickCraft);
        }
        self.click(SLOT_OUTSIDE, (kind << 2) | 2, QuickCraft);
    }
}

/// Raw gold smelts into a gold ingot (200 ticks, 1 experience); coal burns for 1600 ticks.
struct FurnaceCrafter;

impl Crafter for FurnaceCrafter {
    fn craft(&self, _: &[ItemStack], _: usize) -> ItemStack {
        empty()
    }

    fn cook(&self, kind: FurnaceKind, input: &ItemStack) -> Option<CookingOutput> {
        (kind == FurnaceKind::Furnace && input.name() == Some("minecraft:raw_gold")).then(|| {
            CookingOutput {
                result: item("gold_ingot", 1),
                cook_time: 200,
                experience: 1.0,
            }
        })
    }

    fn burn_time(&self, item: &ItemStack) -> i32 {
        if item.name() == Some("minecraft:coal") {
            1600
        } else {
            0
        }
    }
}

#[test]
fn furnace_menu_cooks_consumes_fuel_and_syncs_container_data() {
    let mut f = Fixture::new(Menu::furnace(7, FurnaceKind::Furnace));
    f.menu
        .set_slot(&mut f.inv, &FurnaceCrafter, 0, item("raw_gold", 1));
    f.menu
        .set_slot(&mut f.inv, &FurnaceCrafter, 1, item("coal", 2));
    assert_eq!(f.menu.len(), 39);
    assert_eq!(
        f.menu.kind(),
        crate::MenuKind::Furnace(FurnaceKind::Furnace)
    );
    assert_eq!(f.menu.kind().menu_type(), Some("minecraft:furnace".into()));

    // The first tick lights the furnace, consumes one fuel and starts cooking.
    f.menu.tick(&FurnaceCrafter);
    assert_eq!(f.menu.data_value(0), 1);
    assert_eq!((f.menu.data_value(1), f.menu.data_value(2)), (1, 200));
    assert_eq!((f.menu.data_value(3), f.menu.data_value(4)), (1600, 1600));
    assert_eq!(f.slot(1), item("coal", 1));
    // The first broadcast sends every data slot once, then only changes.
    let first = f.menu.broadcast_changes(&f.inv);
    assert_eq!(
        first
            .iter()
            .filter(|sync| matches!(sync, Sync::Data { .. }))
            .count(),
        5
    );
    assert!(
        !f.menu
            .broadcast_changes(&f.inv)
            .iter()
            .any(|sync| matches!(sync, Sync::Data { .. }))
    );

    for _ in 1..200 {
        f.menu.tick(&FurnaceCrafter);
    }
    assert_eq!(f.slot(0), empty());
    assert_eq!(f.slot(2), item("gold_ingot", 1));
    assert_eq!(f.menu.data_value(0), 1);
    assert_eq!(f.menu.data_value(1), 0);
    assert_eq!(f.menu.furnace_state().unwrap().pending_experience, 1.0);

    // Taking the output awards the experience and leaves the fuel alone.
    let mut p = Clicker::new(&mut f.inv, false, &FurnaceCrafter);
    f.menu.clicked(&mut p, 2, 0, Pickup);
    assert_eq!(p.xp, 1.0);
    assert_eq!(p.drops, Vec::<ItemStack>::new());
    assert_eq!(f.menu.carried(), &item("gold_ingot", 1));
    assert_eq!(f.slot(2), empty());
    assert_eq!(f.menu.furnace_state().unwrap().pending_experience, 0.0);
}

#[test]
fn anvil_repairs_renames_and_charges_levels() {
    let mut f = Fixture::new(Menu::anvil(9));
    let mut first = item("iron_sword", 1);
    first
        .components
        .insert(component_ids::DAMAGE, Component::Damage(100));
    let mut second = first.copy_with_count(1);
    second
        .components
        .insert(component_ids::DAMAGE, Component::Damage(100));
    assert_eq!(first.max_damage(), Some(250));
    f.menu.set_slot(&mut f.inv, &TestCrafter, 0, first.clone());
    assert_eq!(f.menu.anvil_cost(), 0);
    assert_eq!(f.slot(2), empty());
    f.menu.set_slot(&mut f.inv, &TestCrafter, 1, second.clone());
    // Two half-damaged swords plus the 5% combine bonus fully repair the result for 2 levels.
    assert_eq!(f.menu.anvil_cost(), 2);
    assert_eq!(f.slot(2).damage(), 0);
    assert_eq!(f.slot(2).name(), Some("minecraft:iron_sword"));

    let mut p = Clicker::new(&mut f.inv, false, &TestCrafter);
    f.menu.clicked(&mut p, 2, 0, Pickup);
    assert_eq!(p.levels, 2);
    assert_eq!(f.menu.carried().damage(), 0);
    assert_eq!((f.slot(0), f.slot(1)), (empty(), empty()));
    assert_eq!(f.menu.anvil_cost(), 0);

    // A rename alone costs one level and writes the custom name component.
    f.menu.set_slot(&mut f.inv, &TestCrafter, 0, first);
    f.menu.rename("Excalibur".into());
    assert_eq!(f.menu.anvil_cost(), 1);
    assert!(matches!(
        f.menu
            .result()
            .effective_component(component_ids::CUSTOM_NAME),
        Some(Component::Opaque(ferrite_nbt::Tag::String(name))) if name == "Excalibur"
    ));
    // A repair material repairs a quarter of the durability per item.
    let mut damaged = item("iron_sword", 1);
    damaged
        .components
        .insert(component_ids::DAMAGE, Component::Damage(200));
    f.menu.rename(String::new());
    f.menu.set_slot(&mut f.inv, &TestCrafter, 0, damaged);
    f.menu
        .set_slot(&mut f.inv, &TestCrafter, 1, item("iron_ingot", 2));
    assert_eq!(f.slot(2).damage(), 200 - 2 * (250 / 4));
    assert_eq!(f.menu.anvil_cost(), 2);
}

/// Station recipes for the non-furnace crafting stations.
struct StationCrafter;

impl Crafter for StationCrafter {
    fn craft(&self, _: &[ItemStack], _: usize) -> ItemStack {
        empty()
    }

    fn stonecutting(&self, input: &ItemStack) -> Vec<ItemStack> {
        if input.name() == Some("minecraft:stone") {
            vec![item("stone_slab", 2)]
        } else {
            Vec::new()
        }
    }

    fn smithing(
        &self,
        template: &ItemStack,
        base: &ItemStack,
        addition: &ItemStack,
    ) -> Option<ItemStack> {
        (template.name() == Some("minecraft:netherite_upgrade_smithing_template")
            && base.name() == Some("minecraft:diamond_sword")
            && addition.name() == Some("minecraft:netherite_ingot"))
        .then(|| item("netherite_sword", 1))
    }
}

#[test]
fn stonecutter_grindstone_enchanting_smithing_and_loom_work() {
    let mut f = Fixture::new(Menu::stonecutter(3));
    f.menu
        .set_slot(&mut f.inv, &StationCrafter, 0, item("stone", 3));
    assert_eq!(f.menu.kind(), crate::MenuKind::Stonecutter);
    assert_eq!(f.slot(1), item("stone_slab", 2));
    let mut p = Clicker::new(&mut f.inv, false, &StationCrafter);
    f.menu.clicked(&mut p, 1, 0, Pickup);
    assert_eq!(f.menu.carried(), &item("stone_slab", 2));
    assert_eq!(f.slot(0), item("stone", 2));

    // A grindstone strips enchantments and combines the durability of two copies.
    let mut f = Fixture::new(Menu::grindstone(4));
    let mut left = item("iron_sword", 1);
    left.components
        .insert(component_ids::DAMAGE, Component::Damage(50));
    let mut enchantments = Enchantments::new();
    enchantments.set("minecraft:sharpness", 2);
    left.components.insert(
        component_ids::ENCHANTMENTS,
        Component::Enchantments(enchantments),
    );
    f.menu.set_slot(&mut f.inv, &StationCrafter, 0, left);
    f.menu
        .set_slot(&mut f.inv, &StationCrafter, 1, item("iron_sword", 1));
    assert_eq!(f.slot(2).damage(), 0);
    assert!(
        f.slot(2)
            .effective_component(component_ids::ENCHANTMENTS)
            .is_none()
    );
    let mut p = Clicker::new(&mut f.inv, false, &StationCrafter);
    f.menu.clicked(&mut p, 2, 0, Pickup);
    assert!(p.xp >= 2.0);
    assert_eq!(f.slot(0), empty());

    // Enchanting costs one lapis and one level per choice.
    let mut f = Fixture::new(Menu::enchanting(5));
    f.menu
        .set_slot(&mut f.inv, &StationCrafter, 0, item("diamond_sword", 1));
    f.menu
        .set_slot(&mut f.inv, &StationCrafter, 1, item("lapis_lazuli", 3));
    assert_eq!(f.menu.enchant_cost(1), 2);
    assert_eq!(f.menu.button_click(1), Some(2));
    assert_eq!(f.menu.data_value(0), 1);
    match f.slot(0).effective_component(component_ids::ENCHANTMENTS) {
        Some(Component::Enchantments(enchantments)) => {
            assert_eq!(enchantments.level("minecraft:sharpness"), Some(2))
        }
        other => panic!("no enchantments: {other:?}"),
    }
    assert_eq!(f.slot(1), item("lapis_lazuli", 1));

    let mut f = Fixture::new(Menu::smithing(6));
    f.menu.set_slot(
        &mut f.inv,
        &StationCrafter,
        0,
        item("netherite_upgrade_smithing_template", 1),
    );
    f.menu
        .set_slot(&mut f.inv, &StationCrafter, 1, item("diamond_sword", 1));
    f.menu
        .set_slot(&mut f.inv, &StationCrafter, 2, item("netherite_ingot", 1));
    assert_eq!(f.slot(3), item("netherite_sword", 1));

    let mut f = Fixture::new(Menu::loom(7));
    f.menu
        .set_slot(&mut f.inv, &StationCrafter, 0, item("white_banner", 1));
    f.menu
        .set_slot(&mut f.inv, &StationCrafter, 1, item("red_dye", 1));
    assert_eq!(f.slot(3).name(), Some("minecraft:red_banner"));
}

#[test]
fn inventory_menu_maps_vanilla_slots() {
    let mut f = Fixture::inventory();
    f.inv.set(0, item("stone", 1));
    f.inv.set(9, item("dirt", 2));
    f.inv.set(crate::HEAD, item("iron_helmet", 1));
    f.inv.set(crate::FEET, item("iron_boots", 1));
    f.inv.set(crate::OFFHAND, item("torch", 3));
    assert_eq!(f.menu.len(), 46);
    assert_eq!(f.slot(36), item("stone", 1));
    assert_eq!(f.slot(9), item("dirt", 2));
    assert_eq!(f.slot(5), item("iron_helmet", 1));
    assert_eq!(f.slot(8), item("iron_boots", 1));
    assert_eq!(f.slot(45), item("torch", 3));
    assert_eq!(Menu::crafting(1).len(), 46);
    assert_eq!(Menu::generic(2, 3).len(), 27 + 36);
}

#[test]
fn pickup_left_takes_and_places_the_whole_stack() {
    let mut f = Fixture::inventory();
    f.set(36, item("stone", 40));
    f.click(36, 0, Pickup);
    assert_eq!((f.carried(), f.slot(36)), (item("stone", 40), empty()));
    f.click(9, 0, Pickup);
    assert_eq!((f.carried(), f.slot(9)), (empty(), item("stone", 40)));
}

#[test]
fn pickup_right_takes_half_rounded_up_and_places_one() {
    let mut f = Fixture::inventory();
    f.set(36, item("stone", 5));
    f.click(36, 1, Pickup);
    assert_eq!(
        (f.carried(), f.slot(36)),
        (item("stone", 3), item("stone", 2))
    );
    f.click(10, 1, Pickup);
    assert_eq!(
        (f.carried(), f.slot(10)),
        (item("stone", 2), item("stone", 1))
    );
    f.click(36, 1, Pickup);
    assert_eq!(
        (f.carried(), f.slot(36)),
        (item("stone", 1), item("stone", 3))
    );
}

#[test]
fn pickup_merges_up_to_the_stack_limit_and_swaps_other_items() {
    let mut f = Fixture::inventory();
    f.set(9, item("stone", 60));
    f.menu.set_carried(item("stone", 10));
    f.click(9, 0, Pickup);
    assert_eq!(
        (f.carried(), f.slot(9)),
        (item("stone", 6), item("stone", 64))
    );
    f.set(10, item("dirt", 5));
    f.click(10, 0, Pickup);
    assert_eq!(
        (f.carried(), f.slot(10)),
        (item("dirt", 5), item("stone", 6))
    );
    // A full stack never swaps with a stack of the same item.
    f.menu.set_carried(item("stone", 64));
    f.click(9, 0, Pickup);
    assert_eq!(
        (f.carried(), f.slot(9)),
        (item("stone", 64), item("stone", 64))
    );
}

#[test]
fn pickup_outside_drops_the_cursor() {
    let mut f = Fixture::inventory();
    f.menu.set_carried(item("stone", 10));
    assert_eq!(f.click(SLOT_OUTSIDE, 1, Pickup), vec![item("stone", 1)]);
    assert_eq!(f.carried(), item("stone", 9));
    assert_eq!(f.click(SLOT_OUTSIDE, 0, Pickup), vec![item("stone", 9)]);
    assert_eq!(f.carried(), empty());
    assert!(f.click(SLOT_OUTSIDE, 0, Pickup).is_empty());
}

#[test]
fn armor_slots_take_only_matching_equipment_one_at_a_time() {
    let mut f = Fixture::inventory();
    f.menu.set_carried(item("dirt", 3));
    f.click(5, 0, Pickup);
    assert_eq!((f.carried(), f.slot(5)), (item("dirt", 3), empty()));
    f.menu.set_carried(item("iron_chestplate", 1));
    f.click(5, 0, Pickup);
    assert_eq!(f.slot(5), empty());
    f.click(6, 0, Pickup);
    assert_eq!(
        (f.carried(), f.slot(6)),
        (empty(), item("iron_chestplate", 1))
    );
    f.menu.set_carried(item("carved_pumpkin", 5));
    f.click(5, 0, Pickup);
    assert_eq!(
        (f.carried(), f.slot(5)),
        (item("carved_pumpkin", 4), item("carved_pumpkin", 1))
    );
}

#[test]
fn cursed_armor_stays_on_outside_creative() {
    let mut f = Fixture::inventory();
    let mut helmet = item("iron_helmet", 1);
    let mut curse = Enchantments::new();
    curse.set("minecraft:binding_curse", 1);
    helmet
        .components
        .insert(component_ids::ENCHANTMENTS, Component::Enchantments(curse));
    f.inv.set(crate::HEAD, helmet.clone());
    f.click(5, 0, Pickup);
    f.click(5, 0, QuickMove);
    f.click(5, 0, Throw);
    assert_eq!((f.carried(), f.slot(5)), (empty(), helmet.clone()));
    f.creative = true;
    f.click(5, 0, Pickup);
    assert_eq!((f.carried(), f.slot(5)), (helmet, empty()));
}

#[test]
fn quick_move_between_hotbar_and_main_inventory() {
    let mut f = Fixture::inventory();
    f.set(36, item("stone", 64));
    f.click(36, 0, QuickMove);
    assert_eq!((f.slot(36), f.slot(9)), (empty(), item("stone", 64)));
    f.click(9, 0, QuickMove);
    assert_eq!((f.slot(9), f.slot(36)), (empty(), item("stone", 64)));
}

#[test]
fn quick_move_fills_matching_stacks_before_empty_slots() {
    let mut f = Fixture::inventory();
    f.set(38, item("stone", 60));
    f.set(9, item("stone", 10));
    f.click(9, 0, QuickMove);
    assert_eq!(f.slot(9), empty());
    assert_eq!(f.slot(38), item("stone", 64));
    assert_eq!(f.slot(36), item("stone", 6));
}

#[test]
fn quick_move_equips_armor_and_unequips_to_the_inventory() {
    let mut f = Fixture::inventory();
    f.set(20, item("iron_helmet", 1));
    f.click(20, 0, QuickMove);
    assert_eq!((f.slot(20), f.slot(5)), (empty(), item("iron_helmet", 1)));
    // With the head slot taken, a second helmet moves like any item.
    f.set(20, item("golden_helmet", 1));
    f.click(20, 0, QuickMove);
    assert_eq!(
        (f.slot(20), f.slot(36)),
        (empty(), item("golden_helmet", 1))
    );
    // Armor goes back to the first free inventory slot.
    f.click(5, 0, QuickMove);
    assert_eq!((f.slot(5), f.slot(9)), (empty(), item("iron_helmet", 1)));
}

#[test]
fn quick_move_from_offhand_and_full_inventory() {
    let mut f = Fixture::inventory();
    f.set(45, item("torch", 5));
    f.click(45, 0, QuickMove);
    assert_eq!((f.slot(45), f.slot(9)), (empty(), item("torch", 5)));
    // Nothing moves when the target range is full.
    for slot in 36..45 {
        f.set(slot, item("dirt", 64));
    }
    f.set(10, item("stone", 3));
    f.click(10, 0, QuickMove);
    assert_eq!(f.slot(10), item("stone", 3));
}

#[test]
fn swap_with_hotbar_and_offhand() {
    let mut f = Fixture::inventory();
    f.set(9, item("stone", 10));
    f.inv.set(2, item("dirt", 3));
    f.click(9, 2, Swap);
    assert_eq!(
        (f.slot(9), f.inv.get(2).clone()),
        (item("dirt", 3), item("stone", 10))
    );
    f.click(9, 40, Swap);
    assert_eq!(
        (f.slot(9), f.inv.get(crate::OFFHAND).clone()),
        (empty(), item("dirt", 3))
    );
    // Into an empty slot, and armor slots refuse other items.
    f.click(5, 2, Swap);
    assert_eq!(
        (f.slot(5), f.inv.get(2).clone()),
        (empty(), item("stone", 10))
    );
    f.inv.set(3, item("iron_helmet", 1));
    f.click(5, 3, Swap);
    assert_eq!(
        (f.slot(5), f.inv.get(3).clone()),
        (item("iron_helmet", 1), empty())
    );
    // Out-of-range buttons do nothing.
    f.click(9, 9, Swap);
    assert_eq!(f.slot(9), empty());
}

#[test]
fn clone_needs_infinite_materials_and_an_empty_cursor() {
    let mut f = Fixture::inventory();
    f.set(9, item("stone", 3));
    f.click(9, 2, ClickInput::Clone);
    assert_eq!(f.carried(), empty());
    f.creative = true;
    f.click(9, 2, ClickInput::Clone);
    assert_eq!(
        (f.carried(), f.slot(9)),
        (item("stone", 64), item("stone", 3))
    );
    f.set(10, item("ender_pearl", 1));
    f.click(10, 2, ClickInput::Clone);
    assert_eq!(f.carried(), item("stone", 64));
}

#[test]
fn throw_one_or_the_whole_stack() {
    let mut f = Fixture::inventory();
    f.set(9, item("stone", 10));
    assert_eq!(f.click(9, 0, Throw), vec![item("stone", 1)]);
    assert_eq!(f.slot(9), item("stone", 9));
    assert_eq!(f.click(9, 1, Throw), vec![item("stone", 9)]);
    assert_eq!(f.slot(9), empty());
    // Not while holding something.
    f.set(10, item("dirt", 2));
    f.menu.set_carried(item("stone", 1));
    assert!(f.click(10, 0, Throw).is_empty());
}

#[test]
fn left_drag_splits_evenly_and_keeps_the_remainder() {
    let mut f = Fixture::inventory();
    f.menu.set_carried(item("stone", 10));
    f.drag(0, &[9, 10, 11]);
    for slot in 9..12 {
        assert_eq!(f.slot(slot), item("stone", 3));
    }
    assert_eq!(f.carried(), item("stone", 1));
}

#[test]
fn left_drag_tops_up_existing_stacks() {
    let mut f = Fixture::inventory();
    f.set(9, item("stone", 62));
    f.menu.set_carried(item("stone", 10));
    f.drag(0, &[9, 10]);
    assert_eq!(
        (f.slot(9), f.slot(10)),
        (item("stone", 64), item("stone", 5))
    );
    assert_eq!(f.carried(), item("stone", 3));
}

#[test]
fn right_drag_places_one_per_slot() {
    let mut f = Fixture::inventory();
    f.menu.set_carried(item("stone", 10));
    // A slot added twice and an occupied slot of another item are skipped.
    f.set(12, item("dirt", 1));
    f.drag(1, &[9, 10, 10, 12, 11]);
    for slot in 9..12 {
        assert_eq!(f.slot(slot), item("stone", 1));
    }
    assert_eq!(f.slot(12), item("dirt", 1));
    assert_eq!(f.carried(), item("stone", 7));
}

#[test]
fn single_slot_drag_is_a_plain_click() {
    let mut f = Fixture::inventory();
    f.menu.set_carried(item("stone", 10));
    f.drag(1, &[9]);
    assert_eq!(
        (f.slot(9), f.carried()),
        (item("stone", 1), item("stone", 9))
    );
    f.drag(0, &[10]);
    assert_eq!((f.slot(10), f.carried()), (item("stone", 9), empty()));
}

#[test]
fn middle_drag_fills_stacks_only_in_creative() {
    let mut f = Fixture::inventory();
    f.menu.set_carried(item("stone", 10));
    f.drag(2, &[9, 10]);
    assert_eq!((f.slot(9), f.carried()), (empty(), item("stone", 10)));
    f.creative = true;
    f.drag(2, &[9, 10]);
    assert_eq!(
        (f.slot(9), f.slot(10)),
        (item("stone", 64), item("stone", 64))
    );
    assert_eq!(f.carried(), empty());
}

#[test]
fn drag_is_cancelled_by_other_clicks_and_never_reaches_the_result() {
    let mut f = Fixture::inventory();
    f.menu.set_carried(item("stone", 10));
    f.click(SLOT_OUTSIDE, 0, QuickCraft);
    f.click(9, 1, QuickCraft);
    // A pickup in the middle of a drag only cancels it.
    f.click(10, 0, Pickup);
    f.click(SLOT_OUTSIDE, 2, QuickCraft);
    assert_eq!(
        (f.slot(9), f.slot(10), f.carried()),
        (empty(), empty(), item("stone", 10))
    );
    f.drag(0, &[0, 9, 10]);
    assert_eq!(
        (f.slot(0), f.slot(9), f.slot(10)),
        (empty(), item("stone", 5), item("stone", 5))
    );
}

#[test]
fn double_click_collects_partial_stacks_first() {
    let mut f = Fixture::inventory();
    f.set(9, item("stone", 30));
    f.set(10, item("stone", 64));
    f.set(11, item("stone", 20));
    f.set(36, item("dirt", 5));
    f.menu.set_carried(item("stone", 1));
    f.click(12, 0, PickupAll);
    assert_eq!(f.carried(), item("stone", 64));
    assert_eq!(
        (f.slot(9), f.slot(10), f.slot(11)),
        (empty(), item("stone", 51), empty())
    );
    assert_eq!(f.slot(36), item("dirt", 5));
}

#[test]
fn double_click_backwards_with_button_one() {
    let mut f = Fixture::inventory();
    f.set(9, item("stone", 40));
    f.set(44, item("stone", 40));
    f.menu.set_carried(item("stone", 1));
    f.click(12, 1, PickupAll);
    assert_eq!(f.carried(), item("stone", 64));
    assert_eq!((f.slot(9), f.slot(44)), (item("stone", 17), empty()));
}

#[test]
fn two_by_two_crafting_takes_one_craft_per_click() {
    let mut f = Fixture::inventory();
    f.set(1, item("oak_log", 2));
    assert_eq!(f.slot(0), item("oak_planks", 4));
    f.click(0, 0, Pickup);
    assert_eq!(
        (f.carried(), f.slot(1), f.slot(0)),
        (
            item("oak_planks", 4),
            item("oak_log", 1),
            item("oak_planks", 4)
        )
    );
    // With planks on the cursor the next craft stacks onto it.
    f.click(0, 0, Pickup);
    assert_eq!(
        (f.carried(), f.slot(1), f.slot(0)),
        (item("oak_planks", 8), empty(), empty())
    );
    // Nothing can be put into the result slot.
    f.click(0, 0, Pickup);
    assert_eq!((f.carried(), f.slot(0)), (item("oak_planks", 8), empty()));
}

#[test]
fn result_does_not_stack_past_the_cursor_limit() {
    let mut f = Fixture::inventory();
    f.set(1, item("oak_log", 1));
    f.menu.set_carried(item("oak_planks", 62));
    f.click(0, 0, Pickup);
    assert_eq!(
        (f.carried(), f.slot(1)),
        (item("oak_planks", 62), item("oak_log", 1))
    );
}

#[test]
fn shift_click_result_crafts_everything_into_the_hotbar_end_first() {
    let mut f = Fixture::inventory();
    f.set(2, item("oak_log", 3));
    f.click(0, 0, QuickMove);
    assert_eq!(f.slot(44), item("oak_planks", 12));
    assert_eq!((f.slot(2), f.slot(0)), (empty(), empty()));
}

#[test]
fn shift_click_result_drops_the_part_that_does_not_fit() {
    let mut f = Fixture::inventory();
    for slot in 9..45 {
        f.set(slot, item("dirt", 64));
    }
    f.set(44, item("oak_planks", 62));
    f.set(1, item("oak_log", 2));
    let drops = f.click(0, 0, QuickMove);
    assert_eq!(f.slot(44), item("oak_planks", 64));
    assert_eq!(drops, vec![item("oak_planks", 2)]);
    // Only the first craft happened: the rest stays in the grid.
    assert_eq!(
        (f.slot(1), f.slot(0)),
        (item("oak_log", 1), item("oak_planks", 4))
    );
}

#[test]
fn throw_and_swap_from_the_result_slot_craft() {
    let mut f = Fixture::inventory();
    f.set(1, item("oak_log", 3));
    assert_eq!(f.click(0, 0, Throw), vec![item("oak_planks", 4)]);
    assert_eq!(f.slot(1), item("oak_log", 2));
    f.click(0, 4, Swap);
    assert_eq!(
        (f.inv.get(4).clone(), f.slot(1)),
        (item("oak_planks", 4), item("oak_log", 1))
    );
    // Ctrl+Q crafts and drops everything.
    f.set(1, item("oak_log", 2));
    assert_eq!(f.click(0, 1, Throw), vec![item("oak_planks", 4); 2]);
    assert_eq!(f.slot(1), empty());
}

#[test]
fn crafting_leaves_remainders_in_the_grid() {
    let mut f = Fixture::inventory();
    f.set(3, item("milk_bucket", 1));
    assert_eq!(f.slot(0), item("cake", 1));
    f.click(0, 0, Pickup);
    assert_eq!(
        (f.carried(), f.slot(3), f.slot(0)),
        (item("cake", 1), item("bucket", 1), empty())
    );
}

#[test]
fn closing_returns_the_grid_and_cursor_to_the_inventory() {
    let mut f = Fixture::inventory();
    f.set(1, item("oak_log", 2));
    f.menu.set_carried(item("stone", 5));
    let mut p = Clicker::new(&mut f.inv, false, &TestCrafter);
    f.menu.close(&mut p);
    assert!(p.drops.is_empty());
    assert_eq!(
        (f.inv.get(0).clone(), f.inv.get(1).clone()),
        (item("stone", 5), item("oak_log", 2))
    );
    assert_eq!(
        (f.slot(1), f.slot(0), f.carried()),
        (empty(), empty(), empty())
    );
}

#[test]
fn crafting_table_quick_moves_into_the_grid_first() {
    let mut f = Fixture::new(Menu::crafting(1));
    f.set(10, item("oak_log", 5));
    f.click(10, 0, QuickMove);
    assert_eq!((f.slot(10), f.slot(1)), (empty(), item("oak_log", 5)));
    assert_eq!(f.slot(0), item("oak_planks", 4));
    f.click(0, 0, QuickMove);
    assert_eq!(f.slot(45), item("oak_planks", 20));
    assert_eq!(f.slot(1), empty());
    // Hotbar to main inventory once the grid cannot take it.
    f.set(37, item("iron_sword", 1));
    for slot in 1..10 {
        f.set(slot, item("dirt", 64));
    }
    f.click(37, 0, QuickMove);
    assert_eq!((f.slot(37), f.slot(10)), (empty(), item("iron_sword", 1)));
}

#[test]
fn chest_quick_move_goes_to_the_inventory_end_first() {
    let mut f = Fixture::new(Menu::generic(3, 3));
    f.menu.container_mut()[0] = item("stone", 10);
    f.click(0, 0, QuickMove);
    assert_eq!(f.slot(27 + 36 - 1), item("stone", 10));
    f.click(62, 0, QuickMove);
    assert_eq!((f.slot(62), f.slot(0)), (empty(), item("stone", 10)));
}

fn hashed(stack: &ItemStack) -> Option<HashedSlot> {
    (!stack.is_empty()).then(|| HashedSlot {
        item: stack.item,
        count: stack.count,
        added: Vec::new(),
        removed: Vec::new(),
    })
}

#[test]
fn correct_predictions_send_nothing() {
    let mut inv = PlayerInventory::new();
    inv.set(0, item("stone", 40));
    let mut menu = Menu::inventory();
    let Sync::Content { state_id, .. } = menu.broadcast_full_state(&inv) else {
        panic!("expected content");
    };
    assert_eq!(state_id, 1);
    let mut p = Clicker::new(&mut inv, false, &NoCrafting);
    let out = menu.handle_click(
        &mut p,
        1,
        36,
        0,
        Pickup,
        &[(36, None)],
        hashed(&item("stone", 40)),
    );
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(menu.carried(), &item("stone", 40));
}

#[test]
fn wrong_predictions_are_corrected_slot_by_slot() {
    let mut inv = PlayerInventory::new();
    inv.set(0, item("stone", 40));
    let mut menu = Menu::inventory();
    menu.broadcast_full_state(&inv);
    let mut p = Clicker::new(&mut inv, false, &NoCrafting);
    // The client thinks it only took half.
    let out = menu.handle_click(
        &mut p,
        1,
        36,
        0,
        Pickup,
        &[(36, hashed(&item("stone", 20)))],
        hashed(&item("stone", 20)),
    );
    assert_eq!(
        out,
        vec![
            Sync::Slot {
                container_id: 0,
                state_id: 2,
                slot: 36,
                item: empty()
            },
            Sync::Carried(item("stone", 40)),
        ]
    );
}

#[test]
fn stale_state_ids_resync_everything() {
    let mut inv = PlayerInventory::new();
    let mut menu = Menu::inventory();
    menu.broadcast_full_state(&inv);
    let mut p = Clicker::new(&mut inv, false, &NoCrafting);
    let out = menu.handle_click(&mut p, 0, 9, 0, Pickup, &[], None);
    assert!(matches!(
        out.as_slice(),
        [Sync::Content { state_id: 2, items, .. }] if items.len() == 46
    ));
    // Invalid slots are ignored without touching the state.
    let out = menu.handle_click(&mut p, 0, 46, 0, Pickup, &[], None);
    assert!(out.is_empty());
}

#[test]
fn crafting_result_is_sent_after_a_grid_click() {
    let mut inv = PlayerInventory::new();
    inv.set(9, item("oak_log", 1));
    let mut menu = Menu::inventory();
    menu.broadcast_full_state(&inv);
    let mut p = Clicker::new(&mut inv, false, &TestCrafter);
    menu.clicked(&mut p, 9, 0, Pickup);
    let out = menu.handle_click(
        &mut p,
        menu_state(&menu),
        1,
        0,
        Pickup,
        &[(1, hashed(&item("oak_log", 1)))],
        None,
    );
    let slots: Vec<i16> = out
        .iter()
        .filter_map(|s| match s {
            Sync::Slot { slot, .. } => Some(*slot),
            _ => None,
        })
        .collect();
    // Slot 9 was taken in an unreported click, the result is new.
    assert_eq!(slots, vec![0, 9]);
}

fn menu_state(menu: &Menu) -> i32 {
    menu.state_id()
}

#[test]
fn transfer_state_keeps_the_inventory_view() {
    let mut inv = PlayerInventory::new();
    inv.set(0, item("stone", 1));
    let mut inventory_menu = Menu::inventory();
    inventory_menu.broadcast_full_state(&inv);
    let mut table = Menu::crafting(1);
    table.broadcast_full_state(&inv);
    inv.set(0, item("dirt", 1));
    assert_eq!(table.broadcast_changes(&inv).len(), 1);
    inventory_menu.transfer_state(&table);
    assert!(inventory_menu.broadcast_changes(&inv).is_empty());
}

#[test]
fn inventory_add_fills_matching_stacks_then_free_slots_in_vanilla_order() {
    let mut inv = PlayerInventory::new();
    inv.selected = 4;
    let mut s = item("dirt", 3);
    assert!(inv.add(&mut s, false));
    assert_eq!((inv.get(0).clone(), s), (item("dirt", 3), empty()));
    inv.set(4, item("dirt", 60));
    let mut s = item("dirt", 10);
    assert!(inv.add(&mut s, false));
    assert_eq!((inv.get(4).count, inv.get(0).count), (64, 9));
    // The offhand comes before the other slots.
    inv.set(crate::OFFHAND, item("dirt", 1));
    let mut s = item("dirt", 2);
    assert!(inv.add(&mut s, false));
    assert_eq!(inv.get(crate::OFFHAND).count, 3);
    // A full inventory takes nothing, unless the player has infinite materials.
    let mut full = PlayerInventory::new();
    for slot in 0..crate::ITEMS {
        full.set(slot, item("stone", 64));
    }
    let mut s = item("dirt", 1);
    assert!(!full.add(&mut s, false));
    assert_eq!(s.count, 1);
    assert!(full.add(&mut s, true));
    assert_eq!(s, empty());
}

#[test]
fn unstackable_items_take_one_slot_each() {
    let mut inv = PlayerInventory::new();
    inv.set(0, item("iron_sword", 1));
    let mut s = item("iron_sword", 1);
    assert!(inv.add(&mut s, false));
    assert_eq!(inv.get(1).clone(), item("iron_sword", 1));
}

#[test]
fn click_input_ids_round_trip() {
    for id in 0..7 {
        assert_eq!(ClickInput::from_id(id).unwrap().id(), id);
    }
    assert_eq!(ClickInput::from_id(7), None);
}

fn merchant_trade(wants: ItemStack, gives: ItemStack, max_uses: i32, xp: i32) -> crate::Trade {
    crate::Trade::new(wants, gives, max_uses, xp)
}

#[test]
fn merchant_menu_trades_matching_inputs_and_tracks_uses() {
    let gives = item("bread", 6);
    let mut f = Fixture::new(Menu::merchant(
        9,
        vec![merchant_trade(item("emerald", 2), gives.clone(), 2, 3)],
    ));
    assert_eq!(f.menu.len(), 39);
    assert_eq!(f.menu.kind(), crate::MenuKind::Merchant);
    assert_eq!(f.menu.kind().menu_type(), Some("minecraft:merchant".into()));
    assert_eq!(f.menu.trades().len(), 1);

    // A wrong item does not satisfy the offer.
    f.set(0, item("dirt", 2));
    assert!(f.slot(2).is_empty());
    // The right item, but too few.
    f.set(0, item("emerald", 1));
    assert!(f.slot(2).is_empty());
    f.set(0, item("emerald", 2));
    assert_eq!(f.slot(2), gives);

    // Taking the output consumes the input, grants both kinds of experience and bumps the uses.
    {
        let mut p = Clicker::new(&mut f.inv, false, &TestCrafter);
        f.menu.clicked(&mut p, 2, 0, Pickup);
        assert_eq!(p.xp, 3.0);
    }
    assert_eq!(f.carried(), gives);
    assert_eq!(f.menu.take_villager_xp(), 3);
    assert_eq!(f.menu.trades()[0].uses, 1);
    assert!(f.slot(0).is_empty());
    assert!(f.slot(2).is_empty());
    // The XP is drained once.
    assert_eq!(f.menu.take_villager_xp(), 0);
}

#[test]
fn merchant_price_rises_with_demand_and_stops_at_max_uses() {
    let mut trade = merchant_trade(item("emerald", 4), item("bread", 1), 2, 2);
    trade.price_multiplier = 1.0;
    let mut f = Fixture::new(Menu::merchant(3, vec![trade]));
    assert_eq!(f.menu.trades()[0].price(), 4);
    f.set(0, item("emerald", 4));
    assert_eq!(f.slot(2), item("bread", 1));
    f.click(2, 0, Pickup);
    // One sale raised the demand by one, and the multiplier is one item per demand point.
    assert_eq!(f.menu.trades()[0].price(), 5);
    f.set(0, item("emerald", 5));
    assert_eq!(f.slot(2), item("bread", 1));
    f.click(2, 0, Pickup);
    assert!(f.menu.trades()[0].is_out_of_stock());
    // An out-of-stock offer shows no result even with matching inputs.
    f.set(0, item("emerald", 9));
    assert!(f.slot(2).is_empty());

    // Restocking clears the uses and the demand.
    f.menu.set_trades(vec![merchant_trade(
        item("emerald", 4),
        item("bread", 1),
        2,
        2,
    )]);
    f.set(0, item("emerald", 4));
    assert_eq!(f.slot(2), item("bread", 1));
    assert_eq!(f.menu.trades()[0].price(), 4);
}

#[test]
fn merchant_second_input_is_required_and_consumed() {
    let mut trade = merchant_trade(item("emerald", 1), item("enchanted_book", 1), 12, 5);
    trade.second_wants = Some(item("book", 1));
    let mut f = Fixture::new(Menu::merchant(1, vec![trade]));
    f.set(0, item("emerald", 1));
    assert!(f.slot(2).is_empty(), "the second input is still missing");
    f.set(1, item("book", 1));
    assert_eq!(f.slot(2).item, item("enchanted_book", 1).item);
    f.click(2, 0, Pickup);
    assert!(f.slot(0).is_empty());
    assert!(f.slot(1).is_empty());
    // An unselected offer index is rejected and keeps the current selection.
    assert!(!f.menu.select_trade(7));
    assert_eq!(f.menu.selected_trade(), 0);
}

#[test]
fn merchant_selection_changes_the_result() {
    let first = merchant_trade(item("emerald", 1), item("bread", 2), 12, 1);
    let second = merchant_trade(item("wheat", 3), item("cookie", 4), 12, 1);
    let mut f = Fixture::new(Menu::merchant(2, vec![first, second]));
    f.set(0, item("emerald", 1));
    assert_eq!(f.slot(2), item("bread", 2));
    assert!(f.menu.select_trade(1));
    assert!(f.slot(2).is_empty(), "wheat is not in the input slot");
    f.set(0, item("wheat", 3));
    assert_eq!(f.slot(2), item("cookie", 4));
    assert_eq!(f.menu.selected_trade(), 1);
}

#[test]
fn closing_a_merchant_menu_returns_its_inputs() {
    let mut f = Fixture::new(Menu::merchant(
        1,
        vec![merchant_trade(item("emerald", 2), item("bread", 1), 12, 1)],
    ));
    f.set(0, item("emerald", 5));
    let mut p = Clicker::new(&mut f.inv, false, &TestCrafter);
    f.menu.close(&mut p);
    // Five emeralds went back to the player's main inventory.
    assert!(
        (0..36).any(|slot| f.inv.get(slot).name() == Some("minecraft:emerald")
            && f.inv.get(slot).count == 5)
    );
    assert!(f.menu.container().iter().all(ItemStack::is_empty));
}

fn brewing_potion(name: &str) -> ItemStack {
    let mut stack = item("potion", 1);
    stack.components.insert(
        component_ids::POTION_CONTENTS,
        Component::PotionContents(ferrite_item::PotionContents {
            potion: Some(name.into()),
            ..Default::default()
        }),
    );
    stack
}

fn brewing_potion_id(stack: &ItemStack) -> Option<String> {
    match stack.effective_component(component_ids::POTION_CONTENTS) {
        Some(Component::PotionContents(contents)) => contents.potion,
        _ => None,
    }
}

/// Water + nether wart -> awkward, awkward + sugar -> swiftness.
struct BrewingCrafter;

impl Crafter for BrewingCrafter {
    fn craft(&self, _: &[ItemStack], _: usize) -> ItemStack {
        empty()
    }

    fn brew(&self, ingredient: &ItemStack, bottle: &ItemStack) -> Option<ItemStack> {
        if ingredient.is_empty() || bottle.is_empty() {
            return None;
        }
        let current = brewing_potion_id(bottle)?;
        let next = match (ingredient.name()?, current.as_str()) {
            ("minecraft:nether_wart", "minecraft:water") => "minecraft:awkward",
            ("minecraft:sugar", "minecraft:awkward") => "minecraft:swiftness",
            _ => return None,
        };
        let mut out = bottle.clone();
        out.components.insert(
            component_ids::POTION_CONTENTS,
            Component::PotionContents(ferrite_item::PotionContents {
                potion: Some(next.into()),
                ..Default::default()
            }),
        );
        Some(out)
    }

    fn is_brewing_ingredient(&self, item: &ItemStack) -> bool {
        matches!(
            item.name(),
            Some("minecraft:nether_wart" | "minecraft:sugar" | "minecraft:gunpowder")
        )
    }
}

#[test]
fn brewing_stand_brews_after_four_hundred_ticks_and_spends_fuel() {
    let mut f = Fixture::new(Menu::brewing_stand(9));
    f.set(0, brewing_potion("minecraft:water"));
    f.set(3, item("nether_wart", 2));
    f.set(4, item("blaze_powder", 1));

    f.menu.tick(&BrewingCrafter);
    assert_eq!(f.menu.brewing_state().unwrap().fuel, 19);
    assert_eq!(f.menu.data_value(crate::BREWING_BREW_TIME), 400);
    assert_eq!(f.menu.data_value(crate::BREWING_FUEL), 19);

    for _ in 0..399 {
        f.menu.tick(&BrewingCrafter);
    }
    assert_eq!(
        brewing_potion_id(&f.slot(0)).as_deref(),
        Some("minecraft:water")
    );
    assert_eq!(f.menu.data_value(crate::BREWING_BREW_TIME), 1);

    f.menu.tick(&BrewingCrafter);
    assert_eq!(
        brewing_potion_id(&f.slot(0)).as_deref(),
        Some("minecraft:awkward")
    );
    assert_eq!(f.slot(3).count, 1);
    assert_eq!(f.menu.data_value(crate::BREWING_BREW_TIME), 0);
    assert_eq!(f.menu.data_value(crate::BREWING_FUEL), 19);
    assert_eq!(f.menu.brewing_state().unwrap().fuel, 19);
}

#[test]
fn brewing_stand_rejects_items_in_the_wrong_slots() {
    let mut f = Fixture::new(Menu::brewing_stand(5));
    f.menu.set_carried(item("stone", 1));
    f.click(0, 0, Pickup);
    assert!(f.slot(0).is_empty(), "a bottle slot takes no stone");
    assert_eq!(f.carried().count, 1);

    f.menu.set_carried(brewing_potion("minecraft:water"));
    f.click(0, 0, Pickup);
    assert_eq!(
        brewing_potion_id(&f.slot(0)).as_deref(),
        Some("minecraft:water")
    );
    assert!(f.carried().is_empty());

    f.menu.set_carried(item("stone", 1));
    f.click(4, 0, Pickup);
    assert!(
        f.slot(4).is_empty(),
        "the fuel slot takes blaze powder only"
    );
    assert_eq!(f.carried().count, 1);

    f.menu.set_carried(item("blaze_powder", 2));
    f.click(4, 0, Pickup);
    assert_eq!(f.slot(4).count, 2);
}
