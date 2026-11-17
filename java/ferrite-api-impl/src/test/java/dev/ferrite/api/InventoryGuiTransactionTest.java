package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.inventory.FerriteInventoryView;
import dev.ferrite.api.test.TestPluginFactory;
import dev.ferrite.api.test.TestServer;
import dev.ferrite.api.world.FerriteWorld;
import java.io.StringReader;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.atomic.AtomicBoolean;
import net.kyori.adventure.text.Component;
import org.bukkit.GameMode;
import org.bukkit.Material;
import org.bukkit.event.EventHandler;
import org.bukkit.event.Listener;
import org.bukkit.event.inventory.ClickType;
import org.bukkit.event.inventory.DragType;
import org.bukkit.event.inventory.InventoryClickEvent;
import org.bukkit.event.inventory.InventoryCloseEvent;
import org.bukkit.event.inventory.InventoryDragEvent;
import org.bukkit.event.inventory.InventoryType;
import org.bukkit.inventory.Inventory;
import org.bukkit.inventory.ItemStack;
import org.bukkit.plugin.PluginDescriptionFile;
import org.junit.jupiter.api.Test;

class InventoryGuiTransactionTest {

    @Test
    void createInventorySupportsContainerTypesAndRefusesUnsupportedOrInvalidSizes() {
        try (TestServer testServer = new TestServer()) {
            var server = testServer.runtime().server();
            assertEquals(27, server.createInventory(null, 27, Component.text("Menu")).getSize());
            assertEquals(InventoryType.BARREL, server.createInventory(null, InventoryType.BARREL).getType());
            assertEquals(3, server.createInventory(null, InventoryType.FURNACE).getSize());
            assertEquals(3, server.createInventory(null, InventoryType.BLAST_FURNACE).getSize());
            assertEquals(3, server.createInventory(null, InventoryType.SMOKER).getSize());
            assertEquals(10, server.createInventory(null, InventoryType.WORKBENCH).getSize());
            assertEquals(3, server.createInventory(null, InventoryType.ANVIL).getSize());
            assertEquals(2, server.createInventory(null, InventoryType.ENCHANTING).getSize());
            assertEquals(3, server.createInventory(null, InventoryType.MERCHANT).getSize());

            assertThrows(IllegalArgumentException.class, () -> server.createInventory(null, 10));
            assertThrows(IllegalArgumentException.class, () -> server.createInventory(null, 63));
            assertThrows(
                    UnsupportedOperationException.class,
                    () -> server.createInventory(null, InventoryType.PLAYER));
        }
    }

