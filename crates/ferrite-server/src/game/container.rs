//! A player's inventory and open container menu on the server (vanilla `ServerPlayer`'s
//! `inventoryMenu`/`containerMenu` and the container handlers of
//! `ServerGamePacketListenerImpl`).

use ferrite_inventory::{ClickInput, Clicker, Crafter, Menu, PlayerInventory, Sync};
use ferrite_item::{HashedSlot, ItemStack};
use ferrite_protocol::VarInt;
use ferrite_protocol::packets::play::{
    ClientboundContainerClose, ContainerButtonClick, ContainerClick, ContainerContent,
    ContainerSetData, ContainerSlot, CreativeSlot, OpenScreen, SetBeacon, SetCursorItem,
};

use super::anvil::AnvilChunkSource;
use super::beacon;
use super::block_container::{self, Opened};
use super::brewing;
use super::furnace;
use super::region::{Player, send};

/// The player's menus: the inventory menu (container 0) and the open container, if any.
#[derive(Debug)]
pub struct Containers {
    /// Vanilla `inventoryMenu`.
    pub inventory: Menu,
    /// Vanilla `containerMenu` when it is not the inventory menu.
    pub open: Option<Menu>,
    /// Vanilla `containerCounter`, the id of the last opened container (1..=100).
    counter: i32,
    /// Stacks the player dropped, spawned as item entities by the region this tick.
    pub thrown: Vec<ItemStack>,
    /// The block the open menu belongs to, for vanilla `stillValid` and shared contents.
    pub open_at: Option<OpenAt>,
    /// Contents of broken inventory blocks, spilled as item entities by the region this tick.
    pub spilled: Vec<([i32; 3], ItemStack)>,
}

/// What an open menu is attached to.
#[derive(Clone, Debug, PartialEq)]
pub enum OpenAt {
    /// A crafting table, whose grid belongs to the menu.
    Table([i32; 3]),
    /// A chest, trapped chest or barrel, whose items live in its block entities.
    Inventory(Opened),
    /// A furnace, blast furnace or smoker, whose items and timers live in its block entity.
    Furnace([i32; 3]),
    /// A brewing stand, whose items and timers live in its block entity.
    BrewingStand([i32; 3]),
    /// A beacon, whose pyramid level and effects live in its block entity.
    Beacon([i32; 3]),
    /// An anvil, which has no block entity.
    Anvil([i32; 3]),
    /// Any other station block: the position and the block name it must still be.
    Block([i32; 3], &'static str),
}

/// The block name at a position (vanilla `Level.getBlockState`).
fn block_name(source: &AnvilChunkSource, [x, y, z]: [i32; 3]) -> Option<&'static str> {
    source
        .block(x, y, z)
        .and_then(|state| u16::try_from(state).ok())
        .and_then(ferrite_data::blocks::state)
        .map(|(name, _)| name)
}

impl Default for Containers {
    fn default() -> Self {
        Self {
            inventory: Menu::inventory(),
            open: None,
            counter: 0,
            thrown: Vec::new(),
            open_at: None,
            spilled: Vec::new(),
        }
    }
}

impl Containers {
    /// Vanilla `containerMenu`.
    pub fn current(&mut self) -> &mut Menu {
        self.open.as_mut().unwrap_or(&mut self.inventory)
    }

    /// Vanilla `nextContainerCounter`.
    fn next_id(&mut self) -> i32 {
        self.counter = self.counter % 100 + 1;
        self.counter
    }
}

/// The crafting recipes in effect for every menu.
pub fn crafter() -> &'static dyn Crafter {
    super::crafting::crafter()
}

/// Sends menu sync packets to a player.
fn emit(player: &Player, out: Vec<Sync>) {
    for sync in out {
        match sync {
            Sync::Content {
                container_id,
                state_id,
                items,
                carried,
            } => send(
                player,
                &ContainerContent {
                    window: VarInt(container_id),
                    state: VarInt(state_id),
                    slots: items,
                    carried,
                },
            ),
            Sync::Slot {
                container_id,
                state_id,
                slot,
                item,
            } => send(
                player,
                &ContainerSlot {
                    window: VarInt(container_id),
                    state: VarInt(state_id),
                    slot,
                    item,
                },
            ),
            Sync::Carried(item) => send(player, &SetCursorItem { item }),
            Sync::Data {
                container_id,
                id,
                value,
            } => send(
                player,
                &ContainerSetData {
                    window: VarInt(container_id),
                    id,
                    value,
                },
            ),
        }
    }
}

