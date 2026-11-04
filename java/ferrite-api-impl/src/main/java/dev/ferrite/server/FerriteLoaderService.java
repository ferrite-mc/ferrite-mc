package dev.ferrite.server;
@SuppressWarnings({"deprecation","unchecked","rawtypes"})
public class FerriteLoaderService implements org.bukkit.plugin.PluginLoader {
public java.util.Map<java.lang.Class<? extends org.bukkit.event.Event>, java.util.Set<org.bukkit.plugin.RegisteredListener>> createRegisteredListeners(org.bukkit.event.Listener a0, org.bukkit.plugin.Plugin a1) { return (java.util.Map<java.lang.Class<? extends org.bukkit.event.Event>, java.util.Set<org.bukkit.plugin.RegisteredListener>>) HostBridge.call("PluginLoader", "", "createRegisteredListeners(Listener,Plugin)", java.util.Map.class, a0, a1); }
public void disablePlugin(org.bukkit.plugin.Plugin a0) { HostBridge.callVoid("PluginLoader", "", "disablePlugin(Plugin)", a0); }
public void enablePlugin(org.bukkit.plugin.Plugin a0) { HostBridge.callVoid("PluginLoader", "", "enablePlugin(Plugin)", a0); }
public org.bukkit.plugin.PluginDescriptionFile getPluginDescription(java.io.File a0) { return (org.bukkit.plugin.PluginDescriptionFile) HostBridge.call("PluginLoader", "", "getPluginDescription(File)", org.bukkit.plugin.PluginDescriptionFile.class, a0); }
public java.util.regex.Pattern[] getPluginFileFilters() { return (java.util.regex.Pattern[]) HostBridge.call("PluginLoader", "", "getPluginFileFilters()", java.util.regex.Pattern[].class); }
public org.bukkit.plugin.Plugin loadPlugin(java.io.File a0) { return (org.bukkit.plugin.Plugin) HostBridge.call("PluginLoader", "", "loadPlugin(File)", org.bukkit.plugin.Plugin.class, a0); }
}
