package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.test.TestPlugin;
import dev.ferrite.api.test.TestServer;
import java.util.List;
import java.util.UUID;
import org.bukkit.GameMode;
import org.bukkit.event.EventHandler;
import org.bukkit.event.Listener;
import org.bukkit.event.player.PlayerCommandPreprocessEvent;
import org.junit.jupiter.api.Test;

class CommandTest {

    private static final String YAML = """
            commands:
              test-hello:
                description: test command
                usage: /test-hello [name]
                aliases: [th]
                permission: test.hello
            permissions:
              test.hello:
                description: test permission
                default: op
            """;

    @Test
    void consoleAndPlayerDispatchFollowBukkitPermissions() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("CommandPlugin", YAML);
            server.enable(plugin);
            FerritePlayer player = server.onMain(() -> server.runtime().createPlayer(
                    "CmdPlayer", UUID.randomUUID(), false, GameMode.SURVIVAL,
                    server.runtime().world("world"), 0.5, 65, 0.5));

            assertNotNull(server.runtime().server().getPluginCommand("test-hello"));
            assertNotNull(server.runtime().server().getPluginCommand("CommandPlugin:test-hello"));
            assertNotNull(server.runtime().server().getPluginCommand("th"));

            assertTrue(server.onMain(() -> server.runtime().commandDispatcher()
                    .dispatch(server.runtime().consoleSender(), "/test-hello Console")));
            assertEquals(List.of("test-hello|CONSOLE|Console"), plugin.commands());

            assertTrue(server.onMain(() -> server.runtime().commandDispatcher()
                    .dispatch(player, "/test-hello Denied")));
            assertTrue(player.state().messages().stream()
                    .anyMatch(message -> message.contains("do not have permission")));
            assertEquals(1, plugin.commands().size());

            player.setOp(true);
            assertTrue(server.onMain(() -> server.runtime().commandDispatcher()
                    .dispatch(player, "/th Allowed")));
            assertEquals("th|CmdPlayer|Allowed", plugin.commands().get(1));

            assertTrue(server.onMain(() -> server.runtime().commandDispatcher()
                    .dispatch(player, "/commandplugin:test-hello Prefixed")));
            assertEquals("test-hello|CmdPlayer|Prefixed", plugin.commands().get(2));
        }
    }

    @Test
    void unknownCommandsAndDisabledPluginsAreReported() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("DisabledPlugin", YAML);
            server.enable(plugin);
            FerritePlayer player = server.onMain(() -> server.runtime().createPlayer(
                    "CmdPlayer2", UUID.randomUUID(), true, GameMode.SURVIVAL,
                    server.runtime().world("world"), 0.5, 65, 0.5));

            assertTrue(!server.onMain(() -> server.runtime().commandDispatcher()
                    .dispatch(player, "/definitely-not-a-command")));
            assertTrue(player.state().messages().stream()
                    .anyMatch(message -> message.contains("Unknown command")));

            server.disable(plugin);
            int before = player.state().messages().size();
            assertTrue(server.onMain(() -> server.runtime().commandDispatcher()
                    .dispatch(player, "/test-hello After")));
            assertTrue(player.state().messages().get(before).contains("is disabled"));
        }
    }

    @Test
    void preprocessEventCanCancelACommandLine() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("PreprocessPlugin", YAML);
            server.enable(plugin);
            FerritePlayer player = server.onMain(() -> server.runtime().createPlayer(
                    "Preprocess", UUID.randomUUID(), true, GameMode.SURVIVAL,
                    server.runtime().world("world"), 0.5, 65, 0.5));
            Listener listener = new Listener() {
                @EventHandler
                public void preprocess(PlayerCommandPreprocessEvent event) {
                    if (event.getMessage().contains("blocked-command")) {
                        event.setCancelled(true);
                    }
                }
            };
            server.onMain(() -> plugin.getServer().getPluginManager().registerEvents(listener, plugin));

            assertTrue(server.onMain(() -> server.runtime().commandDispatcher()
                    .dispatch(player, "/blocked-command")));
            assertTrue(player.state().messages().stream()
                    .noneMatch(message -> message.contains("Unknown command")));
            assertTrue(plugin.commands().isEmpty());
        }
    }
}