impl Player {
    fn infinite_materials(&self) -> bool {
        self.gamemode == 1
    }

    /// Sends the whole inventory menu (vanilla `initInventoryMenu`), on join.
    pub fn init_inventory_menu(&mut self) {
        let full = self.menus.inventory.broadcast_full_state(&self.inventory);
        emit(self, vec![full]);
    }

    /// Broadcasts the current menu's changed slots (vanilla `broadcastChanges`).
    pub fn sync_containers(&mut self) {
        let out = self.menus.current().broadcast_changes(&self.inventory);
        emit(self, out);
    }

    /// Once per tick, like `ServerPlayer.tick`: `containerMenu.broadcastChanges()`, then the
    /// open menu closes if it is no longer valid.
    pub fn tick_containers(&mut self) {
        self.load_shared_contents();
        // Vanilla ticks the furnace block entity even without a viewer; this server advances the
        // open menu, which owns the same items and timers while it is open.
        self.menus.current().tick(crafter());
        let out = self.menus.current().broadcast_changes(&self.inventory);
        emit(self, out);
        if !self.container_still_valid() {
            self.close_open_container();
        }
    }

    /// Vanilla `AbstractContainerMenu.stillValid` for a block menu: the block is still there
    /// and within the player's block interaction range plus 4 (`canInteractWithBlock(pos, 4)`).
    fn container_still_valid(&self) -> bool {
        let Some(source) = self.source.as_ref() else {
            return true;
        };
        match &self.menus.open_at {
            None => true,
            Some(OpenAt::Table([x, y, z])) => {
                let table = ferrite_data::blocks::default_state("crafting_table").map(u32::from);
                let present = source.block(*x, *y, *z);
                present.is_some() && present == table && self.within_reach([*x, *y, *z])
            }
            // A double chest stays usable while both halves are there and in reach.
            Some(OpenAt::Inventory(opened)) => {
                block_container::still_there(source, opened)
                    && opened.parts.iter().all(|&pos| self.within_reach(pos))
            }
            Some(OpenAt::Furnace([x, y, z])) => {
                block_name(source, [*x, *y, *z])
                    .and_then(furnace::kind)
                    .is_some()
                    && self.within_reach([*x, *y, *z])
            }
            Some(OpenAt::BrewingStand([x, y, z])) => {
                block_name(source, [*x, *y, *z]) == Some("minecraft:brewing_stand")
                    && self.within_reach([*x, *y, *z])
            }
            Some(OpenAt::Beacon([x, y, z])) => {
                block_name(source, [*x, *y, *z]) == Some("minecraft:beacon")
                    && self.within_reach([*x, *y, *z])
            }
            Some(OpenAt::Anvil([x, y, z])) => {
                block_name(source, [*x, *y, *z]) == Some("minecraft:anvil")
                    && self.within_reach([*x, *y, *z])
            }
            Some(OpenAt::Block([x, y, z], block)) => {
                block_name(source, [*x, *y, *z]) == Some(*block) && self.within_reach([*x, *y, *z])
            }
        }
    }

    /// Vanilla `Player.canInteractWithBlock(pos, 4.0)`: the block box is closer to the eyes
    /// than the block interaction range plus 4.
    fn within_reach(&self, [x, y, z]: [i32; 3]) -> bool {
        let eye = [self.pos[0], self.pos[1] + 1.62, self.pos[2]];
        let distance_sq: f64 = [x, y, z]
            .iter()
            .zip(eye)
            .map(|(&min, e)| {
                let min = f64::from(min);
                let d = (min - e).max(e - (min + 1.0)).max(0.0);
                d * d
            })
            .sum();
        let range = if self.gamemode == 1 { 5.0 } else { 4.5 } + 4.0;
        distance_sq < range * range
    }

    /// Vanilla `closeContainer`: tells the client, then closes on the server.
    fn close_open_container(&mut self) {
        if let Some(window) = self.menus.open.as_ref().map(Menu::container_id) {
            send(
                self,
                &ClientboundContainerClose {
                    window: VarInt(window),
                },
            );
            self.close_container();
        }
    }

