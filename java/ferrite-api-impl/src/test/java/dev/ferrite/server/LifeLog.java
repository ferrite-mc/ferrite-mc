package dev.ferrite.server;

import java.util.List;
import java.util.concurrent.CopyOnWriteArrayList;

/**
 * What the compiled fixture plugins of the lifecycle integration tests write to. It is public and on the
 * application class path, so a plugin loaded by its own class loader reaches the same static list as the test.
 */
public final class LifeLog {
    private LifeLog() {}
    public static final List<String> EVENTS = new CopyOnWriteArrayList<>();
    public static void add(String line) { EVENTS.add(line); }

    /** An event the fixture plugins listen to. */
    public static final class Ping extends org.bukkit.event.Event implements org.bukkit.event.Cancellable {
        private static final org.bukkit.event.HandlerList HANDLERS = new org.bukkit.event.HandlerList();
        private boolean cancelled;
        @Override public boolean isCancelled() { return cancelled; }
        @Override public void setCancelled(boolean cancel) { cancelled = cancel; }
        @Override public org.bukkit.event.HandlerList getHandlers() { return HANDLERS; }
        public static org.bukkit.event.HandlerList getHandlerList() { return HANDLERS; }
    }
}
