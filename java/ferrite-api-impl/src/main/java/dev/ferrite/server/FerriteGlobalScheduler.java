package dev.ferrite.server;
import io.papermc.paper.threadedregions.scheduler.*;
import java.util.function.Consumer;
import org.bukkit.plugin.Plugin;
import org.bukkit.scheduler.BukkitTask;
final class FerriteGlobalScheduler implements GlobalRegionScheduler {
    /** Paper's global scheduler refuses a disabled plugin, including one inside its own {@code onDisable}. */
    private static void requireEnabled(Plugin plugin) {
        java.util.Objects.requireNonNull(plugin, "Plugin cannot be null");
        if (!plugin.isEnabled()) throw new org.bukkit.plugin.IllegalPluginAccessException("Plugin attempted to register task while disabled");
    }
    private final FerriteScheduler scheduler;
    FerriteGlobalScheduler(FerriteScheduler scheduler) { this.scheduler = scheduler; }
    private final class Task implements ScheduledTask {
        final Plugin plugin; final boolean repeating; BukkitTask delegate; boolean finished;
        Task(Plugin plugin, boolean repeating) { this.plugin = plugin; this.repeating = repeating; }
        public Plugin getOwningPlugin() { return plugin; }
        public boolean isRepeatingTask() { return repeating; }
        public CancelledState cancel() {
            if (delegate.isCancelled()) return CancelledState.CANCELLED_ALREADY;
            if (finished) return CancelledState.ALREADY_EXECUTED;
            delegate.cancel(); return CancelledState.CANCELLED_BY_CALLER;
        }
        public ExecutionState getExecutionState() { return delegate.isCancelled() && !finished ? ExecutionState.CANCELLED : finished ? ExecutionState.FINISHED : ExecutionState.IDLE; }
    }
    private ScheduledTask schedule(Plugin plugin, Consumer<ScheduledTask> action, long delay, long period) {
        requireEnabled(plugin);
        Task task = new Task(plugin, period != 0);
        task.delegate = period == 0 ? scheduler.runTaskLater(plugin, () -> { action.accept(task); task.finished = true; }, delay) : scheduler.runTaskTimer(plugin, () -> action.accept(task), delay, period);
        return task;
    }
    public void execute(Plugin plugin, Runnable action) { run(plugin, ignored -> action.run()); }
    public ScheduledTask run(Plugin plugin, Consumer<ScheduledTask> action) { return schedule(plugin, action, 1, 0); }
    public ScheduledTask runDelayed(Plugin plugin, Consumer<ScheduledTask> action, long delay) { return schedule(plugin, action, delay, 0); }
    public ScheduledTask runAtFixedRate(Plugin plugin, Consumer<ScheduledTask> action, long delay, long period) { if (period < 1) throw new IllegalArgumentException("period"); return schedule(plugin, action, delay, period); }
    public void cancelTasks(Plugin plugin) { scheduler.cancelTasks(plugin); }
}
