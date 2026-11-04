package dev.ferrite.server;

/** Tick control state (Server#getServerTickManager); the Rust tick loop does not read it yet. */
final class FerriteTickManager implements org.bukkit.ServerTickManager {
    private float tickRate = 20.0f;
    private boolean frozen;
    private int stepTicks;
    private int sprintTicks;

    @Override public boolean isRunningNormally() { return !frozen && sprintTicks == 0; }
    @Override public boolean isStepping() { return frozen && stepTicks > 0; }
    @Override public boolean isSprinting() { return sprintTicks > 0; }
    @Override public boolean isFrozen() { return frozen; }
    @Override public float getTickRate() { return tickRate; }
    @Override public void setTickRate(float rate) {
        if (rate < 1.0f) throw new IllegalArgumentException("Tick rate must be at least 1");
        if (rate > 10000f) throw new IllegalArgumentException("Tick rate must be at most 10000");
        tickRate = rate;
    }
    @Override public void setFrozen(boolean frozen) { this.frozen = frozen; if (!frozen) stepTicks = 0; }
    @Override public boolean stepGameIfFrozen(int ticks) {
        if (!frozen || ticks <= 0) return false;
        stepTicks = ticks;
        return true;
    }
    @Override public boolean stopStepping() { boolean was = stepTicks > 0; stepTicks = 0; return was; }
    @Override public boolean requestGameToSprint(int ticks) {
        boolean was = sprintTicks > 0;
        sprintTicks = Math.max(0, ticks);
        return was;
    }
    @Override public boolean stopSprinting() { boolean was = sprintTicks > 0; sprintTicks = 0; return was; }
    @Override public boolean isFrozen(org.bukkit.entity.Entity entity) { return frozen; }
    @Override public int getFrozenTicksToRun() { return stepTicks; }
}
