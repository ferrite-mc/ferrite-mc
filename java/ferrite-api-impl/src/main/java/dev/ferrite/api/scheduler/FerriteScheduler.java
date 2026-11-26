package dev.ferrite.api.scheduler;

import dev.ferrite.api.FerriteRuntime;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.PriorityQueue;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.Executor;
import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.function.Consumer;
import org.bukkit.plugin.Plugin;
import org.bukkit.scheduler.BukkitRunnable;
import org.bukkit.scheduler.BukkitTask;
import org.bukkit.scheduler.BukkitWorker;

/**
 * A Bukkit scheduler driven by the host's tick loop.
 *
 * <p>Synchronous tasks run only inside {@link #tick()}, which requires the server main
 * thread. Async tasks run on a separate daemon pool and never touch the main-thread queue.
 * Plugin disable cancels that plugin's tasks; a cancelled task never runs again.
 */
public final class FerriteScheduler extends UnsupportedBukkitSchedulerBase {

    private final FerriteRuntime runtime;
    private final AtomicInteger nextTaskId = new AtomicInteger(1);
    private final Map<Integer, FerriteTask> tasks = new ConcurrentHashMap<>();
    private final PriorityQueue<FerriteTask> syncQueue = new PriorityQueue<>(
            Comparator.comparingLong(FerriteTask::dueTick).thenComparingInt(FerriteTask::getTaskId));
    private final AtomicInteger asyncWorkerId = new AtomicInteger(1);
    private final ScheduledExecutorService asyncExecutor = Executors.newScheduledThreadPool(
            2,
            runnable -> {
                Thread thread = new Thread(runnable, "Ferrite-Async-" + asyncWorkerId.getAndIncrement());
                thread.setDaemon(true);
                return thread;
            });

    private long currentTick;

    public FerriteScheduler(FerriteRuntime runtime) {
        this.runtime = runtime;
    }

    public long currentTick() {
        return currentTick;
    }

    public int taskCount() {
        return tasks.size();
    }

    /** Run one server tick of synchronous tasks. Host-only, main thread only. */
    public void tick() {
        runtime.requirePrimaryThread("BukkitScheduler#tick");
        currentTick++;
        int guard = 0;
        while (true) {
            FerriteTask next = syncQueue.peek();
            if (next == null || next.dueTick() > currentTick) {
                break;
            }
            syncQueue.poll();
            if (!tasks.containsKey(next.getTaskId())) {
                continue;
            }
            if (next.isCancelled() || !next.getOwner().isEnabled()) {
                finish(next);
                continue;
            }
            next.runSync();
            if (next.repeating() && !next.isCancelled() && next.getOwner().isEnabled()) {
                next.reschedule(currentTick + next.period());
                syncQueue.add(next);
            } else {
                finish(next);
            }
            if (++guard > 1_000_000) {
                throw new IllegalStateException(
                        "Scheduler tick did not settle; a task keeps scheduling itself");
            }
        }
    }

    public void shutdown() {
        for (FerriteTask task : new ArrayList<>(tasks.values())) {
            finish(task);
        }
        asyncExecutor.shutdownNow();
    }

    void taskCancelled(FerriteTask task) {
        tasks.remove(task.getTaskId());
    }

    private void finish(FerriteTask task) {
        tasks.remove(task.getTaskId());
        task.cancel();
    }

    private FerriteTask scheduleSync(Plugin plugin, Runnable runnable, long delay, long period) {
        Objects.requireNonNull(plugin, "plugin");
        Objects.requireNonNull(runnable, "runnable");
        long effectiveDelay = Math.max(1, delay);
        long effectivePeriod = period <= 0 ? 0 : Math.max(1, period);
        FerriteTask task = new FerriteTask(
                this,
                nextTaskId.getAndIncrement(),
                plugin,
                runnable,
                true,
                effectivePeriod > 0,
                effectivePeriod,
                currentTick + effectiveDelay);
        tasks.put(task.getTaskId(), task);
        syncQueue.add(task);
        return task;
    }

    private FerriteTask scheduleAsync(Plugin plugin, Runnable runnable, long delay, long period) {
        Objects.requireNonNull(plugin, "plugin");
        Objects.requireNonNull(runnable, "runnable");
        long effectivePeriod = period <= 0 ? 0 : Math.max(1, period);
        FerriteTask task = new FerriteTask(
                this,
                nextTaskId.getAndIncrement(),
                plugin,
                runnable,
                false,
                effectivePeriod > 0,
                effectivePeriod,
                0);
        tasks.put(task.getTaskId(), task);
        long delayMillis = Math.max(0, delay) * 50L;
        if (effectivePeriod > 0) {
            task.setFuture(asyncExecutor.scheduleAtFixedRate(
                    () -> runAsync(task), delayMillis, effectivePeriod * 50L, TimeUnit.MILLISECONDS));
        } else {
            task.setFuture(asyncExecutor.schedule(
                    () -> runAsync(task), delayMillis, TimeUnit.MILLISECONDS));
        }
        return task;
    }

