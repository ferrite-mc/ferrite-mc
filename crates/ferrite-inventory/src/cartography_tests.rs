//! The cartography table (vanilla `CartographyTableMenu`) against a fake saved-map store.

use std::{cell::RefCell, collections::BTreeMap};

use ferrite_item::{
    ItemStack, MapPostProcessing, map_id, map_post_processing, set_map_id, set_map_post_processing,
};

use crate::{
    ClickInput, Clicker, MapAccess, MapFinishError, MapInfo, Menu, NoCrafting, PlayerInventory,
};

/// A store that remembers what the menu asked of it.
#[derive(Default)]
struct FakeMaps {
    maps: RefCell<BTreeMap<i32, MapInfo>>,
    next: RefCell<i32>,
    finished: RefCell<Vec<(i32, MapPostProcessing)>>,
    discarded: RefCell<Vec<i32>>,
    fail: RefCell<bool>,
}

impl FakeMaps {
    fn with(maps: &[(i32, u8, bool)]) -> Self {
        let fake = Self::default();
        for &(id, scale, locked) in maps {
            fake.maps.borrow_mut().insert(id, MapInfo { scale, locked });
        }
        *fake.next.borrow_mut() = 100;
        fake
    }
}

impl MapAccess for FakeMaps {
    fn info(&self, id: i32) -> Option<MapInfo> {
        self.maps.borrow().get(&id).copied()
    }

    fn finish(&self, stack: &mut ItemStack) -> Result<Option<i32>, MapFinishError> {
        let Some(work) = map_post_processing(stack) else {
            return Ok(None);
        };
        if *self.fail.borrow() {
            return Err(MapFinishError);
        }
        let source = map_id(stack).ok_or(MapFinishError)?;
        let info = self.info(source).ok_or(MapFinishError)?;
        let id = *self.next.borrow();
        *self.next.borrow_mut() += 1;
        let new = match work {
            MapPostProcessing::Scale => MapInfo {
                scale: info.scale + 1,
                locked: false,
            },
            MapPostProcessing::Lock => MapInfo {
                scale: info.scale,
                locked: true,
            },
        };
        self.maps.borrow_mut().insert(id, new);
        self.finished.borrow_mut().push((id, work));
        set_map_id(stack, id);
        set_map_post_processing(stack, None);
        Ok(Some(id))
    }

    fn discard(&self, id: i32) {
        self.maps.borrow_mut().remove(&id);
        self.discarded.borrow_mut().push(id);
    }
}

fn item(name: &str, count: i32) -> ItemStack {
    ItemStack::from_name(name).unwrap().copy_with_count(count)
}

fn filled(id: i32) -> ItemStack {
    let mut stack = item("minecraft:filled_map", 1);
    set_map_id(&mut stack, id);
    stack
}

struct Table {
    inv: PlayerInventory,
    menu: Menu,
    maps: FakeMaps,
}

impl Table {
    fn new(maps: FakeMaps) -> Self {
        Self {
            inv: PlayerInventory::new(),
            menu: Menu::cartography(5),
            maps,
        }
    }

    fn click(&mut self, slot: i16, button: i8, input: ClickInput) -> Vec<ItemStack> {
        let mut p = Clicker::new(&mut self.inv, false, &NoCrafting).with_maps(&self.maps);
        self.menu.clicked(&mut p, slot, button, input);
        p.drops
    }

    fn slot(&self, slot: usize) -> ItemStack {
        self.menu.slot(&self.inv, slot).clone()
    }

    /// Puts a stack into a menu slot through the menu, as an inventory action does.
    fn put(&mut self, slot: usize, stack: ItemStack) {
        self.menu
            .set_slot_with_maps(&mut self.inv, &NoCrafting, &self.maps, slot, stack);
    }

    fn inventory_total(&self, name: &str) -> i32 {
        (0..crate::SIZE)
            .map(|i| self.inv.get(i))
            .filter(|s| s.name() == Some(name))
            .map(|s| s.count)
            .sum()
    }
}

#[test]
fn the_menu_is_the_registered_cartography_table() {
    let menu = Menu::cartography(1);
    assert_eq!(
        menu.kind().menu_type().as_deref(),
        Some("minecraft:cartography_table")
    );
}

#[test]
fn only_filled_maps_fit_the_map_slot_and_only_the_three_additions_fit_the_second() {
    // Clicking with the item on the cursor goes through the slot rules.
    let mut table = Table::new(FakeMaps::with(&[(1, 0, false)]));
    for (slot, name, fits) in [
        (0, "minecraft:dirt", false),
        (0, "minecraft:map", false),
        (1, "minecraft:dirt", false),
        (1, "minecraft:paper", true),
        (1, "minecraft:map", true),
        (1, "minecraft:glass_pane", true),
    ] {
        let mut table_for = Table::new(FakeMaps::with(&[(1, 0, false)]));
        table_for.menu.set_carried(item(name, 1));
        table_for.click(slot, 0, ClickInput::Pickup);
        assert_eq!(
            !table_for.slot(slot as usize).is_empty(),
            fits,
            "{name} in slot {slot}"
        );
    }
    table.menu.set_carried(filled(1));
    table.click(0, 0, ClickInput::Pickup);
    assert_eq!(table.slot(0), filled(1), "a filled map fits slot 0");
    table.menu.set_carried(filled(1));
    table.click(1, 0, ClickInput::Pickup);
    assert!(table.slot(1).is_empty(), "a filled map is not an addition");
}

