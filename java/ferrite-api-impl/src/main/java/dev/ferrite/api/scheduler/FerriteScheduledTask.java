package dev.ferrite.api.scheduler;

import io.papermc.paper.threadedregions.scheduler.ScheduledTask;
import org.bukkit.plugin.Plugin;

/** Adapter exposing a {@link FerriteTask} through Folia/Paper's {@link ScheduledTask} contract. */
final class FerriteScheduledTask implements ScheduledTask {

    private final FerriteTask task;

    FerriteScheduledTask(FerriteTask task) {
        this.task = task;
    }

    @Override
    public Plugin getOwningPlugin() {
        return task.getOwner();
    }

    @Override
    public boolean isRepeatingTask() {
        return task.repeating();
    }

    @Override
    public CancelledState cancel() {
        if (!task.repeating() && task.isCompleted()) {
            return CancelledState.ALREADY_EXECUTED;
        }
        boolean newlyCancelled = task.tryCancel();
        if (!newlyCancelled) {
            return task.repeating()
                    ? CancelledState.NEXT_RUNS_CANCELLED_ALREADY
                    : CancelledState.CANCELLED_ALREADY;
        }
        if (task.running()) {
            return task.repeating()
                    ? CancelledState.NEXT_RUNS_CANCELLED
                    : CancelledState.RUNNING;
        }
        return task.repeating()
                ? CancelledState.NEXT_RUNS_CANCELLED
                : CancelledState.CANCELLED_BY_CALLER;
    }

    @Override
    public ExecutionState getExecutionState() {
        if (!task.repeating() && task.isCompleted()) {
            return ExecutionState.FINISHED;
        }
        if (task.isCancelled()) {
            return task.running() ? ExecutionState.CANCELLED_RUNNING : ExecutionState.CANCELLED;
        }
        if (task.running()) {
            return ExecutionState.RUNNING;
        }
        return ExecutionState.IDLE;
    }
}
