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

/// Which source stack lands in the `index`th target slot.
fn pick(stacks: &[ItemStack], index: usize) -> Option<ItemStack> {
    match stacks.len() {
        0 => None,
        1 => stacks.first().cloned(),
        _ => stacks.get(index).cloned(),
    }
}

/// Writes `stacks` into `items[slots]` following `mode`; returns how many slots changed.
pub fn write_container(
    items: &mut [ItemStack],
    slots: &[usize],
    mode: SlotMode,
    stacks: &[ItemStack],
) -> usize {
    let mut changed = 0;
    for (index, slot) in slots.iter().enumerate() {
        let new = match mode {
            SlotMode::Replace => pick(stacks, index),
            SlotMode::Fill => pick(stacks, index).filter(|_| items[*slot].is_empty()),
            SlotMode::Override => Some(
                (if stacks.len() > 1 {
                    stacks.get(index).cloned()
                } else {
                    pick(stacks, index)
                })
                .unwrap_or_else(ItemStack::empty),
            ),
            SlotMode::Insert => None,
        };
        if let Some(new) = new
            && items[*slot] != new
        {
            items[*slot] = new;
            changed += 1;
        }
    }
    changed
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
            SlotSource::Stacks(stacks) => Ok(stacks),
            SlotSource::FromBlock { pos, slots } => {
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
                Ok(indexes.iter().map(|i| items[*i].clone()).collect())
            }
            SlotSource::FromEntity { target, slots } => {
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
                    .map_err(|_| {
                        self.reply
                            .key("commands.item.source.no_such_slot", &[&slots])
                    })
            }
        }
    }

    pub(super) fn run_slots(
        &mut self,
        mode: SlotMode,
        target: SlotTarget,
        slots: String,
        source: SlotSource,
    ) {
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
                    for stack in stacks {
                        let left = insert_stack(&mut items, stack);
                        if !left.is_empty() {
                            self.reply.key("commands.loot.insert.full", &[&x, &y, &z]);
                            return;
                        }
                    }
                    block_container::store_slots(&source, pos, &name, &items);
                    self.reply
                        .key("commands.loot.insert.success", &[&x, &y, &z]);
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
                        SlotMode::Fill => SlotFill::FillEmpty(stacks.clone()),
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
    fn container_writes_follow_the_mode() {
        let stone = ferrite_item::item_id("minecraft:stone").unwrap();
        let dirt = ferrite_item::item_id("minecraft:dirt").unwrap();
        let mut items = vec![ItemStack::empty(); 4];
        items[1] = stack(dirt, 5);
        // Fill leaves occupied slots alone.
        let changed = write_container(&mut items, &[0, 1, 2], SlotMode::Fill, &[stack(stone, 1)]);
        assert_eq!(changed, 2);
        assert_eq!(items[1].item, dirt);
        // Replace with several stacks maps pairwise and leaves the rest.
        let changed = write_container(
            &mut items,
            &[0, 1, 2],
            SlotMode::Replace,
            &[stack(dirt, 2), stack(dirt, 3)],
        );
        assert_eq!(changed, 2);
        assert_eq!(items[2].item, stone);
        // Override clears slots without a stack.
        write_container(
            &mut items,
            &[0, 1, 2],
            SlotMode::Override,
            &[stack(dirt, 2), stack(dirt, 3)],
        );
        assert!(items[2].is_empty());
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
