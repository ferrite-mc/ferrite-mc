// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package dev.ferrite.nms.bridge;

import java.util.Map;
import java.util.Objects;
import java.util.WeakHashMap;
import java.util.function.Consumer;
import java.util.function.Supplier;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.entity.Entity;
import org.bukkit.World;
import org.bukkit.craftbukkit.CraftWorld;
import org.bukkit.craftbukkit.entity.CraftEntity;
import org.bukkit.craftbukkit.entity.CraftPlayer;
import org.bukkit.entity.Player;

/**
 * Process-wide bridge from Bukkit facade objects to detached NMS snapshot handles.
 *
 * <p>This is the supported entry point for plugins that need the selected NMS surface without a
 * {@code (CraftWorld)}/{@code (CraftPlayer)} cast. It does <strong>not</strong> make those casts
 * succeed: Lane 2's {@code FerriteWorld}/{@code FerritePlayer} remain final classes whose
 * superclass is not CraftBukkit. A third-party plugin that casts to a Craft type still fails
 * unless Lane 2 later changes that hierarchy.
 *
 * <h2>Canonical weak cache</h2>
 *
 * <p>There is one cache per handle kind (world, player, entity). Keys are the Bukkit facade
 * objects and are held weakly, so the bridge never keeps a world or player alive. Values are
 * detached snapshot handles; the provider must not put the facade inside a handle. The same
 * facade object resolves to the same handle instance while it is reachable and the bridge has not
 * been uninstalled. A {@link Player} resolved through {@link #entity} uses the player cache, so
 * the player handle is canonical across both entry points.
 *
 * <p>Keys use the facade's {@code equals}/{@code hashCode}. If a facade implementation treats its
 * UUID as identity, interchangeable wrappers for the same UUID may share a handle; this is safe
 * because the bridge verifies the handle's UUID before caching it. Implementations must keep that
 * identity stable for the facade's lifetime.
 *
 * <h2>Threading and lifecycle</h2>
 *
 * <p>Registration, cache reads/writes and cache clearing are guarded by one private lock.
 * Provider calls happen outside that lock, so a slow host read does not block other handle types
 * or install/uninstall. Two concurrent misses for the same facade may therefore call the
 * provider more than once, but only the first validated handle is cached; every caller receives
 * that canonical instance.
 *
 * <p>{@link #install} accepts one provider. Installing the same provider again is an idempotent
 * no-op; installing a different provider while one is active fails with
 * {@code provider_already_installed}. {@link #uninstall} requires the current owner, while
 * uninstalling when none is installed and {@link #close()} are idempotent. Uninstall/close clears
 * all caches and increments a generation number.
 *
 * <p>A resolve that overlaps an uninstall or close is epoch-bound. If it finishes after the
 * generation changed it fails with {@code bridge_uninstalled} and never inserts a stale handle
 * into the next generation's cache. If it cached and returned before the clear, it linearized
 * before the uninstall. Existing handles are detached snapshots; uninstall does not invalidate an
 * already returned reference, but any new bridge lookup requires a new install.
 *
 * <p>Reads through a real {@link CraftWorld}/{@link CraftPlayer}/{@link CraftEntity} bypass the
 * provider and use their inherited {@code getHandle()}. Ferrite facade objects take the provider
 * path. Both paths use the same cache and epoch rules. With neither a Craft type nor an installed
 * provider, lookup fails explicitly; there is no fallback.
 */
public final class NmsBridge {
    private static final Object LOCK = new Object();
    private static final Map<World, ServerLevel> LEVELS = new WeakHashMap<>();
    private static final Map<Player, ServerPlayer> PLAYERS = new WeakHashMap<>();
    private static final Map<org.bukkit.entity.Entity, Entity> ENTITIES = new WeakHashMap<>();

    private static NmsHandleProvider provider;
    private static long generation;

    private NmsBridge() {}

