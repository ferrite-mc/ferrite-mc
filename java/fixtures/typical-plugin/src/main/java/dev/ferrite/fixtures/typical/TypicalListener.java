package dev.ferrite.fixtures.typical;

import java.util.List;
import java.util.Map;
import net.kyori.adventure.text.Component;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.event.block.Action;
import org.bukkit.event.block.BlockBreakEvent;
import org.bukkit.event.inventory.ClickType;
import org.bukkit.event.inventory.DragType;
import org.bukkit.event.inventory.InventoryClickEvent;
import org.bukkit.event.inventory.InventoryDragEvent;
import org.bukkit.event.inventory.InventoryType;
import org.bukkit.event.player.PlayerCommandPreprocessEvent;
import org.bukkit.event.player.PlayerInteractEvent;
import org.bukkit.event.player.PlayerJoinEvent;
import org.bukkit.event.player.PlayerQuitEvent;
import org.bukkit.inventory.Inventory;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.PlayerInventory;
import org.bukkit.inventory.meta.Damageable;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.persistence.PersistentDataType;

/** Listener surface the Lane 2 fixture relies on. */
public final class TypicalListener implements Listener {

    private final TypicalPlugin plugin;

    private volatile boolean cancelGuiWrites;

    public TypicalListener(TypicalPlugin plugin) {
        this.plugin = plugin;
    }

    @EventHandler(priority = EventPriority.LOWEST)
    public void onJoinLowest(PlayerJoinEvent event) {
        plugin.record("join:lowest:" + event.getPlayer().getName());
    }

    @EventHandler(priority = EventPriority.NORMAL)
    public void onJoinInventory(PlayerJoinEvent event) {
        inventoryProbe(event.getPlayer());
        guiProbe(event.getPlayer());
        Player joined = event.getPlayer();
        plugin.getServer().getScheduler().runTaskLater(plugin, () -> worldReadsProbe(joined), 10L);
    }

    private void worldReadsProbe(Player player) {
        var world = player.getWorld();
        var chunk = world.getChunkAt(player.getLocation().getBlock());
        boolean loaded = world.isChunkLoaded(chunk);
        boolean generated = world.isChunkGenerated(chunk.getX(), chunk.getZ());
        boolean inUse = world.isChunkInUse(chunk.getX(), chunk.getZ());
        boolean seeing = world.getPlayersSeeingChunk(chunk).contains(player);
        boolean foundEntity = world.getEntity(player.getUniqueId()) == player;
        boolean living = world.getLivingEntities().contains(player);
        boolean nearby = world.getNearbyEntities(player.getLocation(), 2.0, 2.0, 2.0).contains(player);
        plugin.record("world-reads:loaded=" + (loaded && generated)
                + ":inUse=" + inUse
                + ":seeing=" + seeing
                + ":entity=" + foundEntity
                + ":living=" + living
                + ":nearby=" + nearby);
    }

    @EventHandler(priority = EventPriority.MONITOR)
    public void onJoinMonitor(PlayerJoinEvent event) {
        plugin.record("join:monitor:" + event.getPlayer().getName()
                + ":world=" + event.getPlayer().getWorld().getName()
                + ":thread=" + Thread.currentThread().getName());
    }

    private void inventoryProbe(Player player) {
        PlayerInventory inventory = player.getInventory();
        NamespacedKey key = new NamespacedKey(plugin, "inv_tag");
        ItemStack original = new ItemStack(Material.DIAMOND_SWORD, 1);
        ItemMeta meta = original.getItemMeta();
        meta.setDisplayName("Inventory Blade");
        meta.setLore(List.of("inv lore"));
        ((Damageable) meta).setDamage(4);
        meta.getPersistentDataContainer().set(key, PersistentDataType.INTEGER, 77);
        original.setItemMeta(meta);

        inventory.setItemInMainHand(original);
        // Mutating original after storing must not mutate the inventory copy.
        original.editMeta(m -> m.setDisplayName("MutatedOriginal"));

        ItemStack inHand = inventory.getItemInMainHand();
        plugin.record("inventory:main-hand="
                + inHand.getItemMeta().getDisplayName()
                + ":lore=" + inHand.getLore()
                + ":damage=" + inHand.getDurability()
                + ":pdc=" + inHand.getPersistentDataContainer().get(key, PersistentDataType.INTEGER));

        // Mutating item read from inventory must not mutate the inventory copy.
        inHand.editMeta(m -> m.setDisplayName("MutatedRead"));
        plugin.record("inventory:copy-isolated="
                + "Inventory Blade".equals(inventory.getItemInMainHand().getItemMeta().getDisplayName()));

        ItemStack gems = new ItemStack(Material.EMERALD, 5);
        gems.editPersistentDataContainer(pdc -> pdc.set(key, PersistentDataType.INTEGER, 9));
        var addOverflow = inventory.addItem(gems);
        var removeOverflow = inventory.removeItem(new ItemStack(gems).asQuantity(2));
        int emeraldCount = inventory.all(Material.EMERALD).values().stream()
                .mapToInt(ItemStack::getAmount)
                .sum();
        plugin.record("inventory:add-remove=addOverflow=" + addOverflow.size()
                + ":removeOverflow=" + removeOverflow.size()
                + ":remaining=" + emeraldCount);
    }

