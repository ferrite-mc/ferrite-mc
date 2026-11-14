package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import io.papermc.paper.plugin.bootstrap.BootstrapContext;
import java.nio.file.*;
import org.bukkit.plugin.PluginDescriptionFile;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class FerriteBootstrapTest {
    @TempDir Path temp;

    private Path pack(String name) throws Exception {
        var dir = Files.createDirectories(temp.resolve("src-" + name));
        Files.writeString(dir.resolve("pack.mcmeta"), "{\"pack\":{\"description\":\"x\",\"min_format\":1,\"max_format\":99}}");
        Files.createDirectories(dir.resolve("data/test/function"));
        Files.writeString(dir.resolve("data/test/function/hi.mcfunction"), "say hi");
        return dir;
    }

    private void register(BootstrapContext context, io.papermc.paper.plugin.lifecycle.event.handler.LifecycleEventHandler<io.papermc.paper.plugin.lifecycle.event.registrar.RegistrarEvent<io.papermc.paper.datapack.DatapackRegistrar>> handler) {
        var type = FerriteLifecycleTypes.<BootstrapContext, io.papermc.paper.plugin.lifecycle.event.registrar.RegistrarEvent<io.papermc.paper.datapack.DatapackRegistrar>>prioritized(FerriteBootstrap.DATAPACK_DISCOVERY, BootstrapContext.class);
        context.getLifecycleManager().registerEventHandler(type.newHandler(handler));
    }

    @Test void discoveredPackIsCopiedIntoTheWorldAndKeepsItsEnabledState() throws Exception {
        var world = Files.createDirectories(temp.resolve("world"));
        var bootstrap = new FerriteBootstrap(new PluginDescriptionFile("Boot", "1.0", "x.Main"), temp.resolve("boot.jar"), temp.resolve("data"), world);
        var context = bootstrap.context();
        var source = pack("a");
        var disabled = pack("b");
        register(context, event -> {
            try {
                var found = event.registrar().discoverPack(source, "alpha");
                assertEquals("Boot-alpha", found.getName());
                event.registrar().discoverPack(disabled, "beta", configurer -> configurer.autoEnableOnServerStart(false));
                assertTrue(event.registrar().hasPackDiscovered("Boot-alpha"));
            } catch (java.io.IOException failure) { throw new IllegalStateException(failure); }
        });
        bootstrap.fire();
        assertTrue(Files.isRegularFile(world.resolve("datapacks/Boot-alpha/data/test/function/hi.mcfunction")));
        assertEquals("Boot-beta", Files.readString(world.resolve("datapacks/.ferrite-disabled")).trim());
        var manager = new FerriteDatapackManager(() -> world);
        assertTrue(manager.getPack("Boot-alpha").isEnabled());
        assertFalse(manager.getPack("Boot-beta").isEnabled());
        assertEquals(1, manager.getEnabledPacks().stream().filter(p -> p.getName().startsWith("file/")).count());
    }

    @Test void packWithoutMetadataAndUnsupportedEventsAreRefused() throws Exception {
        var world = Files.createDirectories(temp.resolve("world2"));
        var bootstrap = new FerriteBootstrap(new PluginDescriptionFile("Boot", "1.0", "x.Main"), temp.resolve("boot.jar"), temp.resolve("data"), world);
        var context = bootstrap.context();
        var empty = Files.createDirectories(temp.resolve("empty"));
        var errors = new java.util.ArrayList<Throwable>();
        register(context, event -> {
            try { event.registrar().discoverPack(empty, "none"); } catch (Throwable error) { errors.add(error); }
            try { event.registrar().discoverPack(empty, "../bad"); } catch (Throwable error) { errors.add(error); }
        });
        bootstrap.fire();
        assertEquals(2, errors.size());
        assertInstanceOf(java.io.IOException.class, errors.get(0));
        assertInstanceOf(IllegalArgumentException.class, errors.get(1));
        assertFalse(Files.exists(world.resolve("datapacks/Boot-none")));
        var tags = FerriteLifecycleTypes.<BootstrapContext, io.papermc.paper.plugin.lifecycle.event.LifecycleEvent>prioritized("pre_flatten/minecraft:block", BootstrapContext.class);
        assertThrows(UnsupportedOperationException.class, () -> context.getLifecycleManager().registerEventHandler(tags.newHandler(event -> {})));
    }

    @Test void withoutAWorldFolderDiscoveryFailsLoudly() throws Exception {
        var bootstrap = new FerriteBootstrap(new PluginDescriptionFile("Boot", "1.0", "x.Main"), temp.resolve("boot.jar"), temp.resolve("data"), null);
        var context = bootstrap.context();
        var errors = new java.util.ArrayList<Throwable>();
        register(context, event -> { try { event.registrar().discoverPack(pack("c"), "x"); } catch (Throwable error) { errors.add(error); } });
        bootstrap.fire();
        assertInstanceOf(IllegalStateException.class, errors.get(0));
    }
}
