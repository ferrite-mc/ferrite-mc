package dev.ferrite.host;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertSame;

import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;

class PluginClassLoaderTest {
    @Test
    void uses_the_host_class_loader_as_parent_and_closes() throws Exception {
        Path emptyJar = Files.createTempFile("ferrite-plugin-test", ".jar");
        try (PluginClassLoader loader = new PluginClassLoader("Example", emptyJar)) {
            assertEquals("ferrite-plugin/Example", loader.getName());
            assertSame(Json.class, loader.loadClass("dev.ferrite.host.Json"));
            assertSame(
                    HostApi.class,
                    loader.loadClass("dev.ferrite.host.HostApi"));
        } finally {
            Files.deleteIfExists(emptyJar);
        }
    }
}
