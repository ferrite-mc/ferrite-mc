package dev.ferrite.api.scheduler;

import io.papermc.paper.threadedregions.scheduler.GlobalRegionScheduler;
import io.papermc.paper.threadedregions.scheduler.ScheduledTask;
import java.util.Objects;
import java.util.function.Consumer;
import org.bukkit.plugin.Plugin;

/** Paper/Folia {@link GlobalRegionScheduler} backed by Ferrite's tick scheduler. */
public final class FerriteGlobalRegionScheduler implements GlobalRegionScheduler {

    private final FerriteScheduler scheduler;

    public FerriteGlobalRegionScheduler(FerriteScheduler scheduler) {
        this.scheduler = Objects.requireNonNull(scheduler, "scheduler");
    }

    @Override
    public void execute(Plugin plugin, Runnable run) {
        Objects.requireNonNull(run, "run");
        run(plugin, ignored -> run.run());
    }

    @Override
    public ScheduledTask run(Plugin plugin, Consumer<ScheduledTask> task) {
        return scheduler.scheduleSyncFolia(plugin, task, 1L, 0L);
    }

    @Override
    public ScheduledTask runDelayed(Plugin plugin, Consumer<ScheduledTask> task, long delayTicks) {
        return scheduler.scheduleSyncFolia(plugin, task, Math.max(1L, delayTicks), 0L);
    }

    @Override
    public ScheduledTask runAtFixedRate(
            Plugin plugin,
            Consumer<ScheduledTask> task,
            long initialDelayTicks,
            long periodTicks) {
        return scheduler.scheduleSyncFolia(
                plugin, task, Math.max(1L, initialDelayTicks), Math.max(1L, periodTicks));
    }

    @Override
    public void cancelTasks(Plugin plugin) {
        Objects.requireNonNull(plugin, "plugin");
        scheduler.cancelTasks(plugin);
    }
}