    /// Vanilla `handleContainerClick`.
    pub fn handle_container_click(&mut self, p: ContainerClick<HashedSlot>) {
        let infinite = self.infinite_materials();
        let menu = self
            .menus
            .open
            .as_mut()
            .unwrap_or(&mut self.menus.inventory);
        if menu.container_id() != p.window.0 {
            return;
        }
        if self.gamemode == 3 {
            let full = menu.broadcast_full_state(&self.inventory);
            emit(self, vec![full]);
            return;
        }
        let Some(input) = ClickInput::from_id(p.input.0) else {
            return;
        };
        if !self.container_still_valid() {
            return;
        }
        // Vanilla `AnvilMenu.stillValid` cost check: a player without enough levels cannot take
        // the result (creative may).
        if p.slot == 2
            && self.gamemode != 1
            && self.menus.open.as_ref().is_some_and(|menu| {
                menu.kind() == ferrite_inventory::MenuKind::Anvil
                    && menu.anvil_cost() > self.experience_level
            })
        {
            let full = self
                .menus
                .open
                .as_mut()
                .map(|menu| menu.broadcast_full_state(&self.inventory));
            if let Some(full) = full {
                emit(self, vec![full]);
            }
            return;
        }
        let menu = self
            .menus
            .open
            .as_mut()
            .unwrap_or(&mut self.menus.inventory);
        // Another viewer may have changed a shared container since this player last saw it.
        if let (Some(OpenAt::Inventory(opened)), Some(source)) = (&self.menus.open_at, &self.source)
        {
            let items = block_container::load(source, opened);
            menu.container_mut().clone_from_slice(&items);
        }
        let mut clicker = Clicker::new(&mut self.inventory, infinite, crafter());
        let out = menu.handle_click(
            &mut clicker,
            p.state.0,
            p.slot,
            p.button,
            input,
            &p.changed,
            p.carried,
        );
        let drops = clicker.drops;
        if let (Some(OpenAt::Inventory(opened)), Some(source)) = (&self.menus.open_at, &self.source)
        {
            block_container::store(source, opened, menu.container());
        }
        self.menus.thrown.extend(drops);
        let xp = clicker.xp;
        let levels = clicker.levels;
        emit(self, out);
        if xp > 0.0 {
            self.add_experience(xp);
        }
        if levels > 0 {
            self.spend_levels(levels);
        }
    }

    /// Vanilla `ServerPlayer.handleRenameItem`: only an anvil menu accepts a rename, and the name
    /// is limited to 50 characters.
    pub fn rename_item(&mut self, name: String) {
        let Some(menu) = self.menus.open.as_mut() else {
            return;
        };
        menu.rename(name.chars().take(50).collect());
        let out = self.menus.current().broadcast_changes(&self.inventory);
        emit(self, out);
    }

    /// Vanilla `ServerPlayer.handleContainerButtonClick`: enchant choices, stonecutter recipes
    /// and loom patterns all arrive as button clicks on the open menu.
    pub fn handle_container_button(&mut self, p: ContainerButtonClick) {
        let Some(menu) = self
            .menus
            .open
            .as_mut()
            .filter(|menu| menu.container_id() == p.window.0)
        else {
            return;
        };
        // Vanilla `EnchantmentMenu.doClick` refuses a choice the player cannot pay for.
        if self.gamemode != 1 {
            let cost = menu.enchant_cost(p.button.0);
            if cost > self.experience_level {
                let full = menu.broadcast_full_state(&self.inventory);
                emit(self, vec![full]);
                return;
            }
        }
        let spent = menu.button_click(p.button.0);
        let out = menu.broadcast_changes(&self.inventory);
        emit(self, out);
        if let Some(spent) = spent {
            self.spend_levels(spent);
        }
    }