    /**
     * Installs the one provider for this JVM. Re-installing the same provider is a no-op.
     *
     * @throws NmsBridgeException {@code provider_already_installed} when another provider owns
     *     the bridge
     */
    public static void install(NmsHandleProvider next) {
        Objects.requireNonNull(next, "provider");
        synchronized (LOCK) {
            if (provider == next) {
                return;
            }
            if (provider != null) {
                throw new NmsBridgeException(
                        "provider_already_installed",
                        "Another NmsHandleProvider (" + provider.getClass().getName()
                                + ") is already installed; uninstall it before installing "
                                + next.getClass().getName() + ". The bridge has one owner per JVM.");
            }
            generation++;
            clearCachesLocked();
            provider = next;
        }
    }

    /**
     * Removes {@code expected} and clears every cached handle. A no-op when nothing is installed.
     *
     * @throws NmsBridgeException {@code provider_owner_mismatch} when a different provider is
     *     installed; that provider remains active
     */
    public static void uninstall(NmsHandleProvider expected) {
        Objects.requireNonNull(expected, "provider");
        synchronized (LOCK) {
            if (provider == null) {
                return;
            }
            if (provider != expected) {
                throw new NmsBridgeException(
                        "provider_owner_mismatch",
                        "The installed NmsHandleProvider is " + provider.getClass().getName()
                                + ", not " + expected.getClass().getName()
                                + "; the current provider was left installed.");
            }
            detachCacheLocked();
        }
    }

    /**
     * Detaches the current provider and clears caches. Safe to call repeatedly during shutdown;
     * this does not call any provider-specific close hook because the SPI has none.
     */
    public static void close() {
        synchronized (LOCK) {
            detachCacheLocked();
        }
    }

    /** Whether a provider is currently installed. */
    public static boolean isInstalled() {
        synchronized (LOCK) {
            return provider != null;
        }
    }

    /**
     * Resolves a world to its canonical snapshot handle.
     *
     * @throws NmsBridgeException {@code provider_not_installed}, {@code unknown_handle},
     *     {@code handle_identity_mismatch} or {@code bridge_uninstalled}
     */
    public static ServerLevel level(World world) {
        Objects.requireNonNull(world, "world");
        Attachment attachment = attachment();
        if (world instanceof CraftWorld craftWorld) {
            return canonical(
                    LEVELS,
                    world,
                    attachment,
                    craftWorld::getHandle,
                    handle -> requireWorldIdentity(world, handle));
        }
        NmsHandleProvider current = attachment.requireProvider("world", world.getUID());
        return canonical(
                LEVELS,
                world,
                attachment,
                () -> current.level(world),
                handle -> requireWorldIdentity(world, handle));
    }

    /**
     * Resolves a player to its canonical snapshot handle.
     *
     * @throws NmsBridgeException {@code provider_not_installed}, {@code unknown_handle},
     *     {@code handle_identity_mismatch} or {@code bridge_uninstalled}
     */
    public static ServerPlayer player(Player player) {
        Objects.requireNonNull(player, "player");
        Attachment attachment = attachment();
        if (player instanceof CraftPlayer craftPlayer) {
            return canonical(
                    PLAYERS,
                    player,
                    attachment,
                    craftPlayer::getHandle,
                    handle -> requirePlayerIdentity(player, handle));
        }
        NmsHandleProvider current = attachment.requireProvider("player", player.getUniqueId());
        return canonical(
                PLAYERS,
                player,
                attachment,
                () -> current.player(player),
                handle -> requirePlayerIdentity(player, handle));
    }

    /**
     * Resolves a non-player entity to its canonical snapshot handle. A {@link Player} is routed
     * through {@link #player} so the two entry points cannot create different player handles.
     *
     * @throws NmsBridgeException {@code provider_not_installed}, {@code unknown_handle},
     *     {@code handle_identity_mismatch} or {@code bridge_uninstalled}
     */
    public static Entity entity(org.bukkit.entity.Entity entity) {
        Objects.requireNonNull(entity, "entity");
        if (entity instanceof Player player) {
            return player(player);
        }
        Attachment attachment = attachment();
        if (entity instanceof CraftEntity craftEntity) {
            return canonical(
                    ENTITIES,
                    entity,
                    attachment,
                    craftEntity::getHandle,
                    handle -> requireEntityIdentity(entity, handle));
        }
        NmsHandleProvider current = attachment.requireProvider("entity", entity.getUniqueId());
        return canonical(
                ENTITIES,
                entity,
                attachment,
                () -> current.entity(entity),
                handle -> requireEntityIdentity(entity, handle));
    }

