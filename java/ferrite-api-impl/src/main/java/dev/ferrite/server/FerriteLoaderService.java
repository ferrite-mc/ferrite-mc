package dev.ferrite.server;

@SuppressWarnings("deprecation")
public class FerriteLoaderService implements org.bukkit.plugin.PluginLoader {
    private static final java.util.regex.Pattern JAR = java.util.regex.Pattern.compile("\\.jar$", java.util.regex.Pattern.CASE_INSENSITIVE);

    public java.util.Map<Class<? extends org.bukkit.event.Event>, java.util.Set<org.bukkit.plugin.RegisteredListener>> createRegisteredListeners(org.bukkit.event.Listener listener, org.bukkit.plugin.Plugin plugin) {
        java.util.Objects.requireNonNull(listener, "listener");
        java.util.Objects.requireNonNull(plugin, "plugin");
        var result = new java.util.HashMap<Class<? extends org.bukkit.event.Event>, java.util.Set<org.bukkit.plugin.RegisteredListener>>();
        var seen = new java.util.HashSet<String>();
        for (Class<?> type = listener.getClass(); type != null; type = type.getSuperclass()) {
            for (var method : type.getDeclaredMethods()) {
                if (method.isBridge() || method.isSynthetic() || !seen.add(method.getName() + java.util.Arrays.toString(method.getParameterTypes()))) continue;
                var annotation = method.getAnnotation(org.bukkit.event.EventHandler.class);
                if (annotation == null) continue;
                if (method.getParameterCount() != 1 || !org.bukkit.event.Event.class.isAssignableFrom(method.getParameterTypes()[0]) || method.getReturnType() != void.class) throw new IllegalArgumentException("Invalid event handler: " + method);
                method.setAccessible(true);
                var eventType = method.getParameterTypes()[0].asSubclass(org.bukkit.event.Event.class);
                org.bukkit.plugin.EventExecutor executor = (target, event) -> {
                    if (!eventType.isInstance(event)) return;
                    try { method.invoke(target, event); }
                    catch (java.lang.reflect.InvocationTargetException e) { throw new org.bukkit.event.EventException(e.getCause()); }
                    catch (ReflectiveOperationException e) { throw new org.bukkit.event.EventException(e); }
                };
                var registered = new org.bukkit.plugin.RegisteredListener(listener, executor, annotation.priority(), plugin, annotation.ignoreCancelled());
                result.computeIfAbsent(eventType, ignored -> new java.util.HashSet<>()).add(registered);
            }
        }
        return result;
    }
    public void disablePlugin(org.bukkit.plugin.Plugin plugin) { org.bukkit.Bukkit.getPluginManager().disablePlugin(plugin); }
    public void enablePlugin(org.bukkit.plugin.Plugin plugin) { org.bukkit.Bukkit.getPluginManager().enablePlugin(plugin); }
    public org.bukkit.plugin.PluginDescriptionFile getPluginDescription(java.io.File file) throws org.bukkit.plugin.InvalidDescriptionException {
        java.util.Objects.requireNonNull(file, "file");
        try (var jar = new java.util.jar.JarFile(file)) {
            var entry = jar.getJarEntry("plugin.yml");
            if (entry == null) entry = jar.getJarEntry("paper-plugin.yml");
            if (entry == null) throw new org.bukkit.plugin.InvalidDescriptionException("Missing plugin descriptor: " + file);
            try (var input = jar.getInputStream(entry)) { return new org.bukkit.plugin.PluginDescriptionFile(input); }
        } catch (java.io.IOException e) { throw new org.bukkit.plugin.InvalidDescriptionException(e); }
    }
    public java.util.regex.Pattern[] getPluginFileFilters() { return new java.util.regex.Pattern[]{JAR}; }
    public org.bukkit.plugin.Plugin loadPlugin(java.io.File file) throws org.bukkit.plugin.InvalidPluginException, org.bukkit.plugin.UnknownDependencyException {
        try { return org.bukkit.Bukkit.getPluginManager().loadPlugin(file); }
        catch (org.bukkit.plugin.InvalidDescriptionException e) { throw new org.bukkit.plugin.InvalidPluginException(e); }
    }
}
