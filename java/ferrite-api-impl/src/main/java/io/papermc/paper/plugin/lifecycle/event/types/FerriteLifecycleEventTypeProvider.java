package io.papermc.paper.plugin.lifecycle.event.types;

import dev.ferrite.server.FerriteLifecycleTypes;
import io.papermc.paper.plugin.bootstrap.BootstrapContext;
import io.papermc.paper.plugin.lifecycle.event.LifecycleEventOwner;
import io.papermc.paper.plugin.lifecycle.event.LifecycleEvent;
import io.papermc.paper.plugin.lifecycle.event.registrar.ReloadableRegistrarEvent;
import io.papermc.paper.registry.RegistryKey;
import io.papermc.paper.tag.PostFlattenTagRegistrar;
import io.papermc.paper.tag.PreFlattenTagRegistrar;

/**
 * The service {@code LifecycleEvents} loads its event types from. It lives in the API's package because the provider
 * interface is package-private. Types it hands out for bootstrap-only events can be created (so
 * {@code LifecycleEvents} initialises) but a plugin-owned manager refuses to register them.
 */
public final class FerriteLifecycleEventTypeProvider implements LifecycleEventTypeProvider {
    @Override public <O extends LifecycleEventOwner, E extends LifecycleEvent> LifecycleEventType.Monitorable<O, E> monitor(String name, Class<? extends O> owner) {
        return FerriteLifecycleTypes.monitor(name, owner);
    }

    @Override public <O extends LifecycleEventOwner, E extends LifecycleEvent> LifecycleEventType.Prioritizable<O, E> prioritized(String name, Class<? extends O> owner) {
        return FerriteLifecycleTypes.prioritized(name, owner);
    }

    @Override public TagEventTypeProvider tagProvider() {
        return new TagEventTypeProvider() {
            @Override public <T> LifecycleEventType.Prioritizable<BootstrapContext, ReloadableRegistrarEvent<PreFlattenTagRegistrar<T>>> preFlatten(RegistryKey<T> key) {
                return FerriteLifecycleTypes.prioritized("pre_flatten/" + key.key(), BootstrapContext.class);
            }
            @Override public <T> LifecycleEventType.Prioritizable<BootstrapContext, ReloadableRegistrarEvent<PostFlattenTagRegistrar<T>>> postFlatten(RegistryKey<T> key) {
                return FerriteLifecycleTypes.prioritized("post_flatten/" + key.key(), BootstrapContext.class);
            }
        };
    }
}
