package dev.ferrite.server;

import java.util.Collection;
import java.util.Map;

/**
 * Generic route from a Bukkit/Paper method to the Rust host. Every generated stub calls here with
 * the owning API ("Player", "Server", ...), the subject it was called on, a signature key such as
 * {@code addAttachment(Plugin,String,boolean)} and its arguments encoded as text. The host answers
 * with an encoded value, or null when it has no handler yet; then the call throws
 * {@link UnsupportedOperationException} naming the exact method, so gaps stay visible.
 */
public final class HostBridge {
    private HostBridge() {}
    static final char ARG = '\u001f';
    static final char ITEM = '\u001d';
    static final String NULL = "\u001bnull";

    /** Test hook: replaces the native call. Returns null for "not handled". */
    interface Transport { String call(String owner, String subject, String method, String args); }
    static volatile Transport transport;

    static String encode(Object value) {
        if (value == null) return NULL;
        if (value instanceof String || value instanceof Number || value instanceof Boolean || value instanceof Character) return value.toString();
        if (value instanceof Enum<?> e) return e.name();
        if (value instanceof java.util.UUID id) return id.toString();
        if (value instanceof org.bukkit.entity.Entity e) return e.getUniqueId().toString();
        if (value instanceof org.bukkit.OfflinePlayer p) return p.getUniqueId().toString();
        if (value instanceof org.bukkit.World w) return w.getName();
        if (value instanceof org.bukkit.Location l) return (l.getWorld() == null ? "" : l.getWorld().getName()) + ITEM + l.getX() + ITEM + l.getY() + ITEM + l.getZ() + ITEM + l.getYaw() + ITEM + l.getPitch();
        if (value instanceof org.bukkit.util.Vector v) return v.getX() + "" + ITEM + v.getY() + ITEM + v.getZ();
        if (FerriteInventory.isFerrite(value)) return FerriteInventory.wire((org.bukkit.inventory.Inventory) value);
        if (value instanceof org.bukkit.block.data.BlockData b) return b.getMaterial().getKey().toString();
        if (value instanceof org.bukkit.inventory.ItemStack s) return s.getType().name() + ITEM + s.getAmount();
        if (value instanceof org.bukkit.Material m) return m.name();
        if (value instanceof org.bukkit.potion.PotionEffect e) return e.getType().getKey().toString() + ITEM + e.getDuration() + ITEM + e.getAmplifier() + ITEM + e.isAmbient() + ITEM + e.hasParticles() + ITEM + e.hasIcon();
        if (value instanceof org.bukkit.Keyed k) return k.getKey().toString();
        if (value instanceof org.bukkit.plugin.Plugin p) return p.getName();
        if (value instanceof org.bukkit.NamespacedKey k) return k.toString();
        if (value instanceof net.kyori.adventure.key.Keyed k) return k.key().asString();
        if (value instanceof net.kyori.adventure.key.Key k) return k.asString();
        if (value instanceof net.kyori.adventure.text.Component c) return net.kyori.adventure.text.serializer.gson.GsonComponentSerializer.gson().serialize(c);
        if (value instanceof java.time.Duration d) return Long.toString(d.toMillis());
        if (value instanceof java.time.Instant i) return Long.toString(i.toEpochMilli());
        if (value instanceof java.util.Date d) return Long.toString(d.getTime());
        if (value instanceof byte[] b) return java.util.Base64.getEncoder().encodeToString(b);
        if (value instanceof Collection<?> c) { StringBuilder s = new StringBuilder(); boolean first = true; for (Object o : c) { if (!first) s.append(ITEM); first = false; s.append(encode(o)); } return s.toString(); }
        if (value instanceof Map<?, ?> m) { StringBuilder s = new StringBuilder(); boolean first = true; for (Map.Entry<?, ?> e : m.entrySet()) { if (!first) s.append(ITEM); first = false; s.append(encode(e.getKey())).append('=').append(encode(e.getValue())); } return s.toString(); }
        if (value instanceof Runnable || value instanceof java.util.function.Consumer<?> || value instanceof java.util.function.Predicate<?> || value instanceof java.util.function.Function<?, ?>) return "\u001bcallback";
        return value.toString();
    }

