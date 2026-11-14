package dev.ferrite.server;
import java.util.*;
import org.bukkit.plugin.Plugin;
import org.bukkit.scheduler.BukkitTask;
final class FerriteScheduler extends SchedulerStubs {
    private long tick;
    private int nextId = 1;
    private final List<Task> tasks = new ArrayList<>();
    private final java.util.concurrent.ConcurrentMap<Integer, AsyncTask> asyncTasks = new java.util.concurrent.ConcurrentHashMap<>();
    /** Bukkit's {@code CraftScheduler.validate}: a disabled plugin (including one inside its own {@code onDisable}) cannot schedule. */
    private static void requireEnabled(Plugin plugin) {
        java.util.Objects.requireNonNull(plugin, "Plugin cannot be null");
        if (!plugin.isEnabled()) throw new org.bukkit.plugin.IllegalPluginAccessException("Plugin attempted to register task while disabled");
    }
    private final class AsyncTask implements BukkitTask, org.bukkit.scheduler.BukkitWorker {
        final int id; final Plugin owner; volatile boolean cancelled; volatile boolean running; volatile Thread thread; volatile io.papermc.paper.threadedregions.scheduler.ScheduledTask delegate;
        public Thread getThread() { return thread; }
        AsyncTask(int id, Plugin owner) { this.id=id; this.owner=owner; }
        public int getTaskId() { return id; }
        public Plugin getOwner() { return owner; }
        public boolean isSync() { return false; }
        public boolean isCancelled() { return cancelled; }
        public synchronized void cancel() { cancelled=true; if (delegate != null) delegate.cancel(); asyncTasks.remove(id); }
        synchronized void bind(io.papermc.paper.threadedregions.scheduler.ScheduledTask task) { delegate=task; if (cancelled) task.cancel(); }
    }
    private synchronized int allocateId() { return nextId++; }
    private BukkitTask async(Plugin plugin, java.util.function.Consumer<? super BukkitTask> action, long delay, long period) {
        requireEnabled(plugin);
        if (delay < 0 || period < 0) throw new IllegalArgumentException("Invalid timing");
        long millis = Math.multiplyExact(delay, 50);
        long periodMillis = Math.multiplyExact(period, 50);
        AsyncTask task = new AsyncTask(allocateId(), plugin); asyncTasks.put(task.id, task);
        java.util.function.Consumer<io.papermc.paper.threadedregions.scheduler.ScheduledTask> invoke = ignored -> { task.running = true; task.thread = Thread.currentThread(); try { if (!task.cancelled) action.accept(task); } finally { task.running = false; task.thread = null; if (period == 0) asyncTasks.remove(task.id); } };
        var scheduler = org.bukkit.Bukkit.getAsyncScheduler();
        var delegate = period > 0 ? scheduler.runAtFixedRate(plugin, invoke, millis, periodMillis, java.util.concurrent.TimeUnit.MILLISECONDS) : scheduler.runDelayed(plugin, invoke, millis, java.util.concurrent.TimeUnit.MILLISECONDS);
        task.bind(delegate); return task;
    }
    private static final class Task implements BukkitTask {
        final int id; final Plugin owner; final Runnable runnable; long next; final long period; boolean cancelled; boolean running;
        Task(int id, Plugin owner, Runnable runnable, long next, long period) { this.id=id; this.owner=owner; this.runnable=runnable; this.next=next; this.period=period; }
        public int getTaskId() { return id; }
        public Plugin getOwner() { return owner; }
        public boolean isSync() { return true; }
        public boolean isCancelled() { return cancelled; }
        public void cancel() { cancelled=true; }
    }
    private synchronized BukkitTask schedule(Plugin plugin, Runnable runnable, long delay, long period) {
        requireEnabled(plugin);
        if (period < 0 || delay < 0) throw new IllegalArgumentException("Invalid task timing");
        Task task = new Task(allocateId(), plugin, runnable, tick + Math.max(1, delay), period); tasks.add(task); if (Native.enabled()) Native.scheduleGlobal(task.id, delay); return task;
    }
    public BukkitTask runTaskTimer(Plugin plugin, Runnable runnable, long delay, long period) { if (period < 1) throw new IllegalArgumentException("period"); return schedule(plugin, runnable, delay, period); }
    public BukkitTask runTask(Plugin plugin, Runnable runnable) { return schedule(plugin, runnable, 1, 0); }
    public BukkitTask runTaskLater(Plugin plugin, Runnable runnable, long delay) { return schedule(plugin, runnable, delay, 0); }
    public BukkitTask runTask(Plugin p, org.bukkit.scheduler.BukkitRunnable r) { return runTask(p, (Runnable) r); }
    public BukkitTask runTaskLater(Plugin p, org.bukkit.scheduler.BukkitRunnable r, long delay) { return runTaskLater(p, (Runnable) r, delay); }
    public BukkitTask runTaskTimer(Plugin p, org.bukkit.scheduler.BukkitRunnable r, long delay, long period) { return runTaskTimer(p, (Runnable) r, delay, period); }
    public int scheduleSyncDelayedTask(Plugin p, Runnable r, long delay) { return runTaskLater(p, r, delay).getTaskId(); }
    public int scheduleSyncDelayedTask(Plugin p, Runnable r) { return runTask(p, r).getTaskId(); }
    public int scheduleSyncRepeatingTask(Plugin p, Runnable r, long delay, long period) { return runTaskTimer(p, r, delay, period).getTaskId(); }
    public int scheduleSyncDelayedTask(Plugin p, org.bukkit.scheduler.BukkitRunnable r, long delay) { return scheduleSyncDelayedTask(p, (Runnable) r, delay); }
    public int scheduleSyncDelayedTask(Plugin p, org.bukkit.scheduler.BukkitRunnable r) { return scheduleSyncDelayedTask(p, (Runnable) r); }
    public int scheduleSyncRepeatingTask(Plugin p, org.bukkit.scheduler.BukkitRunnable r, long delay, long period) { return scheduleSyncRepeatingTask(p, (Runnable) r, delay, period); }
    public void runTask(Plugin p, java.util.function.Consumer<? super BukkitTask> action) { runTaskLater(p, action, 1); }
    public void runTaskLater(Plugin p, java.util.function.Consumer<? super BukkitTask> action, long delay) { BukkitTask[] task = new BukkitTask[1]; task[0] = runTaskLater(p, () -> action.accept(task[0]), delay); }
    public void runTaskTimer(Plugin p, java.util.function.Consumer<? super BukkitTask> action, long delay, long period) { BukkitTask[] task = new BukkitTask[1]; task[0] = runTaskTimer(p, () -> action.accept(task[0]), delay, period); }
    public <T> java.util.concurrent.Future<T> callSyncMethod(Plugin p, java.util.concurrent.Callable<T> action) { var future = new java.util.concurrent.CompletableFuture<T>(); runTask(p, () -> { try { future.complete(action.call()); } catch (Throwable e) { future.completeExceptionally(e); } }); return future; }
    public java.util.concurrent.Executor getMainThreadExecutor(Plugin plugin) { return action -> runTask(plugin, action); }
    public synchronized boolean isQueued(int id) { return tasks.stream().anyMatch(t -> t.id == id && !t.cancelled) || asyncTasks.containsKey(id); }
    public synchronized boolean isCurrentlyRunning(int id) { AsyncTask async = asyncTasks.get(id); return (async != null && async.running) || tasks.stream().anyMatch(t -> t.id == id && t.running); }
    public synchronized java.util.List<BukkitTask> getPendingTasks() { java.util.List<BukkitTask> result = new ArrayList<>(); tasks.stream().filter(t -> !t.cancelled).forEach(result::add); result.addAll(asyncTasks.values()); return java.util.List.copyOf(result); }
    public java.util.List<org.bukkit.scheduler.BukkitWorker> getActiveWorkers() { return asyncTasks.values().stream().filter(task -> task.running).map(task -> (org.bukkit.scheduler.BukkitWorker) task).toList(); }
    public BukkitTask runTaskAsynchronously(Plugin p, Runnable r) { return async(p, ignored -> r.run(), 0, 0); }
    public BukkitTask runTaskLaterAsynchronously(Plugin p, Runnable r, long delay) { return async(p, ignored -> r.run(), delay, 0); }
    public BukkitTask runTaskTimerAsynchronously(Plugin p, Runnable r, long delay, long period) { if (period < 1) throw new IllegalArgumentException("period"); return async(p, ignored -> r.run(), delay, period); }
    public BukkitTask runTaskAsynchronously(Plugin p, org.bukkit.scheduler.BukkitRunnable r) { return runTaskAsynchronously(p, (Runnable) r); }
    public BukkitTask runTaskLaterAsynchronously(Plugin p, org.bukkit.scheduler.BukkitRunnable r, long delay) { return runTaskLaterAsynchronously(p, (Runnable) r, delay); }
    public BukkitTask runTaskTimerAsynchronously(Plugin p, org.bukkit.scheduler.BukkitRunnable r, long delay, long period) { return runTaskTimerAsynchronously(p, (Runnable) r, delay, period); }
    public void runTaskAsynchronously(Plugin p, java.util.function.Consumer<? super BukkitTask> action) { async(p, action, 0, 0); }
    public void runTaskLaterAsynchronously(Plugin p, java.util.function.Consumer<? super BukkitTask> action, long delay) { async(p, action, delay, 0); }
    public void runTaskTimerAsynchronously(Plugin p, java.util.function.Consumer<? super BukkitTask> action, long delay, long period) { if (period < 1) throw new IllegalArgumentException("period"); async(p, action, delay, period); }
    public int scheduleAsyncDelayedTask(Plugin p, Runnable r, long delay) { return runTaskLaterAsynchronously(p, r, delay).getTaskId(); }
    public int scheduleAsyncDelayedTask(Plugin p, Runnable r) { return runTaskAsynchronously(p, r).getTaskId(); }
    public int scheduleAsyncRepeatingTask(Plugin p, Runnable r, long delay, long period) { return runTaskTimerAsynchronously(p, r, delay, period).getTaskId(); }
    synchronized void tick() {
        tick++;
        if (!Native.enabled()) for (Task task : List.copyOf(tasks)) if (!task.cancelled && task.next <= tick) execute(task);
        tasks.removeIf(task -> task.cancelled);
    }
    synchronized void executeId(long id) { for (Task task : List.copyOf(tasks)) if (task.id == id && !task.cancelled) { execute(task); break; } tasks.removeIf(task -> task.cancelled); }
    private void execute(Task task) {
        task.running = true;
        try { task.runnable.run(); } catch (Exception | LinkageError e) { task.owner.getLogger().log(java.util.logging.Level.SEVERE, "Scheduled task failed", e); } finally { task.running = false; }
        if (task.period == 0) task.cancel();
        else { task.next = tick + task.period; if (Native.enabled() && !task.cancelled) Native.scheduleGlobal(task.id, task.period); }
    }
    public synchronized void cancelTask(int id) { tasks.stream().filter(task -> task.id == id).forEach(Task::cancel); AsyncTask task = asyncTasks.get(id); if (task != null) task.cancel(); }
    public synchronized void cancelTasks(Plugin owner) { tasks.stream().filter(task -> task.owner == owner).forEach(Task::cancel); asyncTasks.values().stream().filter(task -> task.owner == owner).forEach(AsyncTask::cancel); }
}
