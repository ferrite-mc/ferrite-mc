package dev.ferrite.server;

/** Help map over the live command map: every command is a topic unless a plugin overrides it. */
final class FerriteHelpMap implements org.bukkit.help.HelpMap {
    private final java.util.Map<String, org.bukkit.help.HelpTopic> topics = new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.Map<Class<?>, org.bukkit.help.HelpTopicFactory<?>> factories = new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.List<String> ignored = new java.util.ArrayList<>();
    private final FerriteServer server;

    private final java.util.Map<String, String[]> amendments = new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.List<java.util.Map<?, ?>> indexTopics = new java.util.ArrayList<>();

    FerriteHelpMap(FerriteServer server) { this(server, new java.io.File(server.getWorldContainer(), "help.yml")); }

    /**
     * Reads Bukkit's {@code help.yml}: {@code ignore-plugins} hides the commands of those plugins, {@code amended-topics}
     * rewrites a topic's short/full text and permission, and {@code index-topics} adds an index of commands.
     */
    FerriteHelpMap(FerriteServer server, java.io.File config) {
        this.server = server;
        if (!config.isFile()) return;
        Object data;
        try (var in = new java.io.FileInputStream(config)) {
            data = new org.yaml.snakeyaml.Yaml(new org.yaml.snakeyaml.constructor.SafeConstructor(new org.yaml.snakeyaml.LoaderOptions())).load(in);
        } catch (java.io.IOException | RuntimeException e) {
            server.getLogger().warning("Could not read help.yml: " + e);
            return;
        }
        if (!(data instanceof java.util.Map<?, ?> root)) return;
        if (root.get("ignore-plugins") instanceof java.util.List<?> list) for (var name : list) ignored.add(String.valueOf(name));
        if (root.get("amended-topics") instanceof java.util.Map<?, ?> amended) for (var entry : amended.entrySet()) {
            if (!(entry.getValue() instanceof java.util.Map<?, ?> fields)) continue;
            amendments.put(String.valueOf(entry.getKey()).toLowerCase(java.util.Locale.ROOT), new String[]{text(fields.get("shortText")), text(fields.get("fullText")), text(fields.get("permission"))});
        }
        if (root.get("index-topics") instanceof java.util.Map<?, ?> index) for (var entry : index.entrySet()) {
            if (entry.getValue() instanceof java.util.Map<?, ?> fields) {
                var copy = new java.util.LinkedHashMap<Object, Object>(fields);
                copy.put("name", entry.getKey());
                indexTopics.add(copy);
            }
        }
    }
    private static String text(Object value) { return value == null ? null : String.valueOf(value); }

    private boolean hidden(org.bukkit.command.Command command) {
        return command instanceof org.bukkit.command.PluginCommand plugin && ignored.stream().anyMatch(name -> name.equalsIgnoreCase(plugin.getPlugin().getName()));
    }

    private org.bukkit.help.HelpTopic amended(org.bukkit.help.HelpTopic topic) {
        var change = amendments.get(topic.getName().toLowerCase(java.util.Locale.ROOT));
        if (change != null) {
            if (change[0] != null || change[1] != null) topic.amendTopic(change[0], change[1]);
            if (change[2] != null) topic.amendCanSee(change[2]);
        }
        return topic;
    }

    private org.bukkit.help.HelpTopic indexTopic(java.util.Map<?, ?> fields) {
        var members = new java.util.ArrayList<org.bukkit.help.HelpTopic>();
        if (fields.get("commands") instanceof java.util.List<?> names) for (var name : names) {
            var command = server.getCommandMap().getCommand(String.valueOf(name));
            if (command != null && !hidden(command)) members.add(topicFor(command));
        }
        return new org.bukkit.help.IndexHelpTopic(String.valueOf(fields.get("name")), text(fields.get("shortText")) == null ? "" : text(fields.get("shortText")), text(fields.get("permission")), members, text(fields.get("preamble")));
    }

    @Override public org.bukkit.help.HelpTopic getHelpTopic(String name) {
        java.util.Objects.requireNonNull(name, "name");
        var topic = topics.get(name.toLowerCase(java.util.Locale.ROOT));
        if (topic != null) return topic;
        for (var fields : indexTopics) if (String.valueOf(fields.get("name")).equalsIgnoreCase(name)) return indexTopic(fields);
        var command = server.getCommandMap().getCommand(name);
        return command == null || hidden(command) ? null : amended(topicFor(command));
    }
    @Override public java.util.Collection<org.bukkit.help.HelpTopic> getHelpTopics() {
        var result = new java.util.LinkedHashMap<String, org.bukkit.help.HelpTopic>();
        for (var topic : topics.values()) result.put(topic.getName().toLowerCase(java.util.Locale.ROOT), topic);
        // The command map repeats a command under its fallback-prefixed aliases; the topic name
        // deduplicates them, so `/ft` appears once even though `ferrite-test:ft` also exists.
        for (var fields : indexTopics) { var index = indexTopic(fields); result.putIfAbsent(index.getName().toLowerCase(java.util.Locale.ROOT), index); }
        for (var command : server.getCommandMap().getKnownCommands().values()) {
            if (hidden(command)) continue;
            var topic = amended(topicFor(command));
            result.putIfAbsent(topic.getName().toLowerCase(java.util.Locale.ROOT), topic);
        }
        return java.util.List.copyOf(result.values());
    }
    @Override public void addTopic(org.bukkit.help.HelpTopic topic) { topics.put(topic.getName().toLowerCase(java.util.Locale.ROOT), topic); }
    @Override public void clear() { topics.clear(); }
    @Override public void registerHelpTopicFactory(Class<?> commandClass, org.bukkit.help.HelpTopicFactory<?> factory) {
        java.util.Objects.requireNonNull(commandClass, "commandClass");
        java.util.Objects.requireNonNull(factory, "factory");
        if (!org.bukkit.command.Command.class.isAssignableFrom(commandClass)) throw new IllegalArgumentException("commandClass must extend Command: " + commandClass.getName());
        factories.put(commandClass, factory);
    }
    @Override public java.util.List<String> getIgnoredPlugins() { return java.util.List.copyOf(ignored); }
    @SuppressWarnings("unchecked")
    private org.bukkit.help.HelpTopic topicFor(org.bukkit.command.Command command) {
        for (var entry : factories.entrySet()) {
            if (entry.getKey().isInstance(command)) {
                return ((org.bukkit.help.HelpTopicFactory<org.bukkit.command.Command>) entry.getValue()).createTopic(command);
            }
        }
        return new org.bukkit.help.GenericCommandHelpTopic(command);
    }
}