    private static Attachment attachment() {
        synchronized (LOCK) {
            return new Attachment(provider, generation);
        }
    }

    /**
     * Returns the canonical value for {@code key}. The provider call is deliberately outside the
     * lock; the second check discards a duplicate a concurrent resolver may have produced.
     */
    private static <K, V> V canonical(
            Map<K, V> cache,
            K key,
            Attachment attachment,
            Supplier<V> resolver,
            Consumer<V> validator) {
        synchronized (LOCK) {
            attachment.requireGeneration();
            V cached = cache.get(key);
            if (cached != null) {
                return cached;
            }
        }

        V resolved = resolver.get();
        validator.accept(resolved);

        synchronized (LOCK) {
            attachment.requireGeneration();
            V existing = cache.get(key);
            if (existing != null) {
                return existing;
            }
            cache.put(key, resolved);
            return resolved;
        }
    }

    private static void requireWorldIdentity(World world, ServerLevel handle) {
        if (handle == null) {
            throw unknownHandle("world", world.getUID());
        }
        if (!world.getUID().equals(handle.getUUID())) {
            throw new NmsBridgeException(
                    "handle_identity_mismatch",
                    "The provider returned ServerLevel " + handle.getUUID() + " for world "
                            + world.getUID() + "; a provider may only return a handle bound to the "
                            + "requested facade.");
        }
    }

    private static void requirePlayerIdentity(Player player, ServerPlayer handle) {
        if (handle == null) {
            throw unknownHandle("player", player.getUniqueId());
        }
        if (!player.getUniqueId().equals(handle.getUUID())) {
            throw new NmsBridgeException(
                    "handle_identity_mismatch",
                    "The provider returned ServerPlayer " + handle.getUUID() + " for player "
                            + player.getUniqueId() + "; a provider may only return a handle bound to "
                            + "the requested facade.");
        }
    }

    private static void requireEntityIdentity(org.bukkit.entity.Entity entity, Entity handle) {
        if (handle == null) {
            throw unknownHandle("entity", entity.getUniqueId());
        }
        if (!entity.getUniqueId().equals(handle.getUUID())) {
            throw new NmsBridgeException(
                    "handle_identity_mismatch",
                    "The provider returned Entity " + handle.getUUID() + " for entity "
                            + entity.getUniqueId() + "; a provider may only return a handle bound to "
                            + "the requested facade.");
        }
    }

    private static NmsBridgeException unknownHandle(String kind, Object id) {
        return new NmsBridgeException(
                "unknown_handle",
                "The NmsHandleProvider returned null for " + kind + " " + id
                        + ". A provider must return a detached handle or throw an explicit error; "
                        + "null is never treated as missing world/player/entity.");
    }

    private static void detachCacheLocked() {
        provider = null;
        generation++;
        clearCachesLocked();
    }

    private static void clearCachesLocked() {
        LEVELS.clear();
        PLAYERS.clear();
        ENTITIES.clear();
    }

    private record Attachment(NmsHandleProvider provider, long generation) {
        NmsHandleProvider requireProvider(String kind, Object id) {
            if (provider == null) {
                throw new NmsBridgeException(
                        "provider_not_installed",
                        "No NmsHandleProvider is installed for " + kind + " " + id
                                + ". Lane 1 must call NmsBridge.install(provider) after the host is "
                                + "attached and before POSTWORLD plugins enable; Lane 2 implements "
                                + "the provider for Ferrite facade objects. No fallback exists.");
            }
            return provider;
        }

        void requireGeneration() {
            if (generation != NmsBridge.generation) {
                throw new NmsBridgeException(
                        "bridge_uninstalled",
                        "The Ferrite NMS bridge was uninstalled or replaced while this lookup was "
                                + "in flight. The stale handle was not cached; resolve again after "
                                + "the next NmsBridge.install(provider).");
            }
        }
    }
}
