package dev.ferrite.api.scheduler;

import io.papermc.paper.threadedregions.scheduler.RegionScheduler;
import io.papermc.paper.threadedregions.scheduler.ScheduledTask;
import java.util.Objects;
import java.util.function.Consumer;
import org.bukkit.World;
import org.bukkit.plugin.Plugin;

/**
 * Paper/Folia {@link RegionScheduler} routed onto Ferrite's main tick scheduler so region
 * callbacks run safely on the host thread with full plugin lifecycle and cancellation tracking.
 */
public final class FerriteRegionScheduler implements RegionScheduler {

    private final FerriteScheduler scheduler;

    public FerriteRegionScheduler(FerriteScheduler scheduler) {
        this.scheduler = Objects.requireNonNull(scheduler, "scheduler");
    }

    @Override
    public void execute(Plugin plugin, World world, int chunkX, int chunkZ, Runnable run) {
        Objects.requireNonNull(run, "run");
        run(plugin, world, chunkX, chunkZ, ignored -> run.run());
    }

    @Override
    public ScheduledTask run(
            Plugin plugin, World world, int chunkX, int chunkZ, Consumer<ScheduledTask> task) {
        Objects.requireNonNull(world, "world");
        return scheduler.scheduleSyncFolia(plugin, task, 1L, 0L);
    }

    @Override
    public ScheduledTask runDelayed(
            Plugin plugin,
            World world,
            int chunkX,
            int chunkZ,
            Consumer<ScheduledTask> task,
            long delayTicks) {
        Objects.requireNonNull(world, "world");
        return scheduler.scheduleSyncFolia(plugin, task, Math.max(1L, delayTicks), 0L);
    }

    @Override
    public ScheduledTask runAtFixedRate(
            Plugin plugin,
            World world,
            int chunkX,
            int chunkZ,
            Consumer<ScheduledTask> task,
            long initialDelayTicks,
            long periodTicks) {
        Objects.requireNonNull(world, "world");
        return scheduler.scheduleSyncFolia(
                plugin, task, Math.max(1L, initialDelayTicks), Math.max(1L, periodTicks));
    }
}
