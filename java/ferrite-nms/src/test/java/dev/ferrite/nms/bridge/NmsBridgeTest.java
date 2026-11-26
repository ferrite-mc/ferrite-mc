// Clean-room Ferrite NMS bridge test. No Mojang code or assets.
package dev.ferrite.nms.bridge;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNotSame;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.nms.host.NmsHostBinding;
import dev.ferrite.nms.host.WorldSnapshot;
import dev.ferrite.nms.test.FakeNmsHost;
import java.lang.ref.WeakReference;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.Collections;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Set;
import java.util.UUID;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicInteger;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import org.bukkit.World;
import org.bukkit.entity.Player;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

class NmsBridgeTest {
    private static final UUID OTHER_WORLD_ID = UUID.fromString("00000000-0000-0000-0000-000000000099");

    private FakeNmsHost host;
    private TestProvider provider;

    @BeforeEach
    void setUp() {
        NmsBridge.close();
        host = new FakeNmsHost();
        host.worlds.put(
                OTHER_WORLD_ID,
                new WorldSnapshot(
                        OTHER_WORLD_ID,
                        "other",
                        "minecraft:the_nether",
                        "NETHER",
                        99L,
                        0L,
                        0.0,
                        64.0,
                        0.0,
                        0,
                        256));
        NmsHostBinding.install(host);
        provider = new TestProvider();
    }

    @AfterEach
    void tearDown() {
        try {
            NmsBridge.close();
        } finally {
            try {
                NmsHostBinding.uninstall(host);
            } catch (IllegalStateException ignored) {
                // A test may have detached the host deliberately.
            }
            host.close();
        }
    }

    @Test
    void sameWorldFacadeReturnsOneCanonicalHandle() {
        NmsBridge.install(provider);
        World world = world(FakeNmsHost.WORLD_ID, "world");

        ServerLevel first = NmsBridge.level(world);
        ServerLevel second = NmsBridge.level(world);

        assertSame(first, second);
        assertEquals(FakeNmsHost.WORLD_ID, first.getUUID());
        assertEquals(1, provider.levelCalls.get(), "a cache hit must not call the provider again");
    }

    @Test
    void playerAndEntityCachesAreSeparateButAPlayerIsCanonicalAcrossBoth() {
        NmsBridge.install(provider);
        Player player = player(FakeNmsHost.PLAYER_ID, "Steve");
        org.bukkit.entity.Entity entity =
                entity(FakeNmsHost.ENTITY_ID, "armor stand");

        ServerPlayer playerHandle = NmsBridge.player(player);
        ServerPlayer again = NmsBridge.player(player);
        var entityHandle = NmsBridge.entity(entity);
        var entityAgain = NmsBridge.entity(entity);

        assertSame(playerHandle, again);
        assertSame(entityHandle, entityAgain);
        assertSame(playerHandle, NmsBridge.entity(player));
        assertEquals(1, provider.playerCalls.get());
        assertEquals(1, provider.entityCalls.get());
        assertEquals(FakeNmsHost.PLAYER_ID, playerHandle.getUUID());
        assertEquals(FakeNmsHost.ENTITY_ID, entityHandle.getUUID());
    }

    @Test
    void unknownFacadesFailWithExplicitCodes() {
        NmsBridge.install(provider);
        NmsBridgeException unknownWorld = assertThrows(
                NmsBridgeException.class,
                () -> NmsBridge.level(world(UUID.randomUUID(), "missing")));
        NmsBridgeException unknownPlayer = assertThrows(
                NmsBridgeException.class,
                () -> NmsBridge.player(player(UUID.randomUUID(), "missing")));
        NmsBridgeException unknownEntity = assertThrows(
                NmsBridgeException.class,
                () -> NmsBridge.entity(entity(UUID.randomUUID(), "missing")));

        assertEquals("unknown_world", unknownWorld.code());
        assertEquals("unknown_player", unknownPlayer.code());
        assertEquals("unknown_entity", unknownEntity.code());
    }

