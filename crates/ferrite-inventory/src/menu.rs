//! Container menus and `container_click` semantics (vanilla `AbstractContainerMenu`).

use ferrite_item::{Component, Enchantments, HashedSlot, ItemStack, component_ids as ids};
use ferrite_nbt::Tag;

use crate::inventory::{
    CHEST, FEET, HEAD, HOTBAR, ITEMS, LEGS, OFFHAND, PlayerInventory, equipment_slot, is_stackable,
    max_in_slot, prevents_armor_change,
};

/// The slot number of a click outside the window, which drops the carried stack.
pub const SLOT_OUTSIDE: i16 = -999;

/// Slots of a shulker box (vanilla `ShulkerBoxBlockEntity.CONTAINER_SIZE`).
pub const SHULKER_BOX_SLOTS: usize = 27;

/// Whether a stack is a shulker box of any colour, which cannot go inside another one.
pub fn is_shulker_box(stack: &ItemStack) -> bool {
    stack.name().is_some_and(|name| {
        name.strip_prefix("minecraft:")
            .unwrap_or(name)
            .ends_with("shulker_box")
    })
}

/// Vanilla `ContainerInput` (formerly `ClickType`), the `container_click` mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickInput {
    Pickup,
    QuickMove,
    Swap,
    Clone,
    Throw,
    QuickCraft,
    PickupAll,
}

impl ClickInput {
    /// The input with a network id.
    pub fn from_id(id: i32) -> Option<Self> {
        Some(match id {
            0 => Self::Pickup,
            1 => Self::QuickMove,
            2 => Self::Swap,
            3 => Self::Clone,
            4 => Self::Throw,
            5 => Self::QuickCraft,
            6 => Self::PickupAll,
            _ => return None,
        })
    }

    /// The network id.
    pub fn id(self) -> i32 {
        self as i32
    }
}

/// The furnace family (vanilla `AbstractFurnaceMenu` and its three block menus).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FurnaceKind {
    Furnace,
    BlastFurnace,
    Smoker,
}

impl FurnaceKind {
    /// The `minecraft:menu` registry name.
    pub const fn menu_type(self) -> &'static str {
        match self {
            Self::Furnace => "minecraft:furnace",
            Self::BlastFurnace => "minecraft:blast_furnace",
            Self::Smoker => "minecraft:smoker",
        }
    }

    /// The translation key of the screen title.
    pub const fn title(self) -> &'static str {
        match self {
            Self::Furnace => "container.furnace",
            Self::BlastFurnace => "container.blast_furnace",
            Self::Smoker => "container.smoker",
        }
    }

    /// The block this menu belongs to.
    pub const fn block(self) -> &'static str {
        match self {
            Self::Furnace => "minecraft:furnace",
            Self::BlastFurnace => "minecraft:blast_furnace",
            Self::Smoker => "minecraft:smoker",
        }
    }
}

/// Vanilla `AbstractFurnaceMenu` container data slots.
pub const FURNACE_LIT: usize = 0;
pub const FURNACE_COOK_PROGRESS: usize = 1;
pub const FURNACE_COOK_TOTAL: usize = 2;
pub const FURNACE_BURN_LEFT: usize = 3;
pub const FURNACE_BURN_TOTAL: usize = 4;
/// Number of container data slots of a furnace family menu.
pub const FURNACE_DATA_SLOTS: usize = 5;

/// Vanilla `BrewingStandBlockEntity.BREW_TIME`: ticks one brew takes.
pub const BREW_TIME: i32 = 400;
/// Blaze powder charges a brewing stand holds (vanilla `BrewingStandBlockEntity.FUEL_PER_BREWING`).
pub const FUEL_PER_BLAZE_POWDER: i32 = 20;

/// Vanilla `BrewingStandMenu` container data: brew time left, then fuel left.
pub const BREWING_BREW_TIME: usize = 0;
pub const BREWING_FUEL: usize = 1;
/// Number of container data slots of a brewing stand menu.
pub const BREWING_DATA_SLOTS: usize = 2;

/// Vanilla `BeaconMenu` container data: pyramid level, primary effect id + 1, secondary id + 1.
pub const BEACON_LEVELS: usize = 0;
pub const BEACON_PRIMARY: usize = 1;
pub const BEACON_SECONDARY: usize = 2;
/// Number of container data slots of a beacon menu.
pub const BEACON_DATA_SLOTS: usize = 3;

/// One furnace-family recipe result, as [`Crafter::cook`] reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct CookingOutput {
    pub result: ItemStack,
    /// Ticks the recipe takes (vanilla `AbstractCookingRecipe.getCookingTime`).
    pub cook_time: i32,
    /// Experience awarded when the result is taken.
    pub experience: f32,
}

/// Furnace-family burn and cook timers (vanilla `AbstractFurnaceBlockEntity`'s
/// `litTimeRemaining`, `litTotalTime`, `cookingProgress` and `cookingTotalTime`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FurnaceState {
    pub cook: i32,
    pub cook_total: i32,
    pub burn: i32,
    pub burn_total: i32,
    /// Experience of the recipes completed but not yet taken from the output slot
    /// (vanilla `AbstractFurnaceBlockEntity.recipesUsed`).
    pub pending_experience: f32,
}

impl FurnaceState {
    /// Vanilla `isLit`.
    pub fn lit(&self) -> bool {
        self.burn > 0
    }
}

/// Brewing stand timers (vanilla `BrewingStandBlockEntity.brewTime` and `fuel`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BrewingState {
    /// Ticks left of the current brew; [`BREW_TIME`] when it starts.
    pub brew_time: i32,
    /// Blaze powder charges left; one is spent when a brew starts.
    pub fuel: i32,
}

/// A beacon's pyramid level and chosen effects (vanilla `BeaconBlockEntity`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BeaconState {
    /// Pyramid levels, 0 to 4.
    pub levels: i32,
    /// `minecraft:mob_effect` protocol id of the primary effect.
    pub primary: Option<i32>,
    /// The second effect a level-4 beacon can grant.
    pub secondary: Option<i32>,
}

/// The menus this crate implements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKind {
    /// The player's own inventory, container 0 (vanilla `InventoryMenu`).
    Inventory,
    /// A crafting table (vanilla `CraftingMenu`).
    Crafting,
    /// A chest-like grid of `rows` rows of nine slots (vanilla `ChestMenu`).
    Generic { rows: u8 },
    /// A shulker box (vanilla `ShulkerBoxMenu`): three rows of nine slots that refuse shulker
    /// boxes, so boxes never nest.
    ShulkerBox,
    /// A furnace, blast furnace or smoker (vanilla `AbstractFurnaceMenu`).
    Furnace(FurnaceKind),
    /// An anvil (vanilla `AnvilMenu`).
    Anvil,
    /// An enchanting table (vanilla `EnchantmentMenu`).
    Enchanting,
    /// A grindstone (vanilla `GrindstoneMenu`).
    Grindstone,
    /// A stonecutter (vanilla `StonecutterMenu`).
    Stonecutter,
    /// A smithing table (vanilla `SmithingMenu`).
    Smithing,
    /// A loom (vanilla `LoomMenu`).
    Loom,
    /// A cartography table (vanilla `CartographyTableMenu`).
    Cartography,
    /// A brewing stand (vanilla `BrewingStandMenu`).
    BrewingStand,
    /// A beacon (vanilla `BeaconMenu`).
    Beacon,
    /// A villager's merchant screen (vanilla `MerchantMenu`).
    Merchant,
    /// A horse, donkey, mule or llama inventory (vanilla `HorseInventoryMenu`): saddle, body
    /// armour, then three rows of `columns` chest slots. Opened with `mount_screen_open`, so it has
    /// no menu type.
    Horse { columns: u8, llama: bool },
}

impl MenuKind {
    /// The `minecraft:menu` registry name sent in `open_screen`; the inventory has none.
    pub fn menu_type(self) -> Option<String> {
        match self {
            Self::Inventory => None,
            Self::Crafting => Some("minecraft:crafting".into()),
            Self::Generic { rows } => Some(format!("minecraft:generic_9x{rows}")),
            Self::ShulkerBox => Some("minecraft:shulker_box".into()),
            Self::Furnace(kind) => Some(kind.menu_type().into()),
            Self::Anvil => Some("minecraft:anvil".into()),
            Self::Enchanting => Some("minecraft:enchantment".into()),
            Self::Grindstone => Some("minecraft:grindstone".into()),
            Self::Stonecutter => Some("minecraft:stonecutter".into()),
            Self::Smithing => Some("minecraft:smithing".into()),
            Self::Loom => Some("minecraft:loom".into()),
            Self::Cartography => Some("minecraft:cartography_table".into()),
            Self::BrewingStand => Some("minecraft:brewing_stand".into()),
            Self::Beacon => Some("minecraft:beacon".into()),
            Self::Merchant => Some("minecraft:merchant".into()),
            Self::Horse { .. } => None,
        }
    }
}

/// One offer of a merchant menu (vanilla `MerchantOffer`), with the price it currently asks.
#[derive(Clone, Debug, PartialEq)]
pub struct Trade {
    /// Vanilla `baseCostA`: the item and its base price.
    pub wants: ItemStack,
    /// Vanilla `result`.
    pub gives: ItemStack,
    /// Vanilla `costB`, the optional second input.
    pub second_wants: Option<ItemStack>,
    pub uses: i32,
    pub max_uses: i32,
    /// Experience both the player and the villager gain from one sale.
    pub xp: i32,
    /// Vanilla `specialPriceDiff`, added to the base price (negative discounts).
    pub special_price: i32,
    /// Vanilla `priceMultiplier`, applied to the demand.
    pub price_multiplier: f32,
    /// Vanilla `demand`, raised by every sale.
    pub demand: i32,
}

impl Trade {
    /// A never-used trade.
    pub fn new(wants: ItemStack, gives: ItemStack, max_uses: i32, xp: i32) -> Self {
        Self {
            wants,
            gives,
            second_wants: None,
            uses: 0,
            max_uses,
            xp,
            special_price: 0,
            price_multiplier: 0.05,
            demand: 0,
        }
    }

    /// Vanilla `MerchantOffer.getModifiedCostCount`: the base price plus the demand and the
    /// special price, never below one item.
    pub fn price(&self) -> i32 {
        let demand = if self.demand > 0 {
            self.demand as f32 * self.price_multiplier
        } else {
            0.0
        };
        ((self.wants.count as f32 + demand + self.special_price as f32).round() as i32).max(1)
    }

    /// The first input the trade asks for, at its current price.
    pub fn cost_a(&self) -> ItemStack {
        self.wants.copy_with_count(self.price())
    }

    pub fn is_out_of_stock(&self) -> bool {
        self.uses >= self.max_uses
    }

    /// Vanilla `MerchantOffer.updateDemand`: one sale raises the demand, which raises the price
    /// until the next restock.
    pub fn on_sale(&mut self) {
        self.uses = (self.uses + 1).min(self.max_uses);
        self.demand = self.demand.saturating_add(1);
    }

    /// Vanilla `MerchantOffer.resetUses` when the villager restocks.
    pub fn restock(&mut self) {
        self.uses = 0;
        self.demand = 0;
        self.special_price = 0;
    }

    /// Whether `a` and `b` satisfy the trade's two inputs (vanilla `MerchantOffer.satisfiedBy`).
    pub fn satisfied_by(&self, a: &ItemStack, b: &ItemStack) -> bool {
        fn matches(stack: &ItemStack, cost: &ItemStack) -> bool {
            !stack.is_empty()
                && stack.is_same_item(cost)
                && stack.components == cost.components
                && stack.count >= cost.count
        }
        if !matches(a, &self.cost_a()) {
            return false;
        }
        match &self.second_wants {
            Some(cost) => matches(b, cost),
            None => b.is_empty(),
        }
    }
}

/// Matches crafting grids against the recipe book.
pub trait Crafter {
    /// The result of a row-major grid with `width` columns, or an empty stack.
    fn craft(&self, grid: &[ItemStack], width: usize) -> ItemStack;

    /// The anvil rules of `enchantment` (`minecraft:` name) for `item`, or `None` when the
    /// enchantment is unknown and the anvil must leave it out.
    fn enchantment_rules(&self, _enchantment: &str, _item: &ItemStack) -> Option<EnchantRules> {
        None
    }

    /// Whether two enchantments exclude each other (vanilla `Enchantment.areCompatible` false).
    fn enchantments_exclusive(&self, _a: &str, _b: &str) -> bool {
        false
    }

    /// The id of the recipe `craft` matches for this grid, for the `recipe_crafted` trigger.
    fn recipe_id(&self, _grid: &[ItemStack], _width: usize) -> Option<String> {
        None
    }

    /// The items each cell of a `width` x `width` crafting grid must hold for recipe `id`, laid
    /// out row-major (`None` is an empty cell), or `None` when the recipe does not fit the grid.
    /// Names carry the `minecraft:` prefix.
    fn recipe_layout(&self, _id: &str, _width: usize) -> Option<Vec<Option<Vec<String>>>> {
        None
    }

    /// The furnace-family recipe for `input`, if one exists (vanilla `RecipeManager.getRecipeFor`).
    fn cook(&self, _kind: FurnaceKind, _input: &ItemStack) -> Option<CookingOutput> {
        None
    }

    /// Burn ticks of a fuel item (vanilla `FuelValues.burnDuration`), or 0 when it is no fuel.
    fn burn_time(&self, _item: &ItemStack) -> i32 {
        0
    }

    /// The result of brewing `ingredient` into the potion in `bottle` (vanilla
    /// `PotionBrewing.mix`), or `None` when the two do not brew.
    fn brew(&self, _ingredient: &ItemStack, _bottle: &ItemStack) -> Option<ItemStack> {
        None
    }

    /// Whether an item is an ingredient of any brewing mix (vanilla
    /// `PotionBrewing.isIngredient`).
    fn is_brewing_ingredient(&self, _item: &ItemStack) -> bool {
        false
    }

    /// The stonecutter results for `input` (vanilla `StonecutterMenu.getResults`).
    fn stonecutting(&self, _input: &ItemStack) -> Vec<ItemStack> {
        Vec::new()
    }

    /// The result of a smithing-table template/base/addition (vanilla `SmithingMenu.createResult`).
    fn smithing(
        &self,
        _template: &ItemStack,
        _base: &ItemStack,
        _addition: &ItemStack,
    ) -> Option<ItemStack> {
        None
    }