    /// Vanilla `ServerGamePacketListenerImpl.handleSetBeacon`: validates the chosen effects
    /// against the pyramid, spends one payment item and saves the choice in the block entity.
    pub fn handle_set_beacon(&mut self, p: SetBeacon) {
        let Some(menu) = self
            .menus
            .open
            .as_mut()
            .filter(|menu| menu.kind() == ferrite_inventory::MenuKind::Beacon)
        else {
            return;
        };
        let levels = menu.beacon_state().map_or(0, |state| state.levels);
        let primary = p.primary.map(|effect| effect.0);
        let secondary = p.secondary.map(|effect| effect.0);
        if primary.is_some_and(|effect| !beacon::valid_primary(levels, effect))
            || secondary.is_some_and(|effect| !beacon::valid_secondary(levels, effect))
        {
            return;
        }
        if !menu.confirm_beacon(primary, secondary) {
            return;
        }
        if let (Some(OpenAt::Beacon(pos)), Some(source)) =
            (self.menus.open_at.clone(), self.source.as_ref())
        {
            let mut data = beacon::load(source, pos);
            data.levels = levels;
            data.primary_effect = primary.and_then(beacon::effect_name).map(str::to_owned);
            data.secondary_effect = secondary.and_then(beacon::effect_name).map(str::to_owned);
            beacon::store(source, pos, &data);
        }
        let out = self
            .menus
            .open
            .as_mut()
            .map(|menu| menu.broadcast_changes(&self.inventory))
            .unwrap_or_default();
        emit(self, out);
    }

    /// Writes an open station menu's items and timers back to its block entity.
    fn save_station(&mut self) {
        let Some(at) = self.menus.open_at.clone() else {
            return;
        };
        let (Some(menu), Some(source)) = (self.menus.open.as_ref(), self.source.as_ref()) else {
            return;
        };
        if let OpenAt::Furnace(pos) = &at
            && let Some(kind) = block_name(source, *pos).and_then(furnace::kind)
        {
            let data = furnace::FurnaceData {
                items: menu.container().to_vec(),
                state: menu.furnace_state().cloned().unwrap_or_default(),
            };
            furnace::store(source, *pos, kind, &data);
        }
        if let OpenAt::BrewingStand(pos) = &at
            && block_name(source, *pos) == Some("minecraft:brewing_stand")
        {
            let data = brewing::BrewingData {
                items: menu.container().to_vec(),
                state: menu.brewing_state().cloned().unwrap_or_default(),
            };
            brewing::store(source, *pos, &data);
        }
        if let OpenAt::Beacon(pos) = &at
            && let Some(state) = menu.beacon_state()
        {
            let mut data = beacon::load(source, *pos);
            data.levels = state.levels;
            data.primary_effect = state
                .primary
                .and_then(beacon::effect_name)
                .map(str::to_owned);
            data.secondary_effect = state
                .secondary
                .and_then(beacon::effect_name)
                .map(str::to_owned);
            beacon::store(source, *pos, &data);
        }
    }

    /// Copies a shared container's current items into the open menu.
    fn load_shared_contents(&mut self) {
        if let (Some(OpenAt::Inventory(opened)), Some(source), Some(menu)) =
            (&self.menus.open_at, &self.source, self.menus.open.as_mut())
        {
            let items = block_container::load(source, opened);
            if items.len() == menu.container().len() {
                menu.container_mut().clone_from_slice(&items);
            }
        }
    }

    /// Vanilla `doCloseContainer`: the open menu (or the inventory screen's grid and cursor)
    /// returns its items, and the inventory menu takes over what the client was shown.
    pub fn close_container(&mut self) {
        if let Some(plugins) = self.plugins.clone() {
            let _ = plugins.inventory_closed(self.uuid, &self.name);
        }
        self.save_station();
        let infinite = self.infinite_materials();
        let mut clicker = Clicker::new(&mut self.inventory, infinite, crafter());
        self.menus.open_at = None;
        match self.menus.open.take() {
            Some(mut open) => {
                open.close(&mut clicker);
                self.menus.inventory.transfer_state(&open);
            }
            None => self.menus.inventory.close(&mut clicker),
        }
        let drops = clicker.drops;
        self.menus.thrown.extend(drops);
    }

