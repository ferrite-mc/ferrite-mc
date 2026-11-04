package org.bukkit.craftbukkit.entity;

/** Compatibility seam for plugins that replace the Bukkit permissible. */
public abstract class CraftHumanEntity extends dev.ferrite.server.PlayerExtras {
    public org.bukkit.permissions.PermissibleBase perm;
    @Override public boolean hasPermission(String name) { return perm.hasPermission(name); }
    @Override public boolean hasPermission(org.bukkit.permissions.Permission permission) { return perm.hasPermission(permission); }
    @Override public boolean isPermissionSet(String name) { return perm.isPermissionSet(name); }
    @Override public boolean isPermissionSet(org.bukkit.permissions.Permission permission) { return perm.isPermissionSet(permission); }
    @Override public org.bukkit.permissions.PermissionAttachment addAttachment(org.bukkit.plugin.Plugin plugin) { return perm.addAttachment(plugin); }
    @Override public org.bukkit.permissions.PermissionAttachment addAttachment(org.bukkit.plugin.Plugin plugin, String name, boolean value) { return perm.addAttachment(plugin, name, value); }
    @Override public org.bukkit.permissions.PermissionAttachment addAttachment(org.bukkit.plugin.Plugin plugin, int ticks) { return perm.addAttachment(plugin, ticks); }
    @Override public org.bukkit.permissions.PermissionAttachment addAttachment(org.bukkit.plugin.Plugin plugin, String name, boolean value, int ticks) { return perm.addAttachment(plugin, name, value, ticks); }
    @Override public void removeAttachment(org.bukkit.permissions.PermissionAttachment attachment) { perm.removeAttachment(attachment); }
    @Override public void recalculatePermissions() { perm.recalculatePermissions(); }
    @Override public java.util.Set<org.bukkit.permissions.PermissionAttachmentInfo> getEffectivePermissions() { return perm.getEffectivePermissions(); }
}
