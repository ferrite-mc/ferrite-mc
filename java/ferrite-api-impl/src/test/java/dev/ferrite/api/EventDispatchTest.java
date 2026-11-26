package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.test.TestPlugin;
import dev.ferrite.api.test.TestServer;
import dev.ferrite.api.world.FerriteWorld;
import java.util.ArrayList;
import java.util.List;
import java.util.UUID;
import org.bukkit.GameMode;
import org.bukkit.Material;
import org.bukkit.block.Block;
import org.bukkit.event.Event;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.HandlerList;
import org.bukkit.event.Listener;
import org.bukkit.event.block.BlockBreakEvent;
import org.bukkit.event.player.PlayerJoinEvent;
import org.junit.jupiter.api.Test;

class EventDispatchTest {

    @Test
    void dispatchesByPriorityAndHonoursIgnoreCancelled() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("EventPlugin");
            server.enable(plugin);
            FerritePlayer player = server.onMain(() -> server.runtime().createPlayer(
                    "Eventor", UUID.randomUUID(), false, GameMode.SURVIVAL,
                    server.runtime().world("world"), 0.5, 65, 0.5));
            FerriteWorld world = server.runtime().world("world");
            Block block = world.getBlockAt(4, 64, 4);

            List<String> order = new ArrayList<>();
            Listener listener = new Listener() {
                @EventHandler(priority = EventPriority.LOWEST)
                public void lowest(BlockBreakEvent event) {
                    order.add("lowest");
                    event.setCancelled(true);
                }

                @EventHandler(priority = EventPriority.NORMAL)
                public void normal(BlockBreakEvent event) {
                    order.add("normal");
                }

                @EventHandler(priority = EventPriority.HIGHEST, ignoreCancelled = true)
                public void highestSkipCancelled(BlockBreakEvent event) {
                    order.add("highest-ignore");
                }

                @EventHandler(priority = EventPriority.HIGHEST)
                public void highest(BlockBreakEvent event) {
                    order.add("highest");
                }

                @EventHandler(priority = EventPriority.MONITOR)
                public void monitor(BlockBreakEvent event) {
                    order.add("monitor");
                }
            };
            server.onMain(() -> plugin.getServer().getPluginManager().registerEvents(listener, plugin));

            BlockBreakEvent event = server.onMain(() -> {
                BlockBreakEvent created = new BlockBreakEvent(block, player);
                created.callEvent();
                return created;
            });
            assertTrue(event.isCancelled());
            assertEquals(List.of("lowest", "normal", "highest", "monitor"), order);

            server.disable(plugin);
            server.onMain(() -> new BlockBreakEvent(block, player).callEvent());
            assertEquals(4, order.size(), "disabled plugins must be unregistered");
        }
    }

    @Test
    void listenerErrorsAreReportedWithTheEventAndPlugin() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("ExplodingPlugin");
            server.enable(plugin);
            FerritePlayer player = server.onMain(() -> server.runtime().createPlayer(
                    "Boom", UUID.randomUUID(), false, GameMode.SURVIVAL,
                    server.runtime().world("world"), 0.5, 65, 0.5));
            Listener listener = new Listener() {
                @EventHandler
                public void join(PlayerJoinEvent event) {
                    throw new IllegalStateException("listener exploded");
                }
            };
            server.onMain(() -> plugin.getServer().getPluginManager().registerEvents(listener, plugin));

            IllegalStateException exception = assertThrows(IllegalStateException.class,
                    () -> server.onMain(() -> new PlayerJoinEvent(player, "joined").callEvent()));
            assertTrue(exception.getMessage().contains("PlayerJoinEvent"), exception.getMessage());
            assertTrue(exception.getMessage().contains("ExplodingPlugin"));
            assertEquals("listener exploded", exception.getCause().getMessage());
        }
    }

    @Test
    void syncEventsRequireTheMainThreadAndAsyncEventsRejectIt() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("ThreadPlugin");
            server.enable(plugin);
            FerritePlayer player = server.runtime().callOnMainThread(() -> server.runtime().createPlayer(
                    "Threader", UUID.randomUUID(), false, GameMode.SURVIVAL,
                    server.runtime().world("world"), 0.5, 65, 0.5));

            IllegalStateException offThread = assertThrows(IllegalStateException.class,
                    () -> new PlayerJoinEvent(player, "joined").callEvent());
            assertTrue(offThread.getMessage().contains("main thread"));

            server.onMain(() -> assertThrows(IllegalStateException.class,
                    () -> new TestAsyncEvent().callEvent()));
        }
    }

    private static final class TestAsyncEvent extends Event {

        private static final HandlerList HANDLERS = new HandlerList();

        private TestAsyncEvent() {
            super(true);
        }

        @Override
        public HandlerList getHandlers() {
            return HANDLERS;
        }

        public static HandlerList getHandlerList() {
            return HANDLERS;
        }
    }
}
