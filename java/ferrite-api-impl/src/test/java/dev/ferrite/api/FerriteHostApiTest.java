package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.host.Bootstrap;
import dev.ferrite.nms.bridge.NmsBridge;
import dev.ferrite.nms.host.NmsHostBinding;
import java.nio.file.Files;
import java.nio.file.Path;
import org.bukkit.Bukkit;
import org.bukkit.Material;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

/**
 * Regression test for {@link FerriteHostApi}: verifies Paper {@code JavaPlugin} loading through
 * {@code HostRuntime}, per-plugin classloader isolation, {@code NmsBridge} installation, and
 * clean teardown.
 */
class FerriteHostApiTest {

    @Test
    void loadsTypicalFixtureWithIsolatedClassLoaderAndCleanShutdown(@TempDir Path tempDir)
            throws Exception {
        String jarProperty = System.getProperty("ferrite.fixture.typical.jar");
        Path typicalJar = Path.of(jarProperty);
        assertTrue(Files.isRegularFile(typicalJar), "typical fixture jar must exist");

        Path pluginsDir = Files.createDirectories(tempDir.resolve("plugins"));
        Path dataDir = Files.createDirectories(tempDir.resolve("data"));
        Path pluginData = dataDir.resolve("TypicalFixture");

        String startConfig = "{\"contract_version\":2,\"api_version\":\"26.3\",\"server\":{"
                + "\"name\":\"Ferrite\",\"version\":\"0.2.1\",\"motd\":\"test\","
                + "\"max_players\":20,\"online_mode\":true,\"view_distance\":10},"
                + "\"plugins_dir\":\"" + escape(pluginsDir) + "\","
                + "\"data_dir\":\"" + escape(dataDir) + "\","
                + "\"api_impl_class\":\"" + FerriteHostApi.class.getName() + "\"}";

        String started = Bootstrap.start(startConfig);
        assertTrue(started.contains("\"ok\":true"), started);
        try {
            assertTrue(NmsBridge.isInstalled());
            assertTrue(Material.STONE.isBlock());
            assertFalse(Material.DIAMOND.isBlock());
            assertTrue(Bukkit.getServer().createBlockData(Material.STONE).getMaterial()
                    == Material.STONE);

            String pluginJson = "{\"name\":\"TypicalFixture\",\"version\":\"1.0.0\","
                    + "\"main\":\"dev.ferrite.fixtures.typical.TypicalPlugin\","
                    + "\"api_version\":\"26.3\",\"load\":\"POSTWORLD\","
                    + "\"dependencies\":[],\"soft_dependencies\":[],\"load_before\":[],"
                    + "\"provides\":[],\"folia_supported\":true,\"format\":\"plugin.yml\","
                    + "\"jar\":\"" + escape(typicalJar) + "\","
                    + "\"data_folder\":\"" + escape(pluginData) + "\","
                    + "\"commands\":{},\"permissions\":{}}";

            String loaded = Bootstrap.loadPlugin(pluginJson);
            assertTrue(loaded.contains("\"ok\":true"), loaded);

            var plugin = Bukkit.getPluginManager().getPlugin("TypicalFixture");
            assertTrue(plugin != null && plugin.isEnabled());
            ClassLoader pluginLoader = plugin.getClass().getClassLoader();
            assertNotSame(FerriteHostApi.class.getClassLoader(), pluginLoader);
            assertThrows(
                    ClassNotFoundException.class,
                    () -> Class.forName(
                            "dev.ferrite.fixtures.nms.NmsFixturePlugin",
                            false,
                            pluginLoader));
            assertEquals("TypicalFixture", plugin.getName());
        } finally {
            String stopped = Bootstrap.shutdown("{\"reason\":\"regression test\"}");
            assertTrue(stopped.contains("\"ok\":true"), stopped);
            assertFalse(NmsBridge.isInstalled());
            assertThrows(IllegalStateException.class, NmsHostBinding::require);
            assertFalse(FerriteApi.isInstalled());
        }
    }

    private static String escape(Path path) {
        return path.toAbsolutePath().normalize().toString().replace("\\", "\\\\");
    }
}