    FerriteScheduledTask scheduleSyncFolia(
            Plugin plugin,
            Consumer<io.papermc.paper.threadedregions.scheduler.ScheduledTask> consumer,
            long delayTicks,
            long periodTicks) {
        Objects.requireNonNull(plugin, "plugin");
        Objects.requireNonNull(consumer, "task");
        long effectiveDelay = Math.max(1L, delayTicks);
        long effectivePeriod = periodTicks <= 0L ? 0L : Math.max(1L, periodTicks);
        FerriteTask task = new FerriteTask(
                this,
                nextTaskId.getAndIncrement(),
                plugin,
                () -> {
                },
                true,
                effectivePeriod > 0L,
                effectivePeriod,
                currentTick + effectiveDelay);
        FerriteScheduledTask scheduled = new FerriteScheduledTask(task);
        task.bindRunnable(() -> consumer.accept(scheduled));
        tasks.put(task.getTaskId(), task);
        syncQueue.add(task);
        return scheduled;
    }

    FerriteScheduledTask scheduleAsyncFolia(
            Plugin plugin,
            Consumer<io.papermc.paper.threadedregions.scheduler.ScheduledTask> consumer,
            long delay,
            long period,
            TimeUnit unit) {
        Objects.requireNonNull(plugin, "plugin");
        Objects.requireNonNull(consumer, "task");
        Objects.requireNonNull(unit, "unit");
        long effectiveDelay = Math.max(0L, delay);
        long effectivePeriod = period <= 0L ? 0L : Math.max(1L, period);
        FerriteTask task = new FerriteTask(
                this,
                nextTaskId.getAndIncrement(),
                plugin,
                () -> {
                },
                false,
                effectivePeriod > 0L,
                effectivePeriod,
                0L);
        FerriteScheduledTask scheduled = new FerriteScheduledTask(task);
        task.bindRunnable(() -> consumer.accept(scheduled));
        tasks.put(task.getTaskId(), task);
        if (effectivePeriod > 0L) {
            task.setFuture(asyncExecutor.scheduleAtFixedRate(
                    () -> runAsync(task), effectiveDelay, effectivePeriod, unit));
        } else {
            task.setFuture(asyncExecutor.schedule(
                    () -> runAsync(task), effectiveDelay, unit));
        }
        return scheduled;
    }

    private void runAsync(FerriteTask task) {
        if (task.isCancelled() || !task.getOwner().isEnabled()) {
            finish(task);
            return;
        }
        task.runAsync();
        if (!task.repeating()) {
            finish(task);
        }
    }

    @Override
    public int scheduleSyncDelayedTask(Plugin plugin, Runnable task, long delay) {
        return scheduleSync(plugin, task, delay, 0).getTaskId();
    }

    @Override
    public int scheduleSyncDelayedTask(Plugin plugin, BukkitRunnable task, long delay) {
        return scheduleSyncDelayedTask(plugin, (Runnable) task, delay);
    }

    @Override
    public int scheduleSyncDelayedTask(Plugin plugin, Runnable task) {
        return scheduleSyncDelayedTask(plugin, task, 0);
    }

    @Override
    public int scheduleSyncDelayedTask(Plugin plugin, BukkitRunnable task) {
        return scheduleSyncDelayedTask(plugin, (Runnable) task, 0);
    }

    @Override
    public int scheduleSyncRepeatingTask(Plugin plugin, Runnable task, long delay, long period) {
        return scheduleSync(plugin, task, delay, period).getTaskId();
    }

    @Override
    public int scheduleSyncRepeatingTask(Plugin plugin, BukkitRunnable task, long delay, long period) {
        return scheduleSyncRepeatingTask(plugin, (Runnable) task, delay, period);
    }

    @Override
    public int scheduleAsyncDelayedTask(Plugin plugin, Runnable task, long delay) {
        return scheduleAsync(plugin, task, delay, 0).getTaskId();
    }

    @Override
    public int scheduleAsyncDelayedTask(Plugin plugin, Runnable task) {
        return scheduleAsyncDelayedTask(plugin, task, 0);
    }

    @Override
    public int scheduleAsyncRepeatingTask(Plugin plugin, Runnable task, long delay, long period) {
        return scheduleAsync(plugin, task, delay, period).getTaskId();
    }

    @Override
    public <T> java.util.concurrent.Future<T> callSyncMethod(Plugin plugin, java.util.concurrent.Callable<T> task) {
        Objects.requireNonNull(task, "task");
        CompletableFuture<T> future = new CompletableFuture<>();
        scheduleSync(plugin, () -> {
            try {
                future.complete(task.call());
            } catch (Throwable throwable) {
                future.completeExceptionally(throwable);
            }
        }, 0, 0);
        return future;
    }

    @Override
    public void cancelTask(int taskId) {
        FerriteTask task = tasks.remove(taskId);
        if (task != null) {
            task.cancel();
        }
    }

