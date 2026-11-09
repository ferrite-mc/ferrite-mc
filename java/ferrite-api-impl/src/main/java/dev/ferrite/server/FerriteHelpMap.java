package dev.ferrite.server;

/** Help map over the live command map: every command is a topic unless a plugin overrides it. */
final class FerriteHelpMap implements org.bukkit.help.HelpMap {
    private final java.util.Map<String, org.bukkit.help.HelpTopic> topics = new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.Map<Class<?>, org.bukkit.help.HelpTopicFactory<?>> factories = new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.List<String> ignored = new java.util.ArrayList<>();
    private final FerriteServer server;

    FerriteHelpMap(FerriteServer server) { this.server = server; }

    @Override public org.bukkit.help.HelpTopic getHelpTopic(String name) {
        java.util.Objects.requireNonNull(name, "name");
        var topic = topics.get(name.toLowerCase(java.util.Locale.ROOT));
        if (topic != null) return topic;
        var command = server.getCommandMap().getCommand(name);
        return command == null ? null : topicFor(command);
    }
    @Override public java.util.Collection<org.bukkit.help.HelpTopic> getHelpTopics() {
        var result = new java.util.LinkedHashMap<String, org.bukkit.help.HelpTopic>();
        for (var topic : topics.values()) result.put(topic.getName().toLowerCase(java.util.Locale.ROOT), topic);
        // The command map repeats a command under its fallback-prefixed aliases; the topic name
        // deduplicates them, so `/ft` appears once even though `ferrite-test:ft` also exists.
        for (var command : server.getCommandMap().getKnownCommands().values()) {
            var topic = topicFor(command);
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
