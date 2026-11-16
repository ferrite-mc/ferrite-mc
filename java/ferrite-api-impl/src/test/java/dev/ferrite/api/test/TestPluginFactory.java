package dev.ferrite.api.test;

import dev.ferrite.api.plugin.FerritePluginClassLoader;
import java.io.IOException;
import java.io.InputStream;
import java.io.UncheckedIOException;
import java.net.URL;

/** Creates {@link TestPlugin} instances whose classloader satisfies the Paper API. */
public final class TestPluginFactory {

    private TestPluginFactory() {
    }

    public static TestPlugin create() throws Exception {
        return (TestPlugin) createWithLoader().plugin();
    }

    public static Loaded createWithLoader() throws Exception {
        byte[] bytes;
        try (InputStream input = TestPlugin.class.getResourceAsStream("LoadedTestPlugin.class")) {
            if (input == null) {
                throw new IllegalStateException("LoadedTestPlugin.class is missing from the test classpath");
            }
            bytes = input.readAllBytes();
        } catch (IOException exception) {
            throw new UncheckedIOException(exception);
        }
        FerritePluginClassLoader loader = new FerritePluginClassLoader(
                new URL[0], TestPlugin.class.getClassLoader(), null);
        Class<?> type = loader.definePluginClass(
                "dev.ferrite.api.test.LoadedTestPlugin", bytes);
        TestPlugin plugin = (TestPlugin) type.getDeclaredConstructor().newInstance();
        return new Loaded(plugin, loader);
    }

    public record Loaded(TestPlugin plugin, FerritePluginClassLoader classLoader) {
    }
}
