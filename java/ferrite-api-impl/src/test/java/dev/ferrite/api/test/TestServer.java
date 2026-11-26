package dev.ferrite.api.test;

import dev.ferrite.api.FerriteApi;
import dev.ferrite.api.FerriteRuntime;
import dev.ferrite.api.internal.QueuedMainThread;
import dev.ferrite.api.world.WorldSpec;
import java.io.IOException;
import java.io.StringReader;
import java.io.UncheckedIOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Comparator;
import java.util.List;
import java.util.concurrent.Callable;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.function.Consumer;
import java.util.logging.Handler;
import java.util.logging.Level;
import java.util.logging.LogRecord;
import java.util.logging.Logger;
import org.bukkit.World;
import org.bukkit.plugin.PluginDescriptionFile;

/** Installs one facade runtime per test and exposes main-thread helpers. */
public final class TestServer implements AutoCloseable {

    private static final AtomicInteger COUNTER = new AtomicInteger();

    private final Path tempDir;
    private final Logger logger;
    private final List<LogRecord> logRecords = new CopyOnWriteArrayList<>();
    private final FerriteRuntime runtime;
    private final java.util.List<TestPluginFactory.Loaded> pluginLoaders =
            new java.util.concurrent.CopyOnWriteArrayList<>();

    public TestServer() {
        this(builder -> {
        });
    }

    public TestServer(Consumer<FerriteRuntime.Builder> customizer) {
        try {
            this.tempDir = Files.createTempDirectory("ferrite-api-test-");
        } catch (IOException exception) {
            throw new UncheckedIOException(exception);
        }
        int id = COUNTER.incrementAndGet();
        this.logger = Logger.getLogger("FerriteTest-" + id);
        this.logger.setUseParentHandlers(false);
        this.logger.setLevel(Level.ALL);
        this.logger.addHandler(new Handler() {
            @Override
            public void publish(LogRecord record) {
                logRecords.add(record);
            }

            @Override
            public void flush() {
            }

            @Override
            public void close() {
            }
        });

        FerriteRuntime.Builder builder = FerriteRuntime.builder()
                .mainThread(new QueuedMainThread("Test-Main-" + id))
                .logger(logger)
                .world(WorldSpec.of("world", World.Environment.NORMAL).spawn(0.5, 65.0, 0.5));
        customizer.accept(builder);
        this.runtime = builder.build();
        FerriteApi.install(runtime);
    }

    public FerriteRuntime runtime() {
        return runtime;
    }

    public Logger logger() {
        return logger;
    }

    public List<LogRecord> logRecords() {
        return List.copyOf(logRecords);
    }

    public Path tempDir() {
        return tempDir;
    }

    public TestPlugin attachPlugin(String name) throws Exception {
        return attachPlugin(name, "");
    }

    public TestPlugin attachPlugin(String name, String extraYaml) throws Exception {
        TestPluginFactory.Loaded loaded = TestPluginFactory.createWithLoader();
        pluginLoaders.add(loaded);
        PluginDescriptionFile description = new PluginDescriptionFile(new StringReader(
                "name: " + name + "\nversion: 1.0.0\nmain: " + TestPlugin.class.getName() + "\n" + extraYaml));
        return attachPlugin(loaded.plugin(), description);
    }

    public TestPlugin attachPlugin(TestPlugin plugin, PluginDescriptionFile description) {
        Path jar = tempDir.resolve(description.getName() + ".jar");
        Path data = tempDir.resolve("data-" + description.getName());
        runtime.callOnMainThread(() -> runtime.attachPlugin(
                plugin, description, jar.toFile(), data.toFile(), plugin.getClass().getClassLoader()));
        return plugin;
    }

    public void enable(TestPlugin plugin) {
        onMain(() -> runtime.enablePlugin(plugin));
    }

    public void disable(TestPlugin plugin) {
        onMain(() -> runtime.disablePlugin(plugin));
    }

    public void tick(int ticks) {
        for (int index = 0; index < ticks; index++) {
            onMain(runtime::tick);
        }
    }

    public <T> T onMain(Callable<T> action) {
        return runtime.callOnMainThread(action);
    }

    public void onMain(Runnable action) {
        runtime.callOnMainThread(() -> {
            action.run();
            return null;
        });
    }

    public boolean logContains(String fragment) {
        return logRecords.stream().anyMatch(record -> record.getMessage().contains(fragment));
    }

    @Override
    public void close() {
        try {
            FerriteApi.uninstall(runtime);
        } finally {
            for (TestPluginFactory.Loaded loaded : pluginLoaders) {
                try {
                    loaded.classLoader().close();
                } catch (IOException ignored) {
                    // best effort test classloader cleanup
                }
            }
            try (var paths = Files.walk(tempDir)) {
                paths.sorted(Comparator.reverseOrder()).forEach(path -> {
                    try {
                        Files.deleteIfExists(path);
                    } catch (IOException ignored) {
                        // best effort temporary directory cleanup
                    }
                });
            } catch (IOException ignored) {
                // best effort temporary directory cleanup
            }
        }
    }
}
