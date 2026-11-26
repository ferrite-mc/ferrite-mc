package dev.ferrite.host;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * A small, dependency-free JSON reader and writer.
 *
 * <p>Objects become {@link LinkedHashMap}, arrays become {@link ArrayList}, integers become
 * {@link Long}, decimals become {@link Double}. This is deliberately not a general-purpose
 * library: the host only exchanges the shapes fixed by {@code docs/plugin-b-host-contract.md}.
 */
final class Json {
    private Json() {
    }

    /** Parses one JSON value and rejects trailing data. */
    static Object parse(String text) {
        return new Parser(text).parse();
    }

    /** Encodes a value made of maps, lists, strings, numbers, booleans and null. */
    static String stringify(Object value) {
        StringBuilder out = new StringBuilder();
        write(out, value);
        return out.toString();
    }

    /** Builds a mutable object from alternating key/value arguments. */
    static Map<String, Object> object(Object... pairs) {
        if (pairs.length % 2 != 0) {
            throw new IllegalArgumentException("object() needs an even number of arguments");
        }
        Map<String, Object> map = new LinkedHashMap<>();
        for (int i = 0; i < pairs.length; i += 2) {
            if (!(pairs[i] instanceof String key)) {
                throw new IllegalArgumentException("object() keys must be strings");
            }
            map.put(key, pairs[i + 1]);
        }
        return map;
    }

    /** Builds a list from the given values. */
    static List<Object> array(Object... values) {
        List<Object> list = new ArrayList<>(values.length);
        for (Object value : values) {
            list.add(value);
        }
        return list;
    }

    @SuppressWarnings("unchecked")
    static Map<String, Object> asObject(Object value) {
        if (value instanceof Map<?, ?> map) {
            return (Map<String, Object>) map;
        }
        throw new IllegalArgumentException("expected a JSON object, got " + describe(value));
    }

    @SuppressWarnings("unchecked")
    static List<Object> asList(Object value) {
        if (value instanceof List<?> list) {
            return (List<Object>) list;
        }
        throw new IllegalArgumentException("expected a JSON array, got " + describe(value));
    }

    static Map<String, Object> objectOrEmpty(Object value) {
        return value == null ? Map.of() : asObject(value);
    }

    static String string(Map<String, Object> object, String key, String fallback) {
        Object value = object.get(key);
        if (value == null) {
            return fallback;
        }
        if (value instanceof String text) {
            return text;
        }
        throw new IllegalArgumentException("'" + key + "' must be a string");
    }

    static String requireString(Map<String, Object> object, String key) {
        String value = string(object, key, null);
        if (value == null || value.isEmpty()) {
            throw new IllegalArgumentException("'" + key + "' is required");
        }
        return value;
    }

    static long longValue(Map<String, Object> object, String key, long fallback) {
        Object value = object.get(key);
        if (value == null) {
            return fallback;
        }
        if (value instanceof Number number) {
            return number.longValue();
        }
        throw new IllegalArgumentException("'" + key + "' must be a number");
    }

    static int intValue(Map<String, Object> object, String key, int fallback) {
        return Math.toIntExact(longValue(object, key, fallback));
    }

    static boolean boolValue(Map<String, Object> object, String key, boolean fallback) {
        Object value = object.get(key);
        if (value == null) {
            return fallback;
        }
        if (value instanceof Boolean flag) {
            return flag;
        }
        throw new IllegalArgumentException("'" + key + "' must be a boolean");
    }

    static List<String> strings(Object value) {
        if (value == null) {
            return List.of();
        }
        List<String> out = new ArrayList<>();
        for (Object item : asList(value)) {
            if (!(item instanceof String text)) {
                throw new IllegalArgumentException("expected an array of strings");
            }
            out.add(text);
        }
        return List.copyOf(out);
    }

    static Map<String, Boolean> booleans(Object value) {
        if (value == null) {
            return Map.of();
        }
        Map<String, Boolean> out = new LinkedHashMap<>();
        for (Map.Entry<String, Object> entry : asObject(value).entrySet()) {
            if (!(entry.getValue() instanceof Boolean flag)) {
                throw new IllegalArgumentException("expected a map of booleans");
            }
            out.put(entry.getKey(), flag);
        }
        return Map.copyOf(out);
    }

    private static void write(StringBuilder out, Object value) {
        if (value == null) {
            out.append("null");
            return;
        }
        if (value instanceof String text) {
            writeString(out, text);
            return;
        }
        if (value instanceof Character character) {
            writeString(out, character.toString());
            return;
        }
        if (value instanceof Boolean) {
            out.append(value);
            return;
        }
        if (value instanceof Number number) {
            if (number instanceof Double decimal && !Double.isFinite(decimal)) {
                throw new IllegalArgumentException("JSON cannot encode a non-finite double");
            }
            if (number instanceof Float decimal && !Float.isFinite(decimal)) {
                throw new IllegalArgumentException("JSON cannot encode a non-finite float");
            }
            out.append(number);
            return;
        }
        if (value instanceof Map<?, ?> map) {
            out.append('{');
            boolean first = true;
            for (Map.Entry<?, ?> entry : map.entrySet()) {
                if (!first) {
                    out.append(',');
                }
                first = false;
                writeString(out, String.valueOf(entry.getKey()));
                out.append(':');
                write(out, entry.getValue());
            }
            out.append('}');
            return;
        }
        if (value instanceof Iterable<?> iterable) {
            out.append('[');
            boolean first = true;
            for (Object item : iterable) {
                if (!first) {
                    out.append(',');
                }
                first = false;
                write(out, item);
            }
            out.append(']');
            return;
        }
        throw new IllegalArgumentException("cannot encode " + value.getClass().getName());
    }

