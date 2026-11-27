package dev.ferrite.api.server;

import java.util.Objects;
import org.bukkit.ServerTickManager;
import org.bukkit.entity.Entity;

/** State-backed {@link ServerTickManager} for {@link FerriteServer}. */
final class FerriteServerTickManager implements ServerTickManager {

    private float tickRate = 20.0f;
    private boolean frozen;
    private int frozenTicksToRun;
    private int sprintingTicks;

    @Override
    public boolean isRunningNormally() {
        return !frozen && sprintingTicks <= 0;
    }

    @Override
    public boolean isStepping() {
        return frozen && frozenTicksToRun > 0;
    }

    @Override
    public boolean isSprinting() {
        return sprintingTicks > 0;
    }

    @Override
    public boolean isFrozen() {
        return frozen;
    }

    @Override
    public float getTickRate() {
        return tickRate;
    }

    @Override
    public void setTickRate(float tick) {
        if (!Float.isFinite(tick) || tick < 1.0f || tick > 10000.0f) {
            throw new IllegalArgumentException("tickRate must be between 1.0 and 10000.0, was " + tick);
        }
        this.tickRate = tick;
    }

    @Override
    public void setFrozen(boolean frozen) {
        this.frozen = frozen;
        if (!frozen) {
            this.frozenTicksToRun = 0;
        }
    }

    @Override
    public boolean stepGameIfFrozen(int ticks) {
        if (!frozen || ticks < 0) {
            return false;
        }
        this.frozenTicksToRun = ticks;
        return true;
    }

    @Override
    public boolean stopStepping() {
        if (frozenTicksToRun <= 0) {
            return false;
        }
        this.frozenTicksToRun = 0;
        return true;
    }

    @Override
    public boolean requestGameToSprint(int ticks) {
        if (ticks < 0) {
            return false;
        }
        boolean wasSprinting = sprintingTicks > 0;
        this.sprintingTicks = ticks;
        return wasSprinting;
    }

    @Override
    public boolean stopSprinting() {
        if (sprintingTicks <= 0) {
            return false;
        }
        this.sprintingTicks = 0;
        return true;
    }

    @Override
    public boolean isFrozen(Entity entity) {
        Objects.requireNonNull(entity, "entity");
        return frozen && frozenTicksToRun <= 0;
    }

    @Override
    public int getFrozenTicksToRun() {
        return frozenTicksToRun;
    }
}