    /// The id of the recipe `smithing` applies, for the `recipe_crafted` trigger.
    fn smithing_recipe_id(
        &self,
        _template: &ItemStack,
        _base: &ItemStack,
        _addition: &ItemStack,
    ) -> Option<String> {
        None
    }
}

/// What the anvil needs to know about one enchantment on one item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnchantRules {
    /// `Enchantment.getMaxLevel`.
    pub max_level: i32,
    /// The enchantment's `anvil_cost` (its rarity weight).
    pub anvil_cost: i32,
    /// Whether the item is in the enchantment's `supported_items`.
    pub supported: bool,
}

/// What the cartography table needs to know about a saved map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapInfo {
    /// Zoom level, 0 to 4.
    pub scale: u8,
    pub locked: bool,
}

/// The saved-map store could not finish a cartography result; the take is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapFinishError;

/// Saved-map access for the cartography table. The menu itself is a pure state machine, so the
/// server supplies the store behind this trait.
pub trait MapAccess {
    /// The facts about map `id`, or `None` when no readable map has that id.
    fn info(&self, _id: i32) -> Option<MapInfo> {
        None
    }

    /// Completes a taken cartography result (vanilla `MapItem.onCraftedPostProcess`): a stack
    /// marked with `map_post_processing` gets a newly created saved map, its `map_id` is pointed
    /// at it and the marker removed. Returns the new id, or `None` when the stack needed no work.
    fn finish(&self, _stack: &mut ItemStack) -> Result<Option<i32>, MapFinishError> {
        Err(MapFinishError)
    }

    /// Gives back a map [`MapAccess::finish`] created for a result that then did not reach the
    /// player (the inventory had no room).
    fn discard(&self, _id: i32) {}
}

/// A [`MapAccess`] with no maps: a cartography table never offers a result.
pub struct NoMaps;

impl MapAccess for NoMaps {}

/// A [`Crafter`] that never crafts anything.
pub struct NoCrafting;

impl Crafter for NoCrafting {
    fn craft(&self, _: &[ItemStack], _: usize) -> ItemStack {
        ItemStack::empty()
    }
}

/// What an ingredient leaves in the grid after crafting (vanilla `Item.Properties.craftRemainder`;
/// in 26.3 only these five items have one).
pub fn craft_remainder(item: u16) -> Option<ItemStack> {
    let remainder = match ferrite_item::item_name(item)? {
        "minecraft:water_bucket" | "minecraft:lava_bucket" | "minecraft:milk_bucket" => {
            "minecraft:bucket"
        }
        "minecraft:dragon_breath" | "minecraft:honey_bottle" => "minecraft:glass_bottle",
        _ => return None,
    };
    ItemStack::from_name(remainder)
}

/// The player a click acts for, plus everything the click drops.
pub struct Clicker<'a> {
    pub inventory: &'a mut PlayerInventory,
    /// Vanilla `Player.hasInfiniteMaterials` (creative): enables `CLONE`, the creative drag and
    /// taking cursed armor off.
    pub infinite_materials: bool,
    pub crafter: &'a dyn Crafter,
    /// Stacks the player drops (vanilla `player.drop`), in order.
    pub drops: Vec<ItemStack>,
    /// Experience points the click awards (furnace output, later grindstone).
    pub xp: f32,
    /// Experience levels the click spends (anvil cost).
    pub levels: i32,
    /// Ids of the recipes the click crafted, once per result taken.
    pub crafted: Vec<String>,
    /// Saved maps, for the cartography table.
    pub maps: &'a dyn MapAccess,
}

impl<'a> Clicker<'a> {
    pub fn new(
        inventory: &'a mut PlayerInventory,
        infinite_materials: bool,
        crafter: &'a dyn Crafter,
    ) -> Self {
        Self {
            inventory,
            infinite_materials,
            crafter,
            drops: Vec::new(),
            xp: 0.0,
            levels: 0,
            crafted: Vec::new(),
            maps: &NoMaps,
        }
    }

    /// Uses `maps` for cartography tables.
    pub fn with_maps(mut self, maps: &'a dyn MapAccess) -> Self {
        self.maps = maps;
        self
    }

    fn drop(&mut self, stack: ItemStack) {
        if !stack.is_empty() {
            self.drops.push(stack);
        }
    }
}

/// A packet the server sends to keep the client's copy of a menu in sync.
#[derive(Clone, Debug, PartialEq)]
pub enum Sync {
    /// `container_set_content`.
    Content {
        container_id: i32,
        state_id: i32,
        items: Vec<ItemStack>,
        carried: ItemStack,
    },
    /// `container_set_slot`.
    Slot {
        container_id: i32,
        state_id: i32,
        slot: i16,
        item: ItemStack,
    },
    /// `set_cursor_item`.
    Carried(ItemStack),
    /// `container_set_data`: a menu property (vanilla `AbstractContainerMenu.setData`).
    Data {
        container_id: i32,
        id: i16,
        value: i16,
    },
}

/// Where a menu slot keeps its stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Store {
    Inventory(usize),
    Grid(usize),
    Result,
    Container(usize),
}

/// Slot behaviour: a plain `Slot`, an `ArmorSlot` for one equipment slot, or a `ResultSlot`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rule {
    Plain,
    /// A shulker box slot: anything but a shulker box (vanilla `ShulkerBoxSlot`).
    NoShulkerBox,
    /// A mount's saddle slot.
    MountSaddle {
        llama: bool,
    },
    /// A mount's body-armour slot: a carpet for a llama, horse armour otherwise.
    MountArmor {
        llama: bool,
    },
    Armor(&'static str),
    Result,
    /// A furnace input slot (vanilla `AbstractFurnaceMenu` slot 0).
    FurnaceInput,
    /// A furnace fuel slot (vanilla `AbstractFurnaceMenu` slot 1, `FuelSlot`).
    Fuel,
    /// A menu output slot: may not be placed into, and taking from it awards experience or
    /// consumes the menu's ingredients (vanilla `ResultSlot` for stations).
    Output,
    /// An enchanting table's item slot (vanilla `EnchantmentMenu` slot 0).
    EnchantItem,
    /// An enchanting table's lapis slot.
    Lapis,
    /// A loom banner slot.
    Banner,
    /// A loom dye slot.
    Dye,
    /// A loom banner-pattern slot.
    BannerPattern,
    /// A cartography table's map slot: filled maps only.
    FilledMap,
    /// A cartography table's addition slot: paper, an empty map or a glass pane.
    MapAddition,
    /// A merchant's first or second input slot.
    TradeInput,
    /// A merchant's output slot (`MerchantResultSlot`).
    TradeOutput,
    /// A brewing stand's bottle slot (vanilla `BrewingStandMenu.PotionSlot`): potions and empty
    /// glass bottles, one per slot.
    Potion,
    /// A brewing stand's ingredient slot.
    BrewingIngredient,
    /// A brewing stand's fuel slot (vanilla `BrewingStandMenu.FuelSlot`): blaze powder only.
    BrewingFuel,
    /// A beacon's payment slot (vanilla `BeaconMenu.PaymentSlot`): one of the five payment items,
    /// one per slot.
    BeaconPayment,
}

#[derive(Clone, Copy, Debug)]
struct SlotDef {
    store: Store,
    rule: Rule,
}

/// What the server believes the client shows in a slot (vanilla `RemoteSlot`).
#[derive(Clone, Debug, PartialEq)]
enum Remote {
    /// A stack the server sent.
    Known(ItemStack),
    /// The client's own prediction from a click, as a hashed slot.
    Hashed(Option<HashedSlot>),
}

impl Remote {
    fn empty() -> Self {
        Self::Known(ItemStack::empty())
    }

    /// Vanilla `HashedStack.matches`. The component hashes are not recomputed, so a predicted
    /// stack matches when the item, count and the set of patched component types agree.
    fn matches(&self, stack: &ItemStack) -> bool {
        match self {
            Self::Known(known) => {
                (known.is_empty() && stack.is_empty())
                    || (known.count == stack.count && known.is_same_item_same_components(stack))
            }
            Self::Hashed(None) => stack.is_empty(),
            Self::Hashed(Some(hashed)) => {
                if stack.is_empty() || hashed.item != stack.item || hashed.count != stack.count {
                    return false;
                }
                let mut added: Vec<u16> = hashed.added.iter().map(|(id, _)| *id).collect();
                let mut removed = hashed.removed.clone();
                added.sort_unstable();
                removed.sort_unstable();
                added
                    .iter()
                    .copied()
                    .eq(stack.components.added().map(|(id, _)| id))
                    && removed.iter().copied().eq(stack.components.removed())
            }
        }
    }
}

/// One enchanting-table button: the level cost shown on it and the clue about its enchantment
/// (a registry id, `-1` when there is none, and its level).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EnchantOffer {
    pub cost: i32,
    pub clue: i32,
    pub clue_level: i32,
}

/// Where the enchanting table gets its randomness and data (the enchantment registry lives
/// outside this crate). Both calls are pure for a given `seed`, as in vanilla where the menu
/// re-seeds its random source before each.
pub trait EnchantTable: Send + std::marker::Sync + std::fmt::Debug {
    /// `EnchantmentMenu.slotsChanged`: the three buttons for `item` next to `bookshelves`
    /// power blocks (costs, then the clue picked from each button's enchantment list).
    fn offers(&self, item: &ItemStack, bookshelves: i32, seed: i32) -> [EnchantOffer; 3];
    /// `EnchantmentMenu.getEnchantmentList`: the `(enchantment id, level)` pairs button `slot`
    /// applies when its cost is `cost`.
    fn enchantments(
        &self,
        item: &ItemStack,
        slot: usize,
        cost: i32,
        seed: i32,
    ) -> Vec<(String, i32)>;
}

#[derive(Clone, Default)]
struct EnchantingState {
    table: Option<std::sync::Arc<dyn EnchantTable>>,
    bookshelves: i32,
    seed: i32,
    offers: [EnchantOffer; 3],
}

impl std::fmt::Debug for EnchantingState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnchantingState")
            .field("bookshelves", &self.bookshelves)
            .field("seed", &self.seed)
            .field("offers", &self.offers)
            .finish_non_exhaustive()
    }
}

impl PartialEq for EnchantingState {
    fn eq(&self, other: &Self) -> bool {
        self.bookshelves == other.bookshelves
            && self.seed == other.seed
            && self.offers == other.offers
    }
}

/// An open container menu (vanilla `AbstractContainerMenu`).
#[derive(Clone, Debug)]
pub struct Menu {
    /// The enchanting table's offers and the numbers they come from.
    enchanting: EnchantingState,
    container_id: i32,
    kind: MenuKind,
    slots: Vec<SlotDef>,
    grid: Vec<ItemStack>,
    grid_width: usize,
    result: ItemStack,
    container: Vec<ItemStack>,
    /// Container data (vanilla `AbstractContainerMenu.dataSlots`), most recently synced to the
    /// client through `container_set_data`.
    data: Vec<i32>,
    remote_data: Vec<i32>,
    furnace: Option<FurnaceState>,
    /// Brewing stand timers while a brewing menu is open.
    brewing: Option<BrewingState>,
    /// Beacon levels and chosen effects while a beacon menu is open.
    beacon: Option<BeaconState>,
    /// Vanilla `AnvilMenu.itemName`: the name the player typed.
    rename: String,
    /// Vanilla `AnvilMenu.cost`: experience levels taking the result costs.
    anvil_cost: i32,
    /// Vanilla `Player.hasInfiniteMaterials`: the anvil's 40-level cap does not apply.
    infinite_materials: bool,
    /// Vanilla `StonecutterMenu.recipes` and `selectedRecipeIndex`.
    stonecutting: Vec<ItemStack>,
    stonecutting_index: usize,
    /// Vanilla `LoomMenu.selectedPattern` (the pattern list itself is not modelled yet).
    loom_pattern: usize,
    /// Vanilla `MerchantMenu.trades`: the villager's offers, owned by the menu while it is open.
    trades: Vec<Trade>,
    /// Vanilla `MerchantMenu.selectionHint`: the index of the offer the client selected.
    trade: usize,
    /// Villager trade XP earned through completed trades since the server last read it.
    villager_xp: i32,
    /// Experience a grindstone take awards.
    station_xp: f32,
    carried: ItemStack,
    state_id: i32,
    quickcraft_status: i32,
    quickcraft_type: i32,
    quickcraft_slots: Vec<usize>,
    remote: Vec<Remote>,
    remote_carried: Remote,
}

/// Vanilla `addStandardInventorySlots`: the main inventory rows, then the hotbar.
fn standard_inventory_slots(slots: &mut Vec<SlotDef>) {
    for slot in (HOTBAR..ITEMS).chain(0..HOTBAR) {
        slots.push(SlotDef {
            store: Store::Inventory(slot),
            rule: Rule::Plain,
        });
    }
}

impl Menu {
    fn with_slots(container_id: i32, kind: MenuKind, slots: Vec<SlotDef>) -> Self {
        let grid_width = match kind {
            MenuKind::Inventory => 2,
            MenuKind::Crafting => 3,
            MenuKind::Generic { .. }
            | MenuKind::ShulkerBox
            | MenuKind::Furnace(_)
            | MenuKind::Anvil => 0,
            MenuKind::Enchanting
            | MenuKind::Grindstone
            | MenuKind::Stonecutter
            | MenuKind::Smithing
            | MenuKind::Loom
            | MenuKind::Cartography
            | MenuKind::BrewingStand
            | MenuKind::Beacon
            | MenuKind::Merchant
            | MenuKind::Horse { .. } => 0,
        };
        let container = match kind {
            MenuKind::Generic { rows } => vec![ItemStack::empty(); usize::from(rows) * 9],
            MenuKind::ShulkerBox => vec![ItemStack::empty(); SHULKER_BOX_SLOTS],
            MenuKind::Furnace(_) => vec![ItemStack::empty(); 3],
            MenuKind::Anvil => vec![ItemStack::empty(); 2],
            MenuKind::Enchanting => vec![ItemStack::empty(); 2],
            MenuKind::Grindstone => vec![ItemStack::empty(); 2],
            MenuKind::Stonecutter => vec![ItemStack::empty(); 1],
            MenuKind::Smithing | MenuKind::Loom => vec![ItemStack::empty(); 3],
            MenuKind::Cartography => vec![ItemStack::empty(); 2],
            MenuKind::BrewingStand => vec![ItemStack::empty(); 5],
            MenuKind::Beacon => vec![ItemStack::empty(); 1],
            MenuKind::Horse { columns, .. } => {
                vec![ItemStack::empty(); 2 + usize::from(columns) * 3]
            }
            MenuKind::Merchant => vec![ItemStack::empty(); 3],
            _ => Vec::new(),
        };
        let data_len = match kind {
            MenuKind::Furnace(_) => FURNACE_DATA_SLOTS,
            MenuKind::Anvil => 1,
            MenuKind::Enchanting => 10,
            MenuKind::Stonecutter | MenuKind::Loom => 1,
            MenuKind::BrewingStand => BREWING_DATA_SLOTS,
            MenuKind::Beacon => BEACON_DATA_SLOTS,
            _ => 0,
        };
        let remote = vec![Remote::empty(); slots.len()];
        Self {
            container_id,
            kind,
            slots,
            grid: vec![ItemStack::empty(); grid_width * grid_width],
            grid_width,
            result: ItemStack::empty(),
            container,
            data: vec![0; data_len],
            remote_data: vec![i32::MIN; data_len],
            furnace: matches!(kind, MenuKind::Furnace(_)).then(FurnaceState::default),
            brewing: matches!(kind, MenuKind::BrewingStand).then(BrewingState::default),
            beacon: matches!(kind, MenuKind::Beacon).then(BeaconState::default),
            rename: String::new(),
            anvil_cost: 0,
            infinite_materials: false,
            enchanting: EnchantingState {
                offers: [EnchantOffer {
                    cost: 0,
                    clue: -1,
                    clue_level: -1,
                }; 3],
                ..Default::default()
            },
            stonecutting: Vec::new(),
            stonecutting_index: 0,
            loom_pattern: 0,
            trades: Vec::new(),
            trade: 0,
            villager_xp: 0,
            station_xp: 0.0,
            carried: ItemStack::empty(),
            state_id: 0,
            quickcraft_status: 0,
            quickcraft_type: 0,
            quickcraft_slots: Vec::new(),
            remote,
            remote_carried: Remote::empty(),
        }
    }

