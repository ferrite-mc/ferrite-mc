package dev.ferrite.api.inventory;

import dev.ferrite.api.FerriteRuntime;
import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.entity.FerritePlayerInventory;
import dev.ferrite.api.internal.Unsupported;
import java.util.HashMap;
import java.util.Map;
import java.util.Objects;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer;
import org.bukkit.GameMode;
import org.bukkit.Material;
import org.bukkit.entity.HumanEntity;
import org.bukkit.event.inventory.ClickType;
import org.bukkit.event.inventory.DragType;
import org.bukkit.event.inventory.InventoryAction;
import org.bukkit.event.inventory.InventoryClickEvent;
import org.bukkit.event.inventory.InventoryCloseEvent;
import org.bukkit.event.inventory.InventoryDragEvent;
import org.bukkit.event.inventory.InventoryType;
import org.bukkit.inventory.Inventory;
import org.bukkit.inventory.InventoryView;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.MenuType;

/**
 * Transactional {@link InventoryView} connecting a {@link FerritePlayer}'s inventory to a
 * container or custom plugin inventory, with atomic rollback on cancelled click/drag events.
 */
public final class FerriteInventoryView implements InventoryView {

    private static final int PLAYER_MAIN_SLOTS = 36;
    private static final int PLAYER_OFFHAND_SLOT = 40;

    private final FerriteRuntime runtime;
    private final FerritePlayer player;
    private final Inventory topInventory;
    private final FerritePlayerInventory bottomInventory;
    private final Component originalTitle;
    private Component title;
    private ItemStack cursor;
    private boolean closed;

    public FerriteInventoryView(
            FerriteRuntime runtime,
            FerritePlayer player,
            Inventory topInventory,
            FerritePlayerInventory bottomInventory,
            Component title) {
        this.runtime = Objects.requireNonNull(runtime, "runtime");
        this.player = Objects.requireNonNull(player, "player");
        this.topInventory = Objects.requireNonNull(topInventory, "topInventory");
        this.bottomInventory = Objects.requireNonNull(bottomInventory, "bottomInventory");
        this.originalTitle = title == null
                ? Component.text(topInventory.getType().getDefaultTitle())
                : title;
        this.title = this.originalTitle;
    }

    @Override
    public Inventory getTopInventory() {
        return topInventory;
    }

    @Override
    public Inventory getBottomInventory() {
        return bottomInventory;
    }

    @Override
    public HumanEntity getPlayer() {
        return player;
    }

    @Override
    public InventoryType getType() {
        return topInventory.getType();
    }

    @Override
    public Component title() {
        return title;
    }

    @Override
    public String getTitle() {
        return LegacyComponentSerializer.legacySection().serialize(title);
    }

    @Override
    public String getOriginalTitle() {
        return LegacyComponentSerializer.legacySection().serialize(originalTitle);
    }

    @Override
    public void setTitle(String title) {
        this.title = title == null ? Component.empty() : LegacyComponentSerializer.legacySection().deserialize(title);
    }

    @Override
    public ItemStack getItem(int rawSlot) {
        if (rawSlot == OUTSIDE || rawSlot < 0) {
            return null;
        }
        int topSize = topInventory.getSize();
        if (rawSlot < topSize) {
            return topInventory.getItem(rawSlot);
        }
        int converted = convertSlot(rawSlot);
        if (converted < 0 || converted >= bottomInventory.getSize()) {
            return null;
        }
        return bottomInventory.getItem(converted);
    }

    @Override
    public void setItem(int rawSlot, ItemStack item) {
        if (rawSlot == OUTSIDE || rawSlot < 0) {
            return;
        }
        int topSize = topInventory.getSize();
        if (rawSlot < topSize) {
            topInventory.setItem(rawSlot, item);
            return;
        }
        int converted = convertSlot(rawSlot);
        if (converted >= 0 && converted < bottomInventory.getSize()) {
            bottomInventory.setItem(converted, item);
        }
    }

    @Override
    public void setCursor(ItemStack item) {
        this.cursor = copyOrNull(item);
    }

    @Override
    public ItemStack getCursor() {
        return copyOrNull(cursor);
    }

    @Override
    public Inventory getInventory(int rawSlot) {
        if (rawSlot == OUTSIDE || rawSlot < 0) {
            return null;
        }
        if (rawSlot < topInventory.getSize()) {
            return topInventory;
        }
        return rawSlot < countSlots() ? bottomInventory : null;
    }

