//! Item stacks: counts, component resolution, stacking rules and the NBT save format.

use ferrite_nbt::{Compound, Tag};

use crate::Error;
use crate::component::{Component, ComponentPatch, Rarity};
use crate::component_ids as ids;
use crate::defaults::{AIR, default_components, item_defaults, item_id, item_name};

/// Vanilla's hard limit on a stack's count.
pub const MAX_COUNT: i32 = 99;

/// A stack of one item.
///
/// The fields mirror the 26.3 network and save formats: an item protocol id, a count and the
/// component patch relative to the item's defaults. A count of zero or less, or the air item,
/// is an empty stack.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemStack {
    /// Protocol id in the `minecraft:item` registry.
    pub item: u16,
    /// Number of items in the stack.
    pub count: i32,
    /// Components this stack adds, overrides or removes relative to the item's defaults.
    pub components: ComponentPatch,
}

impl ItemStack {
    /// A stack of one copy of an item, with the item's default components.
    pub fn new(item: u16) -> Self {
        Self {
            item,
            count: 1,
            components: ComponentPatch::new(),
        }
    }

    /// A stack of `count` copies of an item, with the item's default components.
    pub fn with_count(item: u16, count: i32) -> Self {
        Self {
            item,
            count,
            components: ComponentPatch::new(),
        }
    }

    /// A stack of an item name, with or without the `minecraft:` prefix.
    pub fn from_name(name: &str) -> Option<Self> {
        item_id(name).map(Self::new)
    }

    /// An empty (air) stack.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Whether the stack holds no items.
    pub fn is_empty(&self) -> bool {
        self.count <= 0 || self.item == AIR
    }