    @Override
    public void cancelTasks(Plugin plugin) {
        for (FerriteTask task : new ArrayList<>(tasks.values())) {
            if (task.getOwner() == plugin) {
                finish(task);
            }
        }
    }

    @Override
    public boolean isCurrentlyRunning(int taskId) {
        FerriteTask task = tasks.get(taskId);
        return task != null && task.running();
    }

    @Override
    public boolean isQueued(int taskId) {
        FerriteTask task = tasks.get(taskId);
        return task != null && !task.isCancelled() && !task.running();
    }

    @Override
    public List<BukkitWorker> getActiveWorkers() {
        return List.of();
    }

    @Override
    public List<BukkitTask> getPendingTasks() {
        return List.copyOf(tasks.values());
    }

    @Override
    public BukkitTask runTask(Plugin plugin, Runnable task) {
        return scheduleSync(plugin, task, 0, 0);
    }

    @Override
    public void runTask(Plugin plugin, Consumer<? super BukkitTask> consumer) {
        Objects.requireNonNull(consumer, "consumer");
        consumer.accept(runTask(plugin, () -> {
        }));
    }

    @Override
    public BukkitTask runTask(Plugin plugin, BukkitRunnable task) {
        return runTask(plugin, (Runnable) task);
    }

    @Override
    public BukkitTask runTaskAsynchronously(Plugin plugin, Runnable task) {
        return scheduleAsync(plugin, task, 0, 0);
    }

    @Override
    public void runTaskAsynchronously(Plugin plugin, Consumer<? super BukkitTask> consumer) {
        Objects.requireNonNull(consumer, "consumer");
        consumer.accept(runTaskAsynchronously(plugin, () -> {
        }));
    }

    @Override
    public BukkitTask runTaskAsynchronously(Plugin plugin, BukkitRunnable task) {
        return runTaskAsynchronously(plugin, (Runnable) task);
    }

    @Override
    public BukkitTask runTaskLater(Plugin plugin, Runnable task, long delay) {
        return scheduleSync(plugin, task, delay, 0);
    }

    @Override
    public void runTaskLater(Plugin plugin, Consumer<? super BukkitTask> consumer, long delay) {
        Objects.requireNonNull(consumer, "consumer");
        consumer.accept(runTaskLater(plugin, () -> {
        }, delay));
    }

    @Override
    public BukkitTask runTaskLater(Plugin plugin, BukkitRunnable task, long delay) {
        return runTaskLater(plugin, (Runnable) task, delay);
    }

    @Override
    public BukkitTask runTaskLaterAsynchronously(Plugin plugin, Runnable task, long delay) {
        return scheduleAsync(plugin, task, delay, 0);
    }

    @Override
    public void runTaskLaterAsynchronously(
            Plugin plugin, Consumer<? super BukkitTask> consumer, long delay) {
        Objects.requireNonNull(consumer, "consumer");
        consumer.accept(runTaskLaterAsynchronously(plugin, () -> {
        }, delay));
    }

    @Override
    public BukkitTask runTaskLaterAsynchronously(Plugin plugin, BukkitRunnable task, long delay) {
        return runTaskLaterAsynchronously(plugin, (Runnable) task, delay);
    }

    @Override
    public BukkitTask runTaskTimer(Plugin plugin, Runnable task, long delay, long period) {
        return scheduleSync(plugin, task, delay, period);
    }

    @Override
    public void runTaskTimer(
            Plugin plugin, Consumer<? super BukkitTask> consumer, long delay, long period) {
        Objects.requireNonNull(consumer, "consumer");
        consumer.accept(runTaskTimer(plugin, () -> {
        }, delay, period));
    }

    @Override
    public BukkitTask runTaskTimer(Plugin plugin, BukkitRunnable task, long delay, long period) {
        return runTaskTimer(plugin, (Runnable) task, delay, period);
    }

    @Override
    public BukkitTask runTaskTimerAsynchronously(
            Plugin plugin, Runnable task, long delay, long period) {
        return scheduleAsync(plugin, task, delay, period);
    }

    @Override
    public void runTaskTimerAsynchronously(
            Plugin plugin, Consumer<? super BukkitTask> consumer, long delay, long period) {
        Objects.requireNonNull(consumer, "consumer");
        consumer.accept(runTaskTimerAsynchronously(plugin, () -> {
        }, delay, period));
    }

    @Override
    public BukkitTask runTaskTimerAsynchronously(
            Plugin plugin, BukkitRunnable task, long delay, long period) {
        return runTaskTimerAsynchronously(plugin, (Runnable) task, delay, period);
    }

    @Override
    public Executor getMainThreadExecutor(Plugin plugin) {
        Objects.requireNonNull(plugin, "plugin");
        return command -> {
            if (runtime.isPrimaryThread()) {
                command.run();
            } else {
                runtime.mainThread().execute(command);
            }
        };
    }
}