    /// Opens a container screen (vanilla `ServerPlayer.openMenu`): any open menu closes first,
    /// then the client gets `open_screen` and the menu's full contents. `at` is the block the
    /// menu belongs to.
    pub fn open_menu(&mut self, make: impl FnOnce(i32) -> Menu, title: &str, at: Option<OpenAt>) {
        self.close_open_container();
        let id = self.menus.next_id();
        let mut menu = make(id);
        let Some(kind) = menu.kind().menu_type() else {
            return;
        };
        let Some(menu_type) = ferrite_data::registries::id("minecraft:menu", &kind) else {
            tracing::warn!(kind, "unknown menu type");
            return;
        };
        send(
            self,
            &OpenScreen {
                window: VarInt(id),
                menu: VarInt(menu_type),
                title: ferrite_protocol::types::Translatable(title.into()),
            },
        );
        let full = menu.broadcast_full_state(&self.inventory);
        // Container data (furnace lit/progress, anvil cost, ...) follows the contents.
        let data = menu.broadcast_changes(&self.inventory);
        self.menus.open = Some(menu);
        self.menus.open_at = at;
        emit(self, std::iter::once(full).chain(data).collect());
    }

    /// Vanilla `useWithoutItem` of blocks with a menu: a crafting table opens `CraftingMenu`
    /// (`CraftingTableBlock`), a chest or barrel a `ChestMenu` over its items (`ChestBlock`,
    /// `BarrelBlock`). Using them opens the menu instead of placing the held item; returns
    /// whether the block was used.
    pub fn use_block(&mut self, location: i64) -> bool {
        // Doors, buttons, beds, signs, buckets and cauldrons first; menus follow.
        if super::blocks::use_block(self, location) {
            return true;
        }
        let pos = ferrite_protocol::types::BlockPos::unpack(location);
        let Some(source) = self.source.clone() else {
            return false;
        };
        let table = ferrite_data::blocks::default_state("crafting_table").map(u32::from);
        let state = source.block(pos.x, pos.y, pos.z);
        if state.is_some() && state == table {
            self.open_menu(
                Menu::crafting,
                "container.crafting",
                Some(OpenAt::Table([pos.x, pos.y, pos.z])),
            );
            return true;
        }
        let station = match block_name(&source, [pos.x, pos.y, pos.z]) {
            Some("minecraft:enchanting_table") => Some((
                Menu::enchanting as fn(i32) -> Menu,
                "container.enchant",
                "minecraft:enchanting_table",
            )),
            Some("minecraft:grindstone") => Some((
                Menu::grindstone as fn(i32) -> Menu,
                "container.grindstone_title",
                "minecraft:grindstone",
            )),
            Some("minecraft:stonecutter") => Some((
                Menu::stonecutter as fn(i32) -> Menu,
                "container.stonecutter",
                "minecraft:stonecutter",
            )),
            Some("minecraft:smithing_table") => Some((
                Menu::smithing as fn(i32) -> Menu,
                "container.upgrade",
                "minecraft:smithing_table",
            )),
            Some("minecraft:loom") => Some((
                Menu::loom as fn(i32) -> Menu,
                "container.loom",
                "minecraft:loom",
            )),
            _ => None,
        };
        if let Some((make, title, block)) = station {
            self.open_menu(
                make,
                title,
                Some(OpenAt::Block([pos.x, pos.y, pos.z], block)),
            );
            return true;
        }
        if block_name(&source, [pos.x, pos.y, pos.z]) == Some("minecraft:anvil") {
            self.open_menu(
                Menu::anvil,
                "container.anvil",
                Some(OpenAt::Anvil([pos.x, pos.y, pos.z])),
            );
            return true;
        }
        if let Some(kind) = block_name(&source, [pos.x, pos.y, pos.z]).and_then(furnace::kind) {
            let furnace::FurnaceData { items, state } =
                furnace::load(&source, [pos.x, pos.y, pos.z]);
            self.open_menu(
                |id| {
                    let mut menu = Menu::furnace(id, kind);
                    menu.container_mut().clone_from_slice(&items);
                    menu.set_furnace_state(state);
                    menu
                },
                kind.title(),
                Some(OpenAt::Furnace([pos.x, pos.y, pos.z])),
            );
            return true;
        }
        if block_name(&source, [pos.x, pos.y, pos.z]) == Some("minecraft:beacon") {
            let levels = beacon::scan_levels(&source, [pos.x, pos.y, pos.z]);
            let mut data = beacon::load(&source, [pos.x, pos.y, pos.z]);
            if data.levels != levels {
                data.levels = levels;
                beacon::store(&source, [pos.x, pos.y, pos.z], &data);
            }
            let state = ferrite_inventory::BeaconState {
                levels,
                primary: data.primary_effect.as_deref().and_then(beacon::effect_id),
                secondary: data.secondary_effect.as_deref().and_then(beacon::effect_id),
            };
            self.open_menu(
                |id| {
                    let mut menu = Menu::beacon(id);
                    menu.set_beacon_state(state);
                    menu
                },
                "container.beacon",
                Some(OpenAt::Beacon([pos.x, pos.y, pos.z])),
            );
            return true;
        }
        if block_name(&source, [pos.x, pos.y, pos.z]) == Some("minecraft:brewing_stand") {
            let brewing::BrewingData { items, state } =
                brewing::load(&source, [pos.x, pos.y, pos.z]);
            self.open_menu(
                |id| {
                    let mut menu = Menu::brewing_stand(id);
                    menu.container_mut().clone_from_slice(&items);
                    menu.set_brewing_state(state);
                    menu
                },
                "container.brewing",
                Some(OpenAt::BrewingStand([pos.x, pos.y, pos.z])),
            );
            return true;
        }
        let Some(opened) = block_container::at(&source, [pos.x, pos.y, pos.z]) else {
            return false;
        };
        let items = block_container::load(&source, &opened);
        let (rows, title) = (opened.rows, opened.title);
        self.open_menu(
            |id| {
                let mut menu = Menu::generic(id, rows);
                menu.container_mut().clone_from_slice(&items);
                menu
            },
            title,
            Some(OpenAt::Inventory(opened)),
        );
        true
    }

