package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.api.test.TestPlugin;
import dev.ferrite.api.test.TestPluginFactory;
import dev.ferrite.api.test.TestServer;
import java.io.File;
import java.io.Reader;
import java.io.StringReader;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import org.bukkit.configuration.file.YamlConfiguration;
import org.bukkit.plugin.PluginDescriptionFile;
import org.junit.jupiter.api.Test;

class PluginLifecycleTest {

    @Test
    void attachAndEnableProvideTheJavaPluginContract() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("LifecyclePlugin");
            assertEquals(List.of("onLoad"), plugin.lifecycle());
            assertTrue(plugin.getDataFolder().isDirectory());
            assertEquals("LifecyclePlugin", plugin.getName());
            assertEquals("1.0.0", plugin.getDescription().getVersion());
            assertSame(server.runtime().server(), plugin.getServer());
            assertSame(plugin.getDescription(), plugin.getPluginMeta());
            assertNotNull(plugin.getLogger());
            assertNotNull(plugin.getPluginLoader());

            server.enable(plugin);
            assertTrue(plugin.isEnabled());
            assertEquals(List.of("onLoad", "onEnable"), plugin.lifecycle());
            // JavaPlugin#getConfig loads defaults but does not create the file; the plugin
            // must opt in with saveDefaultConfig, exactly as Bukkit requires.
            plugin.saveDefaultConfig();
            assertTrue(new File(plugin.getDataFolder(), "config.yml").isFile());
            assertEquals("test greeting", plugin.getConfig().getString("greeting"));

            plugin.getConfig().set("greeting", "changed");
            plugin.saveConfig();
            try (Reader reader = Files.newBufferedReader(
                    new File(plugin.getDataFolder(), "config.yml").toPath())) {
                assertEquals("changed", YamlConfiguration.loadConfiguration(reader).getString("greeting"));
            }

            server.disable(plugin);
            assertFalse(plugin.isEnabled());
            assertEquals(List.of("onLoad", "onEnable", "onDisable"), plugin.lifecycle());
        }
    }

    @Test
    void attachRequiresTheMainThreadAndRejectsDuplicateNames() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = TestPluginFactory.create();
            PluginDescriptionFile description = new PluginDescriptionFile(new StringReader("""
                    name: OffThreadPlugin
                    version: 1.0.0
                    main: dev.ferrite.api.test.TestPlugin
                    """));
            Path data = server.tempDir().resolve("off-thread");
            assertThrows(IllegalStateException.class, () -> server.runtime().attachPlugin(
                    plugin,
                    description,
                    server.tempDir().resolve("off-thread.jar").toFile(),
                    data.toFile(),
                    TestPlugin.class.getClassLoader()));
            assertTrue(plugin.lifecycle().isEmpty());

            server.attachPlugin("DuplicatePlugin");
            TestPlugin duplicate = TestPluginFactory.create();
            PluginDescriptionFile duplicateDescription = new PluginDescriptionFile(new StringReader("""
                    name: DuplicatePlugin
                    version: 1.0.0
                    main: dev.ferrite.api.test.TestPlugin
                    """));
            assertThrows(IllegalStateException.class, () -> server.runtime().callOnMainThread(
                    () -> server.runtime().attachPlugin(
                            duplicate,
                            duplicateDescription,
                            server.tempDir().resolve("duplicate.jar").toFile(),
                            server.tempDir().resolve("duplicate-data").toFile(),
                            TestPlugin.class.getClassLoader())));
        }
    }

    @Test
    void commandsFromPluginYmlAreRegisteredWithThePluginAsDefaultExecutor() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("CommandFallbackPlugin", """
                    commands:
                      fallback:
                        usage: /fallback
                    """);
            server.enable(plugin);
            assertNotNull(plugin.getCommand("fallback"));
            assertSame(plugin, plugin.getCommand("fallback").getExecutor());

            assertTrue(server.onMain(() -> server.runtime().commandDispatcher()
                    .dispatch(server.runtime().consoleSender(), "/fallback")));
            assertEquals(List.of("fallback|CONSOLE|"), plugin.commands());
        }
    }
}
