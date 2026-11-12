package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import com.mojang.brigadier.arguments.IntegerArgumentType;
import io.papermc.paper.command.brigadier.Commands;
import io.papermc.paper.plugin.lifecycle.event.registrar.ReloadableRegistrarEvent;
import io.papermc.paper.plugin.lifecycle.event.types.LifecycleEvents;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.logging.Logger;
import org.bukkit.plugin.Plugin;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

/** Paper lifecycle handlers: registration window, ordering, firing, command ownership and cleanup. */
class FerriteLifecycleTest {
    private final List<Plugin> created = new ArrayList<>();

    @BeforeAll static void start() throws Exception {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
    }

    @AfterEach void clean() { created.forEach(FerriteLifecycleManager::dispose); created.clear(); }

    private Plugin plugin(String name) {
        var enabled = new AtomicBoolean(true);
        var meta = (io.papermc.paper.plugin.configuration.PluginMeta) Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{io.papermc.paper.plugin.configuration.PluginMeta.class},
            (self, method, args) -> method.getName().equals("getName") ? name : null);
        Plugin plugin = (Plugin) Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{Plugin.class}, (self, method, args) -> switch (method.getName()) {
            case "getName" -> name;
            case "isEnabled" -> enabled.get();
            case "getLogger" -> Logger.getLogger(name);
            case "getPluginMeta" -> meta;
            case "hashCode" -> System.identityHashCode(self);
            case "equals" -> self == args[0];
            case "toString" -> name;
            default -> throw new UnsupportedOperationException(method.getName());
        });
        created.add(plugin);
        return plugin;
    }

    @Test void theCommandsEventTypeInitialisesThroughTheServiceProvider() {
        assertEquals("commands", LifecycleEvents.COMMANDS.name());
        assertNotNull(LifecycleEvents.TAGS);
        assertEquals("datapack_discovery", LifecycleEvents.DATAPACK_DISCOVERY.name());
    }

    @Test void handlersRunByMonitorThenPriorityThenRegistrationOrderWithTheRightCause() {
        var manager = new FerriteLifecycleManager(plugin("OrderPlugin"), () -> true);
        var ran = new ArrayList<String>();
        manager.registerEventHandler(LifecycleEvents.COMMANDS.newHandler(e -> ran.add("late-" + e.cause())).priority(10));
        manager.registerEventHandler(LifecycleEvents.COMMANDS.newHandler(e -> ran.add("monitor")).monitor());
        manager.registerEventHandler(LifecycleEvents.COMMANDS, e -> ran.add("default-a"));
        manager.registerEventHandler(LifecycleEvents.COMMANDS.newHandler(e -> ran.add("early")).priority(-5));
        manager.registerEventHandler(LifecycleEvents.COMMANDS, e -> ran.add("default-b"));
        FerriteLifecycleManager.fireCommands(ReloadableRegistrarEvent.Cause.INITIAL);
        assertEquals(List.of("early", "default-a", "default-b", "late-INITIAL", "monitor"), ran);
        ran.clear();
        FerriteLifecycleManager.fireCommands(ReloadableRegistrarEvent.Cause.RELOAD);
        assertEquals("late-RELOAD", ran.get(3));
    }

    @Test void aThrowingHandlerDoesNotStopTheNextOne() {
        var manager = new FerriteLifecycleManager(plugin("ThrowPlugin"), () -> true);
        var ran = new ArrayList<String>();
        manager.registerEventHandler(LifecycleEvents.COMMANDS, e -> { throw new IllegalStateException("boom"); });
        manager.registerEventHandler(LifecycleEvents.COMMANDS, e -> ran.add("after"));
        FerriteLifecycleManager.fireCommands(ReloadableRegistrarEvent.Cause.INITIAL);
        assertEquals(List.of("after"), ran);
    }

    @Test void registrationIsRefusedOutsideTheEnableWindowAndForNullsAndUnknownConfigurations() {
        var open = new AtomicBoolean(true);
        var manager = new FerriteLifecycleManager(plugin("WindowPlugin"), open::get);
        manager.registerEventHandler(LifecycleEvents.COMMANDS, e -> {});
        assertEquals(1, manager.handlerCount());
        open.set(false);
        assertThrows(IllegalStateException.class, () -> manager.registerEventHandler(LifecycleEvents.COMMANDS, e -> {}));
        assertEquals(1, manager.handlerCount());
        open.set(true);
        assertThrows(NullPointerException.class, () -> manager.registerEventHandler(null));
        assertThrows(NullPointerException.class, () -> LifecycleEvents.COMMANDS.newHandler(null));
        assertThrows(IllegalArgumentException.class, () -> manager.registerEventHandler(new io.papermc.paper.plugin.lifecycle.event.handler.configuration.LifecycleEventHandlerConfiguration<Plugin>() {}));
    }

    @Test void bootstrapOnlyEventsAreRefusedBecauseTheyCouldNeverFire() {
        var manager = new FerriteLifecycleManager(plugin("BootPlugin"), () -> true);
        @SuppressWarnings({"unchecked", "rawtypes"}) var type = (io.papermc.paper.plugin.lifecycle.event.types.LifecycleEventType) LifecycleEvents.DATAPACK_DISCOVERY;
        @SuppressWarnings("unchecked") var config = (io.papermc.paper.plugin.lifecycle.event.handler.configuration.LifecycleEventHandlerConfiguration<Plugin>) type.newHandler((io.papermc.paper.plugin.lifecycle.event.handler.LifecycleEventHandler<io.papermc.paper.plugin.lifecycle.event.LifecycleEvent>) e -> {});
        var error = assertThrows(UnsupportedOperationException.class, () -> manager.registerEventHandler(config));
        assertTrue(error.getMessage().contains("datapack_discovery"), error.getMessage());
        assertEquals(0, manager.handlerCount());
    }

    @Test void basicAndBrigadierCommandsReachTheServerAndAreRemovedWithThePlugin() {
        var plugin = plugin("CmdPlugin");
        var manager = new FerriteLifecycleManager(plugin, () -> true);
        var said = new ArrayList<String>();
        var accepted = new ArrayList<java.util.Set<String>>();
        manager.registerEventHandler(LifecycleEvents.COMMANDS, event -> {
            Commands registrar = event.registrar();
            accepted.add(registrar.register("lifehello", "says hello", List.of("lifehi"), (stack, args) -> said.add("basic:" + String.join(",", args))));
            accepted.add(registrar.register(Commands.literal("lifeadd")
                .then(Commands.argument("n", IntegerArgumentType.integer(0, 100)).executes(context -> { said.add("add:" + IntegerArgumentType.getInteger(context, "n")); return 1; }))
                .build(), "adds", List.of("lifeplus")));
        });
        FerriteLifecycleManager.fireCommands(ReloadableRegistrarEvent.Cause.INITIAL);
        assertEquals(java.util.Set.of("lifehello", "lifehi"), accepted.get(0));
        assertEquals(java.util.Set.of("lifeadd", "lifeplus"), accepted.get(1));

        var registered = FerriteLifecycleManager.registeredCommands(plugin);
        assertEquals(List.of("lifehello", "lifeadd"), registered.stream().map(FerriteCommands.Registered::label).toList());
        assertEquals(List.of("lifehi"), registered.get(0).aliases());

        var server = org.bukkit.Bukkit.getServer();
        var console = server.getConsoleSender();
        assertTrue(server.getCommandMap().getKnownCommands().containsKey("cmdplugin:lifehello"));
        assertTrue(server.dispatchCommand(console, "lifehi a b"));
        assertTrue(server.dispatchCommand(console, "lifeplus 42"));
        assertTrue(server.dispatchCommand(console, "cmdplugin:lifeadd 7"));
        assertEquals(List.of("basic:a,b", "add:42", "add:7"), said);
        said.clear();
        assertTrue(server.dispatchCommand(console, "lifeadd 500"), "a syntax error is reported to the sender, not thrown");
        assertTrue(said.isEmpty());

        // Reload forgets the previous round and registers again exactly once.
        FerriteLifecycleManager.fireCommands(ReloadableRegistrarEvent.Cause.RELOAD);
        assertEquals(2, FerriteLifecycleManager.registeredCommands(plugin).size());

        FerriteLifecycleManager.dispose(plugin);
        assertTrue(FerriteLifecycleManager.registeredCommands(plugin).isEmpty());
        assertNull(server.getCommandMap().getCommand("lifehello"));
        assertNull(server.getCommandMap().getCommand("cmdplugin:lifeadd"));
        said.clear();
        FerriteLifecycleManager.fireCommands(ReloadableRegistrarEvent.Cause.INITIAL);
        assertTrue(FerriteLifecycleManager.registeredCommands(plugin).isEmpty(), "a disposed plugin's handlers never fire again");
    }

    @Test void conflictingLabelsAreNotStolenAndInvalidLabelsAreRejected() {
        var first = new FerriteLifecycleManager(plugin("FirstPlugin"), () -> true);
        var second = new FerriteLifecycleManager(plugin("SecondPlugin"), () -> true);
        var result = new ArrayList<java.util.Set<String>>();
        first.registerEventHandler(LifecycleEvents.COMMANDS, e -> result.add(e.registrar().register("shared", "d", List.of(), (s, a) -> {})));
        second.registerEventHandler(LifecycleEvents.COMMANDS, e -> {
            result.add(e.registrar().register("shared", "d", List.of(), (s, a) -> {}));
            assertThrows(IllegalArgumentException.class, () -> e.registrar().register("bad label", "d", List.of(), (s, a) -> {}));
            assertThrows(IllegalArgumentException.class, () -> e.registrar().register("a:b", "d", List.of(), (s, a) -> {}));
            assertThrows(NullPointerException.class, () -> e.registrar().register("x", "d", List.of(), null));
        });
        FerriteLifecycleManager.fireCommands(ReloadableRegistrarEvent.Cause.INITIAL);
        assertEquals(java.util.Set.of("shared"), result.get(0));
        assertEquals(java.util.Set.of(), result.get(1), "the second plugin only gets its prefixed name");
        assertTrue(org.bukkit.Bukkit.getServer().getCommandMap().getKnownCommands().containsKey("secondplugin:shared"));
    }
}
