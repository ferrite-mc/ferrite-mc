package dev.ferrite.server;

import java.util.List;
import java.util.Map;

/** Writes the plain Java values {@link StructureNbt} reads as stringified NBT for {@code /data merge}. */
final class Snbt {
    private Snbt() {}

    static String of(Object value) {
        var out = new StringBuilder();
        write(out, value);
        return out.toString();
    }

    private static void write(StringBuilder out, Object value) {
        switch (value) {
            case Byte b -> out.append(b).append('b');
            case Short s -> out.append(s).append('s');
            case Integer i -> out.append(i);
            case Long l -> out.append(l).append('L');
            case Float f -> out.append(f).append('f');
            case Double d -> out.append(d).append('d');
            case String s -> quote(out, s);
            case byte[] bytes -> {
                out.append("[B;");
                for (int i = 0; i < bytes.length; i++) out.append(i > 0 ? "," : "").append(bytes[i]).append('b');
                out.append(']');
            }
            case int[] ints -> {
                out.append("[I;");
                for (int i = 0; i < ints.length; i++) out.append(i > 0 ? "," : "").append(ints[i]);
                out.append(']');
            }
            case long[] longs -> {
                out.append("[L;");
                for (int i = 0; i < longs.length; i++) out.append(i > 0 ? "," : "").append(longs[i]).append('L');
                out.append(']');
            }
            case List<?> list -> {
                out.append('[');
                for (int i = 0; i < list.size(); i++) {
                    if (i > 0) out.append(',');
                    write(out, list.get(i));
                }
                out.append(']');
            }
            case Map<?, ?> map -> {
                out.append('{');
                boolean first = true;
                for (var entry : map.entrySet()) {
                    if (!first) out.append(',');
                    first = false;
                    var key = String.valueOf(entry.getKey());
                    if (key.matches("[A-Za-z0-9_.+-]+")) out.append(key); else quote(out, key);
                    out.append(':');
                    write(out, entry.getValue());
                }
                out.append('}');
            }
            default -> throw new IllegalArgumentException("Not an NBT value: " + value);
        }
    }

    private static void quote(StringBuilder out, String text) {
        out.append('"');
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            if (c == '"' || c == '\\') out.append('\\').append(c);
            else if (c == '\n') out.append("\\n");
            else if (c == '\r') out.append("\\r");
            else out.append(c);
        }
        out.append('"');
    }
}