    /// Name of the stack's item, if the protocol id is registered.
    pub fn name(&self) -> Option<&'static str> {
        item_name(self.item)
    }

    /// The stack limit: the `minecraft:max_stack_size` component if the patch sets it, otherwise
    /// the item's default. A removed `max_stack_size` falls back to vanilla's missing-component
    /// default of 1.
    pub fn max_stack_size(&self) -> i32 {
        match self.components.get(ids::MAX_STACK_SIZE) {
            Some(Component::MaxStackSize(size)) => *size,
            Some(_) => 1,
            None => {
                if self.components.is_removed(ids::MAX_STACK_SIZE) {
                    1
                } else {
                    item_defaults(self.item).map_or(1, |defaults| defaults.max_stack_size)
                }
            }
        }
    }

    /// The item's durability, from the `minecraft:max_damage` component, if it can take damage.
    pub fn max_damage(&self) -> Option<i32> {
        match self.components.get(ids::MAX_DAMAGE) {
            Some(Component::MaxDamage(value)) => Some(*value),
            _ => {
                if self.components.is_removed(ids::MAX_DAMAGE) {
                    None
                } else {
                    item_defaults(self.item).and_then(|defaults| defaults.max_damage)
                }
            }
        }
    }

    /// Whether the item can take damage.
    pub fn is_damageable(&self) -> bool {
        self.max_damage().is_some()
    }

    /// Durability already used, from the `minecraft:damage` component.
    pub fn damage(&self) -> i32 {
        match self.components.get(ids::DAMAGE) {
            Some(Component::Damage(value)) => *value,
            _ => 0,
        }
    }

    /// Rarity, from the `minecraft:rarity` component or the item's default.
    pub fn rarity(&self) -> Rarity {
        match self.components.get(ids::RARITY) {
            Some(Component::Rarity(rarity)) => *rarity,
            _ => {
                if self.components.is_removed(ids::RARITY) {
                    Rarity::Common
                } else {
                    item_defaults(self.item).map_or(Rarity::Common, |defaults| defaults.rarity)
                }
            }
        }
    }

    /// The effective value of a component: the patch value, the item's default, or `None` when
    /// the component is absent or removed.
    ///
    /// Returns typed values when the shape is known and [`Component::Opaque`] otherwise. The
    /// default is decoded from the embedded table on each call.
    pub fn effective_component(&self, id: u16) -> Option<Component> {
        if let Some(component) = self.components.get(id) {
            return Some(component.clone());
        }
        if self.components.is_removed(id) {
            return None;
        }
        let mut defaults = default_components(self.item)?;
        let value = defaults.remove(&id)?;
        Some(Component::from_nbt(id, &value))
    }

    /// Whether two stacks are of the same item, ignoring count and components.
    pub fn is_same_item(&self, other: &Self) -> bool {
        self.item == other.item
    }

    /// Whether two stacks are of the same item and have the same component patch, which is
    /// vanilla's `isSameItemSameComponents`.
    pub fn is_same_item_same_components(&self, other: &Self) -> bool {
        self.is_same_item(other) && self.components == other.components
    }

    /// Whether this stack can absorb items from `other` without overflowing: same item, same
    /// components and room left under the stack limit.
    pub fn can_stack_with(&self, other: &Self) -> bool {
        !self.is_empty()
            && self.is_same_item_same_components(other)
            && self.count < self.max_stack_size()
    }

    /// A copy of the stack with a different count.
    pub fn copy_with_count(&self, count: i32) -> Self {
        Self {
            item: self.item,
            count,
            components: self.components.clone(),
        }
    }

    /// Splits up to `amount` items off the stack into a new stack, reducing this stack's count.
    ///
    /// Returns `None` for an empty stack or a non-positive amount. Asking for more than the
    /// stack holds moves everything.
    pub fn split(&mut self, amount: i32) -> Option<Self> {
        if self.is_empty() || amount <= 0 {
            return None;
        }
        let taken = amount.min(self.count);
        self.count -= taken;
        Some(self.copy_with_count(taken))
    }

    /// Merges as many items as fit from `other` into this stack and returns how many moved.
    ///
    /// Nothing moves unless the stacks are stackable; an empty stack adopts `other`'s item and
    /// components first. A fully merged source keeps its item id with a count of zero, which
    /// [`ItemStack::is_empty`] reports as empty.
    pub fn merge(&mut self, other: &mut Self) -> i32 {
        if other.is_empty() {
            return 0;
        }
        if self.is_empty() {
            self.item = other.item;
            self.components = other.components.clone();
            self.count = 0;
        } else if !self.can_stack_with(other) {
            return 0;
        }
        let room = (self.max_stack_size() - self.count).max(0);
        let moved = room.min(other.count);
        self.count += moved;
        other.count -= moved;
        moved
    }

    /// Encodes the stack as the 26.3 item save format.
    ///
    /// The `components` compound is omitted for a stack with no patch entries, and removed
    /// components are written as `!<name>` keys with an empty compound value.
    pub fn to_nbt(&self) -> Result<Compound, Error> {
        let name = item_name(self.item).ok_or(Error::UnknownItemId(self.item))?;
        let mut root = Compound::new();
        root.insert("id".to_owned(), Tag::String(name.to_owned()));
        root.insert("count".to_owned(), Tag::Int(self.count));
        if !self.components.is_empty() {
            root.insert(
                "components".to_owned(),
                Tag::Compound(self.components.to_nbt()?),
            );
        }
        Ok(root)
    }

    /// Decodes a stack from the 26.3 item save format.
    ///
    /// A missing `count` defaults to 1, as vanilla's item stack codec does.
    pub fn from_nbt(root: &Compound) -> Result<Self, Error> {
        let id = match root.get("id") {
            Some(Tag::String(id)) => id,
            Some(_) => return Err(Error::InvalidId),
            None => return Err(Error::MissingId),
        };
        let item = item_id(id).ok_or_else(|| Error::UnknownItem(id.clone()))?;
        let count = match root.get("count") {
            Some(Tag::Int(count)) => *count,
            Some(_) => return Err(Error::InvalidCount),
            None => 1,
        };
        if !(0..=MAX_COUNT).contains(&count) {
            return Err(Error::CountOutOfRange(count));
        }
        let components = match root.get("components") {
            None => ComponentPatch::new(),
            Some(Tag::Compound(components)) => ComponentPatch::from_nbt(components)?,
            Some(_) => return Err(Error::InvalidComponents),
        };
        Ok(Self {
            item,
            count,
            components,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::{Food, Rarity};

    fn sword() -> ItemStack {
        ItemStack::from_name("diamond_sword").expect("diamond_sword is an item")
    }

    fn stone() -> ItemStack {
        ItemStack::from_name("stone").expect("stone is an item")
    }

    #[test]
    fn item_names_and_defaults_come_from_the_generated_tables() {
        let sword = sword();
        assert_eq!(sword.name(), Some("minecraft:diamond_sword"));
        assert_eq!(sword.max_damage(), Some(1561));
        assert_eq!(sword.max_stack_size(), 1);
        assert_eq!(sword.damage(), 0);
        assert!(sword.is_damageable());
        assert_eq!(sword.rarity(), Rarity::Common);

        let mut stone = stone();
        assert_eq!(stone.max_damage(), None);
        assert_eq!(stone.max_stack_size(), 64);
        assert!(!stone.is_damageable());

        stone.count = 64;
        assert!(!stone.is_empty());
        stone.count = 0;
        assert!(stone.is_empty());
        assert!(ItemStack::empty().is_empty());
        assert!(ItemStack::new(AIR).is_empty());

        assert_eq!(
            ItemStack::from_name("minecraft:stone").map(|stack| stack.item),
            ItemStack::from_name("stone").map(|stack| stack.item)
        );
        assert_eq!(ItemStack::from_name("not_an_item"), None);
    }

    #[test]
    fn nbt_round_trip_keeps_removed_components_and_custom_data() {
        let mut custom_data = Compound::new();
        custom_data.insert("owner".to_owned(), Tag::String("Steve".to_owned()));
        custom_data.insert("price".to_owned(), Tag::Int(7));

        let mut stack = sword();
        stack
            .components
            .insert(ids::CUSTOM_DATA, Component::CustomData(custom_data.clone()));
        stack.components.insert(ids::DAMAGE, Component::Damage(3));
        stack.components.remove(ids::FOOD);

        let root = stack.to_nbt().expect("stack encodes");
        assert_eq!(
            root.get("id"),
            Some(&Tag::String("minecraft:diamond_sword".to_owned()))
        );
        assert_eq!(root.get("count"), Some(&Tag::Int(1)));
        let Some(Tag::Compound(encoded)) = root.get("components") else {
            panic!("components is a compound");
        };
        assert_eq!(
            encoded.get("minecraft:custom_data"),
            Some(&Tag::Compound(custom_data.clone()))
        );
        assert_eq!(encoded.get("minecraft:damage"), Some(&Tag::Int(3)));
        assert_eq!(
            encoded.get("!minecraft:food"),
            Some(&Tag::Compound(Compound::new())),
            "removed components are `!` keys with an empty compound"
        );

        let decoded = ItemStack::from_nbt(&root).expect("stack decodes");
        assert_eq!(decoded, stack);
        assert_eq!(
            decoded.components.get(ids::CUSTOM_DATA),
            Some(&Component::CustomData(custom_data))
        );
        assert_eq!(
            decoded.components.get(ids::DAMAGE),
            Some(&Component::Damage(3))
        );
        assert!(decoded.components.is_removed(ids::FOOD));
        assert_eq!(decoded.effective_component(ids::FOOD), None);
    }

    #[test]
    fn nbt_omits_an_empty_patch_and_defaults_the_count() {
        let stack = stone();
        let root = stack.to_nbt().expect("stack encodes");
        assert!(!root.contains_key("components"));

        let mut without_count = root.clone();
        without_count.shift_remove("count");
        assert_eq!(ItemStack::from_nbt(&without_count).unwrap().count, 1);

        assert_eq!(
            ItemStack::empty().to_nbt().unwrap().get("count"),
            Some(&Tag::Int(0))
        );
    }

    #[test]
    fn nbt_rejects_invalid_stacks() {
        let mut root = Compound::new();
        root.insert(
            "id".to_owned(),
            Tag::String("minecraft:diamond_sword".to_owned()),
        );
        root.insert("count".to_owned(), Tag::Int(MAX_COUNT + 1));
        assert!(matches!(
            ItemStack::from_nbt(&root),
            Err(Error::CountOutOfRange(100))
        ));

        root.insert("count".to_owned(), Tag::String("3".to_owned()));
        assert!(matches!(
            ItemStack::from_nbt(&root),
            Err(Error::InvalidCount)
        ));

        root.insert("count".to_owned(), Tag::Int(1));
        root.insert(
            "id".to_owned(),
            Tag::String("minecraft:not_an_item".to_owned()),
        );
        assert!(
            matches!(ItemStack::from_nbt(&root), Err(Error::UnknownItem(name)) if name == "minecraft:not_an_item")
        );

        root.insert("id".to_owned(), Tag::Int(1));
        assert!(matches!(ItemStack::from_nbt(&root), Err(Error::InvalidId)));

        root.shift_remove("id");
        assert!(matches!(ItemStack::from_nbt(&root), Err(Error::MissingId)));

        root.insert("id".to_owned(), Tag::String("minecraft:stone".to_owned()));
        root.insert("components".to_owned(), Tag::List(ferrite_nbt::List::End));
        assert!(matches!(
            ItemStack::from_nbt(&root),
            Err(Error::InvalidComponents)
        ));

        let mut unknown = Compound::new();
        unknown.insert("minecraft:not_a_component".to_owned(), Tag::Int(1));
        root.insert("components".to_owned(), Tag::Compound(unknown));
        assert!(matches!(
            ItemStack::from_nbt(&root),
            Err(Error::UnknownComponent(name)) if name == "minecraft:not_a_component"
        ));

        root.insert("components".to_owned(), Tag::Compound(Compound::new()));
        assert_eq!(ItemStack::from_nbt(&root).unwrap(), stone());
    }

    #[test]
    fn stacking_requires_the_same_item_and_components() {
        let stone = stone();
        let other = stone.clone();
        assert!(stone.is_same_item(&other));
        assert!(stone.is_same_item_same_components(&other));
        assert!(stone.can_stack_with(&other));

        let mut damaged = stone.clone();
        damaged.components.insert(ids::DAMAGE, Component::Damage(1));
        assert!(stone.is_same_item(&damaged));
        assert!(!stone.is_same_item_same_components(&damaged));
        assert!(!stone.can_stack_with(&damaged));

        assert!(!stone.is_same_item(&sword()));
        assert!(!stone.can_stack_with(&sword()));

        let mut full = stone.clone();
        full.count = 64;
        assert!(!full.can_stack_with(&stone));
        assert!(!full.can_stack_with(&ItemStack::empty()));
        assert!(
            !sword().can_stack_with(&sword()),
            "a stack limit of 1 leaves no room"
        );
    }

    #[test]
    fn max_stack_size_can_be_overridden_or_removed() {
        let mut stack = stone();
        stack
            .components
            .insert(ids::MAX_STACK_SIZE, Component::MaxStackSize(16));
        assert_eq!(stack.max_stack_size(), 16);

        stack.components.remove(ids::MAX_STACK_SIZE);
        assert_eq!(
            stack.max_stack_size(),
            1,
            "a removed max_stack_size falls back to vanilla's missing-component default"
        );
        assert_eq!(stack.effective_component(ids::MAX_STACK_SIZE), None);
    }

    #[test]
    fn splitting_moves_up_to_the_requested_count() {
        let mut stack = stone();
        stack.count = 10;
        let split = stack.split(3).expect("split");
        assert_eq!(split.count, 3);
        assert_eq!(split.item, stack.item);
        assert_eq!(split.components, stack.components);
        assert_eq!(stack.count, 7);

        let rest = stack.split(100).expect("split");
        assert_eq!(rest.count, 7);
        assert!(stack.is_empty());
        assert_eq!(stack.split(1), None);
        assert_eq!(stone().split(0), None);
    }

    #[test]
    fn merging_stops_at_the_stack_limit() {
        let mut target = stone();
        target.count = 60;
        let mut source = stone();
        source.count = 10;
        assert_eq!(target.merge(&mut source), 4);
        assert_eq!(target.count, 64);
        assert_eq!(source.count, 6);

        let mut first = sword();
        let mut second = sword();
        assert_eq!(first.merge(&mut second), 0, "a sword does not stack");

        let mut empty = ItemStack::empty();
        assert_eq!(
            empty.merge(&mut source),
            6,
            "an empty stack adopts the item"
        );
        assert_eq!(empty.item, stone().item);
        assert_eq!(empty.count, 6);
        assert!(source.is_empty());

        assert_eq!(empty.merge(&mut ItemStack::empty()), 0);
    }

    #[test]
    fn effective_components_read_defaults_and_patches() {
        let sword = sword();
        assert_eq!(
            sword.effective_component(ids::MAX_DAMAGE),
            Some(Component::MaxDamage(1561))
        );
        assert_eq!(
            sword.effective_component(ids::RARITY),
            Some(Component::Rarity(Rarity::Common))
        );
        assert_eq!(
            sword.effective_component(ids::DAMAGE),
            Some(Component::Damage(0))
        );
        assert_eq!(sword.effective_component(ids::CUSTOM_DATA), None);

        let mut damaged = sword.clone();
        damaged
            .components
            .insert(ids::DAMAGE, Component::Damage(12));
        assert_eq!(
            damaged.effective_component(ids::DAMAGE),
            Some(Component::Damage(12))
        );
        assert_eq!(damaged.damage(), 12);

        let mut removed = sword;
        removed.components.remove(ids::MAX_DAMAGE);
        assert_eq!(removed.max_damage(), None);
        assert_eq!(removed.effective_component(ids::MAX_DAMAGE), None);
    }

    #[test]
    fn default_components_decode_to_typed_values() {
        let apple = ItemStack::from_name("apple").expect("apple is an item");
        assert_eq!(
            apple.effective_component(ids::FOOD),
            Some(Component::Food(Food::new(4, 2.4)))
        );

        let sword = sword();
        let Some(Component::AttributeModifiers(modifiers)) =
            sword.effective_component(ids::ATTRIBUTE_MODIFIERS)
        else {
            panic!("diamond_sword has attribute modifiers");
        };
        assert_eq!(modifiers.len(), 2);
        assert_eq!(modifiers[0].attribute, "minecraft:attack_damage");
        assert_eq!(modifiers[0].amount, 6.0);
    }

    #[test]
    fn potion_contents_components_decode_and_round_trip() {
        let potion = ItemStack::from_name("potion").expect("potion is an item");
        assert_eq!(
            potion.effective_component(ids::POTION_CONTENTS),
            Some(Component::PotionContents(crate::PotionContents::default()))
        );
        let contents = crate::PotionContents {
            potion: Some("minecraft:swiftness".to_owned()),
            custom_color: Some(0x1122_3344),
            custom_effects: vec![crate::PotionEffect {
                effect: "minecraft:jump_boost".to_owned(),
                amplifier: 1,
                duration: 200,
                ambient: false,
                visible: true,
                show_icon: false,
            }],
            custom_name: Some("Fast".to_owned()),
        };
        let component = Component::PotionContents(contents.clone());
        let tag = component.to_nbt().unwrap();
        assert_eq!(Component::from_nbt(ids::POTION_CONTENTS, &tag), component);
    }

    #[test]
    fn nbt_round_trip_preserves_full_component_and_nested_pdc_surface() {
        let mut stack = sword();
        stack
            .components
            .insert(ids::MAX_STACK_SIZE, Component::MaxStackSize(16));
        stack
            .components
            .insert(ids::MAX_DAMAGE, Component::MaxDamage(250));
        stack.components.insert(ids::DAMAGE, Component::Damage(42));
        stack
            .components
            .insert(ids::UNBREAKABLE, Component::Unbreakable);
        stack.components.insert(
            ids::CUSTOM_NAME,
            Component::CustomName(ferrite_text::Component::text("Relic Blade")),
        );
        stack.components.insert(
            ids::LORE,
            Component::Lore(vec![ferrite_text::Component::text("Line 1")]),
        );

        let mut ench = crate::Enchantments::new();
        ench.set("minecraft:sharpness", 5);
        stack
            .components
            .insert(ids::ENCHANTMENTS, Component::Enchantments(ench));

        stack.components.insert(
            ids::ATTRIBUTE_MODIFIERS,
            Component::AttributeModifiers(vec![crate::AttributeModifier {
                attribute: "minecraft:attack_damage".to_owned(),
                id: "ferrite:bonus".to_owned(),
                amount: 4.5,
                operation: crate::AttributeOperation::AddValue,
                slot: crate::EquipmentSlot::Mainhand,
            }]),
        );
        stack.components.insert(
            ids::FOOD,
            Component::Food(crate::Food {
                nutrition: 6,
                saturation: 7.2,
                can_always_eat: true,
            }),
        );
        stack.components.insert(
            ids::TOOL,
            Component::Tool(crate::Tool {
                rules: vec![],
                default_mining_speed: 2.5,
                damage_per_block: 1,
                can_destroy_blocks_in_creative: false,
            }),
        );
        stack.components.insert(
            ids::TOOLTIP_DISPLAY,
            Component::TooltipDisplay(crate::TooltipDisplay {
                hide_tooltip: true,
                hidden_components: vec![ids::ATTRIBUTE_MODIFIERS],
            }),
        );

        let mut nested_pdc = Compound::new();
        nested_pdc.insert("tier".to_owned(), Tag::Int(3));
        nested_pdc.insert("flags".to_owned(), Tag::IntArray(vec![1, 2, 3]));
        let mut bukkit_values = Compound::new();
        bukkit_values.insert("ferrite:nested".to_owned(), Tag::Compound(nested_pdc));
        let mut custom_data = Compound::new();
        custom_data.insert(
            "PublicBukkitValues".to_owned(),
            Tag::Compound(bukkit_values),
        );
        stack
            .components
            .insert(ids::CUSTOM_DATA, Component::CustomData(custom_data));

        let mut cd = Compound::new();
        cd.insert("seconds".to_owned(), Tag::Float(1.5));
        cd.insert(
            "cooldown_group".to_owned(),
            Tag::String("ferrite:dash".to_owned()),
        );
        stack
            .components
            .insert(ids::USE_COOLDOWN, Component::Opaque(Tag::Compound(cd)));

        stack.components.insert(
            ids::ITEM_MODEL,
            Component::Opaque(Tag::String("ferrite:relic_sword".to_owned())),
        );

        let root = stack.to_nbt().expect("encode full stack");
        let decoded = ItemStack::from_nbt(&root).expect("decode full stack");
        assert_eq!(decoded, stack);
    }
}
