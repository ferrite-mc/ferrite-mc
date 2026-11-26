package dev.ferrite.api.scheduler;

import io.papermc.paper.threadedregions.scheduler.EntityScheduler;
import io.papermc.paper.threadedregions.scheduler.ScheduledTask;
import java.util.Objects;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.function.Consumer;
import org.bukkit.entity.Entity;
import org.bukkit.plugin.Plugin;

/** Paper/Folia {@link EntityScheduler} scoped to one live entity and its retired callback. */
public final class FerriteEntityScheduler implements EntityScheduler {

    private final FerriteScheduler scheduler;
    private final Entity entity;

    public FerriteEntityScheduler(FerriteScheduler scheduler, Entity entity) {
        this.scheduler = Objects.requireNonNull(scheduler, "scheduler");
        this.entity = Objects.requireNonNull(entity, "entity");
    }

    @Override
    public boolean execute(Plugin plugin, Runnable run, Runnable retired, long delay) {
        Objects.requireNonNull(run, "run");
        return schedule(plugin, ignored -> run.run(), retired, Math.max(1L, delay), 0L) != null;
    }

    @Override
    public ScheduledTask run(Plugin plugin, Consumer<ScheduledTask> task, Runnable retired) {
        return schedule(plugin, task, retired, 1L, 0L);
    }

    @Override
    public ScheduledTask runDelayed(
            Plugin plugin, Consumer<ScheduledTask> task, Runnable retired, long delayTicks) {
        return schedule(plugin, task, retired, Math.max(1L, delayTicks), 0L);
    }

    @Override
    public ScheduledTask runAtFixedRate(
            Plugin plugin,
            Consumer<ScheduledTask> task,
            Runnable retired,
            long initialDelayTicks,
            long periodTicks) {
        return schedule(
                plugin, task, retired, Math.max(1L, initialDelayTicks), Math.max(1L, periodTicks));
    }

    private ScheduledTask schedule(
            Plugin plugin,
            Consumer<ScheduledTask> task,
            Runnable retired,
            long delay,
            long period) {
        Objects.requireNonNull(plugin, "plugin");
        Objects.requireNonNull(task, "task");
        if (!entity.isValid()) {
            return null;
        }
        AtomicBoolean retiredRan = new AtomicBoolean();
        return scheduler.scheduleSyncFolia(
                plugin,
                scheduledTask -> {
                    if (!entity.isValid()) {
                        scheduledTask.cancel();
                        if (retired != null && retiredRan.compareAndSet(false, true)) {
                            retired.run();
                        }
                        return;
                    }
                    task.accept(scheduledTask);
                },
                delay,
                period);
    }
}