    @Override
    public int convertSlot(int rawSlot) {
        if (rawSlot < 0) {
            return OUTSIDE;
        }
        int topSize = topInventory.getSize();
        if (rawSlot < topSize) {
            return rawSlot;
        }
        int rel = rawSlot - topSize;
        if (rel >= PLAYER_MAIN_SLOTS) {
            return OUTSIDE;
        }
        // Standard chest/container bottom mapping: 0..26 main (9..35), 27..35 hotbar (0..8).
        return rel < 27 ? rel + 9 : rel - 27;
    }

    @Override
    public InventoryType.SlotType getSlotType(int rawSlot) {
        if (rawSlot == OUTSIDE || rawSlot < 0) {
            return InventoryType.SlotType.OUTSIDE;
        }
        if (rawSlot < topInventory.getSize()) {
            return switch (topInventory.getType()) {
                case FURNACE, BLAST_FURNACE, SMOKER -> rawSlot == 1
                        ? InventoryType.SlotType.FUEL
                        : rawSlot == 2 ? InventoryType.SlotType.RESULT : InventoryType.SlotType.CONTAINER;
                case WORKBENCH, CRAFTING, ANVIL, MERCHANT ->
                    rawSlot == 0 || (topInventory.getType() != InventoryType.WORKBENCH && topInventory.getType() != InventoryType.CRAFTING && rawSlot == 2)
                            ? InventoryType.SlotType.RESULT
                            : InventoryType.SlotType.CRAFTING;
                default -> InventoryType.SlotType.CONTAINER;
            };
        }
        int converted = convertSlot(rawSlot);
        return converted >= 0 && converted < 9
                ? InventoryType.SlotType.QUICKBAR
                : InventoryType.SlotType.CONTAINER;
    }

    @Override
    public void open() {
        if (closed) {
            throw new IllegalStateException("Cannot reopen a closed InventoryView");
        }
        player.openInventory(this);
    }

    @Override
    public void close() {
        close(InventoryCloseEvent.Reason.PLUGIN);
    }

    public boolean isClosed() {
        return closed;
    }

    /** Closes this view with {@code reason}, firing {@link InventoryCloseEvent}. */
    public void close(InventoryCloseEvent.Reason reason) {
        if (closed) {
            return;
        }
        closed = true;
        if (topInventory instanceof FerriteCustomInventory custom) {
            custom.removeViewer(player);
        }
        runtime.plugins().callEvent(new InventoryCloseEvent(this, reason));
        player.clearOpenViewIf(this);
    }

    @Override
    public int countSlots() {
        return topInventory.getSize() + PLAYER_MAIN_SLOTS;
    }

    @Override
    public boolean setProperty(Property prop, int value) {
        return false;
    }

    @Override
    public MenuType getMenuType() {
        // The facade stores container sizes, not the client's menu descriptor ids.
        throw Unsupported.exception("InventoryView#getMenuType");
    }

    /**
     * Executes a transactional slot click (`LEFT`, `RIGHT`, `SHIFT_LEFT`, `SHIFT_RIGHT`,
     * `NUMBER_KEY`, `SWAP_OFFHAND`, `DROP`, `CONTROL_DROP`, `MIDDLE`). Cancelled clicks roll back
     * atomically to the pre-click top/bottom/cursor state.
     */
    public InventoryClickEvent click(int rawSlot, ClickType clickType, int hotbarButton) {
        Objects.requireNonNull(clickType, "clickType");
        if (closed) {
            throw new IllegalStateException("Cannot click a closed InventoryView");
        }
        if (!runtime.isPrimaryThread()) {
            throw new IllegalStateException("Inventory click must run on the owning server/region thread");
        }

        Snapshot pre = captureSnapshot();
        InventoryAction action = applyClickMutation(rawSlot, clickType, hotbarButton);
        InventoryType.SlotType slotType = getSlotType(rawSlot);
        InventoryClickEvent event = hotbarButton >= 0
                ? new InventoryClickEvent(this, slotType, rawSlot, clickType, action, hotbarButton)
                : new InventoryClickEvent(this, slotType, rawSlot, clickType, action);
        runtime.plugins().callEvent(event);
        if (event.isCancelled()) {
            restoreSnapshot(pre);
        } else {
            bottomInventory.applySnapshot(bottomInventory.snapshot());
        }
        return event;
    }

