package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.api.command.FerriteHelpMap;
import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.test.TestPlugin;
import dev.ferrite.api.test.TestServer;
import java.util.UUID;
import org.bukkit.GameMode;
import org.bukkit.command.CommandSender;
import org.bukkit.command.PluginCommand;
import org.bukkit.help.HelpMap;
import org.bukkit.help.HelpTopic;
import org.junit.jupiter.api.Test;

class HelpMapTest {

    private static final String YAML = """
            commands:
              help-probe:
                description: Probe command description
                usage: /help-probe [arg]
                permission: probe.use
            permissions:
              probe.use:
                description: Probe permission
                default: op
            """;

    private static final class CustomTopic extends HelpTopic {
        private final boolean visible;

        CustomTopic(String name, String shortText, String fullText, boolean visible) {
            this.name = name;
            this.shortText = shortText;
            this.fullText = fullText;
            this.visible = visible;
        }

        @Override
        public boolean canSee(CommandSender sender) {
            return visible;
        }
    }

    @Test
    void pluginCommandsBecomeDiscoverableAndRespectPermissions() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("HelpPlugin", YAML);
            server.enable(plugin);

            HelpMap helpMap = server.runtime().server().getHelpMap();
            FerriteHelpMap ferriteHelpMap = assertInstanceOf(FerriteHelpMap.class, helpMap);
            assertTrue(helpMap.getIgnoredPlugins().isEmpty());

            HelpTopic topic = helpMap.getHelpTopic("/help-probe");
            assertNotNull(topic, "plugin.yml command must be discoverable in HelpMap");
            assertEquals("/help-probe", topic.getName());
            assertEquals("Probe command description", topic.getShortText());
            assertTrue(topic.getFullText(server.runtime().consoleSender())
                    .contains("Probe command description"));
            assertTrue(topic.canSee(server.runtime().consoleSender()));

            FerritePlayer nonOp = server.onMain(() -> server.runtime().createPlayer(
                    "NonOp", UUID.randomUUID(), false, GameMode.SURVIVAL,
                    server.runtime().world("world"), 0.5, 65, 0.5));
            assertFalse(topic.canSee(nonOp));
            nonOp.setOp(true);
            assertTrue(topic.canSee(nonOp));

            assertTrue(ferriteHelpMap.getHelpTopics().contains(topic));
        }
    }

    @Test
    void manualTopicRegistryAndFactoryLookupWork() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("FactoryPlugin", YAML);
            server.enable(plugin);

            FerriteHelpMap helpMap = assertInstanceOf(
                    FerriteHelpMap.class, server.runtime().server().getHelpMap());

            CustomTopic manual = new CustomTopic(
                    "WorldEdit", "WorldEdit summary", "WorldEdit full help", true);
            helpMap.addTopic(manual);
            assertEquals(manual, helpMap.getHelpTopic("worldedit"));
            assertEquals("WorldEdit summary", manual.getShortText());
            assertEquals("WorldEdit full help", manual.getFullText(server.runtime().consoleSender()));
            assertTrue(manual.canSee(server.runtime().consoleSender()));

            assertTrue(helpMap.removeTopic("WorldEdit"));
            assertNull(helpMap.getHelpTopic("WorldEdit"));

            helpMap.registerHelpTopicFactory(PluginCommand.class, command -> new CustomTopic(
                    "/" + command.getName(),
                    "Factory:" + command.getName(),
                    "Full:" + command.getDescription(),
                    true));

            HelpTopic factoryTopic = helpMap.getHelpTopic("/help-probe");
            assertNotNull(factoryTopic);
            assertEquals("Factory:help-probe", factoryTopic.getShortText());
            assertEquals("Full:Probe command description",
                    factoryTopic.getFullText(server.runtime().consoleSender()));

            assertThrows(IllegalArgumentException.class,
                    () -> helpMap.registerHelpTopicFactory(String.class, cmd -> null));

            helpMap.clear();
            assertTrue(helpMap.getHelpTopics().isEmpty());
            assertNull(helpMap.getHelpTopic("/help-probe"));
        }
    }
}
