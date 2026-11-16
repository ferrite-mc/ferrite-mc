package dev.ferrite.api.server;

import io.papermc.paper.registry.RegistryAccess;
import io.papermc.paper.registry.RegistryKey;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.Collections;
import java.util.Spliterators;
import java.util.stream.Stream;
import org.bukkit.Keyed;
import org.bukkit.Registry;

/**
 * Supplies the Paper API's {@code RegistryAccess} service so {@code org.bukkit.Registry},
 * {@code Material} and other keyed enums initialize without CraftBukkit.
 *
 * <p>The facade has no vanilla registry data (that belongs to the host/world layer). Every
 * registry is empty: lookups return {@code null}, iteration is empty, tags are absent. Code
 * that treats a missing entry as "unknown" (the vanilla API contract for {@code get}) works;
 * code that requires registry contents gets absent data rather than a class-init crash.
 */
public final class FerriteRegistryAccess implements RegistryAccess {

    private static final Registry<?> EMPTY = createEmptyRegistry();

    @Override
    public <T extends Keyed> Registry<T> getRegistry(Class<T> type) {
        return empty();
    }

    @Override
    public <T extends Keyed> Registry<T> getRegistry(RegistryKey<T> key) {
        return empty();
    }

    @SuppressWarnings("unchecked")
    private static <T extends Keyed> Registry<T> empty() {
        return (Registry<T>) EMPTY;
    }

    private static Registry<?> createEmptyRegistry() {
        return (Registry<?>) Proxy.newProxyInstance(
                FerriteRegistryAccess.class.getClassLoader(),
                new Class<?>[] {Registry.class},
                new EmptyRegistryHandler());
    }

    private static final class EmptyRegistryHandler implements InvocationHandler {

        @Override
        public Object invoke(Object proxy, Method method, Object[] args) {
            switch (method.getName()) {
                case "toString":
                    return "FerriteEmptyRegistry";
                case "hashCode":
                    return System.identityHashCode(proxy);
                case "equals":
                    return proxy == (args == null ? null : args[0]);
                case "iterator":
                    return Collections.emptyIterator();
                case "spliterator":
                    return Spliterators.emptySpliterator();
                case "stream":
                case "streamTags":
                    return Stream.empty();
                case "hasTag":
                    return false;
                case "get":
                case "getKey":
                case "getTag":
                    return null;
                default:
                    Class<?> returnType = method.getReturnType();
                    if (returnType == boolean.class) {
                        return false;
                    }
                    if (returnType == int.class) {
                        return 0;
                    }
                    if (returnType == long.class) {
                        return 0L;
                    }
                    if (returnType == double.class) {
                        return 0.0D;
                    }
                    if (returnType == float.class) {
                        return 0.0F;
                    }
                    if (returnType == short.class) {
                        return (short) 0;
                    }
                    if (returnType == byte.class) {
                        return (byte) 0;
                    }
                    if (returnType == char.class) {
                        return (char) 0;
                    }
                    return null;
            }
        }
    }
}
