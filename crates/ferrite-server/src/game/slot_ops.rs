//! Applying `/item` and `/loot` slot writes: reads the source stacks, then writes them into a
//! block container (through the chunk source) or a player's inventory (through the region that
//! owns the player).

use std::time::Duration;

use ferrite_item::ItemStack;

use super::CommandRuntime;
use super::block_container;
use super::commands::{SlotMode, SlotSource, SlotTarget};
use super::region::{Command, SlotFill};

/// Slot names of a block container: `container.N` and `container.*`, comma separated.
pub fn block_slots(names: &str, size: usize) -> Result<Vec<usize>, String> {
    let mut slots = Vec::new();
    for name in names.split(',').filter(|n| !n.is_empty()) {
        if name == "container.*" {
            slots.extend(0..size);
            continue;
        }
        match name
            .strip_prefix("container.")
            .and_then(|n| n.parse::<usize>().ok())
            .filter(|n| *n < size)
        {
            Some(slot) => slots.push(slot),
            None => return Err(name.to_owned()),
        }
    }
    Ok(slots)
}

/// Inventory indexes of a player's slot names (`-1` is the selected hotbar slot).
pub fn player_slots(names: &str) -> Result<Vec<i16>, String> {
    let mut slots = Vec::new();
    for name in names.split(',').filter(|n| !n.is_empty()) {
        let one = |prefix: &str, max: i16, base: i16| -> Option<i16> {
            name.strip_prefix(prefix)?
                .parse::<i16>()
                .ok()
                .filter(|n| (0..max).contains(n))
                .map(|n| n + base)
        };
        match name {
            "container.*" => slots.extend(0..36),
            "hotbar.*" => slots.extend(0..9),
            "inventory.*" => slots.extend(9..36),
            "armor.*" => slots.extend(36..40),
            "weapon.*" => slots.extend([-1, 40]),
            "armor.feet" => slots.push(36),
            "armor.legs" => slots.push(37),
            "armor.chest" => slots.push(38),
            "armor.head" => slots.push(39),
            "weapon.mainhand" => slots.push(-1),
            "weapon.offhand" => slots.push(40),
            _ => match one("container.", 36, 0)
                .or_else(|| one("hotbar.", 9, 0))
                .or_else(|| one("inventory.", 27, 9))
            {
                Some(slot) => slots.push(slot),
                None => return Err(name.to_owned()),
            },
        }
    }
    Ok(slots)
}

/// The player inventory indexes of `count` consecutive vanilla slot ids starting at the slot
/// named `first` (`SlotArgument` ids: `container.N` and `hotbar.N` are `N`, `inventory.N` is
/// `9 + N`, `weapon.mainhand` 98, `weapon.offhand` 99, `armor.feet..head` 100..=103). Each entry
/// pairs the index with its offset in the run; ids no player slot answers to are skipped, as
/// vanilla's `getSlot` returns null for them. `None` if `first` is not a single slot name.
pub fn player_slot_run(first: &str, count: i32) -> Option<Vec<(i16, usize)>> {
    let start: i32 = match first {
        "weapon.mainhand" => 98,
        "weapon.offhand" => 99,
        "armor.feet" => 100,
        "armor.legs" => 101,
        "armor.chest" => 102,
        "armor.head" => 103,
        name if name.contains('*') => return None,
        name => {
            let index = *player_slots(name).ok()?.first()?;
            i32::from(index)
        }
    };
    Some(
        (0..count.max(0))
            .filter_map(|offset| {
                let index = match start + offset {
                    id @ 0..=35 => i16::try_from(id).ok()?,
                    98 => -1,
                    99 => 40,
                    id @ 100..=103 => i16::try_from(id - 64).ok()?,
                    _ => return None,
                };
                Some((index, usize::try_from(offset).ok()?))
            })
            .collect(),
    )
}

/// Adds `stack` to the container, merging into matching stacks first. Returns what did not fit.
pub fn insert_stack(items: &mut [ItemStack], mut stack: ItemStack) -> ItemStack {
    let max = stack.max_stack_size().max(1);
    for item in items.iter_mut() {
        if stack.is_empty() {
            break;
        }
        if !item.is_empty() && item.can_stack_with(&stack) && item.count < max {
            let moved = (max - item.count).min(stack.count);
            item.count += moved;
            stack.count -= moved;
        }
    }
    for item in items.iter_mut() {
        if stack.is_empty() {
            break;
        }
        if item.is_empty() {
            let moved = max.min(stack.count);
            *item = stack.copy_with_count(moved);
            stack.count -= moved;
        }
    }
    if stack.count <= 0 {
        ItemStack::empty()
    } else {
        stack
    }
}

