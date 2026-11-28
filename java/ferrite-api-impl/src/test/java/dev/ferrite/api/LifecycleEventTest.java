package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.brigadier.arguments.IntegerArgumentType;
import dev.ferrite.api.server.FerriteLifecycleManager;
import dev.ferrite.api.test.TestPlugin;
import dev.ferrite.api.test.TestServer;
import io.papermc.paper.command.brigadier.Commands;
import io.papermc.paper.plugin.lifecycle.event.handler.configuration.LifecycleEventHandlerConfiguration;
import io.papermc.paper.plugin.lifecycle.event.registrar.ReloadableRegistrarEvent;
import io.papermc.paper.plugin.lifecycle.event.types.LifecycleEventType;
import io.papermc.paper.plugin.lifecycle.event.types.LifecycleEvents;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.List;
import java.util.Set;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.logging.Logger;
import org.bukkit.Bukkit;
import org.bukkit.plugin.Plugin;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;

/** Paper lifecycle handlers: registration window, ordering, firing, commands and cleanup. */
class LifecycleEventTest {

    @AfterEach
    void disposeEveryManager() {
        FerriteLifecycleManager.disposeAll();
    }

    private static Plugin plugin(String name, AtomicBoolean enabled) {
        var meta = (io.papermc.paper.plugin.configuration.PluginMeta) Proxy.newProxyInstance(
                LifecycleEventTest.class.getClassLoader(),
                new Class<?>[] {io.papermc.paper.plugin.configuration.PluginMeta.class},
                (self, method, args) -> method.getName().equals("getName") ? name : null);
        return (Plugin) Proxy.newProxyInstance(
                LifecycleEventTest.class.getClassLoader(),
                new Class<?>[] {Plugin.class},
                (self, method, args) -> switch (method.getName()) {
                    case "getName" -> name;
                    case "isEnabled" -> enabled.get();
                    case "getLogger" -> Logger.getLogger(name);
                    case "getPluginMeta" -> meta;
                    case "hashCode" -> System.identityHashCode(self);
                    case "equals" -> self == args[0];
                    case "toString" -> name;
                    default -> throw new UnsupportedOperationException(method.getName());
                });
    }

    private static Plugin plugin(String name) {
        return plugin(name, new AtomicBoolean(true));
    }

    @Test
    void theCommandsEventTypeInitialisesThroughTheServiceProvider() {
        assertEquals("commands", LifecycleEvents.COMMANDS.name());
        assertNotNull(LifecycleEvents.TAGS);
        assertEquals("datapack_discovery", LifecycleEvents.DATAPACK_DISCOVERY.name());
    }

    @Test
    void handlersRunByMonitorThenPriorityThenRegistrationOrder() {
        try (TestServer server = new TestServer()) {
            FerriteLifecycleManager manager =
                    new FerriteLifecycleManager(plugin("OrderPlugin"), () -> true);
            List<String> ran = new ArrayList<>();
            manager.registerEventHandler(
                    LifecycleEvents.COMMANDS.newHandler(e -> ran.add("late-" + e.cause())).priority(10));
            manager.registerEventHandler(
                    LifecycleEvents.COMMANDS.newHandler(e -> ran.add("monitor")).monitor());
            manager.registerEventHandler(LifecycleEvents.COMMANDS, e -> ran.add("default-a"));
            manager.registerEventHandler(
                    LifecycleEvents.COMMANDS.newHandler(e -> ran.add("early")).priority(-5));
            manager.registerEventHandler(LifecycleEvents.COMMANDS, e -> ran.add("default-b"));
            FerriteLifecycleManager.fireCommands(ReloadableRegistrarEvent.Cause.INITIAL);
            assertEquals(
                    List.of("early", "default-a", "default-b", "late-INITIAL", "monitor"), ran);
            ran.clear();
            FerriteLifecycleManager.fireCommands(ReloadableRegistrarEvent.Cause.RELOAD);
            assertEquals("late-RELOAD", ran.get(3));
            manager.dispose();
        }
    }

    @Test
    void aThrowingHandlerDoesNotStopTheNextOne() {
        try (TestServer server = new TestServer()) {
            FerriteLifecycleManager manager =
                    new FerriteLifecycleManager(plugin("ThrowPlugin"), () -> true);
            List<String> ran = new ArrayList<>();
            manager.registerEventHandler(LifecycleEvents.COMMANDS, e -> {
                throw new IllegalStateException("boom");
            });
            manager.registerEventHandler(LifecycleEvents.COMMANDS, e -> ran.add("after"));
            FerriteLifecycleManager.fireCommands(ReloadableRegistrarEvent.Cause.INITIAL);
            assertEquals(List.of("after"), ran);
            manager.dispose();
        }
    }

    @Test
    void registrationIsRefusedOutsideTheEnableWindowAndForUnknownConfigurations() {
        try (TestServer server = new TestServer()) {
            AtomicBoolean open = new AtomicBoolean(true);
            FerriteLifecycleManager manager =
                    new FerriteLifecycleManager(plugin("WindowPlugin", new AtomicBoolean(true)), open::get);
            manager.registerEventHandler(LifecycleEvents.COMMANDS, e -> {
            });
            assertEquals(1, manager.handlerCount());
            open.set(false);
            assertThrows(
                    IllegalStateException.class,
                    () -> manager.registerEventHandler(LifecycleEvents.COMMANDS, e -> {
                    }));
            assertEquals(1, manager.handlerCount());
            assertThrows(NullPointerException.class, () -> manager.registerEventHandler(null));
            assertThrows(
                    IllegalArgumentException.class,
                    () -> manager.registerEventHandler(
                            new LifecycleEventHandlerConfiguration<Plugin>() {
                            }));
            manager.dispose();
        }
    }

