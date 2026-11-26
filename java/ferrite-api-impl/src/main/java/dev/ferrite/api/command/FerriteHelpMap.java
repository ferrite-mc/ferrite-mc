package dev.ferrite.api.command;

import dev.ferrite.api.FerriteRuntime;
import java.util.Collection;
import java.util.Collections;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import java.util.TreeMap;
import org.bukkit.command.Command;
import org.bukkit.help.GenericCommandHelpTopic;
import org.bukkit.help.HelpMap;
import org.bukkit.help.HelpTopic;
import org.bukkit.help.HelpTopicFactory;

/**
 * In-memory {@link HelpMap} implementation for Ferrite.
 *
 * <p>Tracks manually added {@link HelpTopic} entries as well as topics synthesized from
 * registered {@link Command} instances via {@link HelpTopicFactory} (defaulting to
 * {@link GenericCommandHelpTopic}).
 */
public final class FerriteHelpMap implements HelpMap {

    private final FerriteRuntime runtime;
    private final Map<String, HelpTopic> helpTopics = new TreeMap<>(String.CASE_INSENSITIVE_ORDER);
    private final Set<String> manualTopics = new HashSet<>();
    private final Map<Class<?>, HelpTopicFactory<?>> topicFactories = new HashMap<>();

    public FerriteHelpMap(FerriteRuntime runtime) {
        this.runtime = Objects.requireNonNull(runtime, "runtime");
    }

    @Override
    public synchronized HelpTopic getHelpTopic(String topicName) {
        if (topicName == null || topicName.isEmpty()) {
            return null;
        }
        return helpTopics.get(topicName);
    }

    @Override
    public synchronized Collection<HelpTopic> getHelpTopics() {
        return List.copyOf(helpTopics.values());
    }

    @Override
    public synchronized void addTopic(HelpTopic topic) {
        Objects.requireNonNull(topic, "topic");
        String name = Objects.requireNonNull(topic.getName(), "topic.getName()");
        helpTopics.put(name, topic);
        manualTopics.add(name.toLowerCase(java.util.Locale.ROOT));
    }

    /** Remove a topic by name; returns {@code true} if a topic was removed. */
    public synchronized boolean removeTopic(String topicName) {
        if (topicName == null || topicName.isEmpty()) {
            return false;
        }
        manualTopics.remove(topicName.toLowerCase(java.util.Locale.ROOT));
        return helpTopics.remove(topicName) != null;
    }

    /** Remove a topic instance if present; returns {@code true} if removed. */
    public synchronized boolean removeTopic(HelpTopic topic) {
        if (topic == null || topic.getName() == null) {
            return false;
        }
        return removeTopic(topic.getName());
    }

    @Override
    public synchronized void clear() {
        helpTopics.clear();
        manualTopics.clear();
    }

    @Override
    public synchronized void registerHelpTopicFactory(
            Class<?> commandClass, HelpTopicFactory<?> factory) {
        Objects.requireNonNull(commandClass, "commandClass");
        Objects.requireNonNull(factory, "factory");
        if (!Command.class.isAssignableFrom(commandClass)) {
            throw new IllegalArgumentException(
                    "commandClass must implement/extend org.bukkit.command.Command: "
                            + commandClass.getName());
        }
        topicFactories.put(commandClass, factory);

        // Re-evaluate already-registered commands so factories registered during
        // onLoad/onEnable apply to commands loaded from plugin.yml or CommandMap.
        Set<Command> uniqueCommands =
                new LinkedHashSet<>(runtime.commandDispatcher().commandMap().getCommands());
        for (Command command : uniqueCommands) {
            if (commandClass.isInstance(command)) {
                registerCommandTopic(command);
            }
        }
    }

    @Override
    public List<String> getIgnoredPlugins() {
        return Collections.emptyList();
    }

    /**
     * Synthesizes and registers a {@link HelpTopic} for a command using the most specific
     * registered {@link HelpTopicFactory}, falling back to {@link GenericCommandHelpTopic}.
     */
    public synchronized void registerCommandTopic(Command command) {
        Objects.requireNonNull(command, "command");
        HelpTopic topic = createTopic(command);
        if (topic == null || topic.getName() == null) {
            return;
        }
        String key = topic.getName().toLowerCase(java.util.Locale.ROOT);
        if (!manualTopics.contains(key)) {
            helpTopics.put(topic.getName(), topic);
        }
    }

    /** Creates a {@link HelpTopic} for {@code command} using registered factories or default. */
    @SuppressWarnings("unchecked")
    public synchronized HelpTopic createTopic(Command command) {
        Objects.requireNonNull(command, "command");
        Class<?> current = command.getClass();
        while (current != null && Command.class.isAssignableFrom(current)) {
            HelpTopicFactory<Command> factory =
                    (HelpTopicFactory<Command>) topicFactories.get(current);
            if (factory != null) {
                return factory.createTopic(command);
            }
            current = current.getSuperclass();
        }
        return new GenericCommandHelpTopic(command);
    }
}
