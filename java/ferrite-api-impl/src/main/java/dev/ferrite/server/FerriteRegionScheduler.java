package dev.ferrite.server;
import io.papermc.paper.threadedregions.scheduler.*;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicLong;
import java.util.function.Consumer;
import org.bukkit.World;
import org.bukkit.plugin.Plugin;
final class FerriteRegionScheduler implements RegionScheduler {
    private final AtomicLong nextId = new AtomicLong(1);
    private final ConcurrentHashMap<Long, Task> tasks = new ConcurrentHashMap<>();
    private final class Task implements ScheduledTask {
        final long id; final Plugin plugin; final String world; final int x, z; final long period; final Consumer<ScheduledTask> action;
        volatile ExecutionState state = ExecutionState.IDLE;
        Task(long id, Plugin plugin, String world, int x, int z, long period, Consumer<ScheduledTask> action) { this.id=id; this.plugin=plugin; this.world=world; this.x=x; this.z=z; this.period=period; this.action=action; }
        public Plugin getOwningPlugin() { return plugin; }
        public boolean isRepeatingTask() { return period > 0; }
        public synchronized CancelledState cancel() {
            if (state == ExecutionState.CANCELLED || state == ExecutionState.CANCELLED_RUNNING) return CancelledState.CANCELLED_ALREADY;
            if (state == ExecutionState.FINISHED) return CancelledState.ALREADY_EXECUTED;
            if (state == ExecutionState.RUNNING) { state = ExecutionState.CANCELLED_RUNNING; return period > 0 ? CancelledState.NEXT_RUNS_CANCELLED : CancelledState.RUNNING; }
            state = ExecutionState.CANCELLED; tasks.remove(id); return CancelledState.CANCELLED_BY_CALLER;
        }
        public ExecutionState getExecutionState() { return state; }
        void execute() {
            synchronized (this) {
                if (state != ExecutionState.IDLE || !plugin.isEnabled()) { tasks.remove(id); return; }
                state = ExecutionState.RUNNING;
            }
            try { action.accept(this); }
            catch (Throwable error) { plugin.getLogger().log(java.util.logging.Level.SEVERE, "Plugin " + plugin.getName() + " generated an exception while executing a region task", error); }
            finally {
                synchronized (this) {
                    if (state == ExecutionState.CANCELLED_RUNNING) { state = ExecutionState.CANCELLED; tasks.remove(id); }
                    else if (period == 0) { state = ExecutionState.FINISHED; tasks.remove(id); }
                    else { state = ExecutionState.IDLE; Native.scheduleRegion(id, world, x, z, period, plugin.getName()); }
                }
            }
        }
    }
    private ScheduledTask schedule(Plugin plugin, World world, int x, int z, Consumer<ScheduledTask> action, long delay, long period) {
        if (delay < 0 || period < 0) throw new IllegalArgumentException("Invalid timing");
        long id = nextId.getAndIncrement();
        Task task = new Task(id, plugin, world.getUID().toString(), x, z, period, action);
        tasks.put(id, task); Native.scheduleRegion(id, task.world, x, z, delay, plugin.getName()); return task;
    }
    void execute(long id) { Task task = tasks.get(id); if (task != null) { if (((FerriteServer) org.bukkit.Bukkit.getServer()).plugins.isLegacy(task.plugin)) { synchronized (Bootstrap.LEGACY_LOCK) { task.execute(); } } else task.execute(); } }
    void cancelTasks(Plugin plugin) { tasks.values().stream().filter(t -> t.plugin == plugin).forEach(Task::cancel); }
    public void execute(Plugin plugin, World world, int x, int z, Runnable action) { run(plugin, world, x, z, t -> action.run()); }
    public ScheduledTask run(Plugin p, World w, int x, int z, Consumer<ScheduledTask> action) { return schedule(p,w,x,z,action,1,0); }
    public ScheduledTask runDelayed(Plugin p, World w, int x, int z, Consumer<ScheduledTask> action, long delay) { return schedule(p,w,x,z,action,delay,0); }
    public ScheduledTask runAtFixedRate(Plugin p, World w, int x, int z, Consumer<ScheduledTask> action, long delay, long period) { if (period < 1) throw new IllegalArgumentException("period"); return schedule(p,w,x,z,action,delay,period); }
}