    @Test
    void providerIsRequiredForFerriteFacadesAndThereIsNoFallback() {
        NmsBridge.close();

        assertEquals(
                "provider_not_installed",
                assertThrows(
                                NmsBridgeException.class,
                                () -> NmsBridge.level(world(FakeNmsHost.WORLD_ID, "world")))
                        .code());
        assertEquals(
                "provider_not_installed",
                assertThrows(
                                NmsBridgeException.class,
                                () -> NmsBridge.player(player(FakeNmsHost.PLAYER_ID, "Steve")))
                        .code());
        assertEquals(
                "provider_not_installed",
                assertThrows(
                                NmsBridgeException.class,
                                () -> NmsBridge.entity(entity(FakeNmsHost.ENTITY_ID, "armor stand")))
                        .code());
    }

    @Test
    void duplicateInstallIsIdempotentButASecondOwnerIsRejected() {
        NmsBridge.install(provider);
        World world = world(FakeNmsHost.WORLD_ID, "world");
        ServerLevel first = NmsBridge.level(world);

        NmsBridge.install(provider);
        assertSame(first, NmsBridge.level(world));
        assertEquals(1, provider.levelCalls.get());

        TestProvider secondOwner = new TestProvider();
        NmsBridgeException error = assertThrows(
                NmsBridgeException.class, () -> NmsBridge.install(secondOwner));
        assertEquals("provider_already_installed", error.code());
        assertTrue(NmsBridge.isInstalled());
        assertSame(first, NmsBridge.level(world));
    }

    @Test
    void uninstallRequiresTheOwnerAndIsIdempotent() {
        NmsBridge.install(provider);
        World world = world(FakeNmsHost.WORLD_ID, "world");
        ServerLevel first = NmsBridge.level(world);

        TestProvider other = new TestProvider();
        NmsBridgeException wrongOwner =
                assertThrows(NmsBridgeException.class, () -> NmsBridge.uninstall(other));
        assertEquals("provider_owner_mismatch", wrongOwner.code());
        assertTrue(NmsBridge.isInstalled());
        assertSame(first, NmsBridge.level(world));

        NmsBridge.uninstall(provider);
        assertFalse(NmsBridge.isInstalled());
        NmsBridge.uninstall(provider);
        NmsBridge.uninstall(other);
        NmsBridge.close();
        NmsBridge.close();
        assertFalse(NmsBridge.isInstalled());

        assertEquals(
                "provider_not_installed",
                assertThrows(
                                NmsBridgeException.class,
                                () -> NmsBridge.level(world(FakeNmsHost.WORLD_ID, "world")))
                        .code());
    }

    @Test
    void closeClearsCanonicalCaches() {
        NmsBridge.install(provider);
        World world = world(FakeNmsHost.WORLD_ID, "world");
        ServerLevel first = NmsBridge.level(world);

        NmsBridge.close();
        NmsBridge.close();
        NmsBridge.install(provider);
        ServerLevel second = NmsBridge.level(world);

        assertNotSame(first, second);
        assertEquals(2, provider.levelCalls.get());
    }

    @Test
    void aProviderReturningNullIsAnExplicitUnknownHandle() {
        NmsBridge.install(new TestProvider() {
            @Override
            public ServerLevel level(World world) {
                return null;
            }
        });

        NmsBridgeException error = assertThrows(
                NmsBridgeException.class,
                () -> NmsBridge.level(world(FakeNmsHost.WORLD_ID, "world")));
        assertEquals("unknown_handle", error.code());
    }

    @Test
    void aProviderCannotReturnAnotherWorldsHandle() {
        NmsBridge.install(new TestProvider() {
            @Override
            public ServerLevel level(World world) {
                return ServerLevel.of(OTHER_WORLD_ID);
            }
        });

        NmsBridgeException error = assertThrows(
                NmsBridgeException.class,
                () -> NmsBridge.level(world(FakeNmsHost.WORLD_ID, "world")));
        assertEquals("handle_identity_mismatch", error.code());
    }