    @Test
    void bootstrapOnlyEventsAreRefusedBecauseTheyCouldNeverFire() {
        try (TestServer server = new TestServer()) {
            FerriteLifecycleManager manager =
                    new FerriteLifecycleManager(plugin("BootPlugin"), () -> true);
            @SuppressWarnings({"unchecked", "rawtypes"})
            LifecycleEventType type = (LifecycleEventType) LifecycleEvents.DATAPACK_DISCOVERY;
            @SuppressWarnings("unchecked")
            LifecycleEventHandlerConfiguration<Plugin> config =
                    (LifecycleEventHandlerConfiguration<Plugin>)
                            type.newHandler(
                                    (io.papermc.paper.plugin.lifecycle.event.handler
                                                    .LifecycleEventHandler<
                                            io.papermc.paper.plugin.lifecycle.event
                                                    .LifecycleEvent>)
                                            e -> {
                                            });
            UnsupportedOperationException error =
                    assertThrows(
                            UnsupportedOperationException.class,
                            () -> manager.registerEventHandler(config));
            assertTrue(error.getMessage().contains("datapack_discovery"), error.getMessage());
            assertEquals(0, manager.handlerCount());
            manager.dispose();
        }
    }

    @Test
    void aJavaPluginExposesTheManagerAndEnablingFiresTheInitialEvent() throws Exception {
        try (TestServer server = new TestServer()) {
            List<String> said = new ArrayList<>();
            TestPlugin plugin = server.attachPlugin("AutoLifePlugin");
            assertTrue(plugin.getLifecycleManager() instanceof FerriteLifecycleManager);
            // Paper allows registration from construction to the end of onEnable.
            plugin.getLifecycleManager().registerEventHandler(
                    LifecycleEvents.COMMANDS,
                    event -> event.registrar().register(
                            "autolife", "d", List.of(), (stack, args) -> said.add("ran")));
            server.enable(plugin);
            server.onMain(() -> assertTrue(
                    Bukkit.getServer().dispatchCommand(Bukkit.getConsoleSender(), "autolife"),
                    "COMMANDS must fire after the last plugin enables"));
            assertEquals(List.of("ran"), said);
        }
    }

    @Test
    void basicAndBrigadierCommandsReachTheServerAndAreRemovedWithThePlugin() {
        try (TestServer server = new TestServer()) {
            Plugin plugin = plugin("CmdPlugin");
            FerriteLifecycleManager manager = new FerriteLifecycleManager(plugin, () -> true);
            List<String> said = new ArrayList<>();
            List<Set<String>> accepted = new ArrayList<>();
            manager.registerEventHandler(LifecycleEvents.COMMANDS, event -> {
                Commands registrar = event.registrar();
                accepted.add(registrar.register(
                        "lifehello",
                        "says hello",
                        List.of("lifehi"),
                        (stack, args) -> said.add("basic:" + String.join(",", args))));
                accepted.add(registrar.register(
                        Commands.literal("lifeadd")
                                .then(Commands.argument("n", IntegerArgumentType.integer(0, 100))
                                        .executes(context -> {
                                            said.add(
                                                    "add:"
                                                            + IntegerArgumentType.getInteger(
                                                                    context, "n"));
                                            return 1;
                                        }))
                                .build(),
                        "adds",
                        List.of("lifeplus")));
            });
            FerriteLifecycleManager.fireCommands(ReloadableRegistrarEvent.Cause.INITIAL);
            assertEquals(Set.of("lifehello", "lifehi"), accepted.get(0));
            assertEquals(Set.of("lifeadd", "lifeplus"), accepted.get(1));

            var console = Bukkit.getConsoleSender();
            assertTrue(
                    Bukkit.getCommandMap().getKnownCommands().containsKey("cmdplugin:lifehello"));
            server.onMain(() -> {
                assertTrue(Bukkit.getServer().dispatchCommand(console, "lifehi a b"));
                assertTrue(Bukkit.getServer().dispatchCommand(console, "lifeplus 42"));
                assertTrue(Bukkit.getServer().dispatchCommand(console, "cmdplugin:lifeadd 7"));
            });
            assertEquals(List.of("basic:a,b", "add:42", "add:7"), said);
            said.clear();
            server.onMain(() -> assertTrue(
                    Bukkit.getServer().dispatchCommand(console, "lifeadd 500"),
                    "a syntax error is reported to the sender, not thrown"));
            assertTrue(said.isEmpty());

            // Reload forgets the previous round and registers again exactly once.
            FerriteLifecycleManager.fireCommands(ReloadableRegistrarEvent.Cause.RELOAD);
            assertEquals(4, accepted.size());
            manager.dispose();
            assertNull(Bukkit.getCommandMap().getCommand("lifehello"));
            assertNull(Bukkit.getCommandMap().getCommand("cmdplugin:lifeadd"));
            assertFalse(Bukkit.getCommandMap().getKnownCommands().containsKey("cmdplugin:lifeadd"));
        }
    }
}