    @Test
    void guiClickDragShiftClickHotbarSwapDropAndRollbackWorkAtomically() throws Exception {
        try (TestServer testServer = new TestServer()) {
            FerriteRuntime runtime = testServer.runtime();
            var loaded = TestPluginFactory.createWithLoader();
            var description = new PluginDescriptionFile(new StringReader("""
                    name: GuiTestPlugin
                    version: 1.0.0
                    main: dev.ferrite.api.test.LoadedTestPlugin
                    """));
            org.bukkit.plugin.java.JavaPlugin plugin = testServer.onMain(() -> {
                var attached = runtime.attachPlugin(
                        loaded.plugin(),
                        description,
                        testServer.tempDir().resolve("gui-plugin").toFile(),
                        testServer.tempDir().resolve("gui-plugin.jar").toFile(),
                        loaded.classLoader());
                runtime.plugins().enablePlugin(attached);
                return attached;
            });
            AtomicBoolean cancelNextClick = new AtomicBoolean(false);
            AtomicBoolean cancelNextDrag = new AtomicBoolean(false);
            List<InventoryCloseEvent.Reason> closeReasons = new ArrayList<>();

            runtime.plugins().registerEvents(new Listener() {
                @EventHandler
                public void onClick(InventoryClickEvent event) {
                    if (cancelNextClick.get()) {
                        event.setCancelled(true);
                    }
                }

                @EventHandler
                public void onDrag(InventoryDragEvent event) {
                    if (cancelNextDrag.get()) {
                        event.setCancelled(true);
                    }
                }

                @EventHandler
                public void onClose(InventoryCloseEvent event) {
                    closeReasons.add(event.getReason());
                }
            }, plugin);

            testServer.onMain(() -> {
                FerriteWorld world = runtime.world("world");
                FerritePlayer player = runtime.playerJoined(runtime.createPlayer(
                        "GuiPlayer", UUID.randomUUID(), false, GameMode.SURVIVAL, world, 0.5, 65, 0.5));
                player.getInventory().setItem(0, new ItemStack(Material.IRON_INGOT, 4));
                player.getInventory().setItemInOffHand(new ItemStack(Material.SHIELD, 1));

                Inventory menu = runtime.server().createInventory(null, 9, Component.text("Shop"));
                menu.setItem(0, new ItemStack(Material.DIAMOND, 2));
                FerriteInventoryView view = (FerriteInventoryView) player.openInventory(menu);
                assertEquals(1, menu.getViewers().size());

                // 1. Committed left-click picks up diamond, then right-click places 1 in slot 1.
                assertFalse(view.click(0, ClickType.LEFT, -1).isCancelled());
                assertNull(menu.getItem(0));
                assertEquals(Material.DIAMOND, view.getCursor().getType());
                assertFalse(view.click(1, ClickType.RIGHT, -1).isCancelled());
                assertEquals(1, menu.getItem(1).getAmount());
                assertEquals(1, view.getCursor().getAmount());

                // 2. Cancelled click rolls back atomically (cursor & slot restored).
                cancelNextClick.set(true);
                assertTrue(view.click(1, ClickType.LEFT, -1).isCancelled());
                assertEquals(1, menu.getItem(1).getAmount());
                assertEquals(1, view.getCursor().getAmount());
                cancelNextClick.set(false);

                // 3. Hotbar swap (rawSlot 2 with hotbar 0) and offhand swap (rawSlot 3 with offhand).
                assertFalse(view.click(2, ClickType.NUMBER_KEY, 0).isCancelled());
                assertEquals(Material.IRON_INGOT, menu.getItem(2).getType());
                assertNull(player.getInventory().getItem(0));
                assertFalse(view.click(3, ClickType.SWAP_OFFHAND, -1).isCancelled());
                assertEquals(Material.SHIELD, menu.getItem(3).getType());

                // 4. Shift-click moves slot 2 from top menu into player inventory.
                assertFalse(view.click(2, ClickType.SHIFT_LEFT, -1).isCancelled());
                assertNull(menu.getItem(2));
                assertTrue(player.getInventory().contains(Material.IRON_INGOT, 4));

                // 5. Cancelled drag rolls back atomically.
                cancelNextDrag.set(true);
                assertTrue(view.drag(
                                DragType.SINGLE,
                                Map.of(4, new ItemStack(Material.DIAMOND, 1)),
                                null)
                        .isCancelled());
                assertNull(menu.getItem(4));
                assertNotNull(view.getCursor());
                cancelNextDrag.set(false);

                // 6. Drop from slot 3 removes stack.
                assertFalse(view.click(3, ClickType.CONTROL_DROP, -1).isCancelled());
                assertNull(menu.getItem(3));

                // 7. Reopening fires OPEN_NEW for the previous view, quit fires DISCONNECT.
                Inventory second = runtime.server().createInventory(null, InventoryType.BARREL);
                player.openInventory(second);
                assertTrue(menu.getViewers().isEmpty());
                assertEquals(1, second.getViewers().size());
                runtime.playerQuit(player.getUniqueId());
                assertTrue(second.getViewers().isEmpty());
                assertEquals(
                        List.of(InventoryCloseEvent.Reason.OPEN_NEW, InventoryCloseEvent.Reason.DISCONNECT),
                        closeReasons);
                return null;
            });
        }
    }
}