    /**
     * Executes a transactional drag (`EVEN` or `SINGLE`). Cancelled drags roll back atomically.
     */
    public InventoryDragEvent drag(
            DragType dragType, Map<Integer, ItemStack> newItems, ItemStack nextCursor) {
        Objects.requireNonNull(dragType, "dragType");
        Objects.requireNonNull(newItems, "newItems");
        if (closed) {
            throw new IllegalStateException("Cannot drag in a closed InventoryView");
        }
        if (!runtime.isPrimaryThread()) {
            throw new IllegalStateException("Inventory drag must run on the owning server/region thread");
        }

        Snapshot pre = captureSnapshot();
        ItemStack oldCursor = copyOrNull(cursor);
        Map<Integer, ItemStack> applied = new HashMap<>();
        for (Map.Entry<Integer, ItemStack> entry : newItems.entrySet()) {
            int rawSlot = entry.getKey();
            if (rawSlot < 0 || rawSlot >= countSlots()) {
                restoreSnapshot(pre);
                throw new IndexOutOfBoundsException("Drag rawSlot out of bounds: " + rawSlot);
            }
            ItemStack copy = copyOrNull(entry.getValue());
            setItem(rawSlot, copy);
            if (copy != null) {
                applied.put(rawSlot, copy);
            }
        }
        this.cursor = copyOrNull(nextCursor);

        InventoryDragEvent event = new InventoryDragEvent(
                this,
                copyOrNull(this.cursor),
                oldCursor,
                dragType == DragType.SINGLE,
                applied);
        runtime.plugins().callEvent(event);
        if (event.isCancelled()) {
            restoreSnapshot(pre);
        } else {
            bottomInventory.applySnapshot(bottomInventory.snapshot());
        }
        return event;
    }