#[test]
fn an_empty_map_clones_the_filled_map_into_two_with_the_same_id() {
    let mut table = Table::new(FakeMaps::with(&[(7, 1, false)]));
    table.put(0, filled(7));
    table.put(1, item("minecraft:map", 1));
    let result = table.slot(2);
    assert_eq!(result.name(), Some("minecraft:filled_map"));
    assert_eq!(result.count, 2);
    assert_eq!(map_id(&result), Some(7));
    assert!(table.maps.finished.borrow().is_empty(), "no new data yet");
}

#[test]
fn taking_a_clone_consumes_each_input_once_and_allocates_nothing() {
    let mut table = Table::new(FakeMaps::with(&[(7, 1, false)]));
    table.put(0, filled(7));
    table.put(1, item("minecraft:map", 3));
    table.click(2, 0, ClickInput::Pickup);
    assert_eq!(table.menu.carried().count, 2);
    assert_eq!(map_id(table.menu.carried()), Some(7));
    assert!(table.slot(0).is_empty());
    assert_eq!(table.slot(1).count, 2, "one empty map was used");
    assert!(
        table.slot(2).is_empty(),
        "the source map is gone, so the result is too"
    );
    assert!(table.maps.finished.borrow().is_empty());
}

#[test]
fn paper_zooms_out_into_a_new_map_and_leaves_the_source_data_alone() {
    let mut table = Table::new(FakeMaps::with(&[(7, 1, false)]));
    table.put(0, filled(7));
    table.put(1, item("minecraft:paper", 2));
    let preview = table.slot(2);
    assert_eq!(preview.count, 1);
    assert_eq!(
        map_id(&preview),
        Some(7),
        "the preview still names the source"
    );
    assert_eq!(
        map_post_processing(&preview),
        Some(MapPostProcessing::Scale)
    );
    assert!(
        table.maps.finished.borrow().is_empty(),
        "nothing is allocated for a preview"
    );
    table.click(2, 0, ClickInput::Pickup);
    let taken = table.menu.carried().clone();
    assert_eq!(map_id(&taken), Some(100));
    assert_eq!(map_post_processing(&taken), None, "the marker is consumed");
    assert_eq!(
        table.maps.info(100),
        Some(MapInfo {
            scale: 2,
            locked: false
        })
    );
    assert_eq!(
        table.maps.info(7),
        Some(MapInfo {
            scale: 1,
            locked: false
        })
    );
    assert!(table.slot(0).is_empty());
    assert_eq!(table.slot(1).count, 1);
    assert_eq!(
        *table.maps.finished.borrow(),
        vec![(100, MapPostProcessing::Scale)]
    );
}

#[test]
fn a_glass_pane_locks_a_copy_and_a_locked_map_rejects_paper_and_panes() {
    let mut table = Table::new(FakeMaps::with(&[(3, 2, false)]));
    table.put(0, filled(3));
    table.put(1, item("minecraft:glass_pane", 1));
    assert_eq!(
        map_post_processing(&table.slot(2)),
        Some(MapPostProcessing::Lock)
    );
    table.click(2, 0, ClickInput::Pickup);
    assert_eq!(
        table.maps.info(map_id(table.menu.carried()).unwrap()),
        Some(MapInfo {
            scale: 2,
            locked: true
        })
    );
    assert_eq!(
        table.maps.info(3),
        Some(MapInfo {
            scale: 2,
            locked: false
        })
    );

    for name in ["minecraft:paper", "minecraft:glass_pane"] {
        let mut table = Table::new(FakeMaps::with(&[(3, 2, true)]));
        table.put(0, filled(3));
        table.put(1, item(name, 1));
        assert!(table.slot(2).is_empty(), "{name} on a locked map");
    }
    let mut table = Table::new(FakeMaps::with(&[(3, 2, true)]));
    table.put(0, filled(3));
    table.put(1, item("minecraft:map", 1));
    assert_eq!(table.slot(2).count, 2, "a locked map can still be cloned");
}

#[test]
fn a_map_at_the_largest_scale_cannot_zoom_out_further_but_can_still_lock() {
    let mut table = Table::new(FakeMaps::with(&[(9, 4, false)]));
    table.put(0, filled(9));
    table.put(1, item("minecraft:paper", 1));
    assert!(table.slot(2).is_empty());
    table.put(1, item("minecraft:glass_pane", 1));
    assert_eq!(
        map_post_processing(&table.slot(2)),
        Some(MapPostProcessing::Lock)
    );
}