/// The stack each of `slot_count` target slots receives, `None` meaning "left alone".
///
/// This is vanilla 26.3's `ItemCommands` distribution (read from the server jar's
/// `ItemProvider` and `SlotCollection.replaceSlotItems`): the target slots are walked in order
/// and every slot takes the next stack from the provider while the provider has one.
/// - `replace`: `ItemProvider.of(stacks)`, so slot `i` gets `stacks[i]` and slots past the end of
///   the list are untouched (a single stack therefore lands in the first slot only).
/// - `fill`: `ItemProvider.cycle(stacks)`, so slot `i` gets `stacks[i % len]`; every slot is set,
///   whether it was empty or not.
/// - `override`: `of(stacks).orElseProvide(EMPTY)`, so slots past the end of the list are cleared.
pub fn distribute(
    mode: SlotMode,
    stacks: &[ItemStack],
    slot_count: usize,
) -> Vec<Option<ItemStack>> {
    (0..slot_count)
        .map(|index| match mode {
            SlotMode::Replace => stacks.get(index).cloned(),
            SlotMode::Fill => (!stacks.is_empty()).then(|| stacks[index % stacks.len()].clone()),
            SlotMode::Override => Some(stacks.get(index).cloned().unwrap_or_else(ItemStack::empty)),
            SlotMode::Insert => None,
        })
        .collect()
}

/// Writes `stacks` into `items[slots]` following `mode`; returns how many slots were set.
/// Vanilla counts every slot `SlotAccess.set` accepted, even when the stack did not change.
pub fn write_container(
    items: &mut [ItemStack],
    slots: &[usize],
    mode: SlotMode,
    stacks: &[ItemStack],
) -> usize {
    let mut set = 0;
    for (slot, new) in slots.iter().zip(distribute(mode, stacks, slots.len())) {
        if let Some(new) = new {
            items[*slot] = new;
            set += 1;
        }
    }
    set
}

fn rng() -> ferrite_entity::Rng {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64);
    ferrite_entity::Rng::new(seed)
}

/// `stacks` run through `modifier` (empty slots stay empty).
fn modified(
    stacks: Vec<ItemStack>,
    modifier: Option<&super::item_modifier::ItemModifier>,
) -> Vec<ItemStack> {
    let Some(modifier) = modifier else {
        return stacks;
    };
    let mut rng = rng();
    stacks
        .into_iter()
        .map(|stack| {
            if stack.is_empty() {
                stack
            } else {
                modifier.apply(stack, &mut rng)
            }
        })
        .collect()
}

/// Indexes (into `slots`) whose stack differs once the modifier ran, with the new stacks.
fn changed(
    before: &[ItemStack],
    modifier: &super::item_modifier::ItemModifier,
) -> Vec<(usize, ItemStack)> {
    modified(before.to_vec(), Some(modifier))
        .into_iter()
        .enumerate()
        .filter(|(index, after)| *after != before[*index])
        .collect()
}

