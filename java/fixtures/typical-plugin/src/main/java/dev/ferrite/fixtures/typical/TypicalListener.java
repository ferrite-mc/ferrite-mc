package dev.ferrite.fixtures.typical;

import org.bukkit.Material;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.event.block.Action;
import org.bukkit.event.block.BlockBreakEvent;
import org.bukkit.event.player.PlayerCommandPreprocessEvent;
import org.bukkit.event.player.PlayerInteractEvent;
import org.bukkit.event.player.PlayerJoinEvent;
import org.bukkit.event.player.PlayerQuitEvent;

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

    @EventHandler(priority = EventPriority.MONITOR)
    public void onJoinMonitor(PlayerJoinEvent event) {
        plugin.record("join:monitor:" + event.getPlayer().getName()
                + ":world=" + event.getPlayer().getWorld().getName()
                + ":thread=" + Thread.currentThread().getName());
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