    /// Vanilla `handleSetCreativeModeSlot`: slots 1..=45 of the inventory menu, or a drop for a
    /// negative slot.
    pub fn handle_creative_slot(&mut self, p: CreativeSlot<ItemStack>) {
        if !self.infinite_materials() {
            return;
        }
        let stack = p.item;
        let valid = stack.is_empty() || stack.count <= stack.max_stack_size();
        if !valid {
            return;
        }
        if (1..=45).contains(&p.slot) {
            let slot = p.slot as usize;
            self.menus
                .inventory
                .set_slot(&mut self.inventory, crafter(), slot, stack.clone());
            self.menus.inventory.set_remote_slot(slot, &stack);
            let out = self.menus.inventory.broadcast_changes(&self.inventory);
            emit(self, out);
        } else if p.slot < 0 && !stack.is_empty() {
            self.menus.thrown.push(stack);
        }
    }

    /// Vanilla `ServerPlayer.drop(all)` for the Q key: drops one or all of the selected stack.
    /// The client already removed it, so the slot is not sent back.
    pub fn drop_selected(&mut self, all: bool) {
        if self.gamemode == 3 {
            return;
        }
        let selected = self.inventory.selected_slot();
        let held = self.inventory.get(selected).count;
        if held <= 0 {
            return;
        }
        let dropped = self.inventory.remove(selected, if all { held } else { 1 });
        let left = self.inventory.get(selected).clone();
        let menu = self.menus.current();
        if let Some(slot) = menu.menu_slot_of(selected) {
            menu.set_remote_slot(slot, &left);
        }
        self.menus.thrown.push(dropped);
    }

    /// Vanilla `SWAP_ITEM_WITH_OFFHAND`.
    pub fn swap_hands(&mut self) {
        if self.gamemode == 3 {
            return;
        }
        let selected = self.inventory.selected_slot();
        let main = self.inventory.get(selected).clone();
        let off = self.inventory.get(ferrite_inventory::OFFHAND).clone();
        self.inventory.set(selected, off);
        self.inventory.set(ferrite_inventory::OFFHAND, main);
    }

    /// The stack in a hand: 0 the main hand, 1 the offhand.
    pub fn hand_slot(&self, hand: i32) -> usize {
        if hand == 0 {
            self.inventory.selected_slot()
        } else {
            ferrite_inventory::OFFHAND
        }
    }
}

/// The player's inventory as stored and restored by [`super::playerdata`].
pub fn restore(saved: Option<&super::playerdata::Saved>) -> PlayerInventory {
    saved.map_or_else(PlayerInventory::new, super::playerdata::Saved::inventory)
}
