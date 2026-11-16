package dev.ferrite.api.command;

import dev.ferrite.api.FerriteRuntime;
import dev.ferrite.api.internal.Unsupported;
import java.util.Collections;
import java.util.Set;
import java.util.UUID;
import net.kyori.adventure.text.Component;
import org.bukkit.Server;
import org.bukkit.command.CommandSender;
import org.bukkit.permissions.Permission;
import org.bukkit.permissions.PermissionAttachment;
import org.bukkit.permissions.PermissionAttachmentInfo;

/** The console command sender: names itself CONSOLE and has every permission. */
public final class FerriteConsoleSender extends UnsupportedConsoleCommandSenderBase {

    private final FerriteRuntime runtime;

    public FerriteConsoleSender(FerriteRuntime runtime) {
        this.runtime = runtime;
    }

    @Override
    public void sendMessage(String message) {
        runtime.logger().info("CONSOLE: " + message);
    }

    @Override
    public void sendMessage(String... messages) {
        for (String message : messages) {
            sendMessage(message);
        }
    }

    @Override
    public void sendMessage(UUID sender, String message) {
        sendMessage(message);
    }

    @Override
    public void sendMessage(UUID sender, String... messages) {
        sendMessage(messages);
    }

    @Override
    public Server getServer() {
        return runtime.server();
    }

    @Override
    public String getName() {
        return "CONSOLE";
    }

    @Override
    public Component name() {
        return Component.text("CONSOLE");
    }

    @Override
    public CommandSender.Spigot spigot() {
        throw Unsupported.exception("ConsoleCommandSender#spigot");
    }

    @Override
    public boolean isPermissionSet(String permission) {
        return true;
    }

    @Override
    public boolean isPermissionSet(Permission permission) {
        return true;
    }

    @Override
    public boolean hasPermission(String permission) {
        return true;
    }

    @Override
    public boolean hasPermission(Permission permission) {
        return true;
    }

    @Override
    public PermissionAttachment addAttachment(org.bukkit.plugin.Plugin plugin, String permission, boolean value) {
        throw Unsupported.exception("ConsoleCommandSender#addAttachment");
    }

    @Override
    public PermissionAttachment addAttachment(org.bukkit.plugin.Plugin plugin) {
        throw Unsupported.exception("ConsoleCommandSender#addAttachment");
    }

    @Override
    public PermissionAttachment addAttachment(
            org.bukkit.plugin.Plugin plugin, String permission, boolean value, int ticks) {
        throw Unsupported.exception("ConsoleCommandSender#addAttachment");
    }

    @Override
    public PermissionAttachment addAttachment(org.bukkit.plugin.Plugin plugin, int ticks) {
        throw Unsupported.exception("ConsoleCommandSender#addAttachment");
    }

    @Override
    public void removeAttachment(PermissionAttachment attachment) {
        throw Unsupported.exception("ConsoleCommandSender#removeAttachment");
    }

    @Override
    public void recalculatePermissions() {
        // The console has no attachments to recalculate.
    }

    @Override
    public Set<PermissionAttachmentInfo> getEffectivePermissions() {
        return Collections.emptySet();
    }

    @Override
    public boolean isOp() {
        return true;
    }

    @Override
    public void setOp(boolean value) {
        // The console is always op in Bukkit.
    }

    @Override
    public String toString() {
        return "FerriteConsoleSender";
    }
}