#[test]
fn a_map_with_no_saved_data_gives_no_result() {
    let mut table = Table::new(FakeMaps::with(&[]));
    table.put(0, filled(42));
    table.put(1, item("minecraft:map", 1));
    assert!(table.slot(2).is_empty());
}

#[test]
fn shift_clicking_the_result_moves_it_once_and_consumes_the_inputs_once() {
    let mut table = Table::new(FakeMaps::with(&[(7, 0, false)]));
    table.put(0, filled(7));
    table.put(1, item("minecraft:paper", 5));
    table.click(2, 0, ClickInput::QuickMove);
    assert_eq!(table.maps.finished.borrow().len(), 1, "one allocation");
    assert_eq!(table.inventory_total("minecraft:filled_map"), 1);
    assert_eq!(table.slot(1).count, 4);
    assert!(table.slot(0).is_empty());
    assert!(table.slot(2).is_empty());
}

#[test]
fn shift_clicking_with_a_full_inventory_changes_nothing_and_frees_the_new_id() {
    let mut table = Table::new(FakeMaps::with(&[(7, 0, false)]));
    for slot in 0..crate::SIZE {
        table.inv.set(slot, item("minecraft:stick", 64));
    }
    table.put(0, filled(7));
    table.put(1, item("minecraft:paper", 1));
    table.click(2, 0, ClickInput::QuickMove);
    assert_eq!(table.slot(0), filled(7), "the source map stays");
    assert_eq!(table.slot(1).count, 1, "the paper stays");
    assert!(!table.slot(2).is_empty(), "the result is still offered");
    assert_eq!(
        *table.maps.discarded.borrow(),
        vec![100],
        "the unused map is freed"
    );
}

#[test]
fn a_busy_cursor_blocks_taking_and_allocates_nothing() {
    let mut table = Table::new(FakeMaps::with(&[(7, 0, false)]));
    table.put(0, filled(7));
    table.put(1, item("minecraft:paper", 1));
    table.menu.set_carried(item("minecraft:dirt", 1));
    table.click(2, 0, ClickInput::Pickup);
    assert!(table.maps.finished.borrow().is_empty());
    assert_eq!(table.slot(0), filled(7));
    assert_eq!(table.slot(1).count, 1);
}

#[test]
fn a_store_failure_refuses_the_take_without_consuming_anything() {
    let mut table = Table::new(FakeMaps::with(&[(7, 0, false)]));
    table.put(0, filled(7));
    table.put(1, item("minecraft:paper", 1));
    *table.maps.fail.borrow_mut() = true;
    table.click(2, 0, ClickInput::Pickup);
    assert!(table.menu.carried().is_empty());
    assert_eq!(table.slot(0), filled(7));
    assert_eq!(table.slot(1).count, 1);
    assert!(!table.slot(2).is_empty());
    *table.maps.fail.borrow_mut() = false;
    table.click(2, 0, ClickInput::Pickup);
    assert_eq!(map_id(table.menu.carried()), Some(100));
}

#[test]
fn shift_clicking_inventory_items_fills_the_matching_input_slot() {
    let mut table = Table::new(FakeMaps::with(&[(7, 0, false)]));
    table.inv.set(9, filled(7));
    table.inv.set(10, item("minecraft:paper", 4));
    table.inv.set(11, item("minecraft:dirt", 4));
    let slot_of = |table: &Table, name: &str| {
        (3..39)
            .find(|&i| table.slot(i).name() == Some(name))
            .unwrap() as i16
    };
    let map_slot = slot_of(&table, "minecraft:filled_map");
    table.click(map_slot, 0, ClickInput::QuickMove);
    assert_eq!(table.slot(0), filled(7));
    let paper_slot = slot_of(&table, "minecraft:paper");
    table.click(paper_slot, 0, ClickInput::QuickMove);
    assert_eq!(table.slot(1).name(), Some("minecraft:paper"));
    let dirt_slot = slot_of(&table, "minecraft:dirt");
    table.click(dirt_slot, 0, ClickInput::QuickMove);
    assert_eq!(
        table.slot(1).name(),
        Some("minecraft:paper"),
        "dirt stays out"
    );
    assert!(!table.slot(2).is_empty());
}

#[test]
fn closing_the_table_returns_both_inputs_and_drops_no_result() {
    let mut table = Table::new(FakeMaps::with(&[(7, 0, false)]));
    table.put(0, filled(7));
    table.put(1, item("minecraft:paper", 3));
    let mut p = Clicker::new(&mut table.inv, false, &NoCrafting).with_maps(&table.maps);
    table.menu.close(&mut p);
    assert!(p.drops.is_empty());
    assert_eq!(table.inventory_total("minecraft:filled_map"), 1);
    assert_eq!(table.inventory_total("minecraft:paper"), 3);
    assert!(table.maps.finished.borrow().is_empty());
}