    /// The player's inventory menu, container 0 (vanilla `InventoryMenu`): 0 the crafting
    /// result, 1..5 the 2x2 grid, 5..9 armor (head to feet), 9..36 the main inventory, 36..45
    /// the hotbar and 45 the offhand.
    pub fn inventory() -> Self {
        let mut slots = vec![SlotDef {
            store: Store::Result,
            rule: Rule::Result,
        }];
        slots.extend((0..4).map(|i| SlotDef {
            store: Store::Grid(i),
            rule: Rule::Plain,
        }));
        for (slot, name) in [
            (HEAD, "head"),
            (CHEST, "chest"),
            (LEGS, "legs"),
            (FEET, "feet"),
        ] {
            slots.push(SlotDef {
                store: Store::Inventory(slot),
                rule: Rule::Armor(name),
            });
        }
        standard_inventory_slots(&mut slots);
        slots.push(SlotDef {
            store: Store::Inventory(OFFHAND),
            rule: Rule::Plain,
        });
        Self::with_slots(0, MenuKind::Inventory, slots)
    }

    /// A crafting table (vanilla `CraftingMenu`): 0 the result, 1..10 the 3x3 grid, 10..37 the
    /// main inventory and 37..46 the hotbar.
    pub fn crafting(container_id: i32) -> Self {
        let mut slots = vec![SlotDef {
            store: Store::Result,
            rule: Rule::Result,
        }];
        slots.extend((0..9).map(|i| SlotDef {
            store: Store::Grid(i),
            rule: Rule::Plain,
        }));
        standard_inventory_slots(&mut slots);
        Self::with_slots(container_id, MenuKind::Crafting, slots)
    }

    /// A chest-like menu (vanilla `ChestMenu`) of `rows` rows (1..=6) followed by the player's
    /// inventory. The menu owns the container's stacks; see [`Menu::container_mut`].
    pub fn generic(container_id: i32, rows: u8) -> Self {
        let rows = rows.clamp(1, 6);
        let mut slots: Vec<SlotDef> = (0..usize::from(rows) * 9)
            .map(|i| SlotDef {
                store: Store::Container(i),
                rule: Rule::Plain,
            })
            .collect();
        standard_inventory_slots(&mut slots);
        Self::with_slots(container_id, MenuKind::Generic { rows }, slots)
    }

    /// A shulker box menu (vanilla `ShulkerBoxMenu`): 27 container slots, each refusing shulker
    /// boxes (vanilla `ShulkerBoxSlot.mayPlace`), followed by the player's inventory.
    pub fn shulker_box(container_id: i32) -> Self {
        let mut slots: Vec<SlotDef> = (0..SHULKER_BOX_SLOTS)
            .map(|i| SlotDef {
                store: Store::Container(i),
                rule: Rule::NoShulkerBox,
            })
            .collect();
        standard_inventory_slots(&mut slots);
        Self::with_slots(container_id, MenuKind::ShulkerBox, slots)
    }

    /// A mount inventory (vanilla `HorseInventoryMenu`): 0 saddle, 1 body armour, then three rows
    /// of `columns` chest slots, then the player's inventory.
    pub fn horse(container_id: i32, columns: u8, llama: bool) -> Self {
        let mut slots = vec![
            SlotDef {
                store: Store::Container(0),
                rule: Rule::MountSaddle { llama },
            },
            SlotDef {
                store: Store::Container(1),
                rule: Rule::MountArmor { llama },
            },
        ];
        for i in 0..usize::from(columns) * 3 {
            slots.push(SlotDef {
                store: Store::Container(2 + i),
                rule: Rule::Plain,
            });
        }
        standard_inventory_slots(&mut slots);
        Self::with_slots(container_id, MenuKind::Horse { columns, llama }, slots)
    }

    /// A furnace-family menu (vanilla `FurnaceMenu`, `BlastFurnaceMenu`, `SmokerMenu`):
    /// 0 input, 1 fuel, 2 result, 3..39 the player's inventory.
    pub fn furnace(container_id: i32, kind: FurnaceKind) -> Self {
        let mut slots = vec![
            SlotDef {
                store: Store::Container(0),
                rule: Rule::FurnaceInput,
            },
            SlotDef {
                store: Store::Container(1),
                rule: Rule::Fuel,
            },
            SlotDef {
                store: Store::Result,
                rule: Rule::Output,
            },
        ];
        standard_inventory_slots(&mut slots);
        Self::with_slots(container_id, MenuKind::Furnace(kind), slots)
    }

    /// An anvil menu (vanilla `AnvilMenu`): 0 and 1 inputs, 2 result, 3..39 the player's
    /// inventory. Container data slot 0 is the level cost.
    pub fn anvil(container_id: i32) -> Self {
        let mut slots = vec![
            SlotDef {
                store: Store::Container(0),
                rule: Rule::Plain,
            },
            SlotDef {
                store: Store::Container(1),
                rule: Rule::Plain,
            },
            SlotDef {
                store: Store::Result,
                rule: Rule::Output,
            },
        ];
        standard_inventory_slots(&mut slots);
        Self::with_slots(container_id, MenuKind::Anvil, slots)
    }

    /// An enchanting table (vanilla `EnchantmentMenu`): 0 item, 1 lapis, 2..38 inventory.
    pub fn enchanting(container_id: i32) -> Self {
        let mut slots = vec![
            SlotDef {
                store: Store::Container(0),
                rule: Rule::EnchantItem,
            },
            SlotDef {
                store: Store::Container(1),
                rule: Rule::Lapis,
            },
        ];
        standard_inventory_slots(&mut slots);
        Self::with_slots(container_id, MenuKind::Enchanting, slots)
    }

    /// A grindstone (vanilla `GrindstoneMenu`): 0 and 1 inputs, 2 result, 3..39 inventory.
    pub fn grindstone(container_id: i32) -> Self {
        let mut slots = vec![
            SlotDef {
                store: Store::Container(0),
                rule: Rule::Plain,
            },
            SlotDef {
                store: Store::Container(1),
                rule: Rule::Plain,
            },
            SlotDef {
                store: Store::Result,
                rule: Rule::Output,
            },
        ];
        standard_inventory_slots(&mut slots);
        Self::with_slots(container_id, MenuKind::Grindstone, slots)
    }

    /// A stonecutter (vanilla `StonecutterMenu`): 0 input, 1 result, 2..38 inventory.
    pub fn stonecutter(container_id: i32) -> Self {
        let mut slots = vec![
            SlotDef {
                store: Store::Container(0),
                rule: Rule::Plain,
            },
            SlotDef {
                store: Store::Result,
                rule: Rule::Output,
            },
        ];
        standard_inventory_slots(&mut slots);
        Self::with_slots(container_id, MenuKind::Stonecutter, slots)
    }

    /// A smithing table (vanilla `SmithingMenu`): 0 template, 1 base, 2 addition, 3 result,
    /// 4..40 inventory.
    pub fn smithing(container_id: i32) -> Self {
        let mut slots: Vec<SlotDef> = (0..3)
            .map(|i| SlotDef {
                store: Store::Container(i),
                rule: Rule::Plain,
            })
            .collect();
        slots.push(SlotDef {
            store: Store::Result,
            rule: Rule::Output,
        });
        standard_inventory_slots(&mut slots);
        Self::with_slots(container_id, MenuKind::Smithing, slots)
    }

    /// A loom (vanilla `LoomMenu`): 0 banner, 1 dye, 2 pattern, 3 result, 4..40 inventory.
    pub fn loom(container_id: i32) -> Self {
        let mut slots = vec![
            SlotDef {
                store: Store::Container(0),
                rule: Rule::Banner,
            },
            SlotDef {
                store: Store::Container(1),
                rule: Rule::Dye,
            },
            SlotDef {
                store: Store::Container(2),
                rule: Rule::BannerPattern,
            },
            SlotDef {
                store: Store::Result,
                rule: Rule::Output,
            },
        ];
        standard_inventory_slots(&mut slots);
        Self::with_slots(container_id, MenuKind::Loom, slots)
    }

    /// A cartography table (vanilla `CartographyTableMenu`): 0 map, 1 addition, 2 result,
    /// 3..39 inventory.
    pub fn cartography(container_id: i32) -> Self {
        let mut slots = vec![
            SlotDef {
                store: Store::Container(0),
                rule: Rule::FilledMap,
            },
            SlotDef {
                store: Store::Container(1),
                rule: Rule::MapAddition,
            },
            SlotDef {
                store: Store::Result,
                rule: Rule::Output,
            },
        ];
        standard_inventory_slots(&mut slots);
        Self::with_slots(container_id, MenuKind::Cartography, slots)
    }

    /// A brewing stand (vanilla `BrewingStandMenu`): `0..3` the bottles, `3` the ingredient,
    /// `4` the fuel and `5..41` the player's inventory.
    pub fn brewing_stand(container_id: i32) -> Self {
        let mut slots = vec![
            SlotDef {
                store: Store::Container(0),
                rule: Rule::Potion,
            },
            SlotDef {
                store: Store::Container(1),
                rule: Rule::Potion,
            },
            SlotDef {
                store: Store::Container(2),
                rule: Rule::Potion,
            },
            SlotDef {
                store: Store::Container(3),
                rule: Rule::BrewingIngredient,
            },
            SlotDef {
                store: Store::Container(4),
                rule: Rule::BrewingFuel,
            },
        ];
        standard_inventory_slots(&mut slots);
        Self::with_slots(container_id, MenuKind::BrewingStand, slots)
    }

    /// A beacon (vanilla `BeaconMenu`): `0` the payment item, `1..37` the player's inventory.
    pub fn beacon(container_id: i32) -> Self {
        let mut slots = vec![SlotDef {
            store: Store::Container(0),
            rule: Rule::BeaconPayment,
        }];
        standard_inventory_slots(&mut slots);
        Self::with_slots(container_id, MenuKind::Beacon, slots)
    }

    /// A villager merchant screen (vanilla `MerchantMenu`): offers `0` and `1` are the inputs,
    /// `2` the result and `3..39` the player's inventory.
    pub fn merchant(container_id: i32, trades: Vec<Trade>) -> Self {
        let mut slots = vec![
            SlotDef {
                store: Store::Container(0),
                rule: Rule::TradeInput,
            },
            SlotDef {
                store: Store::Container(1),
                rule: Rule::TradeInput,
            },
            SlotDef {
                store: Store::Result,
                rule: Rule::TradeOutput,
            },
        ];
        standard_inventory_slots(&mut slots);
        let mut menu = Self::with_slots(container_id, MenuKind::Merchant, slots);
        menu.trades = trades;
        menu
    }

    /// The payment item shown in a beacon menu's slot (vanilla `BeaconMenu`'s container id).
    pub fn beacon_payment(&self) -> &ItemStack {
        self.container.first().unwrap_or(&self.result)
    }

    /// The merchant menu's offers.
    pub fn trades(&self) -> &[Trade] {
        &self.trades
    }

    /// Replaces the offers (after a restock or a villager level-up).
    pub fn set_trades(&mut self, trades: Vec<Trade>) {
        self.trades = trades;
        self.trade = self.trade.min(self.trades.len().saturating_sub(1));
        self.update_merchant();
    }

    /// Vanilla `MerchantMenu.setSelectionHint`: selects an offer and recomputes the result.
    /// Returns whether the index names an offer.
    pub fn select_trade(&mut self, index: usize) -> bool {
        if self.kind != MenuKind::Merchant || index >= self.trades.len() {
            return false;
        }
        self.trade = index;
        self.update_merchant();
        true
    }

    /// The selected offer's index.
    pub fn selected_trade(&self) -> usize {
        self.trade
    }

    /// Vanilla `MerchantMenu.getFutureXp`: villager XP earned since the last call.
    pub fn take_villager_xp(&mut self) -> i32 {
        std::mem::take(&mut self.villager_xp)
    }

    /// Recomputes the merchant result from the selected offer and the two input slots (vanilla
    /// `MerchantMenu.slotsChanged`).
    fn update_merchant(&mut self) {
        self.result = ItemStack::empty();
        let Some(trade) = self.trades.get(self.trade) else {
            return;
        };
        if trade.is_out_of_stock() {
            return;
        }
        if trade.satisfied_by(&self.container[0], &self.container[1]) {
            self.result = trade.gives.clone();
        }
    }

    pub fn container_id(&self) -> i32 {
        self.container_id
    }

    pub fn kind(&self) -> MenuKind {
        self.kind
    }