    private static void writeString(StringBuilder out, String text) {
        out.append('"');
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            switch (c) {
                case '"' -> out.append("\\\"");
                case '\\' -> out.append("\\\\");
                case '\b' -> out.append("\\b");
                case '\f' -> out.append("\\f");
                case '\n' -> out.append("\\n");
                case '\r' -> out.append("\\r");
                case '\t' -> out.append("\\t");
                default -> {
                    if (c < 0x20) {
                        out.append(String.format("\\u%04x", (int) c));
                    } else {
                        out.append(c);
                    }
                }
            }
        }
        out.append('"');
    }

    private static String describe(Object value) {
        return value == null ? "null" : value.getClass().getName();
    }

    private static final class Parser {
        private final String text;
        private int index;

        Parser(String text) {
            this.text = text;
        }

        Object parse() {
            skipWhitespace();
            Object value = value();
            skipWhitespace();
            if (index != text.length()) {
                throw error("unexpected trailing data");
            }
            return value;
        }

        private Object value() {
            skipWhitespace();
            if (index >= text.length()) {
                throw error("unexpected end of input");
            }
            char c = text.charAt(index);
            return switch (c) {
                case '{' -> object();
                case '[' -> array();
                case '"' -> string();
                case 't' -> literal("true", Boolean.TRUE);
                case 'f' -> literal("false", Boolean.FALSE);
                case 'n' -> literal("null", null);
                default -> number();
            };
        }

        private Map<String, Object> object() {
            expect('{');
            skipWhitespace();
            Map<String, Object> map = new LinkedHashMap<>();
            if (peek('}')) {
                index++;
                return map;
            }
            while (true) {
                skipWhitespace();
                if (!peek('"')) {
                    throw error("expected an object key");
                }
                String key = string();
                skipWhitespace();
                expect(':');
                skipWhitespace();
                map.put(key, value());
                skipWhitespace();
                if (peek('}')) {
                    index++;
                    return map;
                }
                expect(',');
            }
        }

        private List<Object> array() {
            expect('[');
            skipWhitespace();
            List<Object> list = new ArrayList<>();
            if (peek(']')) {
                index++;
                return list;
            }
            while (true) {
                list.add(value());
                skipWhitespace();
                if (peek(']')) {
                    index++;
                    return list;
                }
                expect(',');
                skipWhitespace();
            }
        }

        private String string() {
            expect('"');
            StringBuilder out = new StringBuilder();
            while (index < text.length()) {
                char c = text.charAt(index++);
                if (c == '"') {
                    return out.toString();
                }
                if (c == '\\') {
                    if (index >= text.length()) {
                        throw error("unterminated escape");
                    }
                    char escape = text.charAt(index++);
                    switch (escape) {
                        case '"' -> out.append('"');
                        case '\\' -> out.append('\\');
                        case '/' -> out.append('/');
                        case 'b' -> out.append('\b');
                        case 'f' -> out.append('\f');
                        case 'n' -> out.append('\n');
                        case 'r' -> out.append('\r');
                        case 't' -> out.append('\t');
                        case 'u' -> out.append((char) unicode());
                        default -> throw error("invalid escape");
                    }
                } else if (c < 0x20) {
                    throw error("control character in a string");
                } else {
                    out.append(c);
                }
            }
            throw error("unterminated string");
        }

        private int unicode() {
            if (index + 4 > text.length()) {
                throw error("truncated unicode escape");
            }
            int code = 0;
            for (int i = 0; i < 4; i++) {
                int digit = Character.digit(text.charAt(index++), 16);
                if (digit < 0) {
                    throw error("invalid unicode escape");
                }
                code = code * 16 + digit;
            }
            return code;
        }

        private Object number() {
            int start = index;
            if (peek('-')) {
                index++;
            }
            while (index < text.length() && Character.isDigit(text.charAt(index))) {
                index++;
            }
            boolean decimal = false;
            if (index < text.length() && text.charAt(index) == '.') {
                decimal = true;
                index++;
                while (index < text.length() && Character.isDigit(text.charAt(index))) {
                    index++;
                }
            }
            if (index < text.length() && (text.charAt(index) == 'e' || text.charAt(index) == 'E')) {
                decimal = true;
                index++;
                if (index < text.length() && (text.charAt(index) == '+' || text.charAt(index) == '-')) {
                    index++;
                }
                while (index < text.length() && Character.isDigit(text.charAt(index))) {
                    index++;
                }
            }
            if (start == index) {
                throw error("expected a value");
            }
            String number = text.substring(start, index);
            try {
                if (decimal) {
                    return Double.valueOf(number);
                }
                return Long.valueOf(number);
            } catch (NumberFormatException error) {
                throw error("invalid number");
            }
        }

        private Object literal(String word, Object value) {
            if (!text.startsWith(word, index)) {
                throw error("invalid literal");
            }
            index += word.length();
            return value;
        }

        private void skipWhitespace() {
            while (index < text.length() && Character.isWhitespace(text.charAt(index))) {
                index++;
            }
        }

        private boolean peek(char expected) {
            return index < text.length() && text.charAt(index) == expected;
        }

        private void expect(char expected) {
            if (!peek(expected)) {
                throw error("expected '" + expected + "'");
            }
            index++;
        }

        private IllegalArgumentException error(String message) {
            return new IllegalArgumentException(message + " at offset " + index);
        }
    }
}