    @Test
    void weakCachesDoNotRetainWorldPlayerOrEntityFacades() throws Exception {
        NmsBridge.install(provider);
        WeakReference<World> worldFacade = resolveWorldAndForget(FakeNmsHost.WORLD_ID);
        WeakReference<Player> playerFacade = resolvePlayerAndForget(FakeNmsHost.PLAYER_ID);
        WeakReference<org.bukkit.entity.Entity> entityFacade =
                resolveEntityAndForget(FakeNmsHost.ENTITY_ID);

        awaitCollected(worldFacade);
        awaitCollected(playerFacade);
        awaitCollected(entityFacade);

        assertNull(worldFacade.get(), "the weak-key cache retained the world facade");
        assertNull(playerFacade.get(), "the weak-key cache retained the player facade");
        assertNull(entityFacade.get(), "the weak-key cache retained the entity facade");
        ServerLevel replacement = NmsBridge.level(world(FakeNmsHost.WORLD_ID, "replacement"));
        assertNotNull(replacement);
        assertEquals(FakeNmsHost.WORLD_ID, replacement.getUUID());
    }

    @Test
    void concurrentResolveReturnsOneCanonicalHandle() throws Exception {
        NmsBridge.install(provider);
        World world = world(FakeNmsHost.WORLD_ID, "world");
        int threads = 16;
        ExecutorService pool = Executors.newFixedThreadPool(threads);
        try {
            CountDownLatch ready = new CountDownLatch(threads);
            CountDownLatch start = new CountDownLatch(1);
            List<Future<ServerLevel>> results = new ArrayList<>();
            for (int i = 0; i < threads; i++) {
                results.add(pool.submit(() -> {
                    ready.countDown();
                    assertTrue(start.await(5, TimeUnit.SECONDS));
                    return NmsBridge.level(world);
                }));
            }
            assertTrue(ready.await(5, TimeUnit.SECONDS));
            start.countDown();

            Set<ServerLevel> unique =
                    Collections.newSetFromMap(new IdentityHashMap<>());
            for (Future<ServerLevel> result : results) {
                unique.add(result.get(5, TimeUnit.SECONDS));
            }
            assertEquals(1, unique.size(), "concurrent misses were not canonicalized");
        } finally {
            pool.shutdownNow();
        }
    }

    @Test
    void uninstallDuringResolveFailsInsteadOfRepopulatingTheCache() throws Exception {
        CountDownLatch entered = new CountDownLatch(1);
        CountDownLatch release = new CountDownLatch(1);
        AtomicBoolean blockOnce = new AtomicBoolean(true);
        TestProvider blocking = new TestProvider() {
            @Override
            public ServerLevel level(World world) {
                if (blockOnce.compareAndSet(true, false)) {
                    entered.countDown();
                    try {
                        assertTrue(release.await(5, TimeUnit.SECONDS));
                    } catch (InterruptedException interrupted) {
                        Thread.currentThread().interrupt();
                        throw new IllegalStateException("interrupted while blocking the test resolve", interrupted);
                    }
                }
                return super.level(world);
            }
        };
        NmsBridge.install(blocking);
        World world = world(FakeNmsHost.WORLD_ID, "world");
        ExecutorService pool = Executors.newSingleThreadExecutor();
        try {
            Future<ServerLevel> result = pool.submit(() -> NmsBridge.level(world));
            assertTrue(entered.await(5, TimeUnit.SECONDS));

            NmsBridge.uninstall(blocking);
            release.countDown();

            ExecutionException failure = assertThrows(
                    ExecutionException.class, () -> result.get(5, TimeUnit.SECONDS));
            NmsBridgeException bridgeError =
                    assertInstanceOf(NmsBridgeException.class, failure.getCause());
            assertEquals("bridge_uninstalled", bridgeError.code());
            assertFalse(NmsBridge.isInstalled());

            NmsBridge.install(blocking);
            ServerLevel fresh = NmsBridge.level(world);
            assertNotNull(fresh);
            assertEquals(FakeNmsHost.WORLD_ID, fresh.getUUID());
        } finally {
            release.countDown();
            pool.shutdownNow();
        }
    }

