package dev.ferrite.server;
import io.papermc.paper.threadedregions.scheduler.*;
import java.util.*;
import java.util.concurrent.*;
import java.util.function.Consumer;
import org.bukkit.plugin.Plugin;
final class FerriteAsyncScheduler implements AsyncScheduler {
    private static ScheduledExecutorService newExecutor() { return Executors.newScheduledThreadPool(2, runnable -> { Thread t = new Thread(runnable, "Ferrite plugin async"); t.setDaemon(true); return t; }); }
    private volatile ScheduledExecutorService executor = newExecutor();
    private final Set<Task> tasks = ConcurrentHashMap.newKeySet();
    private final class Task implements ScheduledTask, Runnable {
        final Plugin plugin; final Consumer<ScheduledTask> action; final boolean repeating;
        volatile ExecutionState state = ExecutionState.IDLE; volatile ScheduledFuture<?> future;
        Task(Plugin plugin, Consumer<ScheduledTask> action, boolean repeating) { this.plugin=plugin; this.action=action; this.repeating=repeating; }
        public Plugin getOwningPlugin() { return plugin; }
        public boolean isRepeatingTask() { return repeating; }
        public synchronized CancelledState cancel() {
            if (state == ExecutionState.CANCELLED || state == ExecutionState.CANCELLED_RUNNING) return CancelledState.CANCELLED_ALREADY;
            if (state == ExecutionState.FINISHED) return CancelledState.ALREADY_EXECUTED;
            boolean running = state == ExecutionState.RUNNING;
            state = running ? ExecutionState.CANCELLED_RUNNING : ExecutionState.CANCELLED;
            if (future != null) future.cancel(false); if (!running) tasks.remove(this);
            return running ? (repeating ? CancelledState.NEXT_RUNS_CANCELLED : CancelledState.RUNNING) : CancelledState.CANCELLED_BY_CALLER;
        }
        public ExecutionState getExecutionState() { return state; }
        public void run() { execute(); }
        private void execute() {
            synchronized (this) { if (state != ExecutionState.IDLE || !plugin.isEnabled()) { tasks.remove(this); return; } state = ExecutionState.RUNNING; }
            try { action.accept(this); }
            // Like Paper's scheduler a failing run is logged and a repeating task keeps its schedule; an exception left for the executor would silently end it.
            catch (Throwable error) { plugin.getLogger().log(java.util.logging.Level.SEVERE, "Plugin " + plugin.getName() + " generated an exception while executing an async task", error); }
            finally { synchronized (this) { if (state == ExecutionState.CANCELLED_RUNNING) { state = ExecutionState.CANCELLED; tasks.remove(this); } else if (repeating) state = ExecutionState.IDLE; else { state = ExecutionState.FINISHED; tasks.remove(this); } } }
        }
    }
    private ScheduledTask schedule(Plugin p, Consumer<ScheduledTask> action, long delay, long period, TimeUnit unit) {
        if (delay < 0 || period < 0) throw new IllegalArgumentException("Invalid timing");
        Task task = new Task(p, action, period > 0); tasks.add(task);
        ScheduledFuture<?> future = period == 0 ? executor.schedule(task, delay, unit) : executor.scheduleAtFixedRate(task, delay, period, unit);
        synchronized (task) { task.future = future; if (task.state == ScheduledTask.ExecutionState.CANCELLED || task.state == ScheduledTask.ExecutionState.CANCELLED_RUNNING) future.cancel(false); }
        return task;
    }
    public ScheduledTask runNow(Plugin p, Consumer<ScheduledTask> action) { return schedule(p, action, 0, 0, TimeUnit.MILLISECONDS); }
    public ScheduledTask runDelayed(Plugin p, Consumer<ScheduledTask> action, long delay, TimeUnit unit) { return schedule(p, action, delay, 0, unit); }
    public ScheduledTask runAtFixedRate(Plugin p, Consumer<ScheduledTask> action, long delay, long period, TimeUnit unit) { if (period < 1) throw new IllegalArgumentException("period"); return schedule(p, action, delay, period, unit); }
    public void cancelTasks(Plugin plugin) { tasks.stream().filter(t -> t.plugin == plugin).forEach(Task::cancel); }
    void shutdown() { executor.shutdownNow(); }
    /** A server that is started again after {@link #shutdown()} (in-process restarts, tests) gets working async threads back. */
    void restart() { if (executor.isShutdown()) executor = newExecutor(); }
}
