package dev.ferrite.server;

import io.papermc.paper.plugin.lifecycle.event.LifecycleEvent;
import io.papermc.paper.plugin.lifecycle.event.LifecycleEventOwner;
import io.papermc.paper.plugin.lifecycle.event.handler.LifecycleEventHandler;
import io.papermc.paper.plugin.lifecycle.event.handler.configuration.MonitorLifecycleEventHandlerConfiguration;
import io.papermc.paper.plugin.lifecycle.event.handler.configuration.PrioritizedLifecycleEventHandlerConfiguration;
import io.papermc.paper.plugin.lifecycle.event.types.LifecycleEventType;
import java.util.Objects;
import org.bukkit.plugin.Plugin;

/** The {@code LifecycleEventType} objects behind {@code LifecycleEvents}; handlers they build are read by {@link FerriteLifecycleManager}. */
public final class FerriteLifecycleTypes {
    private FerriteLifecycleTypes() {}

    public static <O extends LifecycleEventOwner, E extends LifecycleEvent> LifecycleEventType.Prioritizable<O, E> prioritized(String name, Class<? extends O> owner) {
        Objects.requireNonNull(name, "name"); Objects.requireNonNull(owner, "owner");
        return new Prioritizable<>(name);
    }

    public static <O extends LifecycleEventOwner, E extends LifecycleEvent> LifecycleEventType.Monitorable<O, E> monitor(String name, Class<? extends O> owner) {
        Objects.requireNonNull(name, "name"); Objects.requireNonNull(owner, "owner");
        return new Monitorable<>(name);
    }

    private record Prioritizable<O extends LifecycleEventOwner, E extends LifecycleEvent>(String name) implements LifecycleEventType.Prioritizable<O, E> {
        @Override public PrioritizedLifecycleEventHandlerConfiguration<O> newHandler(LifecycleEventHandler<? super E> handler) {
            return new Prioritized<>(this, Objects.requireNonNull(handler, "handler"), 0, false);
        }
    }

    private record Monitorable<O extends LifecycleEventOwner, E extends LifecycleEvent>(String name) implements LifecycleEventType.Monitorable<O, E> {
        @Override public MonitorLifecycleEventHandlerConfiguration<O> newHandler(LifecycleEventHandler<? super E> handler) {
            return new Monitor<>(this, Objects.requireNonNull(handler, "handler"));
        }
    }

    /** Configuration with a priority; {@code priority} and {@code monitor} return a changed copy, so a shared builder is never mutated. */
    @SuppressWarnings({"unchecked", "rawtypes"})
    private record Prioritized<O extends LifecycleEventOwner>(LifecycleEventType type, LifecycleEventHandler handler, int level, boolean monitoring)
            implements PrioritizedLifecycleEventHandlerConfiguration<O>, FerriteLifecycleManager.Configuration {
        @Override public PrioritizedLifecycleEventHandlerConfiguration<O> priority(int value) { return new Prioritized<>(type, handler, value, monitoring); }
        @Override public PrioritizedLifecycleEventHandlerConfiguration<O> monitor() { return new Prioritized<>(type, handler, level, true); }
        @Override public LifecycleEventType eventType() { return type; }
        @Override public LifecycleEventHandler eventHandler() { return handler; }
        @Override public int eventPriority() { return level; }
        @Override public boolean isMonitorHandler() { return monitoring; }
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    private record Monitor<O extends LifecycleEventOwner>(LifecycleEventType type, LifecycleEventHandler handler)
            implements MonitorLifecycleEventHandlerConfiguration<O>, FerriteLifecycleManager.Configuration {
        @Override public MonitorLifecycleEventHandlerConfiguration<O> monitor() { return this; }
        @Override public LifecycleEventType eventType() { return type; }
        @Override public LifecycleEventHandler eventHandler() { return handler; }
        @Override public int eventPriority() { return 0; }
        @Override public boolean isMonitorHandler() { return true; }
    }
}
