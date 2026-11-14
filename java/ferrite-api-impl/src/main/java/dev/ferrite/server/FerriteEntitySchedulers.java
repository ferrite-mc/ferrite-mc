package dev.ferrite.server;
import io.papermc.paper.threadedregions.scheduler.*;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicLong;
import java.util.function.*;
import org.bukkit.plugin.Plugin;
final class FerriteEntitySchedulers {
    private final ThreadLocal<String> currentEntity = new ThreadLocal<>();
    boolean isOwned(UUID id) { return id.toString().equals(currentEntity.get()); }
    private final AtomicLong nextId = new AtomicLong(1);
    private final ConcurrentHashMap<Long, Task> tasks = new ConcurrentHashMap<>();
    private final class Task implements ScheduledTask {
        final long id; final String entity; final BooleanSupplier alive; final Plugin plugin; final Consumer<ScheduledTask> action; final Runnable retired; final long period;
        volatile ExecutionState state = ExecutionState.IDLE;
        Task(long id, String entity, BooleanSupplier alive, Plugin plugin, Consumer<ScheduledTask> action, Runnable retired, long period) { this.id=id; this.entity=entity; this.alive=alive; this.plugin=plugin; this.action=action; this.retired=retired; this.period=period; }
        public Plugin getOwningPlugin() { return plugin; }
        public boolean isRepeatingTask() { return period > 0; }
        public synchronized CancelledState cancel() {
            if (state == ExecutionState.CANCELLED || state == ExecutionState.CANCELLED_RUNNING) return CancelledState.CANCELLED_ALREADY;
            if (state == ExecutionState.FINISHED) return CancelledState.ALREADY_EXECUTED;
            if (state == ExecutionState.RUNNING) { state = ExecutionState.CANCELLED_RUNNING; return period > 0 ? CancelledState.NEXT_RUNS_CANCELLED : CancelledState.RUNNING; }
            state = ExecutionState.CANCELLED; tasks.remove(id); return CancelledState.CANCELLED_BY_CALLER;
        }
        public ExecutionState getExecutionState() { return state; }
        void run() {
            synchronized (this) {
                if (state != ExecutionState.IDLE || !plugin.isEnabled()) { tasks.remove(id); return; }
                state = ExecutionState.RUNNING;
            }
            boolean retiredNow = !alive.getAsBoolean();
            try { if (retiredNow) { if (retired != null) retired.run(); } else action.accept(this); }
            catch (Throwable error) { plugin.getLogger().log(java.util.logging.Level.SEVERE, "Plugin " + plugin.getName() + " generated an exception while executing an entity task", error); }
            finally {
                synchronized (this) {
                    if (state == ExecutionState.CANCELLED_RUNNING) { state = ExecutionState.CANCELLED; tasks.remove(id); }
                    else if (period == 0 || retiredNow) { state = ExecutionState.FINISHED; tasks.remove(id); }
                    else { state = ExecutionState.IDLE; Native.scheduleEntity(id, entity, period, plugin.getName()); }
                }
            }
        }
    }
    EntityScheduler forEntity(UUID id, BooleanSupplier alive) {
        String entity = id.toString();
        return new EntityScheduler() {
            private ScheduledTask schedule(Plugin plugin, Consumer<ScheduledTask> action, Runnable retired, long delay, long period) {
                if (!alive.getAsBoolean()) return null;
                if (delay < 0 || period < 0) throw new IllegalArgumentException("Invalid timing");
                Task task = new Task(nextId.getAndIncrement(), entity, alive, plugin, action, retired, period);
                tasks.put(task.id, task); Native.scheduleEntity(task.id, entity, delay, plugin.getName()); return task;
            }
            public boolean execute(Plugin p, Runnable action, Runnable retired, long delay) { return schedule(p, t -> action.run(), retired, delay, 0) != null; }
            public ScheduledTask run(Plugin p, Consumer<ScheduledTask> action, Runnable retired) { return schedule(p, action, retired, 1, 0); }
            public ScheduledTask runDelayed(Plugin p, Consumer<ScheduledTask> action, Runnable retired, long delay) { return schedule(p, action, retired, delay, 0); }
            public ScheduledTask runAtFixedRate(Plugin p, Consumer<ScheduledTask> action, Runnable retired, long delay, long period) { if (period < 1) throw new IllegalArgumentException("period"); return schedule(p, action, retired, delay, period); }
        };
    }
    void execute(long id) { Task task = tasks.get(id); if (task != null) { String previous = currentEntity.get(); currentEntity.set(task.entity); try { if (((FerriteServer) org.bukkit.Bukkit.getServer()).plugins.isLegacy(task.plugin)) { synchronized (Bootstrap.LEGACY_LOCK) { task.run(); } } else task.run(); } finally { if (previous == null) currentEntity.remove(); else currentEntity.set(previous); } } }
    void cancelTasks(Plugin plugin) { tasks.values().stream().filter(t -> t.plugin == plugin).forEach(Task::cancel); }
}
