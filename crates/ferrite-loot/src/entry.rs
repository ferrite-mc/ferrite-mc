//! Pools, entries and weighted selection.

use crate::condition::Condition;
use crate::function::{self, Function};
use crate::number::NumberProvider;
use crate::table::LootTable;
use crate::{LootContext, LootStack, Rng};

/// One pool of a loot table.
#[derive(Debug, Clone)]
pub struct LootPool {
    /// Number of rolls taken from this pool.
    pub rolls: NumberProvider,
    /// Extra rolls added to [`LootPool::rolls`].
    pub bonus_rolls: NumberProvider,
    /// Conditions guarding the whole pool.
    pub conditions: Vec<Condition>,
    /// Functions applied to every stack the pool produces.
    pub functions: Vec<Function>,
    /// Weighted candidates selected once per roll.
    pub entries: Vec<Entry>,
}

impl LootPool {
    /// Rolls this pool into `out`, applying pool-level conditions and functions.
    pub(crate) fn roll_into(
        &self,
        context: &LootContext,
        rng: &mut impl Rng,
        out: &mut Vec<LootStack>,
    ) {
        if !self
            .conditions
            .iter()
            .all(|condition| condition.test(context, rng))
        {
            return;
        }

        let rolls = self
            .rolls
            .get_int(rng)
            .max(0)
            .saturating_add(self.bonus_rolls.get_int(rng).max(0));
        for _ in 0..rolls {
            let Some(entry) = select(&self.entries, rng) else {
                break;
            };
            if !entry.passes(context, rng) {
                continue;
            }

            let mut stacks = Vec::new();
            entry.expand(context, rng, &mut stacks);
            for stack in &mut stacks {
                function::apply(&self.functions, stack, context, rng);
            }
            out.extend(stacks.into_iter().filter(|stack| stack.count > 0));
        }
    }
}

/// One weighted entry in a loot pool.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Selection weight.
    pub weight: i32,
    /// Quality bonus applied to the weight per point of luck.
    pub quality: i32,
    /// Conditions guarding the entry.
    pub conditions: Vec<Condition>,
    /// Functions applied to stacks this entry produces.
    pub functions: Vec<Function>,
    /// What the entry produces when selected.
    pub kind: EntryKind,
}

/// The shape of a loot entry.
#[derive(Debug, Clone)]
pub enum EntryKind {
    /// A single item.
    Item {
        /// Namespaced item id.
        name: String,
    },
    /// A uniformly chosen item from a tag.
    Tag {
        /// Tag name as written in the JSON.
        tag: String,
        /// Whether nested tags are expanded.
        expand: bool,
        /// Resolved tag members (sorted and de-duplicated).
        items: Vec<String>,
    },
    /// The first child whose conditions pass.
    Alternatives(Vec<Entry>),
    /// The first child whose conditions pass, like [`EntryKind::Alternatives`].
    Sequence(Vec<Entry>),
    /// Every child whose conditions pass.
    Group(Vec<Entry>),
    /// Another loot table, inlined while loading.
    Table(Box<LootTable>),
    /// A block-entity supplied dynamic drop; yields nothing from this crate.
    Dynamic {
        /// Dynamic drop name (for example `minecraft:sherds`).
        name: String,
    },
    /// Explicitly no drop.
    Empty,
    /// An entry type this crate keeps opaque; yields nothing.
    Opaque {
        /// Vanilla entry type.
        kind: String,
    },
}

impl Entry {
    /// Evaluates this entry's conditions.
    pub(crate) fn passes(&self, context: &LootContext, rng: &mut impl Rng) -> bool {
        self.conditions
            .iter()
            .all(|condition| condition.test(context, rng))
    }

    /// Expands an entry whose conditions have already passed.
    pub(crate) fn expand(
        &self,
        context: &LootContext,
        rng: &mut impl Rng,
        out: &mut Vec<LootStack>,
    ) {
        let mut stacks = Vec::new();
        match &self.kind {
            EntryKind::Item { name } => stacks.push(LootStack::new(name.clone())),
            EntryKind::Tag { items, .. } => {
                if let Some(bound) = i32::try_from(items.len()).ok().filter(|bound| *bound > 0) {
                    let index = rng.next_int(bound) as usize;
                    stacks.push(LootStack::new(items[index].clone()));
                }
            }
            EntryKind::Alternatives(children) | EntryKind::Sequence(children) => {
                for child in children {
                    if child.passes(context, rng) {
                        child.expand(context, rng, &mut stacks);
                        break;
                    }
                }
            }
            EntryKind::Group(children) => {
                for child in children {
                    if child.passes(context, rng) {
                        child.expand(context, rng, &mut stacks);
                    }
                }
            }
            EntryKind::Table(table) => table.roll_into(context, rng, &mut stacks),
            EntryKind::Dynamic { .. } | EntryKind::Empty | EntryKind::Opaque { .. } => {}
        }

        for stack in &mut stacks {
            function::apply(&self.functions, stack, context, rng);
        }
        out.extend(stacks.into_iter().filter(|stack| stack.count > 0));
    }
}

/// Picks one entry by weight. A non-positive total selects nothing.
fn select<'a>(entries: &'a [Entry], rng: &mut impl Rng) -> Option<&'a Entry> {
    let total: i64 = entries
        .iter()
        .map(|entry| i64::from(entry.weight.max(0)))
        .sum();
    if total <= 0 {
        return None;
    }

    let bound = i32::try_from(total).unwrap_or(i32::MAX);
    let mut roll = i64::from(rng.next_int(bound));
    for entry in entries {
        roll -= i64::from(entry.weight.max(0));
        if roll < 0 {
            return Some(entry);
        }
    }
    entries.last()
}
