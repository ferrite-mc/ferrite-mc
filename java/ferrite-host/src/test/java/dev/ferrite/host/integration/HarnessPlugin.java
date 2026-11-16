package dev.ferrite.host.integration;

import dev.ferrite.host.JavaPlugin;

/** Test-only plugin loaded from the harness jar (the class itself is on the test classpath). */
public final class HarnessPlugin extends JavaPlugin {
    static volatile boolean enabled;
    static volatile boolean disabled;

    @Override
    public void onEnable() {
        enabled = true;
    }

    @Override
    public void onDisable() {
        disabled = true;
    }
}
