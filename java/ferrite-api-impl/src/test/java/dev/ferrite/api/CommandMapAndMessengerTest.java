package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.test.TestPlugin;
import dev.ferrite.api.test.TestServer;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.Set;
import java.util.UUID;
import org.bukkit.GameMode;
import org.bukkit.command.Command;
import org.bukkit.command.CommandMap;
import org.bukkit.command.CommandSender;
import org.bukkit.entity.Player;
import org.bukkit.plugin.messaging.Messenger;
import org.bukkit.plugin.messaging.PluginMessageListenerRegistration;
import org.junit.jupiter.api.Test;

class CommandMapAndMessengerTest {

    private static final String YAML = """
            commands:
              map-probe:
                description: Command map probe
                usage: /map-probe
            """;

    @Test
    void commandMapExposesPluginAndDynamicCommands() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("MapPlugin", YAML);
            server.enable(plugin);

            CommandMap map = server.runtime().server().getCommandMap();
            assertNotNull(map);

            Command pluginCmd = map.getCommand("map-probe");
            assertNotNull(pluginCmd);
            assertTrue(pluginCmd.isRegistered());
            assertSame(pluginCmd, map.getKnownCommands().get("map-probe"));
            assertSame(pluginCmd, map.getKnownCommands().get("mapplugin:map-probe"));

            // Legacy Spigot plugins reflect on PluginManager#commandMap.
            Field field = server.runtime().plugins().getClass().getDeclaredField("commandMap");
            field.setAccessible(true);
            assertSame(map, field.get(server.runtime().plugins()));

            List<String> dynamicCalls = new ArrayList<>();
            Command dynamic = new Command("dyn-cmd", "Dynamic", "/dyn-cmd", List.of("dc")) {
                @Override
                public boolean execute(CommandSender sender, String label, String[] args) {
                    dynamicCalls.add(sender.getName() + ":" + label + ":" + String.join(",", args));
                    return true;
                }
            };
            assertTrue(map.register("mapplugin", dynamic));
            assertTrue(dynamic.isRegistered());
            assertNotNull(server.runtime().server().getHelpMap().getHelpTopic("/dyn-cmd"));
            assertTrue(server.onMain(() -> server.runtime().commandDispatcher()
                    .dispatch(server.runtime().consoleSender(), "/dc hello")));
            assertEquals(List.of("CONSOLE:dc:hello"), dynamicCalls);
        }
    }

    @Test
    void messengerRegistersChannelsAndDispatchesIncomingMessages() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("MsgPlugin", "");
            server.enable(plugin);

            Messenger messenger = server.runtime().server().getMessenger();
            assertNotNull(messenger);

            messenger.registerOutgoingPluginChannel(plugin, "test:out");
            assertTrue(messenger.isOutgoingChannelRegistered(plugin, "test:out"));
            assertEquals(Set.of("test:out"), messenger.getOutgoingChannels());
            assertEquals(Set.of("test:out"), messenger.getOutgoingChannels(plugin));

            List<String> receivedA = new ArrayList<>();
            List<String> receivedB = new ArrayList<>();
            PluginMessageListenerRegistration regA = messenger.registerIncomingPluginChannel(
                    plugin,
                    "test:in",
                    (channel, player, message) -> receivedA.add(
                            channel + "|" + player.getName() + "|" + new String(message, StandardCharsets.UTF_8)));
            PluginMessageListenerRegistration regB = messenger.registerIncomingPluginChannel(
                    plugin,
                    "test:other",
                    (channel, player, message) -> receivedB.add(channel));

            assertTrue(regA.isValid());
            assertTrue(messenger.isRegistrationValid(regA));
            assertTrue(messenger.isIncomingChannelRegistered(plugin, "test:in"));
            assertEquals(Set.of("test:in", "test:other"), messenger.getIncomingChannels());
            assertEquals(Set.of("test:in", "test:other"), messenger.getIncomingChannels(plugin));
            assertEquals(1, messenger.getIncomingChannelRegistrations("test:in").size());

            FerritePlayer player = server.onMain(() -> server.runtime().createPlayer(
                    "MsgPlayer", UUID.randomUUID(), false, GameMode.SURVIVAL,
                    server.runtime().world("world"), 0.5, 65, 0.5));
            byte[] payload = "ping".getBytes(StandardCharsets.UTF_8);
            messenger.dispatchIncomingMessage(player, "test:in", payload);
            assertEquals(List.of("test:in|MsgPlayer|ping"), receivedA);
            assertTrue(receivedB.isEmpty());

            // Verify byte array contents match via a direct listener check.
            List<byte[]> rawBytes = new ArrayList<>();
            messenger.registerIncomingPluginChannel(
                    plugin, "test:bytes", (ch, p, msg) -> rawBytes.add(msg));
            messenger.dispatchIncomingMessage(player, "test:bytes", payload);
            assertEquals(1, rawBytes.size());
            assertArrayEquals(payload, rawBytes.get(0));

            messenger.unregisterIncomingPluginChannel(plugin, "test:in");
            assertFalse(messenger.isIncomingChannelRegistered(plugin, "test:in"));
            assertFalse(messenger.isRegistrationValid(regA));
            assertTrue(messenger.isRegistrationValid(regB));

            messenger.unregisterOutgoingPluginChannel(plugin, "test:out");
            assertFalse(messenger.isOutgoingChannelRegistered(plugin, "test:out"));

            // Disabling the plugin clears its remaining registrations.
            server.onMain(() -> {
                server.runtime().disablePlugin(plugin);
                return null;
            });
            assertFalse(messenger.isIncomingChannelRegistered(plugin, "test:other"));
            assertTrue(messenger.getIncomingChannels(plugin).isEmpty());
        }
    }
}
