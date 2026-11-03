package dev.ferrite.server;
import java.io.*;
import java.util.*;
import java.util.jar.JarFile;
import org.bukkit.plugin.*;
import org.bukkit.plugin.java.JavaPlugin;
final class FerritePluginManager extends PluginManagerStubs {
    private static String permissionKey(String name) { return name.toLowerCase(Locale.ROOT); }
    @Override public synchronized org.bukkit.permissions.Permission getPermission(String name) { return server.facade.permissions.get(permissionKey(name)); }
    @Override public synchronized void addPermission(org.bukkit.permissions.Permission permission) {
        if (server.facade.permissions.putIfAbsent(permissionKey(permission.getName()), permission) != null) throw new IllegalArgumentException("Permission already registered: " + permission.getName());
        recalculatePermissionDefaults(permission);
    }
    @Override public void addPermissions(List<org.bukkit.permissions.Permission> entries) { entries.forEach(this::addPermission); }
    @Override public synchronized void removePermission(String name) { var removed = server.facade.permissions.remove(permissionKey(name)); if (removed != null) recalculatePermissionDefaults(removed); }
    @Override public void removePermission(org.bukkit.permissions.Permission permission) { removePermission(permission.getName()); }
    @Override public synchronized void clearPermissions() { server.facade.permissions.clear(); for (boolean op : new boolean[]{false, true}) { server.facade.defaultPerms.get(op).clear(); getDefaultPermSubscriptions(op).forEach(org.bukkit.permissions.Permissible::recalculatePermissions); } }
    @Override public synchronized Set<org.bukkit.permissions.Permission> getPermissions() { return Set.copyOf(server.facade.permissions.values()); }
    @Override public synchronized Set<org.bukkit.permissions.Permission> getDefaultPermissions(boolean op) { return Set.copyOf(server.facade.defaultPerms.get(op)); }
    @Override public synchronized void recalculatePermissionDefaults(org.bukkit.permissions.Permission permission) {
        boolean registered = server.facade.permissions.get(permissionKey(permission.getName())) == permission;
        for (boolean op : new boolean[]{false, true}) {
            var defaults = server.facade.defaultPerms.get(op);
            if (registered && permission.getDefault().getValue(op)) defaults.add(permission); else defaults.remove(permission);
            getDefaultPermSubscriptions(op).forEach(org.bukkit.permissions.Permissible::recalculatePermissions);
        }
        getPermissionSubscriptions(permission.getName()).forEach(org.bukkit.permissions.Permissible::recalculatePermissions);
    }
    @Override public synchronized void subscribeToPermission(String name, org.bukkit.permissions.Permissible permissible) { server.facade.permSubs.computeIfAbsent(permissionKey(name), ignored -> new java.util.WeakHashMap<>()).put(permissible, true); }
    @Override public synchronized void unsubscribeFromPermission(String name, org.bukkit.permissions.Permissible permissible) { var map = server.facade.permSubs.get(permissionKey(name)); if (map != null) map.remove(permissible); }
    @Override public synchronized Set<org.bukkit.permissions.Permissible> getPermissionSubscriptions(String name) { var map = server.facade.permSubs.get(permissionKey(name)); return map == null ? Set.of() : Set.copyOf(map.keySet()); }
    @Override public synchronized void subscribeToDefaultPerms(boolean op, org.bukkit.permissions.Permissible permissible) { server.facade.defSubs.computeIfAbsent(op, ignored -> new java.util.WeakHashMap<>()).put(permissible, true); }
    @Override public synchronized void unsubscribeFromDefaultPerms(boolean op, org.bukkit.permissions.Permissible permissible) { var map = server.facade.defSubs.get(op); if (map != null) map.remove(permissible); }
    @Override public synchronized Set<org.bukkit.permissions.Permissible> getDefaultPermSubscriptions(boolean op) { var map = server.facade.defSubs.get(op); return map == null ? Set.of() : Set.copyOf(map.keySet()); }
    private record Handler(Class<?> type, org.bukkit.event.EventPriority priority, boolean ignoreCancelled, Plugin owner, java.lang.invoke.MethodHandle method) {}
    private final List<Handler> handlers = new ArrayList<>();
    private final java.util.Set<Class<?>> listened = java.util.concurrent.ConcurrentHashMap.newKeySet();
    private void noteListener(Class<?> type) { if (listened.add(type)) Native.eventRegistered(type.getName()); }
    /** Whether any plugin registered a listener for the exact event class. */
    boolean hasListeners(Class<? extends org.bukkit.event.Event> type) { return listened.contains(type); }
    public synchronized void registerEvents(org.bukkit.event.Listener listener, Plugin owner) {
        java.util.Objects.requireNonNull(listener, "listener");
        requireLoaded(owner);
        Set<String> seen = new HashSet<>();
        for (Class<?> type = listener.getClass(); type != null; type = type.getSuperclass()) for (java.lang.reflect.Method method : type.getDeclaredMethods()) {
            if (method.isBridge() || method.isSynthetic() || !seen.add(method.getName() + java.util.Arrays.toString(method.getParameterTypes()))) continue;
            org.bukkit.event.EventHandler annotation = method.getAnnotation(org.bukkit.event.EventHandler.class);
            if (annotation == null) continue;
            if (method.getParameterCount() != 1 || !org.bukkit.event.Event.class.isAssignableFrom(method.getParameterTypes()[0]) || method.getReturnType() != void.class) throw new IllegalArgumentException("Invalid event handler: " + method);
            try {
                method.setAccessible(true);
                handlers.add(new Handler(method.getParameterTypes()[0], annotation.priority(), annotation.ignoreCancelled(), owner, java.lang.invoke.MethodHandles.lookup().unreflect(method).bindTo(listener)));
                noteListener(method.getParameterTypes()[0]);
            } catch (IllegalAccessException e) { throw new IllegalArgumentException(e); }
        }
        handlers.sort(Comparator.comparing(Handler::priority));
    }
    public void registerEvent(Class<? extends org.bukkit.event.Event> type, org.bukkit.event.Listener listener, org.bukkit.event.EventPriority priority, EventExecutor executor, Plugin plugin) { registerEvent(type, listener, priority, executor, plugin, false); }
    public synchronized void registerEvent(Class<? extends org.bukkit.event.Event> type, org.bukkit.event.Listener listener, org.bukkit.event.EventPriority priority, EventExecutor executor, Plugin plugin, boolean ignoreCancelled) {
        java.util.Objects.requireNonNull(type, "type");
        java.util.Objects.requireNonNull(listener, "listener");
        java.util.Objects.requireNonNull(priority, "priority");
        java.util.Objects.requireNonNull(executor, "executor");
        requireLoaded(plugin);
        try {
            var method = java.lang.invoke.MethodHandles.lookup().findVirtual(EventExecutor.class, "execute", java.lang.invoke.MethodType.methodType(void.class, org.bukkit.event.Listener.class, org.bukkit.event.Event.class)).bindTo(executor).bindTo(listener);
            handlers.add(new Handler(type, priority, ignoreCancelled, plugin, method));
            handlers.sort(Comparator.comparing(Handler::priority));
            noteListener(type);
        } catch (ReflectiveOperationException e) { throw new IllegalStateException(e); }
    }
    public void callEvent(org.bukkit.event.Event event) {
        List<Handler> snapshot;
        synchronized (this) { snapshot = List.copyOf(handlers); }
        for (Handler handler : snapshot) {
            if (!handler.owner().isEnabled() || !handler.type().isInstance(event) || (handler.ignoreCancelled() && event instanceof org.bukkit.event.Cancellable c && c.isCancelled())) continue;
            try { if (isLegacy(handler.owner())) { synchronized (Bootstrap.LEGACY_LOCK) { if (handler.owner().isEnabled()) handler.method().invoke(event); } } else handler.method().invoke(event); } catch (Throwable e) { handler.owner().getLogger().log(java.util.logging.Level.SEVERE, "Event handler failed", e); }
        }
    }
    private final java.util.Map<Plugin, Boolean> regionAware = java.util.Collections.synchronizedMap(new java.util.IdentityHashMap<>());
    boolean isLegacy(Plugin plugin) { return !regionAware.getOrDefault(plugin, false); }
    /** Permissions each plugin declared in its descriptor, removed when it disables. */
    private final Map<Plugin, List<org.bukkit.permissions.Permission>> declaredPermissions = new IdentityHashMap<>();
    private final FerriteServer server;
    private final List<JavaPlugin> plugins = new ArrayList<>();
    private final Map<String, FerriteClassLoader> loaders = new java.util.concurrent.ConcurrentHashMap<>();
    FerritePluginManager(FerriteServer server) { this.server = server; }
    /** Registration is only legal for a plugin this manager loaded and still tracks. */
    private void requireLoaded(Plugin plugin) {
        java.util.Objects.requireNonNull(plugin, "plugin");
        if (!(plugin instanceof JavaPlugin javaPlugin) || !plugins.contains(javaPlugin)) throw new IllegalArgumentException("Plugin is not loaded: " + plugin.getName());
    }
    @Override public void enablePlugin(Plugin plugin) {
        java.util.Objects.requireNonNull(plugin, "plugin");
        if (!(plugin instanceof JavaPlugin javaPlugin) || !plugins.contains(javaPlugin)) throw new IllegalArgumentException("Plugin is not loaded: " + plugin.getName());
        if (!plugin.isEnabled()) {
            javaPlugin.setEnabled(true);
            callEvent(new org.bukkit.event.server.PluginEnableEvent(plugin));
        }
    }
    @Override public void disablePlugin(Plugin plugin) {
        java.util.Objects.requireNonNull(plugin, "plugin");
        if (!(plugin instanceof JavaPlugin javaPlugin) || !plugins.contains(javaPlugin)) throw new IllegalArgumentException("Plugin is not loaded: " + plugin.getName());
        if (!plugin.isEnabled()) return;
        callEvent(new org.bukkit.event.server.PluginDisableEvent(plugin));
        javaPlugin.setEnabled(false);
        server.scheduler.cancelTasks(plugin); server.regionScheduler.cancelTasks(plugin); server.asyncScheduler.cancelTasks(plugin); server.entitySchedulers.cancelTasks(plugin);
    }
    @Override public void clearPlugins() { disablePlugins(); }
    @Override public boolean useTimings() { return false; }
    void loadOrdered(String[] files, boolean[] modes) {
        if (modes != null && modes.length != files.length) throw new IllegalArgumentException("Plugin mode count mismatch");
        if (server.pluginsFolder == null && files.length > 0) server.pluginsFolder = new File(files[0]).getAbsoluteFile().getParent();
        try {
            for (int index = 0; index < files.length; index++) {
                String path = files[index];
                File file = new File(path);
                PluginDescriptionFile desc;
                boolean aware;
                boolean paper;
                Map<?, ?> metadata;
                try (JarFile jar = new JarFile(file)) {
                    paper = jar.getJarEntry("paper-plugin.yml") != null;
                    var descriptionEntry = jar.getJarEntry("plugin.yml");
                    if (descriptionEntry == null) descriptionEntry = jar.getJarEntry("paper-plugin.yml");
                    if (descriptionEntry == null) throw new IllegalArgumentException("Missing plugin descriptor: " + file);
                    try (InputStream in = jar.getInputStream(descriptionEntry)) { desc = new PluginDescriptionFile(in); }
                    try (InputStream in = jar.getInputStream(jar.getJarEntry(paper ? "paper-plugin.yml" : "plugin.yml"))) {
                        Object data = new org.yaml.snakeyaml.Yaml(new org.yaml.snakeyaml.constructor.SafeConstructor(new org.yaml.snakeyaml.LoaderOptions())).load(in);
                        if (!(data instanceof Map<?, ?> map)) throw new IllegalArgumentException("Invalid descriptor: " + file);
                        metadata = map;
                        aware = modes != null ? modes[index] : Boolean.TRUE.equals(map.get("folia-supported"));
                    }
                }
                List<String> dependencies = new ArrayList<>();
                if (paper) {
                    Object section = metadata.get("dependencies");
                    Object serverSection = section instanceof Map<?, ?> map ? map.get("server") : null;
                    if (serverSection instanceof Map<?, ?> declared) for (var entry : declared.entrySet()) {
                        if (!(entry.getKey() instanceof String name)) continue;
                        Map<?, ?> options = entry.getValue() instanceof Map<?, ?> map ? map : Map.of();
                        if (!Boolean.FALSE.equals(options.get("join-classpath"))) dependencies.add(name.toLowerCase(Locale.ROOT));
                    }
                } else {
                    for (String name : desc.getPluginDependencies()) {
                        dependencies.add(name.toLowerCase(Locale.ROOT));
                    }
                    for (String name : desc.getPluginSoftDependencies()) {
                        dependencies.add(name.toLowerCase(Locale.ROOT));
                    }
                }
                FerriteClassLoader loader = new FerriteClassLoader(server, file, desc, dependencies, loaders);
                loaders.put(desc.getName().toLowerCase(Locale.ROOT), loader);
                for (String alias : desc.getProvidedPlugins()) loaders.putIfAbsent(alias.toLowerCase(Locale.ROOT), loader);
                JavaPlugin plugin = Class.forName(desc.getMain(), true, loader).asSubclass(JavaPlugin.class).getDeclaredConstructor().newInstance();
                plugins.add(plugin); regionAware.put(plugin, aware); server.registerCommands(plugin); plugin.onLoad();
                List<org.bukkit.permissions.Permission> declared = new ArrayList<>();
                for (org.bukkit.permissions.Permission permission : desc.getPermissions()) {
                    if (getPermission(permission.getName()) != null) continue;
                    addPermission(permission);
                    declared.add(permission);
                }
                declaredPermissions.put(plugin, declared);
            }
            for (JavaPlugin plugin : plugins) enablePlugin(plugin);
        } catch (Throwable e) { disablePlugins(); throw new IllegalStateException("Failed to load plugin", e); }
    }
    public Plugin getPlugin(String name) { Plugin direct = plugins.stream().filter(p -> p.getName().equalsIgnoreCase(name)).findFirst().orElse(null); return direct != null ? direct : plugins.stream().filter(p -> p.getDescription().getProvidedPlugins().stream().anyMatch(alias -> alias.equalsIgnoreCase(name))).findFirst().orElse(null); }
    public Plugin[] getPlugins() { return plugins.toArray(Plugin[]::new); }
    public boolean isPluginEnabled(String name) { Plugin p = getPlugin(name); return p != null && p.isEnabled(); }
    public boolean isPluginEnabled(Plugin p) { return p != null && p.isEnabled(); }
    public void disablePlugins() {
        for (int i = plugins.size() - 1; i >= 0; i--) {
            JavaPlugin plugin = plugins.get(i);
            try { disablePlugin(plugin); }
            catch (Throwable e) { plugin.getLogger().log(java.util.logging.Level.SEVERE, "Plugin disable failed", e); }
            finally {
                server.scheduler.cancelTasks(plugin); server.regionScheduler.cancelTasks(plugin); server.asyncScheduler.cancelTasks(plugin); server.entitySchedulers.cancelTasks(plugin);
                for (org.bukkit.permissions.Permission permission : declaredPermissions.getOrDefault(plugin, List.of())) removePermission(permission.getName());
            }
        }
        synchronized (this) { handlers.clear(); }
        listened.clear();
        Native.eventsCleared();
        for (FerriteClassLoader loader : new HashSet<>(loaders.values())) {
            try { loader.close(); } catch (IOException e) { server.getLogger().warning("Could not close plugin classloader: " + e); }
        }
        for (JavaPlugin plugin : plugins) server.unregisterCommands(plugin);
        plugins.clear(); regionAware.clear(); loaders.clear(); declaredPermissions.clear();
    }
}
