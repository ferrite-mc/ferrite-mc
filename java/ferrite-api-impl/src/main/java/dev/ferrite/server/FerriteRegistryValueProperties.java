package dev.ferrite.server;

import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.Set;
import org.bukkit.Keyed;
import org.bukkit.NamespacedKey;

/** Data-backed read-only APIs. These values never delegate unsupported properties to a host. */
final class FerriteRegistryValueProperties implements InvocationHandler {
    private static final Set<String> KINDS = Set.of(
            "trim_material", "trim_pattern", "jukebox_song", "painting_variant", "cat_variant", "frog_variant");
    private final String kind;
    private final NamespacedKey key;
    private final Class<? extends Keyed> type;
    private final FerriteRegistryValueData data;

    private FerriteRegistryValueProperties(String kind, NamespacedKey key, Class<? extends Keyed> type) {
        this.kind = kind;
        this.key = key;
        this.type = type;
        this.data = new FerriteRegistryValueData(kind, key);
    }

    static boolean supports(String kind) { return KINDS.contains(kind); }

    static <T extends Keyed> T create(String kind, NamespacedKey key, Class<T> type) {
        return type.cast(Proxy.newProxyInstance(type.getClassLoader(), new Class<?>[]{type},
                new FerriteRegistryValueProperties(kind, key, type)));
    }

    private int ordinal(NamespacedKey value) {
        int id = FerriteRegistryAccess.protocolId(kind, value);
        if (id < 0) throw new UnsupportedOperationException("Ferrite: no generated " + kind + " id for " + value);
        return id;
    }

    @Override public Object invoke(Object proxy, Method method, Object[] args) throws Throwable {
        return switch (method.getName()) {
            case "getKey", "key" -> key;
            case "toString" -> kind + "[" + key + "]";
            case "hashCode" -> key.hashCode();
            case "equals" -> type.isInstance(args[0]) && key.equals(((Keyed) args[0]).getKey());
            case "name" -> key.getKey().toUpperCase(java.util.Locale.ROOT);
            case "ordinal", "getId" -> ordinal(key);
            case "compareTo" -> Integer.compare(ordinal(key), ordinal(type.cast(args[0]).getKey()));
            case "description", "getDescription" -> data.component("description", false);
            case "getTranslationKey" -> data.translationKey("description");
            case "getSound" -> data.sound("sound_event");
            case "getLengthInSeconds" -> data.number("length_in_seconds", Float.MIN_VALUE, Float.MAX_VALUE);
            case "getComparatorOutput" -> data.integer("comparator_output", 0, 15);
            case "getBlockWidth" -> data.integer("width", 1, 16);
            case "getBlockHeight" -> data.integer("height", 1, 16);
            case "assetId" -> data.key("asset_id");
            case "title" -> data.component("title", true);
            case "author" -> data.component("author", true);
            default -> {
                if (method.isDefault()) yield InvocationHandler.invokeDefault(proxy, method, args);
                throw new UnsupportedOperationException("Ferrite: unsupported " + kind + " property " + method);
            }
        };
    }
}