    private static String raw(String owner, Object subject, String method, Object[] args) {
        StringBuilder joined = new StringBuilder();
        for (int i = 0; i < args.length; i++) { if (i > 0) joined.append(ARG); joined.append(encode(args[i])); }
        String subjectText = subject == null ? "" : subject.toString();
        Transport t = transport;
        String reply;
        if (t != null) reply = t.call(owner, subjectText, method, joined.toString());
        else if (Native.enabled()) reply = Native.hostCall(owner, subjectText, method, joined.toString());
        else reply = null;
        if (reply != null) rememberSetter(owner, subjectText, method, joined.toString());
        else reply = localProperty(owner, subjectText, method, joined.toString());
        if (reply == null) throw new UnsupportedOperationException("Ferrite: not implemented: " + owner + "#" + method);
        return reply;
    }

    /**
     * Properties the host does not track yet (flying, fire ticks, walk speed, ...). A plugin that sets one reads
     * the same value back; the effect on the client needs a dedicated host handler. Anything that was never set
     * and has no known default stays unimplemented, so wrong defaults are never invented.
     */
    private static final java.util.concurrent.ConcurrentHashMap<String, String> LOCAL = new java.util.concurrent.ConcurrentHashMap<>();
    private static final Map<String, String> DEFAULTS = Map.ofEntries(
        Map.entry("WalkSpeed", "0.2"), Map.entry("FlySpeed", "0.1"), Map.entry("MaximumAir", "300"), Map.entry("RemainingAir", "300"),
        Map.entry("Gravity", "true"), Map.entry("Flying", "false"), Map.entry("AllowFlight", "false"), Map.entry("Sneaking", "false"),
        Map.entry("Sprinting", "false"), Map.entry("Glowing", "false"), Map.entry("Invulnerable", "false"), Map.entry("Silent", "false"),
        Map.entry("FireTicks", "0"), Map.entry("FreezeTicks", "0"), Map.entry("Saturation", "5.0"), Map.entry("Exhaustion", "0.0"),
        Map.entry("PVP", "true"), Map.entry("AutoSave", "true"), Map.entry("KeepSpawnInMemory", "true"), Map.entry("Persistent", "true"),
        Map.entry("Swimming", "false"), Map.entry("Gliding", "false"), Map.entry("Climbing", "false"), Map.entry("Blocking", "false"),
        Map.entry("MaxStackSize", "64"), Map.entry("MaxDurability", "0"), Map.entry("HealthScaled", "false"), Map.entry("SleepingIgnored", "false"), Map.entry("CustomNameVisible", "false"));

    private static String property(String method, String prefix) {
        int paren = method.indexOf('(');
        String name = method.substring(0, paren);
        return name.length() > prefix.length() && name.startsWith(prefix) ? name.substring(prefix.length()) : null;
    }

    /** A setter the host accepted is remembered too, so the matching getter reads it back. */
    private static void rememberSetter(String owner, String subject, String method, String args) {
        String prop = property(method, "set");
        if (prop != null && !method.contains(",") && !args.isEmpty()) LOCAL.put(owner + '|' + subject + '|' + prop, args);
    }

    private static String localProperty(String owner, String subject, String method, String args) {
        boolean noArgs = method.endsWith("()");
        if (noArgs) {
            for (String prefix : new String[]{"get", "is", "has"}) {
                String prop = property(method, prefix);
                if (prop == null) continue;
                String stored = LOCAL.get(owner + '|' + subject + '|' + prop);
                return stored != null ? (stored.equals(NULL) ? NULL : stored) : DEFAULTS.get(prop);
            }
            return null;
        }
        String prop = property(method, "set");
        if (prop != null && !method.contains(",") && !args.isEmpty()) { LOCAL.put(owner + '|' + subject + '|' + prop, args); return "true"; }
        return null;
    }

    /** Test hook: forgets locally kept properties. */
    static void clearLocalProperties() { LOCAL.clear(); }