    private WeakReference<World> resolveWorldAndForget(UUID worldId) {
        World world = world(worldId, "weak world");
        assertNotNull(NmsBridge.level(world));
        return new WeakReference<>(world);
    }

    private WeakReference<Player> resolvePlayerAndForget(UUID playerId) {
        Player player = player(playerId, "weak player");
        assertNotNull(NmsBridge.player(player));
        return new WeakReference<>(player);
    }

    private WeakReference<org.bukkit.entity.Entity> resolveEntityAndForget(UUID entityId) {
        org.bukkit.entity.Entity entity = entity(entityId, "weak entity");
        assertNotNull(NmsBridge.entity(entity));
        return new WeakReference<>(entity);
    }

    private static void awaitCollected(WeakReference<?> reference) throws InterruptedException {
        for (int attempt = 0; attempt < 100 && reference.get() != null; attempt++) {
            System.gc();
            Thread.sleep(20);
            // Allocate pressure so a collector that defers to allocation still runs.
            byte[] pressure = new byte[1_000_000];
            if (pressure.length == 0) {
                throw new AssertionError("unreachable");
            }
        }
        assertNull(reference.get(), "the weak-key cache retained the world facade after GC");
    }

    private static World world(UUID id, String name) {
        return facade(World.class, id, name);
    }

    private static Player player(UUID id, String name) {
        return facade(Player.class, id, name);
    }

    private static org.bukkit.entity.Entity entity(UUID id, String name) {
        return facade(org.bukkit.entity.Entity.class, id, name);
    }

    @SuppressWarnings("unchecked")
    private static <T> T facade(Class<T> type, UUID id, String name) {
        return (T) Proxy.newProxyInstance(
                type.getClassLoader(),
                new Class<?>[] {type},
                (proxy, method, arguments) -> switch (method.getName()) {
                    case "getUID", "getUniqueId" -> id;
                    case "getName" -> name;
                    case "hashCode" -> System.identityHashCode(proxy);
                    case "equals" -> proxy == (arguments == null ? null : arguments[0]);
                    case "toString" -> name + "[" + id + "]";
                    default -> throw new UnsupportedOperationException(
                            "The fake facade does not implement " + method);
                });
    }

    private class TestProvider implements NmsHandleProvider {
        final AtomicInteger levelCalls = new AtomicInteger();
        final AtomicInteger playerCalls = new AtomicInteger();
        final AtomicInteger entityCalls = new AtomicInteger();

        @Override
        public ServerLevel level(World world) {
            levelCalls.incrementAndGet();
            UUID id = world.getUID();
            if (!host.worlds.containsKey(id)) {
                throw new NmsBridgeException("unknown_world", "world " + id + " is not loaded");
            }
            return ServerLevel.of(id);
        }

        @Override
        public ServerPlayer player(Player player) {
            playerCalls.incrementAndGet();
            UUID id = player.getUniqueId();
            if (!host.players.containsKey(id)) {
                throw new NmsBridgeException("unknown_player", "player " + id + " is not online");
            }
            return ServerPlayer.of(id);
        }

        @Override
        public net.minecraft.world.entity.Entity entity(org.bukkit.entity.Entity entity) {
            entityCalls.incrementAndGet();
            UUID id = entity.getUniqueId();
            if (!host.entities.containsKey(id)) {
                throw new NmsBridgeException("unknown_entity", "entity " + id + " is not loaded");
            }
            return net.minecraft.world.entity.Entity.of(id);
        }
    }
}