    private InventoryAction applyClickMutation(int rawSlot, ClickType clickType, int hotbarButton) {
        if (rawSlot == OUTSIDE) {
            if (cursor != null && (clickType == ClickType.LEFT || clickType == ClickType.RIGHT)) {
                if (clickType == ClickType.LEFT) {
                    cursor = null;
                    return InventoryAction.DROP_ALL_CURSOR;
                }
                int next = cursor.getAmount() - 1;
                cursor = next <= 0 ? null : withAmount(cursor, next);
                return InventoryAction.DROP_ONE_CURSOR;
            }
            return InventoryAction.NOTHING;
        }
        if (rawSlot < 0 || rawSlot >= countSlots()) {
            throw new IndexOutOfBoundsException("rawSlot " + rawSlot + " out of bounds 0.." + countSlots());
        }

        ItemStack slotItem = getItem(rawSlot);
        switch (clickType) {
            case LEFT -> {
                if (cursor == null && slotItem == null) {
                    return InventoryAction.NOTHING;
                }
                if (cursor == null) {
                    setCursor(slotItem);
                    setItem(rawSlot, null);
                    return InventoryAction.PICKUP_ALL;
                }
                if (slotItem == null) {
                    setItem(rawSlot, cursor);
                    setCursor(null);
                    return InventoryAction.PLACE_ALL;
                }
                if (slotItem.isSimilar(cursor)) {
                    int max = Math.max(1, slotItem.getMaxStackSize());
                    int space = max - slotItem.getAmount();
                    if (space <= 0) {
                        return InventoryAction.NOTHING;
                    }
                    int transfer = Math.min(space, cursor.getAmount());
                    setItem(rawSlot, withAmount(slotItem, slotItem.getAmount() + transfer));
                    int left = cursor.getAmount() - transfer;
                    setCursor(left <= 0 ? null : withAmount(cursor, left));
                    return left <= 0 ? InventoryAction.PLACE_ALL : InventoryAction.PLACE_SOME;
                }
                setItem(rawSlot, cursor);
                setCursor(slotItem);
                return InventoryAction.SWAP_WITH_CURSOR;
            }
            case RIGHT -> {
                if (cursor == null) {
                    if (slotItem == null) {
                        return InventoryAction.NOTHING;
                    }
                    int take = (slotItem.getAmount() + 1) / 2;
                    int remain = slotItem.getAmount() - take;
                    setCursor(withAmount(slotItem, take));
                    setItem(rawSlot, remain <= 0 ? null : withAmount(slotItem, remain));
                    return InventoryAction.PICKUP_HALF;
                }
                if (slotItem == null) {
                    setItem(rawSlot, withAmount(cursor, 1));
                    int left = cursor.getAmount() - 1;
                    setCursor(left <= 0 ? null : withAmount(cursor, left));
                    return InventoryAction.PLACE_ONE;
                }
                if (slotItem.isSimilar(cursor) && slotItem.getAmount() < Math.max(1, slotItem.getMaxStackSize())) {
                    setItem(rawSlot, withAmount(slotItem, slotItem.getAmount() + 1));
                    int left = cursor.getAmount() - 1;
                    setCursor(left <= 0 ? null : withAmount(cursor, left));
                    return InventoryAction.PLACE_ONE;
                }
                setItem(rawSlot, cursor);
                setCursor(slotItem);
                return InventoryAction.SWAP_WITH_CURSOR;
            }
            case SHIFT_LEFT, SHIFT_RIGHT -> {
                if (slotItem == null) {
                    return InventoryAction.NOTHING;
                }
                Inventory target = rawSlot < topInventory.getSize() ? bottomInventory : topInventory;
                HashMap<Integer, ItemStack> leftover = target.addItem(slotItem);
                if (leftover.isEmpty()) {
                    setItem(rawSlot, null);
                } else {
                    setItem(rawSlot, leftover.get(0));
                }
                return InventoryAction.MOVE_TO_OTHER_INVENTORY;
            }
            case NUMBER_KEY -> {
                if (hotbarButton < 0 || hotbarButton > 8) {
                    throw new IllegalArgumentException("hotbarButton must be 0..8, got " + hotbarButton);
                }
                ItemStack hotbar = bottomInventory.getItem(hotbarButton);
                setItem(rawSlot, hotbar);
                bottomInventory.setItem(hotbarButton, slotItem);
                return InventoryAction.HOTBAR_SWAP;
            }
            case SWAP_OFFHAND -> {
                ItemStack offhand = bottomInventory.getItem(PLAYER_OFFHAND_SLOT);
                setItem(rawSlot, offhand);
                bottomInventory.setItem(PLAYER_OFFHAND_SLOT, slotItem);
                return InventoryAction.HOTBAR_SWAP;
            }
            case DROP -> {
                if (slotItem == null) {
                    return InventoryAction.NOTHING;
                }
                int remain = slotItem.getAmount() - 1;
                setItem(rawSlot, remain <= 0 ? null : withAmount(slotItem, remain));
                return InventoryAction.DROP_ONE_SLOT;
            }
            case CONTROL_DROP -> {
                if (slotItem == null) {
                    return InventoryAction.NOTHING;
                }
                setItem(rawSlot, null);
                return InventoryAction.DROP_ALL_SLOT;
            }
            case MIDDLE -> {
                if (player.getGameMode() == GameMode.CREATIVE && cursor == null && slotItem != null) {
                    ItemStack cloned = slotItem.clone();
                    cloned.setAmount(Math.max(1, cloned.getMaxStackSize()));
                    setCursor(cloned);
                    return InventoryAction.CLONE_STACK;
                }
                return InventoryAction.NOTHING;
            }
            // UNKNOWN and any future click type have no transactional facade semantics.
            default -> throw Unsupported.exception("InventoryView#click(" + clickType.name() + ")");
        }
    }

    private Snapshot captureSnapshot() {
        return new Snapshot(
                topInventory.getContents(),
                bottomInventory.snapshot(),
                copyOrNull(cursor));
    }

    private void restoreSnapshot(Snapshot snapshot) {
        topInventory.setContents(snapshot.topContents);
        bottomInventory.applySnapshot(snapshot.bottomSnapshot);
        this.cursor = copyOrNull(snapshot.cursor);
    }

    private record Snapshot(
            ItemStack[] topContents, PlayerInventorySnapshot bottomSnapshot, ItemStack cursor) {}

    private static ItemStack copyOrNull(ItemStack stack) {
        if (stack == null || stack.getType() == Material.AIR || stack.getAmount() <= 0) {
            return null;
        }
        return stack.clone();
    }

    private static ItemStack withAmount(ItemStack stack, int amount) {
        ItemStack copy = stack.clone();
        copy.setAmount(amount);
        return copy;
    }
}
