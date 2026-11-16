package dev.ferrite.api.server;

import io.papermc.paper.plugin.lifecycle.event.handler.configuration.LifecycleEventHandlerConfiguration;
import io.papermc.paper.plugin.lifecycle.event.LifecycleEventManager;
import org.bukkit.plugin.Plugin;

/**
 * Placeholder lifecycle manager needed to construct {@link org.bukkit.plugin.java.JavaPlugin}.
 *
 * <p>Paper lifecycle events (bootstrap/loader/datapack registration) are outside the first
 * slice. Registration fails explicitly instead of pretending the lifecycle was registered.
 */
public final class FerriteLifecycleEventManager implements LifecycleEventManager<Plugin> {

    @Override
    public void registerEventHandler(
            LifecycleEventHandlerConfiguration<? super Plugin> configuration) {
        throw new UnsupportedOperationException(
                "Paper lifecycle event registration is not supported by the Ferrite facade yet");
    }
}