    /// Number of slots.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn state_id(&self) -> i32 {
        self.state_id
    }

    /// Vanilla `incrementStateId`.
    pub fn increment_state_id(&mut self) -> i32 {
        self.state_id = (self.state_id + 1) & 32767;
        self.state_id
    }

    /// The stack on the cursor.
    pub fn carried(&self) -> &ItemStack {
        &self.carried
    }

    pub fn set_carried(&mut self, stack: ItemStack) {
        self.carried = normalize(stack);
    }

    /// The crafting grid, row-major.
    pub fn grid(&self) -> &[ItemStack] {
        &self.grid
    }

    /// The crafting result slot.
    pub fn result(&self) -> &ItemStack {
        &self.result
    }

    /// The generic container's own stacks.
    pub fn container(&self) -> &[ItemStack] {
        &self.container
    }

    pub fn container_mut(&mut self) -> &mut [ItemStack] {
        &mut self.container
    }

    /// The menu's container data (vanilla `AbstractContainerMenu.dataSlots`).
    pub fn data(&self) -> &[i32] {
        &self.data
    }

    /// One container data value, or 0 when the menu has no such slot.
    pub fn data_value(&self, id: usize) -> i32 {
        self.data.get(id).copied().unwrap_or(0)
    }

    /// The furnace timers, for a furnace-family menu.
    pub fn furnace_state(&self) -> Option<&FurnaceState> {
        self.furnace.as_ref()
    }

    /// Restores the furnace timers when the menu is opened from a block entity.
    pub fn set_furnace_state(&mut self, state: FurnaceState) {
        self.furnace = Some(state);
        self.sync_data();
    }

    /// The brewing stand timers, for a brewing menu.
    pub fn brewing_state(&self) -> Option<&BrewingState> {
        self.brewing.as_ref()
    }

    /// Restores the brewing stand timers when the menu is opened from a block entity.
    pub fn set_brewing_state(&mut self, state: BrewingState) {
        self.brewing = Some(state);
        self.sync_data();
    }

    /// The beacon's pyramid level and chosen effects, for a beacon menu.
    pub fn beacon_state(&self) -> Option<&BeaconState> {
        self.beacon.as_ref()
    }

    /// Restores a beacon's saved state when the menu is opened from a block entity.
    pub fn set_beacon_state(&mut self, state: BeaconState) {
        self.beacon = Some(state);
        self.sync_data();
    }

    /// Vanilla `BeaconMenu.updateEffects`: stores the chosen effects and spends one payment item.
    /// The server checks the effects the beacon's level allows before calling this.
    pub fn confirm_beacon(&mut self, primary: Option<i32>, secondary: Option<i32>) -> bool {
        if self.kind != MenuKind::Beacon {
            return false;
        }
        let levels = self.beacon.as_ref().map_or(0, |state| state.levels);
        if levels <= 0 || (primary.is_none() && secondary.is_none()) {
            return false;
        }
        let mut payment = self.container[0].clone();
        if !is_beacon_payment(&payment) {
            return false;
        }
        payment.count -= 1;
        self.container[0] = normalize(payment);
        self.beacon = Some(BeaconState {
            levels,
            primary,
            secondary,
        });
        self.sync_data();
        true
    }

    /// Copies the menu's properties into its container data slots (vanilla
    /// `AbstractContainerMenu.broadcastChanges` calling `setData`).
    fn sync_data(&mut self) {
        match self.kind {
            MenuKind::Furnace(_) => {
                let Some(furnace) = &self.furnace else {
                    return;
                };
                if self.data.len() < FURNACE_DATA_SLOTS {
                    self.data.resize(FURNACE_DATA_SLOTS, 0);
                    self.remote_data.resize(FURNACE_DATA_SLOTS, i32::MIN);
                }
                self.data[FURNACE_LIT] = i32::from(furnace.lit());
                self.data[FURNACE_COOK_PROGRESS] = furnace.cook;
                self.data[FURNACE_COOK_TOTAL] = furnace.cook_total;
                self.data[FURNACE_BURN_LEFT] = furnace.burn;
                self.data[FURNACE_BURN_TOTAL] = furnace.burn_total;
            }
            MenuKind::Anvil => {
                if self.data.is_empty() {
                    self.data.resize(1, 0);
                    self.remote_data.resize(1, i32::MIN);
                }
                self.data[0] = self.anvil_cost;
            }
            MenuKind::Enchanting => {
                if self.data.len() < 10 {
                    self.data.resize(10, 0);
                    self.remote_data.resize(10, i32::MIN);
                }
                self.refresh_offers();
                let state = &self.enchanting;
                for (i, offer) in state.offers.iter().enumerate() {
                    self.data[i] = offer.cost;
                    self.data[4 + i] = offer.clue;
                    self.data[7 + i] = offer.clue_level;
                }
                self.data[3] = state.seed;
            }
            MenuKind::Stonecutter => {
                if self.data.is_empty() {
                    self.data.resize(1, 0);
                    self.remote_data.resize(1, i32::MIN);
                }
                self.data[0] = self.stonecutting_index as i32;
            }
            MenuKind::Loom => {
                if self.data.is_empty() {
                    self.data.resize(1, 0);
                    self.remote_data.resize(1, i32::MIN);
                }
                self.data[0] = self.loom_pattern as i32;
            }
            MenuKind::BrewingStand => {
                if self.data.len() < BREWING_DATA_SLOTS {
                    self.data.resize(BREWING_DATA_SLOTS, 0);
                    self.remote_data.resize(BREWING_DATA_SLOTS, i32::MIN);
                }
                let state = self.brewing.clone().unwrap_or_default();
                self.data[BREWING_BREW_TIME] = state.brew_time;
                self.data[BREWING_FUEL] = state.fuel;
            }
            MenuKind::Beacon => {
                if self.data.len() < BEACON_DATA_SLOTS {
                    self.data.resize(BEACON_DATA_SLOTS, 0);
                    self.remote_data.resize(BEACON_DATA_SLOTS, i32::MIN);
                }
                let state = self.beacon.clone().unwrap_or_default();
                self.data[BEACON_LEVELS] = state.levels;
                self.data[BEACON_PRIMARY] = state.primary.map_or(0, |id| id + 1);
                self.data[BEACON_SECONDARY] = state.secondary.map_or(0, |id| id + 1);
            }
            _ => {}
        }
    }

    /// The anvil's experience level cost (vanilla `AnvilMenu.getCost`).
    pub fn anvil_cost(&self) -> i32 {
        self.anvil_cost
    }

    /// Recomputes a station menu's result from its inputs. Crafting result slots are refreshed
    /// by [`Menu::update_result`] instead.
    fn refresh_station_result(&mut self, crafter: &dyn Crafter, maps: &dyn MapAccess) {
        match self.kind {
            MenuKind::Anvil => self.update_anvil(crafter),
            MenuKind::Grindstone => self.update_grindstone(),
            MenuKind::Stonecutter => {
                self.stonecutting = if self.container[0].is_empty() {
                    Vec::new()
                } else {
                    crafter.stonecutting(&self.container[0])
                };
                if self.stonecutting_index >= self.stonecutting.len() {
                    self.stonecutting_index = 0;
                }
                self.result = self
                    .stonecutting
                    .get(self.stonecutting_index)
                    .cloned()
                    .unwrap_or_default();
                self.sync_data();
            }
            MenuKind::Smithing => {
                self.result = crafter
                    .smithing(&self.container[0], &self.container[1], &self.container[2])
                    .unwrap_or_default();
                self.sync_data();
            }
            MenuKind::Loom => self.update_loom(),
            MenuKind::Cartography => self.update_cartography(maps),
            MenuKind::Merchant => self.update_merchant(),
            _ => {}
        }
    }

    fn update_grindstone(&mut self) {
        let left = self.container[0].clone();
        let right = self.container[1].clone();
        self.result = ItemStack::empty();
        self.station_xp = 0.0;
        if left.is_empty() && right.is_empty() {
            self.sync_data();
            return;
        }
        if !left.is_empty() && !right.is_empty() && !left.is_same_item(&right) {
            self.sync_data();
            return;
        }
        let mut out = if left.is_empty() {
            right.clone()
        } else {
            left.clone()
        };
        let mut xp = 0.0;
        for stack in [&left, &right] {
            if stack.effective_component(ids::ENCHANTMENTS).is_some()
                || stack
                    .effective_component(ids::STORED_ENCHANTMENTS)
                    .is_some()
            {
                xp += 2.0;
            }
        }
        out.components.remove(ids::ENCHANTMENTS);
        out.components.remove(ids::STORED_ENCHANTMENTS);
        if !left.is_empty() && !right.is_empty() && left.is_damageable() {
            let max = left.max_damage().unwrap_or(0);
            let total = ((max - left.damage()) + (max - right.damage())).min(max);
            out.components
                .insert(ids::DAMAGE, Component::Damage(max - total));
        }
        out.count = 1;
        self.station_xp = xp;
        self.result = out;
        self.sync_data();
    }

    /// Vanilla `CartographyTableMenu.setupResultSlot`: paper zooms out an unlocked map below the
    /// largest scale, a glass pane locks an unlocked map and an empty map clones. Zooming out and
    /// locking only mark the result; the new saved map is created when the player takes it.
    fn update_cartography(&mut self, maps: &dyn MapAccess) {
        use ferrite_item::{MapPostProcessing, set_map_post_processing};
        self.result = ItemStack::empty();
        let map = self.container[0].clone();
        let addition = self.container[1].clone();
        if let Some(id) = ferrite_item::map_id(&map)
            && !addition.is_empty()
            && let Some(info) = maps.info(id)
        {
            match addition.name() {
                Some("minecraft:paper") if !info.locked && info.scale < 4 => {
                    let mut out = map.copy_with_count(1);
                    set_map_post_processing(&mut out, Some(MapPostProcessing::Scale));
                    self.result = out;
                }
                Some("minecraft:glass_pane") if !info.locked => {
                    let mut out = map.copy_with_count(1);
                    set_map_post_processing(&mut out, Some(MapPostProcessing::Lock));
                    self.result = out;
                }
                Some("minecraft:map") => self.result = map.copy_with_count(2),
                _ => {}
            }
        }
        self.sync_data();
    }

    fn update_loom(&mut self) {
        self.result = ItemStack::empty();
        let banner = &self.container[0];
        let dye = &self.container[1];
        if !banner.is_empty() && !dye.is_empty() {
            let color = dye.name().and_then(|name| {
                name.strip_prefix("minecraft:")
                    .and_then(|name| name.strip_suffix("_dye"))
            });
            if let Some(color) = color {
                let target = format!("minecraft:{color}_banner");
                if banner.name() != Some(target.as_str())
                    && let Some(mut out) = ItemStack::from_name(&target)
                {
                    out.components = banner.components.clone();
                    self.result = out;
                }
            }
        }
        self.sync_data();
    }

    /// Points the menu at the data behind an enchanting table: the power blocks around it and
    /// the player's enchantment seed (vanilla `EnchantmentMenu.enchantmentSeed`).
    pub fn set_enchanting(
        &mut self,
        table: std::sync::Arc<dyn EnchantTable>,
        bookshelves: i32,
        seed: i32,
    ) {
        self.enchanting.table = Some(table);
        self.enchanting.bookshelves = bookshelves;
        self.enchanting.seed = seed;
        self.sync_data();
    }

    /// The shelves around the table, counted again by the caller before each click (vanilla
    /// counts them in `slotsChanged`); the click's own sync refreshes the buttons.
    pub fn set_bookshelves(&mut self, bookshelves: i32) {
        self.enchanting.bookshelves = bookshelves;
    }

    /// The seed the menu offers from; it changes after every enchant (vanilla
    /// `Player.onEnchantmentPerformed`) and the player keeps it for the next table.
    pub fn enchanting_seed(&self) -> i32 {
        self.enchanting.seed
    }

    /// Vanilla `EnchantmentMenu.slotsChanged`: no button without an enchantable item.
    fn refresh_offers(&mut self) {
        let none = EnchantOffer {
            cost: 0,
            clue: -1,
            clue_level: -1,
        };
        let item = &self.container[0];
        self.enchanting.offers = match &self.enchanting.table {
            Some(table) if !item.is_empty() && enchantable(item) => {
                table.offers(item, self.enchanting.bookshelves, self.enchanting.seed)
            }
            _ => [none; 3],
        };
    }

    /// Vanilla `EnchantmentMenu.clickMenuButton`: applies button `choice`'s enchantments and
    /// consumes `choice + 1` lapis. The caller has checked the player's levels against
    /// [`Menu::enchant_cost`]. Returns the levels to spend (`choice + 1`, not the displayed cost)
    /// and sets a fresh seed from `next_seed`.
    fn enchant(&mut self, choice: i32, next_seed: i32) -> Option<i32> {
        if !(0..3).contains(&choice) {
            return None;
        }
        let slot = choice as usize;
        let item = self.container[0].clone();
        let cost = self.enchanting.offers[slot].cost;
        let needed = choice + 1;
        if item.is_empty() || cost <= 0 {
            return None;
        }
        if !self.infinite_materials && self.container[1].count < needed {
            return None;
        }
        let table = self.enchanting.table.clone()?;
        let list = table.enchantments(&item, slot, cost, self.enchanting.seed);
        if list.is_empty() {
            return None;
        }
        let mut out = item;
        if out.name() == Some("minecraft:book") {
            out = ItemStack {
                item: ferrite_item::item_id("minecraft:enchanted_book")?,
                ..out
            };
        }
        let stored = out.name() == Some("minecraft:enchanted_book");
        let id = if stored {
            ids::STORED_ENCHANTMENTS
        } else {
            ids::ENCHANTMENTS
        };
        let mut enchantments = match out.effective_component(id) {
            Some(Component::Enchantments(e)) | Some(Component::StoredEnchantments(e)) => e,
            _ => Enchantments::new(),
        };
        for (name, level) in list {
            let keep = enchantments.level(&name).unwrap_or(0).max(level);
            enchantments.set(&name, keep);
        }
        out.components.insert(
            id,
            if stored {
                Component::StoredEnchantments(enchantments)
            } else {
                Component::Enchantments(enchantments)
            },
        );
        self.container[0] = out;
        if !self.infinite_materials {
            let mut lapis = self.container[1].clone();
            lapis.count -= needed;
            self.container[1] = normalize(lapis);
        }
        self.enchanting.seed = next_seed;
        self.sync_data();
        Some(needed)
    }

    /// Vanilla `AnvilMenu.createResult`: renames, combines two damaged copies of one item, or
    /// repairs with the item's repair material, charging the two prior-work penalties and one
    /// level per operation. Enchantments of the second slot (an enchanted book, or an item of the
    /// same kind) merge into the first: equal levels combine to one more, the cap is the
    /// enchantment's maximum, and an enchantment the item does not support or that excludes one
    /// already present is left out (and makes the result empty when nothing else applied).
    /// Whether the viewer has infinite materials (creative mode), which lifts the anvil's level cap.
    pub fn set_infinite_materials(&mut self, infinite: bool) {
        self.infinite_materials = infinite;
    }

    fn update_anvil(&mut self, crafter: &dyn Crafter) {
        let left = self.container[0].clone();
        let right = self.container[1].clone();
        self.result = ItemStack::empty();
        self.anvil_cost = 0;
        if left.is_empty() {
            self.sync_data();
            return;
        }
        let mut result = left.clone();
        let base_cost = repair_cost(&left) + repair_cost(&right);
        let mut cost = base_cost;
        let mut rename_cost = 0;
        let mut changed = false;

        let current_name = custom_name(&left).unwrap_or_default();
        let wanted = self.rename.trim();
        if wanted != current_name {
            if wanted.is_empty() {
                result.components.remove(ids::CUSTOM_NAME);
            } else {
                result
                    .components
                    .insert_opaque(ids::CUSTOM_NAME, Tag::String(wanted.into()));
            }
            cost += 1;
            rename_cost = 1;
            changed = true;
        }

        if !right.is_empty() {
            let right_is_book = matches!(
                right.effective_component(ids::STORED_ENCHANTMENTS),
                Some(Component::StoredEnchantments(_))
            );
            let same_damageable =
                left.is_same_item(&right) && left.is_damageable() && right.is_damageable();
            if !right_is_book && !same_damageable && repairs_with(&left, &right) {
                let max = left.max_damage().unwrap_or(0);
                let per_item = (max / 4).max(1);
                let used = right.count.min(4);
                let repaired = (per_item * used).min(left.damage());
                if repaired > 0 {
                    result
                        .components
                        .insert(ids::DAMAGE, Component::Damage(left.damage() - repaired));
                    cost += used;
                    changed = true;
                }
            } else if right_is_book || same_damageable {
                if same_damageable && !right_is_book {
                    let max = left.max_damage().unwrap_or(0);
                    let left_durability = max - left.damage();
                    let right_durability = max - right.damage();
                    // Vanilla adds 12% of the maximum durability to a combined repair.
                    let total = (left_durability + right_durability + max * 12 / 100).min(max);
                    // Vanilla only charges when the combination actually repairs something.
                    if total > 0 && max - total < left.damage() {
                        result
                            .components
                            .insert(ids::DAMAGE, Component::Damage(max - total));
                        cost += 2;
                        changed = true;
                    }
                }
                let (merged, enchant_cost, applied, refused) =
                    merge_enchantments(&left, &right, right_is_book, crafter);
                if applied {
                    cost += enchant_cost;
                    changed = true;
                    if left.name() == Some("minecraft:enchanted_book") {
                        result.components.insert(
                            ids::STORED_ENCHANTMENTS,
                            Component::StoredEnchantments(merged),
                        );
                    } else {
                        result
                            .components
                            .insert(ids::ENCHANTMENTS, Component::Enchantments(merged));
                    }
                    if left.count > 1 {
                        cost = 40;
                    }
                } else if refused {
                    // Every enchantment of the second slot was refused: no result at all.
                    self.sync_data();
                    return;
                }
            } else {
                // Vanilla: a second item that neither repairs nor enchants gives no result.
                self.sync_data();
                return;
            }
        }

        // Vanilla caps a rename-only result at 39 levels; any other result at 40 or more is refused
        // unless the viewer has infinite materials.
        let work = cost - base_cost;
        let rename_only = work > 0 && rename_cost == work;
        if rename_only && cost >= 40 {
            cost = 39;
        }
        if cost >= 40 && !self.infinite_materials {
            self.anvil_cost = 40;
            self.sync_data();
            return;
        }
        if changed {
            let mut penalty = repair_cost(&left).max(repair_cost(&right));
            // Vanilla leaves the repair cost alone when the only work is a rename.
            if !rename_only {
                penalty = penalty * 2 + 1;
            }
            result
                .components
                .insert(ids::REPAIR_COST, Component::RepairCost(penalty));
            self.anvil_cost = cost.max(1);
            self.result = result;
        }
        self.sync_data();
    }

    /// Advances the furnace timers by one tick (vanilla `AbstractFurnaceBlockEntity.serverTick`),
    /// then refreshes the container data. Other menus do nothing.
    pub fn tick(&mut self, crafter: &dyn Crafter) {
        match self.kind {
            MenuKind::Furnace(kind) => self.tick_furnace(kind, crafter),
            MenuKind::BrewingStand => self.tick_brewing(crafter),
            _ => {}
        }
        self.sync_data();
    }

    fn tick_furnace(&mut self, kind: FurnaceKind, crafter: &dyn Crafter) {
        let input = self.container[0].clone();
        let fuel = self.container[1].clone();
        let recipe = if input.is_empty() {
            None
        } else {
            crafter.cook(kind, &input)
        };
        let can_cook = recipe.as_ref().is_some_and(|recipe| {
            self.result.is_empty()
                || (self.result.is_same_item_same_components(&recipe.result)
                    && self.result.count + recipe.result.count <= self.result.max_stack_size())
        });
        let mut state = self.furnace.clone().unwrap_or_default();
        if state.burn > 0 {
            state.burn -= 1;
        }
        if can_cook {
            if state.burn == 0 {
                let burn = crafter.burn_time(&fuel);
                if burn > 0 {
                    state.burn = burn;
                    state.burn_total = burn;
                    let mut left = fuel;
                    left.count -= 1;
                    self.container[1] = normalize(left);
                }
            }
            if state.burn > 0 {
                let recipe = recipe.expect("can_cook implies a recipe");
                if state.cook == 0 {
                    state.cook_total = recipe.cook_time.max(1);
                }
                state.cook += 1;
                if state.cook >= state.cook_total {
                    if self.result.is_empty() {
                        self.result = recipe.result.clone();
                    } else {
                        self.result.count += recipe.result.count;
                    }
                    let mut left = self.container[0].clone();
                    left.count -= 1;
                    self.container[0] = normalize(left);
                    state.pending_experience += recipe.experience;
                    state.cook = 0;
                    state.cook_total = 0;
                }
            } else {
                state.cook = 0;
                state.cook_total = 0;
            }
        } else {
            state.cook = 0;
            state.cook_total = 0;
        }
        self.furnace = Some(state);
    }

    /// Advances a brewing stand by one tick (vanilla `BrewingStandBlockEntity.serverTick`).
    fn tick_brewing(&mut self, crafter: &dyn Crafter) {
        let mut state = self.brewing.clone().unwrap_or_default();
        // A blaze powder in the fuel slot tops the stand up to twenty brews.
        if state.fuel <= 0 && self.container[4].name() == Some("minecraft:blaze_powder") {
            state.fuel = FUEL_PER_BLAZE_POWDER;
            let mut fuel = self.container[4].clone();
            fuel.count -= 1;
            self.container[4] = normalize(fuel);
        }
        let brewable = self.is_brewable(crafter);
        if state.brew_time > 0 {
            state.brew_time -= 1;
            if state.brew_time == 0 {
                if brewable {
                    self.do_brew(crafter);
                }
            } else if !brewable {
                state.brew_time = 0;
            }
        } else if brewable && state.fuel > 0 {
            state.fuel -= 1;
            state.brew_time = BREW_TIME;
        }
        self.brewing = Some(state);
    }

    /// Vanilla `BrewingStandBlockEntity.isBrewable`: the ingredient mixes with at least one of
    /// the three bottle slots.
    fn is_brewable(&self, crafter: &dyn Crafter) -> bool {
        let ingredient = &self.container[3];
        if ingredient.is_empty() {
            return false;
        }
        (0..3).any(|slot| crafter.brew(ingredient, &self.container[slot]).is_some())
    }

    /// Vanilla `BrewingStandBlockEntity.doBrew`: every bottle that mixes changes, then the
    /// ingredient is used up.
    fn do_brew(&mut self, crafter: &dyn Crafter) {
        let ingredient = self.container[3].clone();
        for slot in self.container.iter_mut().take(3) {
            if let Some(result) = crafter.brew(&ingredient, slot) {
                *slot = normalize(result);
            }
        }
        let mut left = ingredient;
        left.count -= 1;
        self.container[3] = normalize(left);
    }

    /// The stack in a menu slot.
    pub fn slot<'s>(&'s self, inventory: &'s PlayerInventory, slot: usize) -> &'s ItemStack {
        match self.slots[slot].store {
            Store::Inventory(i) => inventory.get(i),
            Store::Grid(i) => &self.grid[i],
            Store::Result => &self.result,
            Store::Container(i) => &self.container[i],
        }
    }

    /// Every slot's stack, in menu order.
    pub fn items(&self, inventory: &PlayerInventory) -> Vec<ItemStack> {
        (0..self.slots.len())
            .map(|slot| self.slot(inventory, slot).clone())
            .collect()
    }

    /// The menu slot showing an inventory slot, if this menu shows it.
    pub fn menu_slot_of(&self, inventory_slot: usize) -> Option<usize> {
        self.slots
            .iter()
            .position(|def| def.store == Store::Inventory(inventory_slot))
    }

    /// Writes a menu slot directly (vanilla `Slot.setByPlayer`, as the creative inventory does),
    /// recomputing the crafting result when the grid changes. Out-of-range slots are ignored.
    pub fn set_slot(
        &mut self,
        inventory: &mut PlayerInventory,
        crafter: &dyn Crafter,
        slot: usize,
        stack: ItemStack,
    ) {
        if slot < self.slots.len() {
            let mut p = Clicker::new(inventory, true, crafter);
            self.put(&mut p, slot, stack);
        }
    }

    /// [`Menu::set_slot`] for a menu whose result depends on saved maps (the cartography table).
    pub fn set_slot_with_maps(
        &mut self,
        inventory: &mut PlayerInventory,
        crafter: &dyn Crafter,
        maps: &dyn MapAccess,
        slot: usize,
        stack: ItemStack,
    ) {
        if slot < self.slots.len() {
            let mut p = Clicker::new(inventory, true, crafter).with_maps(maps);
            self.put(&mut p, slot, stack);
        }
    }

    /// Vanilla `ServerPlaceRecipe`: moves the crafting grid back to the inventory, then fills each
    /// cell of `layout` (row-major, one entry per grid cell) with items from the inventory, one
    /// craft's worth, or as many crafts as the stacks allow when `max` is set. Returns whether the
    /// grid was filled; otherwise nothing changes.
    pub fn place_recipe(
        &mut self,
        inventory: &mut PlayerInventory,
        crafter: &dyn Crafter,
        layout: &[Option<Vec<String>>],
        max: bool,
    ) -> bool {
        if self.grid_width == 0 || layout.len() != self.grid.len() {
            return false;
        }
        let (saved_inventory, saved_grid) = (inventory.clone(), self.grid.clone());
        let mut placed = false;
        'fill: {
            for cell in 0..self.grid.len() {
                let stack = std::mem::take(&mut self.grid[cell]);
                if inventory.place_back(stack).is_some() {
                    break 'fill;
                }
            }
            let key = |name: &str| name.strip_prefix("minecraft:").unwrap_or(name).to_owned();
            for pass in 0..if max { 64 } else { 1 } {
                let (pass_inventory, pass_grid) = (inventory.clone(), self.grid.clone());
                let mut filled = true;
                for (cell, wanted) in layout.iter().enumerate() {
                    let Some(wanted) = wanted else { continue };
                    let have = &self.grid[cell];
                    let source = (0..crate::inventory::ITEMS).find(|&slot| {
                        let stack = inventory.get(slot);
                        !stack.is_empty()
                            && stack
                                .name()
                                .is_some_and(|name| wanted.iter().any(|w| key(w) == key(name)))
                            && (have.is_empty()
                                || (have.is_same_item_same_components(stack)
                                    && have.count < have.max_stack_size()))
                    });
                    let Some(source) = source else {
                        filled = false;
                        break;
                    };
                    let taken = inventory.remove(source, 1);
                    if self.grid[cell].is_empty() {
                        self.grid[cell] = taken;
                    } else {
                        self.grid[cell].count += 1;
                    }
                }
                if !filled {
                    *inventory = pass_inventory;
                    self.grid = pass_grid;
                    placed = pass > 0;
                    break 'fill;
                }
                placed = true;
            }
        }
        if !placed {
            *inventory = saved_inventory;
            self.grid = saved_grid;
            return false;
        }
        self.refresh_result(crafter);
        true
    }

    /// Vanilla `isValidSlotIndex`.
    pub fn is_valid_slot_index(&self, slot: i16) -> bool {
        slot == -1 || slot == SLOT_OUTSIDE || (slot >= 0 && (slot as usize) < self.slots.len())
    }

    fn stack(&self, p: &Clicker, slot: usize) -> ItemStack {
        self.slot(p.inventory, slot).clone()
    }

    /// Vanilla `Slot.setByPlayer`; a changed crafting grid recomputes the result
    /// (`CraftingMenu.slotChangedCraftingGrid`).
    fn put(&mut self, p: &mut Clicker, slot: usize, stack: ItemStack) {
        let stack = normalize(stack);
        match self.slots[slot].store {
            Store::Inventory(i) => p.inventory.set(i, stack),
            Store::Grid(i) => {
                self.grid[i] = stack;
                self.update_result(p);
            }
            Store::Result => self.result = stack,
            Store::Container(i) => {
                self.container[i] = stack;
                self.refresh_station_result(p.crafter, p.maps);
            }
        }
    }

    fn update_result(&mut self, p: &Clicker) {
        self.result = normalize(p.crafter.craft(&self.grid, self.grid_width));
    }

    /// Recomputes the crafting result from the grid.
    pub fn refresh_result(&mut self, crafter: &dyn Crafter) {
        if self.grid_width > 0 {
            self.result = normalize(crafter.craft(&self.grid, self.grid_width));
        }
    }

    /// `Slot.mayPlace`: nothing goes into a result slot, armor slots take matching equipment.
    fn may_place(&self, slot: usize, stack: &ItemStack) -> bool {
        match self.slots[slot].rule {
            Rule::Plain => true,
            Rule::NoShulkerBox => !is_shulker_box(stack),
            Rule::Armor(name) => equipment_slot(stack) == name,
            Rule::Result | Rule::Output => false,
            Rule::MountSaddle { llama } => !llama && stack.name() == Some("minecraft:saddle"),
            Rule::MountArmor { llama } => stack.name().is_some_and(|name| {
                if llama {
                    name.ends_with("_carpet")
                } else {
                    name.ends_with("_horse_armor")
                }
            }),
            // Vanilla furnace input and fuel slots accept any stack; the recipe and fuel checks
            // happen when the block ticks, as in `AbstractFurnaceBlockEntity`.
            Rule::FurnaceInput | Rule::Fuel => true,
            Rule::EnchantItem => stack.max_stack_size() == 1,
            Rule::Lapis => stack.name() == Some("minecraft:lapis_lazuli"),
            Rule::Banner => stack.name().is_some_and(|name| name.ends_with("_banner")),
            Rule::Dye => stack.name().is_some_and(|name| name.ends_with("_dye")),
            Rule::BannerPattern => stack
                .name()
                .is_some_and(|name| name.ends_with("_banner_pattern")),
            // Vanilla merchant input slots take any stack; the selected offer decides what the
            // result slot shows. The result slot itself only ever gives items out.
            Rule::TradeInput => true,
            Rule::TradeOutput => false,
            Rule::FilledMap => ferrite_item::map_id(stack).is_some(),
            Rule::MapAddition => matches!(
                stack.name(),
                Some("minecraft:paper" | "minecraft:map" | "minecraft:glass_pane")
            ),
            Rule::Potion => is_potion(stack) || stack.name() == Some("minecraft:glass_bottle"),
            Rule::BrewingIngredient => true,
            Rule::BrewingFuel => stack.name() == Some("minecraft:blaze_powder"),
            Rule::BeaconPayment => is_beacon_payment(stack),
        }
    }

    /// `Slot.mayPickup`: `ArmorSlot` keeps cursed armor on outside creative.
    fn may_pickup(&self, p: &Clicker, slot: usize) -> bool {
        match self.slots[slot].rule {
            Rule::Armor(_) => {
                let item = self.slot(p.inventory, slot);
                item.is_empty() || p.infinite_materials || !prevents_armor_change(item)
            }
            _ => true,
        }
    }

    /// `Slot.allowModification`.
    fn allow_modification(&self, p: &Clicker, slot: usize) -> bool {
        self.may_pickup(p, slot) && self.may_place(slot, self.slot(p.inventory, slot))
    }

    /// `Slot.getMaxStackSize(stack)`: armor, potion and beacon payment slots hold one item.
    fn slot_max(&self, slot: usize, stack: &ItemStack) -> i32 {
        match self.slots[slot].rule {
            Rule::Armor(_)
            | Rule::Potion
            | Rule::BeaconPayment
            | Rule::MountSaddle { .. }
            | Rule::MountArmor { .. } => 1.min(max_in_slot(stack)),
            _ => max_in_slot(stack),
        }
    }

    /// `CraftingMenu.canDragTo` / `canTakeItemForPickAll`: never the result slot.
    fn is_result(&self, slot: usize) -> bool {
        matches!(self.slots[slot].rule, Rule::Result | Rule::TradeOutput)
    }

    /// `Slot.remove`; a `ResultContainer` always gives up its whole stack.
    fn remove(&mut self, p: &mut Clicker, slot: usize, amount: i32) -> ItemStack {
        if self.slots[slot].rule == Rule::Result {
            return std::mem::take(&mut self.result);
        }
        if self.kind == MenuKind::Cartography && self.slots[slot].rule == Rule::Output {
            // A `ResultContainer` gives up its whole stack. The saved map for a zoomed-out or
            // locked result is created now, and a store failure refuses the take.
            let mut out = self.result.clone();
            if p.maps.finish(&mut out).is_err() {
                return ItemStack::empty();
            }
            self.result = ItemStack::empty();
            return out;
        }
        let mut stack = self.stack(p, slot);
        let taken = stack.split(amount).unwrap_or_default();
        if !taken.is_empty() {
            self.put(p, slot, stack);
        }
        taken
    }

    /// `Slot.tryRemove`.
    fn try_remove(
        &mut self,
        p: &mut Clicker,
        slot: usize,
        amount: i32,
        decrement: i32,
    ) -> Option<ItemStack> {
        if !self.may_pickup(p, slot) {
            return None;
        }
        if !self.allow_modification(p, slot) && decrement < self.slot(p.inventory, slot).count {
            return None;
        }
        let taken = self.remove(p, slot, amount.min(decrement));
        (!taken.is_empty()).then_some(taken)
    }

    /// `Slot.safeTake`.
    fn safe_take(
        &mut self,
        p: &mut Clicker,
        slot: usize,
        amount: i32,
        decrement: i32,
    ) -> ItemStack {
        match self.try_remove(p, slot, amount, decrement) {
            Some(taken) => {
                self.on_take(p, slot);
                taken
            }
            None => ItemStack::empty(),
        }
    }

    /// `Slot.safeInsert`: moves up to `count` items of `stack` into the slot and returns the
    /// rest.
    fn safe_insert(
        &mut self,
        p: &mut Clicker,
        slot: usize,
        mut stack: ItemStack,
        count: i32,
    ) -> ItemStack {
        if stack.is_empty() || !self.may_place(slot, &stack) {
            return stack;
        }
        let existing = self.stack(p, slot);
        let moved = count
            .min(stack.count)
            .min(self.slot_max(slot, &stack) - existing.count);
        if moved <= 0 {
            return stack;
        }
        if existing.is_empty() {
            let part = stack.split(moved).unwrap_or_default();
            self.put(p, slot, part);
        } else if existing.is_same_item_same_components(&stack) {
            stack.count -= moved;
            let grown = existing.copy_with_count(existing.count + moved);
            self.put(p, slot, grown);
        }
        normalize(stack)
    }

    /// `Slot.onTake`; for a `ResultSlot` this consumes one of each ingredient and leaves the
    /// crafting remainders (vanilla `ResultSlot.onTake`).
    fn on_take(&mut self, p: &mut Clicker, slot: usize) {
        if matches!(self.slots[slot].rule, Rule::Output | Rule::TradeOutput) {
            match self.kind {
                MenuKind::Furnace(_) => {
                    if let Some(furnace) = &mut self.furnace {
                        p.xp += furnace.pending_experience;
                        furnace.pending_experience = 0.0;
                    }
                }
                MenuKind::Anvil => {
                    p.levels += self.anvil_cost;
                    for cell in 0..self.container.len() {
                        let mut left = self.container[cell].clone();
                        left.count -= 1;
                        self.container[cell] = normalize(left);
                    }
                    self.result = ItemStack::empty();
                    self.anvil_cost = 0;
                }
                MenuKind::Grindstone => {
                    for cell in self.container.iter_mut().take(2) {
                        let mut left = cell.clone();
                        left.count -= 1;
                        *cell = normalize(left);
                    }
                    self.result = ItemStack::empty();
                    p.xp += self.station_xp;
                    self.station_xp = 0.0;
                }
                MenuKind::Stonecutter => {
                    let mut left = self.container[0].clone();
                    left.count -= 1;
                    self.container[0] = normalize(left);
                    self.result = ItemStack::empty();
                }
                MenuKind::Smithing | MenuKind::Loom => {
                    if self.kind == MenuKind::Smithing
                        && let Some(id) = p.crafter.smithing_recipe_id(
                            &self.container[0],
                            &self.container[1],
                            &self.container[2],
                        )
                    {
                        p.crafted.push(id);
                    }
                    for cell in self.container.iter_mut().take(3) {
                        let mut left = cell.clone();
                        left.count -= 1;
                        *cell = normalize(left);
                    }
                    self.result = ItemStack::empty();
                }
                MenuKind::Cartography => {
                    for cell in self.container.iter_mut().take(2) {
                        let mut left = cell.clone();
                        left.count -= 1;
                        *cell = normalize(left);
                    }
                    self.result = ItemStack::empty();
                    self.update_cartography(p.maps);
                }
                MenuKind::Merchant => {
                    let Some(trade) = self.trades.get_mut(self.trade) else {
                        return;
                    };
                    let price = trade.price();
                    let second = trade.second_wants.clone();
                    let xp = trade.xp;
                    let mut first = self.container[0].clone();
                    first.count -= price;
                    self.container[0] = normalize(first);
                    if let Some(cost) = second {
                        let mut other = self.container[1].clone();
                        other.count -= cost.count;
                        self.container[1] = normalize(other);
                    }
                    trade.on_sale();
                    p.xp += xp as f32;
                    self.villager_xp += xp;
                    self.result = ItemStack::empty();
                    self.update_merchant();
                }
                _ => {}
            }
            self.sync_data();
            return;
        }
        if self.slots[slot].rule != Rule::Result {
            return;
        }
        if let Some(id) = p.crafter.recipe_id(&self.grid, self.grid_width) {
            p.crafted.push(id);
        }
        for cell in 0..self.grid.len() {
            let ingredient = self.grid[cell].clone();
            if ingredient.is_empty() {
                continue;
            }
            let mut left = ingredient.copy_with_count(ingredient.count - 1);
            if let Some(mut remainder) = craft_remainder(ingredient.item) {
                if left.is_empty() {
                    left = remainder;
                } else if left.is_same_item_same_components(&remainder) {
                    left.count += remainder.count;
                } else {
                    // Vanilla drops a remainder the inventory cannot take.
                    p.inventory.add(&mut remainder, p.infinite_materials);
                    p.drop(remainder);
                }
            }
            self.grid[cell] = normalize(left);
        }
        self.update_result(p);
    }

    /// Vanilla `canItemQuickReplace(slot, stack, true)`.
    fn can_quick_replace(&self, p: &Clicker, slot: usize, stack: &ItemStack) -> bool {
        let existing = self.slot(p.inventory, slot);
        if existing.is_empty() {
            return true;
        }
        existing.is_same_item_same_components(stack) && existing.count <= stack.max_stack_size()
    }

    fn reset_quick_craft(&mut self) {
        self.quickcraft_status = 0;
        self.quickcraft_slots.clear();
    }

    /// Vanilla `AbstractContainerMenu.doClick`: applies one `container_click`.
    ///
    /// `slot` is a menu slot, `-1` or [`SLOT_OUTSIDE`]; other values are ignored.
    pub fn clicked(&mut self, p: &mut Clicker, slot: i16, button: i8, input: ClickInput) {
        let button = i32::from(button);
        let index = (slot >= 0 && (slot as usize) < self.slots.len()).then_some(slot as usize);
        if input == ClickInput::QuickCraft {
            self.quick_craft(p, index, button);
        } else if self.quickcraft_status != 0 {
            self.reset_quick_craft();
        } else if matches!(input, ClickInput::Pickup | ClickInput::QuickMove)
            && (button == 0 || button == 1)
        {
            let primary = button == 0;
            if slot == SLOT_OUTSIDE {
                if !self.carried.is_empty() {
                    let dropped = if primary {
                        std::mem::take(&mut self.carried)
                    } else {
                        self.carried.split(1).unwrap_or_default()
                    };
                    self.carried = normalize(std::mem::take(&mut self.carried));
                    p.drop(dropped);
                }
            } else if input == ClickInput::QuickMove {
                let Some(i) = index else { return };
                if !self.may_pickup(p, i) {
                    return;
                }
                let mut moved = self.quick_move_stack(p, i);
                while !moved.is_empty() && self.slot(p.inventory, i).is_same_item(&moved) {
                    moved = self.quick_move_stack(p, i);
                }
            } else {
                let Some(i) = index else { return };
                self.pickup(p, i, primary);
            }
        } else if input == ClickInput::Swap && ((0..9).contains(&button) || button == 40) {
            let Some(i) = index else { return };
            self.swap(p, i, button as usize);
        } else if input == ClickInput::Clone
            && p.infinite_materials
            && self.carried.is_empty()
            && let Some(i) = index
        {
            let item = self.stack(p, i);
            if !item.is_empty() {
                let max = item.max_stack_size();
                self.carried = item.copy_with_count(max);
            }
        } else if input == ClickInput::Throw
            && self.carried.is_empty()
            && let Some(i) = index
        {
            let amount = if button == 0 {
                1
            } else {
                self.slot(p.inventory, i).count
            };
            let mut taken = self.safe_take(p, i, amount, i32::MAX);
            p.drop(taken.clone());
            if button == 1 {
                while !taken.is_empty() && self.slot(p.inventory, i).is_same_item(&taken) {
                    taken = self.safe_take(p, i, amount, i32::MAX);
                    p.drop(taken.clone());
                }
            }
        } else if input == ClickInput::PickupAll
            && let Some(i) = index
        {
            self.pickup_all(p, i, button);
        }
    }

    /// The `QUICK_CRAFT` (drag) branch of `doClick`.
    fn quick_craft(&mut self, p: &mut Clicker, index: Option<usize>, button: i32) {
        let previous = self.quickcraft_status;
        self.quickcraft_status = button & 3;
        let out_of_order =
            (previous != 1 || self.quickcraft_status != 2) && previous != self.quickcraft_status;
        if out_of_order || self.carried.is_empty() {
            self.reset_quick_craft();
        } else if self.quickcraft_status == 0 {
            self.quickcraft_type = (button >> 2) & 3;
            if self.quickcraft_type == 0
                || self.quickcraft_type == 1
                || (self.quickcraft_type == 2 && p.infinite_materials)
            {
                self.quickcraft_status = 1;
                self.quickcraft_slots.clear();
            } else {
                self.reset_quick_craft();
            }
        } else if self.quickcraft_status == 1 {
            let Some(i) = index else { return };
            let carried = &self.carried;
            if self.can_quick_replace(p, i, carried)
                && self.may_place(i, carried)
                && (self.quickcraft_type == 2
                    || carried.count as usize > self.quickcraft_slots.len())
                && !self.is_result(i)
                && !self.quickcraft_slots.contains(&i)
            {
                self.quickcraft_slots.push(i);
            }
        } else if self.quickcraft_status == 2 {
            if !self.quickcraft_slots.is_empty() {
                if self.quickcraft_slots.len() == 1 {
                    let only = self.quickcraft_slots[0];
                    let kind = self.quickcraft_type;
                    self.reset_quick_craft();
                    self.clicked(p, only as i16, kind as i8, ClickInput::Pickup);
                    return;
                }
                let template = self.carried.clone();
                let mut remaining = template.count;
                let spread = self.quickcraft_slots.len();
                for target in self.quickcraft_slots.clone() {
                    if self.can_quick_replace(p, target, &template)
                        && self.may_place(target, &template)
                        && (self.quickcraft_type == 2 || template.count as usize >= spread)
                        && !self.is_result(target)
                    {
                        let existing = self.slot(p.inventory, target);
                        let had = if existing.is_empty() {
                            0
                        } else {
                            existing.count
                        };
                        let max = template
                            .max_stack_size()
                            .min(self.slot_max(target, &template));
                        let each = match self.quickcraft_type {
                            0 => template.count / spread as i32,
                            1 => 1,
                            _ => template.max_stack_size(),
                        };
                        let placed = (each + had).min(max);
                        remaining -= placed - had;
                        self.put(p, target, template.copy_with_count(placed));
                    }
                }
                self.carried = normalize(template.copy_with_count(remaining));
            }
            self.reset_quick_craft();
        } else {
            self.reset_quick_craft();
        }
    }

    /// The `PICKUP` branch of `doClick` for a slot inside the menu.
    fn pickup(&mut self, p: &mut Clicker, i: usize, primary: bool) {
        let item = self.stack(p, i);
        let carried = self.carried.clone();
        if item.is_empty() {
            if !carried.is_empty() {
                let count = if primary { carried.count } else { 1 };
                self.carried = self.safe_insert(p, i, carried, count);
            }
        } else if self.may_pickup(p, i) {
            if carried.is_empty() {
                let count = if primary {
                    item.count
                } else {
                    (item.count + 1) / 2
                };
                if let Some(taken) = self.try_remove(p, i, count, i32::MAX) {
                    self.carried = taken;
                    self.on_take(p, i);
                }
            } else if self.may_place(i, &carried) {
                if item.is_same_item_same_components(&carried) {
                    let count = if primary { carried.count } else { 1 };
                    self.carried = self.safe_insert(p, i, carried, count);
                } else if carried.count <= self.slot_max(i, &carried) {
                    self.carried = item;
                    self.put(p, i, carried);
                }
            } else if item.is_same_item_same_components(&carried)
                && let Some(taken) =
                    self.try_remove(p, i, item.count, carried.max_stack_size() - carried.count)
            {
                self.carried.count += taken.count;
                self.on_take(p, i);
            }
        }
    }

    /// The `SWAP` branch of `doClick`: `button` is a hotbar slot or 40 (the offhand).
    fn swap(&mut self, p: &mut Clicker, i: usize, button: usize) {
        let held = p.inventory.get(button).clone();
        let item = self.stack(p, i);
        if held.is_empty() && item.is_empty() {
            return;
        }
        if held.is_empty() {
            if self.may_pickup(p, i) {
                p.inventory.set(button, item);
                self.put(p, i, ItemStack::empty());
                self.on_take(p, i);
            }
        } else if item.is_empty() {
            if self.may_place(i, &held) {
                let max = self.slot_max(i, &held);
                let mut held = held;
                if held.count > max {
                    let part = held.split(max).unwrap_or_default();
                    p.inventory.set(button, held);
                    self.put(p, i, part);
                } else {
                    p.inventory.set(button, ItemStack::empty());
                    self.put(p, i, held);
                }
            }
        } else if self.may_pickup(p, i) && self.may_place(i, &held) {
            let max = self.slot_max(i, &held);
            let mut held = held;
            if held.count > max {
                let part = held.split(max).unwrap_or_default();
                p.inventory.set(button, held);
                self.put(p, i, part);
                self.on_take(p, i);
                let mut item = item;
                p.inventory.add(&mut item, p.infinite_materials);
                p.drop(item);
            } else {
                p.inventory.set(button, item);
                self.put(p, i, held);
                self.on_take(p, i);
            }
        }
    }

    /// The `PICKUP_ALL` (double click) branch of `doClick`.
    fn pickup_all(&mut self, p: &mut Clicker, i: usize, button: i32) {
        let mut carried = self.carried.clone();
        if carried.is_empty() || (!self.slot(p.inventory, i).is_empty() && self.may_pickup(p, i)) {
            return;
        }
        let len = self.slots.len() as isize;
        let (start, step) = if button == 0 { (0, 1) } else { (len - 1, -1) };
        for pass in 0..2 {
            let mut q = start;
            while q >= 0 && q < len && carried.count < carried.max_stack_size() {
                let s = q as usize;
                let item = self.stack(p, s);
                if !item.is_empty()
                    && self.can_quick_replace(p, s, &carried)
                    && self.may_pickup(p, s)
                    && !self.is_result(s)
                    && (pass != 0 || item.count != item.max_stack_size())
                {
                    let room = carried.max_stack_size() - carried.count;
                    let taken = self.safe_take(p, s, item.count, room);
                    carried.count += taken.count;
                }
                q += step;
            }
        }
        self.carried = carried;
    }

    /// Vanilla `moveItemStackTo`: merges `stack` into matching stacks of `start..end`, then
    /// into the first empty slot that accepts it. Returns whether anything moved.
    fn move_item_stack_to(
        &mut self,
        p: &mut Clicker,
        stack: &mut ItemStack,
        start: usize,
        end: usize,
        reverse: bool,
    ) -> bool {
        let order: Vec<usize> = if reverse {
            (start..end).rev().collect()
        } else {
            (start..end).collect()
        };
        let mut moved = false;
        if is_stackable(stack) {
            for &k in &order {
                if stack.is_empty() {
                    break;
                }
                let target = self.stack(p, k);
                if target.is_empty() || !stack.is_same_item_same_components(&target) {
                    continue;
                }
                let total = target.count + stack.count;
                let max = self.slot_max(k, &target);
                if total <= max {
                    stack.count = 0;
                    self.put(p, k, target.copy_with_count(total));
                    moved = true;
                } else if target.count < max {
                    stack.count -= max - target.count;
                    self.put(p, k, target.copy_with_count(max));
                    moved = true;
                }
            }
        }
        if !stack.is_empty() {
            for &k in &order {
                if self.slot(p.inventory, k).is_empty() && self.may_place(k, stack) {
                    let max = self.slot_max(k, stack);
                    let part = stack.split(stack.count.min(max)).unwrap_or_default();
                    self.put(p, k, part);
                    moved = true;
                    break;
                }
            }
        }
        moved
    }

    /// Vanilla `quickMoveStack` (shift-click) of `InventoryMenu`, `CraftingMenu` and
    /// `ChestMenu`. Returns the stack as it was before the move, or empty when nothing moved.
    fn quick_move_stack(&mut self, p: &mut Clicker, i: usize) -> ItemStack {
        let original = self.stack(p, i);
        if original.is_empty() {
            return ItemStack::empty();
        }
        let mut stack = original.clone();
        // A saved map a cartography result needed; given back if the result did not move.
        let mut created: Option<i32> = None;
        let moved = match self.kind {
            MenuKind::Inventory => {
                let worn = equipment_slot(&original);
                let armor = match worn {
                    "head" => Some(5),
                    "chest" => Some(6),
                    "legs" => Some(7),
                    "feet" => Some(8),
                    _ => None,
                };
                if i == 0 {
                    self.move_item_stack_to(p, &mut stack, 9, 45, true)
                } else if (1..9).contains(&i) {
                    self.move_item_stack_to(p, &mut stack, 9, 45, false)
                } else if let Some(j) = armor
                    && self.slot(p.inventory, j).is_empty()
                {
                    self.move_item_stack_to(p, &mut stack, j, j + 1, false)
                } else if worn == "offhand" && self.slot(p.inventory, 45).is_empty() {
                    self.move_item_stack_to(p, &mut stack, 45, 46, false)
                } else if (9..36).contains(&i) {
                    self.move_item_stack_to(p, &mut stack, 36, 45, false)
                } else if (36..45).contains(&i) {
                    self.move_item_stack_to(p, &mut stack, 9, 36, false)
                } else {
                    self.move_item_stack_to(p, &mut stack, 9, 45, false)
                }
            }
            MenuKind::Crafting => {
                if i == 0 {
                    self.move_item_stack_to(p, &mut stack, 10, 46, true)
                } else if (10..46).contains(&i) {
                    self.move_item_stack_to(p, &mut stack, 1, 10, false)
                        || if i < 37 {
                            self.move_item_stack_to(p, &mut stack, 37, 46, false)
                        } else {
                            self.move_item_stack_to(p, &mut stack, 10, 37, false)
                        }
                } else {
                    self.move_item_stack_to(p, &mut stack, 10, 46, false)
                }
            }
            MenuKind::ShulkerBox => {
                let len = self.slots.len();
                if i < SHULKER_BOX_SLOTS {
                    self.move_item_stack_to(p, &mut stack, SHULKER_BOX_SLOTS, len, true)
                } else {
                    self.move_item_stack_to(p, &mut stack, 0, SHULKER_BOX_SLOTS, false)
                }
            }
            MenuKind::Generic { rows } => {
                let size = usize::from(rows) * 9;
                let len = self.slots.len();
                if i < size {
                    self.move_item_stack_to(p, &mut stack, size, len, true)
                } else {
                    self.move_item_stack_to(p, &mut stack, 0, size, false)
                }
            }
            MenuKind::Anvil => {
                if i == 2 {
                    self.move_item_stack_to(p, &mut stack, 3, 39, true)
                } else if i == 0 || i == 1 {
                    self.move_item_stack_to(p, &mut stack, 3, 39, false)
                } else {
                    self.move_item_stack_to(p, &mut stack, 0, 1, false)
                        || self.move_item_stack_to(p, &mut stack, 1, 2, false)
                }
            }
            MenuKind::Enchanting => {
                if i < 2 {
                    self.move_item_stack_to(p, &mut stack, 2, 38, false)
                } else {
                    self.move_item_stack_to(p, &mut stack, 0, 1, false)
                        || self.move_item_stack_to(p, &mut stack, 1, 2, false)
                }
            }
            MenuKind::Stonecutter => {
                if i < 2 {
                    self.move_item_stack_to(p, &mut stack, 2, 38, false)
                } else {
                    self.move_item_stack_to(p, &mut stack, 0, 1, false)
                }
            }
            MenuKind::Grindstone => {
                if i < 3 {
                    self.move_item_stack_to(p, &mut stack, 3, 39, false)
                } else {
                    self.move_item_stack_to(p, &mut stack, 0, 1, false)
                        || self.move_item_stack_to(p, &mut stack, 1, 2, false)
                }
            }
            MenuKind::Smithing | MenuKind::Loom => {
                if i < 4 {
                    self.move_item_stack_to(p, &mut stack, 4, 40, false)
                } else {
                    self.move_item_stack_to(p, &mut stack, 0, 1, false)
                        || self.move_item_stack_to(p, &mut stack, 1, 2, false)
                        || self.move_item_stack_to(p, &mut stack, 2, 3, false)
                }
            }
            MenuKind::Cartography => {
                if i == 2 {
                    match p.maps.finish(&mut stack) {
                        Ok(id) => created = id,
                        Err(MapFinishError) => return ItemStack::empty(),
                    }
                    self.move_item_stack_to(p, &mut stack, 3, 39, true)
                } else if i < 2 {
                    self.move_item_stack_to(p, &mut stack, 3, 39, false)
                } else if ferrite_item::map_id(&original).is_some() {
                    self.move_item_stack_to(p, &mut stack, 0, 1, false)
                } else if matches!(
                    original.name(),
                    Some("minecraft:paper" | "minecraft:map" | "minecraft:glass_pane")
                ) {
                    self.move_item_stack_to(p, &mut stack, 1, 2, false)
                } else if i < 30 {
                    self.move_item_stack_to(p, &mut stack, 30, 39, false)
                } else {
                    self.move_item_stack_to(p, &mut stack, 3, 30, false)
                }
            }
            MenuKind::Merchant => {
                if i == 2 {
                    self.move_item_stack_to(p, &mut stack, 3, 39, true)
                } else if i >= 3 {
                    // Vanilla `MerchantMenu.quickMoveStack`: fill the first input before the
                    // second, then fall back to whichever still accepts the stack.
                    self.move_item_stack_to(p, &mut stack, 0, 1, false)
                        || self.move_item_stack_to(p, &mut stack, 1, 2, false)
                        || self.move_item_stack_to(p, &mut stack, 0, 2, false)
                } else {
                    self.move_item_stack_to(p, &mut stack, 3, 39, false)
                }
            }
            MenuKind::Horse { columns, .. } => {
                let size = 2 + usize::from(columns) * 3;
                let len = self.slots.len();
                if i < size {
                    self.move_item_stack_to(p, &mut stack, size, len, true)
                } else {
                    let free = |menu: &Self, p: &Clicker, slot: usize| {
                        menu.may_place(slot, &original) && menu.slot(p.inventory, slot).is_empty()
                    };
                    if free(self, p, 1) && self.move_item_stack_to(p, &mut stack, 1, 2, false)
                        || free(self, p, 0) && self.move_item_stack_to(p, &mut stack, 0, 1, false)
                        || columns > 0 && self.move_item_stack_to(p, &mut stack, 2, size, false)
                    {
                        true
                    } else if i < size + 27 {
                        self.move_item_stack_to(p, &mut stack, size + 27, len, false)
                    } else {
                        self.move_item_stack_to(p, &mut stack, size, size + 27, false)
                    }
                }
            }
            MenuKind::BrewingStand => {
                if i < 5 {
                    self.move_item_stack_to(p, &mut stack, 5, 41, false)
                } else if is_potion(&original) {
                    self.move_item_stack_to(p, &mut stack, 0, 3, false)
                } else if p.crafter.is_brewing_ingredient(&original) {
                    self.move_item_stack_to(p, &mut stack, 3, 4, false)
                } else if original.name() == Some("minecraft:blaze_powder") {
                    self.move_item_stack_to(p, &mut stack, 4, 5, false)
                } else {
                    false
                }
            }
            MenuKind::Beacon => {
                if i == 0 {
                    self.move_item_stack_to(p, &mut stack, 1, 37, true)
                } else {
                    self.move_item_stack_to(p, &mut stack, 0, 1, false)
                }
            }
            MenuKind::Furnace(kind) => {
                if i == 2 {
                    self.move_item_stack_to(p, &mut stack, 3, 39, true)
                } else if i >= 3 {
                    // Vanilla `AbstractFurnaceMenu.quickMoveStack`: input first when the item can
                    // be smelted, otherwise the fuel slot when it burns.
                    let cookable = p.crafter.cook(kind, &original).is_some();
                    let fuel = p.crafter.burn_time(&original) > 0;
                    (cookable && self.move_item_stack_to(p, &mut stack, 0, 1, false))
                        || (fuel && self.move_item_stack_to(p, &mut stack, 1, 2, false))
                } else {
                    self.move_item_stack_to(p, &mut stack, 3, 39, false)
                }
            }
        };
        if !moved {
            if let Some(id) = created {
                p.maps.discard(id);
            }
            return ItemStack::empty();
        }
        let left = stack.count.max(0);
        self.put(p, i, normalize(stack.clone()));
        if left == original.count {
            if let Some(id) = created {
                p.maps.discard(id);
            }
            return ItemStack::empty();
        }
        self.on_take(p, i);
        if self.is_result(i) {
            // Vanilla drops the part of a crafted stack that did not fit.
            p.drop(normalize(stack));
        }
        original
    }

    /// Vanilla `AbstractContainerMenu.removed` plus the menu's own `removed`: the cursor stack
    /// and the crafting grid go back into the inventory (what does not fit is dropped) and the
    /// result is cleared.
    pub fn close(&mut self, p: &mut Clicker) {
        let carried = std::mem::take(&mut self.carried);
        if let Some(rest) = p.inventory.place_back(carried) {
            p.drop(rest);
        }
        self.reset_quick_craft();
        if self.grid_width > 0 {
            self.result = ItemStack::empty();
            for cell in 0..self.grid.len() {
                let stack = std::mem::take(&mut self.grid[cell]);
                if let Some(rest) = p.inventory.place_back(stack) {
                    p.drop(rest);
                }
            }
        }
        // Vanilla `BeaconMenu.removed` returns the payment item to the player.
        if self.kind == MenuKind::Beacon {
            let stack = std::mem::take(&mut self.container[0]);
            if let Some(rest) = p.inventory.place_back(stack) {
                p.drop(rest);
            }
        }
        // Vanilla `CartographyTableMenu.removed` returns both inputs and drops the result.
        if self.kind == MenuKind::Cartography {
            self.result = ItemStack::empty();
            for cell in self.container.iter_mut().take(2) {
                let stack = std::mem::take(cell);
                if let Some(rest) = p.inventory.place_back(stack) {
                    p.drop(rest);
                }
            }
        }
        // Vanilla `MerchantMenu.removed` returns both input stacks to the player.
        if self.kind == MenuKind::Merchant {
            self.result = ItemStack::empty();
            for cell in self.container.iter_mut().take(2) {
                let stack = std::mem::take(cell);
                if let Some(rest) = p.inventory.place_back(stack) {
                    p.drop(rest);
                }
            }
        }
    }

    /// Vanilla `ServerGamePacketListenerImpl.handleContainerClick` after the container id and
    /// spectator checks: runs the click, records the client's predicted slots and carried
    /// stack, then sends what differs, or the whole menu when the client's state id was stale.
    #[allow(clippy::too_many_arguments)]
    pub fn handle_click(
        &mut self,
        p: &mut Clicker,
        state_id: i32,
        slot: i16,
        button: i8,
        input: ClickInput,
        changed: &[(i16, Option<HashedSlot>)],
        carried: Option<HashedSlot>,
    ) -> Vec<Sync> {
        if !self.is_valid_slot_index(slot) {
            return Vec::new();
        }
        let stale = state_id != self.state_id;
        self.clicked(p, slot, button, input);
        for (slot, predicted) in changed {
            if let Some(remote) = usize::try_from(*slot)
                .ok()
                .and_then(|slot| self.remote.get_mut(slot))
            {
                *remote = Remote::Hashed(predicted.clone());
            }
        }
        self.remote_carried = Remote::Hashed(carried);
        if stale {
            vec![self.broadcast_full_state(p.inventory)]
        } else {
            self.broadcast_changes(p.inventory)
        }
    }

    /// Vanilla `broadcastChanges`: a `container_set_slot` (with a fresh state id) for every slot
    /// the client shows differently, then `set_cursor_item` if the cursor differs.
    pub fn broadcast_changes(&mut self, inventory: &PlayerInventory) -> Vec<Sync> {
        // The table's buttons follow the item in its slot (vanilla `slotsChanged`).
        if self.kind == MenuKind::Enchanting {
            self.sync_data();
        }
        let mut out = Vec::new();
        for slot in 0..self.slots.len() {
            let item = self.slot(inventory, slot).clone();
            if !self.remote[slot].matches(&item) {
                self.remote[slot] = Remote::Known(item.clone());
                out.push(Sync::Slot {
                    container_id: self.container_id,
                    state_id: self.increment_state_id(),
                    slot: slot as i16,
                    item,
                });
            }
        }
        if !self.remote_carried.matches(&self.carried) {
            self.remote_carried = Remote::Known(self.carried.clone());
            out.push(Sync::Carried(self.carried.clone()));
        }
        for (id, &value) in self.data.iter().enumerate() {
            if self.remote_data.get(id).copied() == Some(value) {
                continue;
            }
            if id >= self.remote_data.len() {
                self.remote_data.resize(id + 1, i32::MIN);
            }
            self.remote_data[id] = value;
            out.push(Sync::Data {
                container_id: self.container_id,
                id: id as i16,
                value: value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
            });
        }
        out
    }

    /// Vanilla `sendAllDataToRemote`: the whole menu in one `container_set_content`.
    pub fn broadcast_full_state(&mut self, inventory: &PlayerInventory) -> Sync {
        let items = self.items(inventory);
        self.remote = items.iter().cloned().map(Remote::Known).collect();
        self.remote_carried = Remote::Known(self.carried.clone());
        Sync::Content {
            container_id: self.container_id,
            state_id: self.increment_state_id(),
            items,
            carried: self.carried.clone(),
        }
    }

    /// Vanilla `setRemoteSlot`: records that the client already shows `stack` in a slot (it
    /// predicted the change itself), so no update is sent for it.
    pub fn set_remote_slot(&mut self, slot: usize, stack: &ItemStack) {
        if let Some(remote) = self.remote.get_mut(slot) {
            *remote = Remote::Known(stack.clone());
        }
    }

    /// Vanilla `AnvilMenu.setItemName`: records the typed name and recomputes the result.
    pub fn rename(&mut self, name: String, crafter: &dyn Crafter) {
        if self.kind != MenuKind::Anvil {
            return;
        }
        self.rename = name;
        self.update_anvil(crafter);
    }

    /// Vanilla `AbstractContainerMenu.clickMenuButton`: an enchant choice, stonecutter recipe or
    /// loom pattern. Returns the experience levels the click spends.
    pub fn button_click(&mut self, button: i32, next_seed: i32) -> Option<i32> {
        match self.kind {
            MenuKind::Enchanting => self.enchant(button, next_seed),
            MenuKind::Stonecutter => {
                if button >= 0 && (button as usize) < self.stonecutting.len() {
                    self.stonecutting_index = button as usize;
                    self.result = self.stonecutting[self.stonecutting_index].clone();
                    self.sync_data();
                }
                None
            }
            MenuKind::Loom => {
                self.loom_pattern = if button < 0 { 0 } else { button as usize };
                self.update_loom();
                None
            }
            _ => None,
        }
    }

    /// The name the player typed into an anvil (vanilla `AnvilMenu.itemName`).
    pub fn rename_text(&self) -> &str {
        &self.rename
    }

    /// The levels a player needs for an enchanting-table choice (vanilla `EnchantmentMenu.clickMenuButton`
    /// wants `choice + 1` and at least the displayed cost); `0` when the button offers nothing.
    pub fn enchant_cost(&self, choice: i32) -> i32 {
        if self.kind != MenuKind::Enchanting || !(0..3).contains(&choice) {
            return 0;
        }
        let cost = self.enchanting.offers[choice as usize].cost;
        if cost <= 0 { 0 } else { cost.max(choice + 1) }
    }

    /// Vanilla `transferState`: adopts what the client was last sent for the inventory slots
    /// `other` shares with this menu, so returning to this menu only sends real changes.
    pub fn transfer_state(&mut self, other: &Menu) {
        for (slot, def) in self.slots.iter().enumerate() {
            if let Store::Inventory(i) = def.store
                && let Some(from) = other.menu_slot_of(i)
            {
                self.remote[slot] = other.remote[from].clone();
            }
        }
    }
}