impl CommandRuntime<'_> {
    fn block_container_at(&self, pos: [i32; 3]) -> Option<(String, usize)> {
        let source = self.source.as_ref()?;
        let name = source
            .block(pos[0], pos[1], pos[2])
            .and_then(|state| u16::try_from(state).ok())
            .and_then(ferrite_data::blocks::state)
            .map(|(name, _)| name)?;
        let size = block_container::inventory_slots(name)?;
        let name = if name.contains(':') {
            name.to_owned()
        } else {
            format!("minecraft:{name}")
        };
        Some((name, size))
    }

    fn slot_stacks(&self, source: SlotSource) -> Result<Vec<ItemStack>, ()> {
        match source {
            SlotSource::Stacks(stacks) | SlotSource::Loot { stacks, .. } => Ok(stacks),
            SlotSource::FromBlock {
                pos,
                slots,
                modifier,
            } => {
                let Some((_, size)) = self.block_container_at(pos) else {
                    self.reply.key(
                        "commands.item.source.not_a_container",
                        &[
                            &pos[0].to_string(),
                            &pos[1].to_string(),
                            &pos[2].to_string(),
                        ],
                    );
                    return Err(());
                };
                let Ok(indexes) = block_slots(&slots, size) else {
                    self.reply
                        .key("commands.item.source.no_such_slot", &[&slots]);
                    return Err(());
                };
                let items = block_container::load_slots(self.source.as_ref().unwrap(), pos, size);
                Ok(modified(
                    indexes.iter().map(|i| items[*i].clone()).collect(),
                    modifier.as_deref(),
                ))
            }
            SlotSource::FromEntity {
                target,
                slots,
                modifier,
            } => {
                let Ok(targets) = self.selection(&target) else {
                    self.reply.key("argument.entity.notfound.entity", &[]);
                    return Err(());
                };
                let Some(first) = targets.first().filter(|t| t.player) else {
                    self.reply
                        .key("commands.item.source.no_such_slot", &[&slots]);
                    return Err(());
                };
                let Ok(indexes) = player_slots(&slots) else {
                    self.reply
                        .key("commands.item.source.no_such_slot", &[&slots]);
                    return Err(());
                };
                let Some((_, mailbox, _)) = self.mailboxes.get(&first.uuid) else {
                    self.reply
                        .key("commands.item.source.no_such_slot", &[&slots]);
                    return Err(());
                };
                let (reply, received) = std::sync::mpsc::channel();
                mailbox.lock().unwrap().push_back(Command::ReadSlots {
                    slots: indexes,
                    reply,
                });
                received
                    .recv_timeout(Duration::from_millis(1500))
                    .map(|stacks| modified(stacks, modifier.as_deref()))
                    .map_err(|_| {
                        self.reply
                            .key("commands.item.source.no_such_slot", &[&slots])
                    })
            }
        }
    }

    /// `/item modify`: applies `modifier` to each addressed slot of the target; slots it does not
    /// change are left alone and a run that changes nothing fails.
    pub(super) fn run_modify(
        &mut self,
        target: SlotTarget,
        slots: String,
        modifier: &super::item_modifier::ItemModifier,
    ) {
        match target {
            SlotTarget::Block(pos) => {
                let (x, y, z) = (pos[0].to_string(), pos[1].to_string(), pos[2].to_string());
                let Some((name, size)) = self.block_container_at(pos) else {
                    self.reply
                        .key("commands.item.target.not_a_container", &[&x, &y, &z]);
                    return;
                };
                let source = self.source.clone().unwrap();
                let Ok(indexes) = block_slots(&slots, size) else {
                    self.reply
                        .key("commands.item.target.no_such_slot", &[&slots]);
                    return;
                };
                let mut items = block_container::load_slots(&source, pos, size);
                let before: Vec<ItemStack> = indexes.iter().map(|i| items[*i].clone()).collect();
                let updates = changed(&before, modifier);
                if updates.is_empty() {
                    self.reply.key("commands.item.target.no_changes", &[&slots]);
                    return;
                }
                let shown = super::commands::item_display(updates[0].1.item);
                for (index, stack) in updates {
                    items[indexes[index]] = stack;
                }
                block_container::store_slots(&source, pos, &name, &items);
                self.reply
                    .key("commands.item.block.set.success", &[&x, &y, &z, &shown]);
            }
            SlotTarget::Entities(target) => {
                let Ok(selected) = self.selection(&target) else {
                    self.reply.key("argument.entity.notfound.entity", &[]);
                    return;
                };
                let Ok(indexes) = player_slots(&slots) else {
                    self.reply
                        .key("commands.item.target.no_such_slot", &[&slots]);
                    return;
                };
                let (mut done, mut who, mut shown) = (0, String::new(), String::new());
                for entity in selected.iter().filter(|e| e.player) {
                    let Some((_, mailbox, _)) = self.mailboxes.get(&entity.uuid) else {
                        continue;
                    };
                    let (reply, received) = std::sync::mpsc::channel();
                    mailbox.lock().unwrap().push_back(Command::ReadSlots {
                        slots: indexes.clone(),
                        reply,
                    });
                    let Ok(before) = received.recv_timeout(Duration::from_millis(1500)) else {
                        continue;
                    };
                    let updates = changed(&before, modifier);
                    if updates.is_empty() {
                        continue;
                    }
                    shown = super::commands::item_display(updates[0].1.item);
                    let (slots_changed, stacks): (Vec<i16>, Vec<ItemStack>) = updates
                        .into_iter()
                        .map(|(index, stack)| (indexes[index], stack))
                        .unzip();
                    mailbox.lock().unwrap().push_back(Command::ItemSlots {
                        slots: slots_changed,
                        fill: SlotFill::Set(stacks),
                    });
                    who = entity.name.clone();
                    done += 1;
                }
                match done {
                    0 => self.reply.key("commands.item.target.no_changes", &[&slots]),
                    1 => self
                        .reply
                        .key("commands.item.entity.set.success.single", &[&who, &shown]),
                    _ => self.reply.key(
                        "commands.item.entity.set.success.multiple",
                        &[&done.to_string(), &shown],
                    ),
                }
            }
        }
    }

    /// `/loot insert` and `/loot replace` (vanilla `LootCommand.blockDistribute`,
    /// `blockReplace`, `entityReplace` and `setSlots`): every stack a slot or container accepted
    /// is tracked once, and the answer is the `commands.drop.success.*` message for the tracked
    /// stacks, so a full container reports "Dropped 0 items" instead of failing.
    fn run_loot_slots(
        &mut self,
        mode: SlotMode,
        target: SlotTarget,
        first_slot: &str,
        stacks: Vec<ItemStack>,
        table: Option<String>,
        count: Option<i32>,
    ) {
        let count = count.unwrap_or_else(|| i32::try_from(stacks.len()).unwrap_or(i32::MAX));
        let mut tracked: Vec<ItemStack> = Vec::new();
        match target {
            SlotTarget::Block(pos) => {
                let (x, y, z) = (pos[0].to_string(), pos[1].to_string(), pos[2].to_string());
                let Some((name, size)) = self.block_container_at(pos) else {
                    self.reply
                        .key("commands.item.target.not_a_container", &[&x, &y, &z]);
                    return;
                };
                let source = self.source.clone().unwrap();
                let mut items = block_container::load_slots(&source, pos, size);
                if mode == SlotMode::Insert {
                    for stack in &stacks {
                        let left = insert_stack(&mut items, stack.clone());
                        if left.count < stack.count {
                            tracked.push(stack.clone());
                        }
                    }
                } else {
                    let Some(start) = block_slots(first_slot, size)
                        .ok()
                        .and_then(|slots| (slots.len() == 1).then(|| slots[0]))
                    else {
                        let number = first_slot
                            .strip_prefix("container.")
                            .unwrap_or(first_slot)
                            .to_owned();
                        self.reply
                            .key("commands.item.target.no_such_slot.unnamed", &[&number]);
                        return;
                    };
                    for offset in 0..usize::try_from(count).unwrap_or(0) {
                        let Some(slot) = items.get_mut(start + offset) else {
                            break;
                        };
                        let stack = stacks.get(offset).cloned().unwrap_or_else(ItemStack::empty);
                        *slot = stack.clone();
                        tracked.push(stack);
                    }
                }
                if !tracked.is_empty() {
                    block_container::store_slots(&source, pos, &name, &items);
                }
            }
            SlotTarget::Entities(target) => {
                let Ok(selected) = self.selection(&target) else {
                    self.reply.key("argument.entity.notfound.entity", &[]);
                    return;
                };
                let Some(run) = player_slot_run(first_slot, count) else {
                    self.reply
                        .key("commands.item.target.no_such_slot", &[first_slot]);
                    return;
                };
                for entity in selected.iter().filter(|e| e.player) {
                    let Some((_, mailbox, _)) = self.mailboxes.get(&entity.uuid) else {
                        continue;
                    };
                    let (slots, offsets): (Vec<i16>, Vec<usize>) = run.iter().copied().unzip();
                    let fill: Vec<ItemStack> = offsets
                        .iter()
                        .map(|offset| {
                            stacks
                                .get(*offset)
                                .cloned()
                                .unwrap_or_else(ItemStack::empty)
                        })
                        .collect();
                    tracked.extend(fill.iter().cloned());
                    mailbox.lock().unwrap().push_back(Command::ItemSlots {
                        slots,
                        fill: SlotFill::Set(fill),
                    });
                }
            }
        }
        let (key, args) = super::commands_loot::success_feedback(&tracked, table.as_deref());
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        self.reply.key(key, &args);
    }

    pub(super) fn run_slots(
        &mut self,
        mode: SlotMode,
        target: SlotTarget,
        slots: String,
        source: SlotSource,
    ) {
        if let SlotSource::Loot {
            stacks,
            table,
            count,
        } = source
        {
            self.run_loot_slots(mode, target, &slots, stacks, table, count);
            return;
        }
        let Ok(stacks) = self.slot_stacks(source) else {
            return;
        };
        let shown = stacks
            .iter()
            .find(|s| !s.is_empty())
            .map(|s| super::commands::item_display(s.item))
            .unwrap_or_default();
        match target {
            SlotTarget::Block(pos) => {
                let (x, y, z) = (pos[0].to_string(), pos[1].to_string(), pos[2].to_string());
                let Some((name, size)) = self.block_container_at(pos) else {
                    self.reply
                        .key("commands.item.target.not_a_container", &[&x, &y, &z]);
                    return;
                };
                let source = self.source.clone().unwrap();
                let mut items = block_container::load_slots(&source, pos, size);
                if mode == SlotMode::Insert {
                    for stack in stacks.iter().cloned() {
                        let left = insert_stack(&mut items, stack);
                        if !left.is_empty() {
                            self.reply.key("commands.item.target.failed", &[]);
                            return;
                        }
                    }
                    block_container::store_slots(&source, pos, &name, &items);
                    let total: i32 = stacks.iter().map(|s| s.count).sum();
                    self.reply
                        .key("commands.drop.success.multiple", &[&total.to_string()]);
                    return;
                }
                let Ok(indexes) = block_slots(&slots, size) else {
                    self.reply
                        .key("commands.item.target.no_such_slot", &[&slots]);
                    return;
                };
                if write_container(&mut items, &indexes, mode, &stacks) == 0 {
                    self.reply.key("commands.item.target.no_changes", &[&slots]);
                    return;
                }
                block_container::store_slots(&source, pos, &name, &items);
                self.reply
                    .key("commands.item.block.set.success", &[&x, &y, &z, &shown]);
            }
            SlotTarget::Entities(target) => {
                let Ok(selected) = self.selection(&target) else {
                    self.reply.key("argument.entity.notfound.entity", &[]);
                    return;
                };
                let Ok(indexes) = player_slots(&slots) else {
                    self.reply
                        .key("commands.item.target.no_such_slot", &[&slots]);
                    return;
                };
                let mut done = 0;
                let mut who = String::new();
                for entity in selected.iter().filter(|e| e.player) {
                    let Some((_, mailbox, _)) = self.mailboxes.get(&entity.uuid) else {
                        continue;
                    };
                    let fill = match mode {
                        SlotMode::Fill => SlotFill::Cycle(stacks.clone()),
                        SlotMode::Override => SlotFill::Override(stacks.clone()),
                        _ => SlotFill::Set(stacks.clone()),
                    };
                    mailbox.lock().unwrap().push_back(Command::ItemSlots {
                        slots: indexes.clone(),
                        fill,
                    });
                    who = entity.name.clone();
                    done += 1;
                }
                if done == 0 {
                    self.reply
                        .key("commands.item.target.no_such_slot", &[&slots]);
                } else if done == 1 {
                    self.reply
                        .key("commands.item.entity.set.success.single", &[&who, &shown]);
                } else {
                    self.reply.key(
                        "commands.item.entity.set.success.multiple",
                        &[&done.to_string(), &shown],
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(item: u16, count: i32) -> ItemStack {
        ItemStack::with_count(item, count)
    }

    #[test]
    fn slot_names_parse_for_blocks_and_players() {
        assert_eq!(
            block_slots("container.2,container.4", 27).unwrap(),
            vec![2, 4]
        );
        assert_eq!(block_slots("container.*", 5).unwrap().len(), 5);
        assert!(block_slots("container.27", 27).is_err());
        assert!(block_slots("hotbar.0", 27).is_err());
        assert_eq!(player_slots("weapon.mainhand").unwrap(), vec![-1]);
        assert_eq!(player_slots("weapon.*").unwrap(), vec![-1, 40]);
        assert_eq!(player_slots("inventory.0,hotbar.8").unwrap(), vec![9, 8]);
        assert!(player_slots("hotbar.9").is_err());
    }

    #[test]
    fn replace_runs_walk_vanilla_slot_ids() {
        // hotbar.8 + 2 continues into inventory.0 and inventory.1 (ids 9 and 10).
        assert_eq!(
            player_slot_run("hotbar.7", 3).unwrap(),
            vec![(7, 0), (8, 1), (9, 2)]
        );
        // weapon.mainhand (98) is followed by weapon.offhand (99); id 100 is armor.feet.
        assert_eq!(
            player_slot_run("weapon.mainhand", 3).unwrap(),
            vec![(-1, 0), (40, 1), (36, 2)]
        );
        // Past the last armor slot nothing answers, so nothing is tracked.
        assert_eq!(player_slot_run("armor.head", 2).unwrap(), vec![(39, 0)]);
        assert!(player_slot_run("hotbar.*", 1).is_none());
        assert!(player_slot_run("hotbar.9", 1).is_none());
    }

    #[test]
    fn container_writes_follow_the_mode() {
        // Semantics read from vanilla 26.3 `ItemCommands` (ItemProvider.of / cycle / orElseProvide).
        let stone = ferrite_item::item_id("minecraft:stone").unwrap();
        let dirt = ferrite_item::item_id("minecraft:dirt").unwrap();
        let blank = || {
            let mut items = vec![ItemStack::empty(); 4];
            items[1] = stack(dirt, 5);
            items
        };
        // Fill cycles over every selected slot, occupied or not.
        let mut items = blank();
        let set = write_container(&mut items, &[0, 1, 2], SlotMode::Fill, &[stack(stone, 1)]);
        assert_eq!(set, 3);
        assert!(items[..3].iter().all(|s| s.item == stone));
        assert!(
            items[3].is_empty(),
            "slots outside the selection are untouched"
        );
        let mut items = blank();
        write_container(
            &mut items,
            &[0, 1, 2],
            SlotMode::Fill,
            &[stack(stone, 1), stack(dirt, 2)],
        );
        assert_eq!(
            (items[0].item, items[1].item, items[2].item),
            (stone, dirt, stone),
            "two stacks alternate"
        );
        // Replace walks the stacks in order; a single stack reaches the first slot only.
        let mut items = blank();
        let set = write_container(
            &mut items,
            &[0, 1, 2],
            SlotMode::Replace,
            &[stack(stone, 3)],
        );
        assert_eq!(set, 1);
        assert_eq!(items[0], stack(stone, 3));
        assert_eq!(
            items[1],
            stack(dirt, 5),
            "left alone once the stacks run out"
        );
        let mut items = blank();
        let set = write_container(
            &mut items,
            &[0, 1, 2],
            SlotMode::Replace,
            &[stack(dirt, 2), stack(dirt, 3)],
        );
        assert_eq!(set, 2);
        assert!(items[2].is_empty());
        // The count is the slots set, even when the stack was already there.
        let mut items = blank();
        assert_eq!(
            write_container(&mut items, &[1], SlotMode::Replace, &[stack(dirt, 5)]),
            1
        );
        // Override clears the slots after the last stack.
        let mut items = blank();
        write_container(
            &mut items,
            &[0, 1, 2],
            SlotMode::Override,
            &[stack(stone, 2)],
        );
        assert_eq!(items[0], stack(stone, 2));
        assert!(items[1].is_empty() && items[2].is_empty());
    }

    #[test]
    fn modify_reports_only_the_slots_the_modifier_changed() {
        let stone = ferrite_item::item_id("minecraft:stone").unwrap();
        let modifier = super::super::item_modifier::ItemModifier::parse(
            r#"{"function":"set_count","count":5}"#,
        )
        .unwrap();
        let before = vec![
            stack(stone, 1),
            ItemStack::empty(),
            stack(stone, 5),
            stack(stone, 2),
        ];
        let after = changed(&before, &modifier);
        assert_eq!(
            after,
            vec![(0, stack(stone, 5)), (3, stack(stone, 5))],
            "the empty slot stays empty and the stack already at 5 is not rewritten"
        );
    }

    #[test]
    fn insert_merges_then_uses_empty_slots_and_reports_leftovers() {
        let stone = ferrite_item::item_id("minecraft:stone").unwrap();
        let mut items = vec![ItemStack::empty(); 2];
        items[0] = stack(stone, 60);
        assert!(insert_stack(&mut items, stack(stone, 10)).is_empty());
        assert_eq!(items[0].count, 64);
        assert_eq!(items[1].count, 6);
        let left = insert_stack(&mut items, stack(stone, 200));
        assert_eq!(left.count, 142);
    }
}