    private void guiProbe(Player player) {
        Inventory menu = plugin.getServer().createInventory(null, 9, Component.text("Ferrite GUI"));
        menu.setItem(0, new ItemStack(Material.GOLD_INGOT, 2));
        org.bukkit.inventory.InventoryView view = player.openInventory(menu);
        if (view == null) {
            plugin.record("gui:error=null-view");
            return;
        }
        try {
            var clickMethod = view.getClass().getMethod("click", int.class, ClickType.class, int.class);
            var dragMethod = view.getClass().getMethod("drag", DragType.class, Map.class, ItemStack.class);

            // Cancelled click + drag must roll back atomically.
            cancelGuiWrites = true;
            boolean clickCancelled = ((InventoryClickEvent) clickMethod.invoke(view, 0, ClickType.LEFT, -1)).isCancelled();
            boolean dragCancelled = ((InventoryDragEvent) dragMethod.invoke(
                    view, DragType.SINGLE, Map.of(1, new ItemStack(Material.GOLD_INGOT, 1)), null)).isCancelled();
            boolean rolledBack = clickCancelled
                    && dragCancelled
                    && menu.getItem(0) != null
                    && menu.getItem(0).getAmount() == 2
                    && menu.getItem(1) == null;

            // Committed shift-click moves gold into player inventory.
            cancelGuiWrites = false;
            boolean committed = !((InventoryClickEvent) clickMethod.invoke(view, 0, ClickType.SHIFT_LEFT, -1)).isCancelled()
                    && menu.getItem(0) == null
                    && player.getInventory().contains(Material.GOLD_INGOT, 2);

            // Reopen with a barrel and close cleanly.
            Inventory barrel = plugin.getServer().createInventory(null, InventoryType.BARREL);
            player.openInventory(barrel);
            boolean reopened = menu.getViewers().isEmpty() && barrel.getViewers().size() == 1;
            player.closeInventory();
            boolean closed = barrel.getViewers().isEmpty();

            plugin.record("gui:rollback=" + rolledBack
                    + ":committed=" + committed
                    + ":reopen-closed=" + (reopened && closed));
        } catch (ReflectiveOperationException error) {
            plugin.record("gui:error=" + error);
        }
    }

    @EventHandler
    public void onInventoryClick(InventoryClickEvent event) {
        if (cancelGuiWrites) {
            event.setCancelled(true);
        }
    }

    @EventHandler
    public void onInventoryDrag(InventoryDragEvent event) {
        if (cancelGuiWrites) {
            event.setCancelled(true);
        }
    }

    @EventHandler
    public void onQuit(PlayerQuitEvent event) {
        plugin.record("quit:" + event.getPlayer().getName());
    }

    @EventHandler(ignoreCancelled = true)
    public void onBlockBreak(BlockBreakEvent event) {
        if (event.getBlock().getType() == Material.DIAMOND_BLOCK) {
            event.setCancelled(true);
            plugin.record("break:cancelled:" + event.getBlock().getType().name());
        } else {
            plugin.record("break:allowed:" + event.getBlock().getType().name());
        }
    }

    @EventHandler(ignoreCancelled = true)
    public void onInteract(PlayerInteractEvent event) {
        if (event.getAction() == Action.RIGHT_CLICK_BLOCK
                && event.getClickedBlock() != null
                && event.getClickedBlock().getType() == Material.DIAMOND_BLOCK) {
            event.setCancelled(true);
            plugin.record("interact:cancelled");
        } else {
            plugin.record("interact:seen:" + event.getAction());
        }
    }

    @EventHandler
    public void onCommandPreprocess(PlayerCommandPreprocessEvent event) {
        plugin.record("preprocess:" + event.getMessage());
        if (event.getMessage().contains("blocked-command")) {
            event.setCancelled(true);
        }
    }
}