/// Vanilla `ItemStack.getOrDefault(DataComponents.REPAIR_COST, 0)`.
/// The enchantments the anvil reads from a stack: the stored ones of an enchanted book,
/// otherwise the applied ones (vanilla `EnchantmentHelper.getEnchantmentsForCrafting`).
fn crafting_enchantments(stack: &ItemStack) -> Enchantments {
    if stack.name() == Some("minecraft:enchanted_book") {
        if let Some(Component::StoredEnchantments(stored)) =
            stack.effective_component(ids::STORED_ENCHANTMENTS)
        {
            return stored.clone();
        }
        return Enchantments::new();
    }
    match stack.effective_component(ids::ENCHANTMENTS) {
        Some(Component::Enchantments(applied)) => applied.clone(),
        _ => Enchantments::new(),
    }
}

/// Vanilla `AnvilMenu.createResult` enchantment loop. Returns the merged enchantments, the level
/// cost they add, whether any applied, and whether some enchantment was refused.
fn merge_enchantments(
    left: &ItemStack,
    right: &ItemStack,
    right_is_book: bool,
    crafter: &dyn Crafter,
) -> (Enchantments, i32, bool, bool) {
    let left_is_book = left.name() == Some("minecraft:enchanted_book");
    let mut merged = crafting_enchantments(left);
    let mut cost = 0;
    let (mut applied, mut refused) = (false, false);
    for (name, &level) in &crafting_enchantments(right).levels {
        let Some(rules) = crafter.enchantment_rules(name, left) else {
            continue;
        };
        let current = merged.level(name).unwrap_or(0);
        let mut new_level = if current == level {
            level + 1
        } else {
            level.max(current)
        };
        let mut allowed = rules.supported || left_is_book;
        for other in merged.levels.keys() {
            if other != name && crafter.enchantments_exclusive(name, other) {
                allowed = false;
                cost += 1;
            }
        }
        if !allowed {
            refused = true;
            continue;
        }
        new_level = new_level.min(rules.max_level);
        merged.set(name.clone(), new_level);
        applied = true;
        let weight = if right_is_book {
            (rules.anvil_cost / 2).max(1)
        } else {
            rules.anvil_cost
        };
        cost += weight * new_level;
    }
    (merged, cost, applied, refused)
}

