package dev.ferrite.api.scheduler;

import io.papermc.paper.threadedregions.scheduler.AsyncScheduler;
import io.papermc.paper.threadedregions.scheduler.ScheduledTask;
import java.util.Objects;
import java.util.concurrent.TimeUnit;
import java.util.function.Consumer;
import org.bukkit.plugin.Plugin;

/** Paper/Folia {@link AsyncScheduler} backed by Ferrite's async executor pool. */
public final class FerriteAsyncScheduler implements AsyncScheduler {

    private final FerriteScheduler scheduler;

    public FerriteAsyncScheduler(FerriteScheduler scheduler) {
        this.scheduler = Objects.requireNonNull(scheduler, "scheduler");
    }

    @Override
    public ScheduledTask runNow(Plugin plugin, Consumer<ScheduledTask> task) {
        return scheduler.scheduleAsyncFolia(plugin, task, 0L, 0L, TimeUnit.MILLISECONDS);
    }

    @Override
    public ScheduledTask runDelayed(
            Plugin plugin, Consumer<ScheduledTask> task, long delay, TimeUnit unit) {
        Objects.requireNonNull(unit, "unit");
        return scheduler.scheduleAsyncFolia(plugin, task, Math.max(0L, delay), 0L, unit);
    }

    @Override
    public ScheduledTask runAtFixedRate(
            Plugin plugin,
            Consumer<ScheduledTask> task,
            long initialDelay,
            long period,
            TimeUnit unit) {
        Objects.requireNonNull(unit, "unit");
        return scheduler.scheduleAsyncFolia(
                plugin, task, Math.max(0L, initialDelay), Math.max(1L, period), unit);
    }

    @Override
    public void cancelTasks(Plugin plugin) {
        Objects.requireNonNull(plugin, "plugin");
        scheduler.cancelTasks(plugin);
    }
}
