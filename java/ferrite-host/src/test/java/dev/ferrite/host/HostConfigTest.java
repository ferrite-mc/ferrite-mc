package dev.ferrite.host;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.file.Path;
import java.util.Map;
import org.junit.jupiter.api.Test;

class HostConfigTest {
    private static String configJson() {
        return Json.stringify(
                Json.object(
                        "contract_version",
                        2L,
                        "api_version",
                        "26.3",
                        "server",
                        Json.object(
                                "name",
                                "Ferrite",
                                "version",
                                "0.2.1",
                                "motd",
                                "Hello",
                                "max_players",
                                20,
                                "online_mode",
                                true,
                                "view_distance",
                                10),
                        "plugins_dir",
                        "plugins",
                        "data_dir",
                        "plugins",
                        "api_impl_class",
                        null));
    }

    @Test
    void parses_the_start_configuration() {
        HostConfig config = HostConfig.parse(configJson());
        assertEquals(2, config.contractVersion());
        assertEquals("26.3", config.apiVersion());
        assertEquals(Path.of("plugins"), config.pluginsDir());
        assertEquals(Path.of("plugins"), config.dataDir());
        assertNull(config.apiImplClass());
        assertEquals("Ferrite", config.server().name());
        assertEquals(20, config.server().maxPlayers());
        assertTrue(config.server().onlineMode());
    }

    @Test
    void parses_plugin_metadata_including_commands_and_permissions() {
        Map<String, Object> json =
                Json.asObject(
                        Json.parse(
                                Json.stringify(
                                        Json.object(
                                                "name",
                                                "Example",
                                                "version",
                                                "1.2.3",
                                                "main",
                                                "com.example.ExamplePlugin",
                                                "api_version",
                                                "1.21",
                                                "load",
                                                "STARTUP",
                                                "dependencies",
                                                Json.array("Base"),
                                                "soft_dependencies",
                                                Json.array(),
                                                "load_before",
                                                Json.array(),
                                                "provides",
                                                Json.array("Alias"),
                                                "folia_supported",
                                                false,
                                                "format",
                                                "plugin.yml",
                                                "jar",
                                                "plugins/Example.jar",
                                                "data_folder",
                                                "plugins/Example",
                                                "commands",
                                                Json.object(
                                                        "example",
                                                        Json.object(
                                                                "description",
                                                                "Runs it",
                                                                "usage",
                                                                "/example",
                                                                "aliases",
                                                                Json.array("ex"),
                                                                "permission",
                                                                "example.use")),
                                                "permissions",
                                                Json.object(
                                                        "example.use",
                                                        Json.object(
                                                                "description",
                                                                "Allows it",
                                                                "default",
                                                                "op",
                                                                "children",
                                                                Json.object()))))));
        PluginSpec spec = PluginSpec.fromJson(json);
        assertEquals("Example", spec.name());
        assertEquals("STARTUP", spec.load());
        assertEquals(Path.of("plugins/Example"), spec.dataFolder());
        assertEquals(1, spec.commands().size());
        assertEquals("example.use", spec.commands().get("example").permission());
        assertEquals("op", spec.permissions().get("example.use").defaultPermission());
        assertFalse(spec.foliaSupported());
    }

    @Test
    void unknown_command_origin_is_rejected() {
        Map<String, Object> json = Json.object("type", "remote");
        org.junit.jupiter.api.Assertions.assertThrows(
                IllegalArgumentException.class, () -> CommandOrigin.fromJson(json));
    }
}