    public static void callVoid(String owner, Object subject, String method, Object... args) { raw(owner, subject, method, args); }
    public static boolean callBoolean(String owner, Object subject, String method, Object... args) { return Boolean.parseBoolean(raw(owner, subject, method, args)); }
    public static int callInt(String owner, Object subject, String method, Object... args) { return (int) Long.parseLong(raw(owner, subject, method, args)); }
    public static long callLong(String owner, Object subject, String method, Object... args) { return Long.parseLong(raw(owner, subject, method, args)); }
    public static short callShort(String owner, Object subject, String method, Object... args) { return (short) Long.parseLong(raw(owner, subject, method, args)); }
    public static byte callByte(String owner, Object subject, String method, Object... args) { return (byte) Long.parseLong(raw(owner, subject, method, args)); }
    public static char callChar(String owner, Object subject, String method, Object... args) { return raw(owner, subject, method, args).charAt(0); }
    public static float callFloat(String owner, Object subject, String method, Object... args) { return Float.parseFloat(raw(owner, subject, method, args)); }
    public static double callDouble(String owner, Object subject, String method, Object... args) { return Double.parseDouble(raw(owner, subject, method, args)); }

    /** Reference returns: text, UUIDs, enums, keys and null are decoded; richer objects need a dedicated override. */
    @SuppressWarnings({"unchecked", "rawtypes"})
    public static Object call(String owner, Object subject, String method, Class<?> type, Object... args) {
        String reply = raw(owner, subject, method, args);
        if (reply.equals(NULL)) return null;
        if (type == String.class || type == CharSequence.class || type == Object.class) return reply;
        if (type == java.util.UUID.class) return java.util.UUID.fromString(reply);
        if (type == Boolean.class) return Boolean.valueOf(reply);
        if (type == Integer.class) return Integer.valueOf(reply);
        if (type == Long.class) return Long.valueOf(reply);
        if (type == Double.class) return Double.valueOf(reply);
        if (type == Float.class) return Float.valueOf(reply);
        if (type.isEnum()) return Enum.valueOf((Class) type, reply);
        if (type == org.bukkit.NamespacedKey.class) return org.bukkit.NamespacedKey.fromString(reply);
        if (type == byte[].class) return java.util.Base64.getDecoder().decode(reply);
        if (type == net.kyori.adventure.text.Component.class) return net.kyori.adventure.text.serializer.gson.GsonComponentSerializer.gson().deserialize(reply);
        if (type == java.util.List.class || type == java.util.Collection.class) return reply.isEmpty() ? new java.util.ArrayList<String>() : new java.util.ArrayList<>(java.util.Arrays.asList(reply.split(String.valueOf(ITEM), -1)));
        if (type == java.util.Set.class) return reply.isEmpty() ? new java.util.LinkedHashSet<String>() : new java.util.LinkedHashSet<>(java.util.Arrays.asList(reply.split(String.valueOf(ITEM), -1)));
        throw new UnsupportedOperationException("Ferrite: host reply for " + owner + "#" + method + " cannot be turned into " + type.getName());
    }

    /** Routes a dynamic-proxy call (World, Block, Inventory, ...) whose method has no Java implementation. */
    public static Object dispatch(String owner, Object subject, java.lang.reflect.Method method, Object[] args) {
        StringBuilder key = new StringBuilder(method.getName()).append('(');
        Class<?>[] types = method.getParameterTypes();
        for (int i = 0; i < types.length; i++) { if (i > 0) key.append(','); key.append(types[i].getSimpleName()); }
        key.append(')');
        Object[] values = args == null ? new Object[0] : args;
        Class<?> r = method.getReturnType();
        String k = key.toString();
        if (r == void.class) { callVoid(owner, subject, k, values); return null; }
        if (r == boolean.class) return callBoolean(owner, subject, k, values);
        if (r == int.class) return callInt(owner, subject, k, values);
        if (r == long.class) return callLong(owner, subject, k, values);
        if (r == short.class) return callShort(owner, subject, k, values);
        if (r == byte.class) return callByte(owner, subject, k, values);
        if (r == char.class) return callChar(owner, subject, k, values);
        if (r == float.class) return callFloat(owner, subject, k, values);
        if (r == double.class) return callDouble(owner, subject, k, values);
        return call(owner, subject, k, r, values);
    }
}
