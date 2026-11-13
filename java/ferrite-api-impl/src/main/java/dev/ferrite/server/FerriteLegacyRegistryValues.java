package dev.ferrite.server;

import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Proxy;
import java.util.Locale;
import java.util.Map;
import org.bukkit.Keyed;
import org.bukkit.NamespacedKey;

/** Legacy enum-shaped APIs whose ordinal is the actual built-in registry report ID. */
final class FerriteLegacyRegistryValues {
    private static final Map<String, Class<? extends Keyed>> TYPES = Map.of(
            "sound_event", org.bukkit.Sound.class,
            "villager_profession", org.bukkit.entity.Villager.Profession.class,
            "villager_type", org.bukkit.entity.Villager.Type.class);
    static boolean supports(String kind) { return TYPES.containsKey(kind); }

    private static int id(String kind, NamespacedKey key) {
        int result = FerriteRegistryAccess.protocolId(kind, key);
        if (result < 0) throw new IllegalArgumentException("No generated " + kind + " id for " + key);
        return result;
    }

    static <T extends Keyed> T create(String kind, NamespacedKey key, Class<T> type) {
        if (TYPES.get(kind) != type) throw new IllegalArgumentException("Wrong legacy registry type for " + kind);
        int ordinal = id(kind, java.util.Objects.requireNonNull(key, "key"));
        return type.cast(Proxy.newProxyInstance(type.getClassLoader(), new Class<?>[]{type}, (proxy, method, args) -> switch (method.getName()) {
            case "getKey", "key" -> key;
            case "name" -> key.getKey().toUpperCase(Locale.ROOT).replace('.', '_');
            case "ordinal" -> ordinal;
            case "compareTo" -> Integer.compare(ordinal, id(kind, type.cast(java.util.Objects.requireNonNull(args[0], "other")).getKey()));
            case "hashCode" -> key.hashCode();
            case "equals" -> type.isInstance(args[0]) && key.equals(((Keyed) args[0]).getKey());
            case "toString" -> key.toString();
            default -> {
                // In particular, Profession.translationKey belongs to the installed API.
                if (method.isDefault()) yield InvocationHandler.invokeDefault(proxy, method, args);
                throw new UnsupportedOperationException("Unsupported " + kind + " value method " + method);
            }
        }));
    }
}
