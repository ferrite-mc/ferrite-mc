//! Vanilla player inventory and container menus, as pure logic.
//!
//! [`PlayerInventory`] is vanilla `Inventory`: 36 item slots (0..9 the hotbar), four armor
//! slots and the offhand, in vanilla's own slot numbering. [`Menu`] is
//! `AbstractContainerMenu` with the menus the server opens: the player's [`MenuKind::Inventory`]
//! (container 0, vanilla `InventoryMenu`), the 3x3 [`MenuKind::Crafting`] table (`CraftingMenu`)
//! and plain [`MenuKind::Generic`] chests (`ChestMenu`).
//!
//! [`Menu::clicked`] implements every `container_click` input exactly like
//! `AbstractContainerMenu.doClick`; [`Menu::handle_click`] adds the network side of
//! `ServerGamePacketListenerImpl.handleContainerClick` (client predictions, state ids and the
//! resync of a stale client), and [`Menu::broadcast_changes`] produces the slot updates the
//! server sends. Nothing here touches the network or the world: dropped items and crafting
//! results go through [`Clicker`] and [`Crafter`].

mod inventory;
mod menu;

pub use inventory::{
    CHEST, FEET, HEAD, HOTBAR, ITEMS, LEGS, OFFHAND, PlayerInventory, SIZE, equipment_slot,
};
pub use menu::{
    BEACON_DATA_SLOTS, BEACON_LEVELS, BEACON_PRIMARY, BEACON_SECONDARY, BREW_TIME,
    BREWING_BREW_TIME, BREWING_DATA_SLOTS, BREWING_FUEL, BeaconState, BrewingState, ClickInput,
    Clicker, CookingOutput, Crafter, FUEL_PER_BLAZE_POWDER, FURNACE_BURN_LEFT, FURNACE_BURN_TOTAL,
    FURNACE_COOK_PROGRESS, FURNACE_COOK_TOTAL, FURNACE_DATA_SLOTS, FURNACE_LIT, FurnaceKind,
    FurnaceState, Menu, MenuKind, NoCrafting, SLOT_OUTSIDE, Sync, Trade, craft_remainder,
};

#[cfg(test)]
mod tests;