fn repair_cost(stack: &ItemStack) -> i32 {
    match stack.effective_component(ids::REPAIR_COST) {
        Some(Component::RepairCost(value)) => value,
        _ => 0,
    }
}

/// The custom name of a stack, if it has one (vanilla `DataComponents.CUSTOM_NAME`).
fn custom_name(stack: &ItemStack) -> Option<String> {
    match stack.effective_component(ids::CUSTOM_NAME) {
        Some(Component::CustomName(text)) => Some(text.to_plain_text()),
        Some(Component::Opaque(Tag::String(name))) => Some(name),
        _ => None,
    }
}

/// Whether `material` is the vanilla repair material of `item` (the `minecraft:repairable`
/// component's item list, approximated by the item family's material).
fn repairs_with(item: &ItemStack, material: &ItemStack) -> bool {
    let Some(item) = item.name() else {
        return false;
    };
    let Some(material_name) = material
        .name()
        .map(|name| name.strip_prefix("minecraft:").unwrap_or(name))
    else {
        return false;
    };
    let wanted = match item.strip_prefix("minecraft:").unwrap_or(item) {
        "leather_helmet" | "leather_chestplate" | "leather_leggings" | "leather_boots" => "leather",
        "chainmail_helmet" | "chainmail_chestplate" | "chainmail_leggings" | "chainmail_boots" => {
            "iron_ingot"
        }
        "iron_helmet" | "iron_chestplate" | "iron_leggings" | "iron_boots" | "iron_sword"
        | "iron_pickaxe" | "iron_axe" | "iron_shovel" | "iron_hoe" => "iron_ingot",
        "golden_helmet" | "golden_chestplate" | "golden_leggings" | "golden_boots"
        | "golden_sword" | "golden_pickaxe" | "golden_axe" | "golden_shovel" | "golden_hoe" => {
            "gold_ingot"
        }
        "diamond_helmet" | "diamond_chestplate" | "diamond_leggings" | "diamond_boots"
        | "diamond_sword" | "diamond_pickaxe" | "diamond_axe" | "diamond_shovel"
        | "diamond_hoe" => "diamond",
        "netherite_helmet"
        | "netherite_chestplate"
        | "netherite_leggings"
        | "netherite_boots"
        | "netherite_sword"
        | "netherite_pickaxe"
        | "netherite_axe"
        | "netherite_shovel"
        | "netherite_hoe" => "netherite_ingot",
        "turtle_helmet" => "turtle_scute",
        _ if item.ends_with("_wooden_sword")
            || item.ends_with("_wooden_pickaxe")
            || item.ends_with("_wooden_axe")
            || item.ends_with("_wooden_shovel")
            || item.ends_with("_wooden_hoe") =>
        {
            return material_name.ends_with("_planks");
        }
        _ if item.ends_with("_stone_sword")
            || item.ends_with("_stone_pickaxe")
            || item.ends_with("_stone_axe")
            || item.ends_with("_stone_shovel")
            || item.ends_with("_stone_hoe") =>
        {
            return matches!(material_name, "stone" | "cobblestone");
        }
        _ => return false,
    };
    material_name == wanted
}

