package dev.ferrite.fixtures.typical;

import java.util.List;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.event.block.Action;
import org.bukkit.event.block.BlockBreakEvent;
import org.bukkit.event.player.PlayerCommandPreprocessEvent;
import org.bukkit.event.player.PlayerInteractEvent;
import org.bukkit.event.player.PlayerJoinEvent;
import org.bukkit.event.player.PlayerQuitEvent;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.PlayerInventory;
import org.bukkit.inventory.meta.Damageable;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.persistence.PersistentDataType;

/** Listener surface the Lane 2 fixture relies on. */
public final class TypicalListener implements Listener {

    private final TypicalPlugin plugin;

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
