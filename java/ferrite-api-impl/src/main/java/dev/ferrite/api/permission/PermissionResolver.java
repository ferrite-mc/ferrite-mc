package dev.ferrite.api.permission;

import java.util.UUID;

/**
 * In-process permission decision seam.
 *
 * <p>Lane 1's contract does not exist yet, so the facade decides from the registered
 * {@code plugin.yml} permissions plus the player's op flag. A host adapter can replace
 * this to consult Rust-side op/permission state without changing any Bukkit call site.
 */
public interface PermissionResolver {

    boolean hasPermission(String playerName, UUID playerUniqueId, boolean op, String permission);
}
