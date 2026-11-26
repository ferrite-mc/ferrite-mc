// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package dev.ferrite.nms.bridge;

import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.entity.Entity;

/**
 * Lane 2's adapter from Ferrite facade objects to the selected NMS snapshot handles.
 *
 * <p>Lane 2 implements this interface in {@code ferrite-api-impl}; the dependency is one-way
 * ({@code ferrite-api-impl -> ferrite-nms}). The facade classes themselves stay final and are not
 * modified. Lane 1 installs one provider with {@link NmsBridge#install(NmsHandleProvider)} after
 * the host is attached and before POSTWORLD plugins enable.
 *
 * <p>Contract for implementations:
 *
 * <ul>
 *   <li>Only facade objects owned by this host may resolve. Reject anything else with an explicit
 *       error; do not fabricate a world/player/entity from a name or UUID that the host has not
 *       confirmed.</li>
 *   <li>Return a detached snapshot handle bound to the exact facade UUID. Never return a live
 *       Rust object, a pointer, a {@code JNIEnv}, or a handle that can read another world.</li>
 *   <li>Return values must not strongly reference the Bukkit facade object. The bridge cache is
 *       weakly keyed by that object; a handle retaining it would defeat collection.</li>
 *   <li>Methods are thread-safe and may be called concurrently, including for the same facade.
 *       Reads must not park the caller waiting for a region tick; writes are already exposed as
 *       futures on the handle.</li>
 *   <li>Throw {@link NmsBridgeException} with {@code unknown_world}, {@code unknown_player} or
 *       {@code unknown_entity} for an object that is not a known facade. Host-level failures may
 *       propagate as {@link dev.ferrite.nms.host.NmsHostException}. Never return null; the bridge
 *       reports that as {@code unknown_handle}.</li>
 *   <li>Do not call {@link NmsBridge#install} or {@link NmsBridge#uninstall} from a resolve
 *       method. Ownership changes belong to Lane 1's lifecycle.</li>
 * </ul>
 */
public interface NmsHandleProvider {
    /** Resolves a world to a snapshot handle bound to {@code world.getUID()}. */
    ServerLevel level(org.bukkit.World world);

    /** Resolves a player to a snapshot handle bound to {@code player.getUniqueId()}. */
    ServerPlayer player(org.bukkit.entity.Player player);

    /**
     * Resolves a non-player entity to a snapshot handle bound to
     * {@code entity.getUniqueId()}. The bridge routes {@link org.bukkit.entity.Player} through
     * {@link #player(org.bukkit.entity.Player)} so one player has one canonical handle.
     */
    Entity entity(org.bukkit.entity.Entity entity);
}
