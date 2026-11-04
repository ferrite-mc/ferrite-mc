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
        if (value instanceof org.bukkit.inventory.ItemStack s) return s.getType().name() + ITEM + s.getAmount();
        if (value instanceof org.bukkit.Material m) return m.name();
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
        if (reply == null) throw new UnsupportedOperationException("Ferrite: not implemented: " + owner + "#" + method);
        return reply;
    }

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
}
