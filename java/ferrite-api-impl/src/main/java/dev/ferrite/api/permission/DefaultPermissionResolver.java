package dev.ferrite.api.permission;

import dev.ferrite.api.plugin.FerritePluginManager;
import java.util.Objects;
import java.util.UUID;
import org.bukkit.permissions.Permission;

/**
 * Bukkit-shaped fallback: a registered permission applies its declared default, and an
 * unknown permission falls back to the player's op flag.
 */
public final class DefaultPermissionResolver implements PermissionResolver {

    private final FerritePluginManager permissions;

    public DefaultPermissionResolver(FerritePluginManager permissions) {
        this.permissions = Objects.requireNonNull(permissions, "permissions");
    }

    @Override
    public boolean hasPermission(String playerName, UUID playerUniqueId, boolean op, String permission) {
        Permission registered = permissions.getPermission(permission);
        return registered == null ? op : registered.getDefault().getValue(op);
    }
}
