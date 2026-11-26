package dev.ferrite.api.scheduler;

import dev.ferrite.api.internal.FerriteContext;
import java.util.concurrent.ScheduledFuture;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.logging.Level;
import org.bukkit.plugin.Plugin;
import org.bukkit.scheduler.BukkitTask;

/** A scheduled sync/async task with Bukkit and Folia cancellation semantics. */
final class FerriteTask implements BukkitTask {

    private final FerriteScheduler scheduler;
    private final int taskId;
    private final Plugin owner;
    private Runnable runnable;
    private final boolean sync;
    private final boolean repeating;
    private final long period;

    private final AtomicBoolean cancelled = new AtomicBoolean();
    private final AtomicBoolean completed = new AtomicBoolean();
    private volatile boolean running;
    private volatile ScheduledFuture<?> future;
    private volatile long dueTick;

    FerriteTask(
            FerriteScheduler scheduler,
            int taskId,
            Plugin owner,
            Runnable runnable,
            boolean sync,
            boolean repeating,
            long period,
            long dueTick) {
        this.scheduler = scheduler;
        this.taskId = taskId;
        this.owner = owner;
        this.runnable = runnable;
        this.sync = sync;
        this.repeating = repeating;
        this.period = period;
        this.dueTick = dueTick;
    }

    void bindRunnable(Runnable runnable) {
        this.runnable = runnable;
    }

    @Override
    public int getTaskId() {
        return taskId;
    }

    @Override
    public Plugin getOwner() {
        return owner;
    }

    @Override
    public boolean isSync() {
        return sync;
    }

    @Override
    public boolean isCancelled() {
        return cancelled.get();
    }

    boolean isCompleted() {
        return completed.get();
    }

    void markCompleted() {
        completed.set(true);
    }

    @Override
    public void cancel() {
        tryCancel();
    }

    boolean tryCancel() {
        if (cancelled.compareAndSet(false, true)) {
            ScheduledFuture<?> scheduled = future;
            if (scheduled != null) {
                scheduled.cancel(false);
            }
            scheduler.taskCancelled(this);
            return true;
        }
        return false;
    }

    void runSync() {
        if (isCancelled()) {
            return;
        }
        running = true;
        try {
            FerriteContext.withPlugin(owner.getName(), runnable);
        } catch (Throwable throwable) {
            owner.getLogger().log(Level.SEVERE,
                    "Task #" + taskId + " for plugin " + owner.getName()
                            + " threw an exception; the scheduler continues",
                    throwable);
        } finally {
            running = false;
            if (!repeating) {
                completed.set(true);
            }
        }
    }

    void runAsync() {
        if (isCancelled()) {
            return;
        }
        running = true;
        try {
            FerriteContext.withPlugin(owner.getName(), runnable);
        } catch (Throwable throwable) {
            owner.getLogger().log(Level.SEVERE,
                    "Async task #" + taskId + " for plugin " + owner.getName()
                            + " threw an exception; the scheduler continues",
                    throwable);
        } finally {
            running = false;
            if (!repeating) {
                completed.set(true);
            }
        }
    }

    boolean repeating() {
        return repeating;
    }

    long period() {
        return period;
    }

    long dueTick() {
        return dueTick;
    }

    void reschedule(long nextTick) {
        this.dueTick = nextTick;
    }

    boolean running() {
        return running;
    }

    void setFuture(ScheduledFuture<?> scheduledFuture) {
        this.future = scheduledFuture;
        if (isCancelled()) {
            scheduledFuture.cancel(false);
        }
    }
}
