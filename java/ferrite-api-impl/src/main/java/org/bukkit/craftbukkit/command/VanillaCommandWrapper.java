package org.bukkit.craftbukkit.command;

/**
 * CraftBukkit's wrapper that exposes a Brigadier command as a Bukkit command. Command libraries
 * look this class up to detect CraftBukkit; Ferrite has no Brigadier dispatcher behind it, so
 * {@link #getListener} refuses instead of returning a source that would not work.
 */
public class VanillaCommandWrapper extends org.bukkit.command.Command {
    protected VanillaCommandWrapper(String name) { super(name); }
    public Object getListener(org.bukkit.command.CommandSender sender) {
        throw new UnsupportedOperationException("Ferrite: VanillaCommandWrapper#getListener is not implemented: there is no Brigadier command source for " + sender.getName());
    }
    @Override public boolean execute(org.bukkit.command.CommandSender sender, String label, String[] args) { return false; }
}