/// The bottle items a brewing stand's potion slots accept (vanilla
/// `BrewingStandMenu.PotionSlot`).
fn is_potion(stack: &ItemStack) -> bool {
    matches!(
        stack.name(),
        Some("minecraft:potion" | "minecraft:splash_potion" | "minecraft:lingering_potion")
    )
}

/// The five items a beacon accepts as payment (vanilla `BeaconBlockEntity.PAYMENT_ITEMS`).
fn is_beacon_payment(stack: &ItemStack) -> bool {
    matches!(
        stack.name(),
        Some(
            "minecraft:iron_ingot"
                | "minecraft:gold_ingot"
                | "minecraft:emerald"
                | "minecraft:diamond"
                | "minecraft:netherite_ingot"
        )
    )
}

fn normalize(stack: ItemStack) -> ItemStack {
    if stack.is_empty() {
        ItemStack::empty()
    } else {
        stack
    }
}

/// Whether the item has a `minecraft:enchantable` component (vanilla `ItemStack.isEnchantable`:
/// that, and no enchantments yet).
fn enchantable(stack: &ItemStack) -> bool {
    let has = ferrite_item::default_components(stack.item)
        .is_some_and(|c| c.contains_key(&ids::ENCHANTABLE));
    let enchanted = matches!(
        stack.effective_component(ids::ENCHANTMENTS),
        Some(Component::Enchantments(e)) if !e.is_empty()
    );
    has && !enchanted
}
