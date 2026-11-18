// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.world.entity;

import dev.ferrite.nms.host.EntitySnapshot;
import dev.ferrite.nms.host.EntityState;
import dev.ferrite.nms.host.NmsHost;
import dev.ferrite.nms.host.NmsHostBinding;
import dev.ferrite.nms.host.NmsHostException;
import dev.ferrite.nms.host.PlayerSnapshot;
import dev.ferrite.nms.host.PlayerState;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.UUID;
import org.bukkit.Bukkit;
import org.bukkit.World;

/**
 * Reads live player and entity snapshots from the Rust server via contract v2
 * {@code player.state_get} / {@code entity.state_get} when the embedded host is active, falling
 * back to {@link NmsHostBinding#require()} in pure-Java unit test environments without JNI.
 */
public final class HostStateReader {
    private static final String BOOTSTRAP_CLASS = "dev.ferrite.host.Bootstrap";

    private HostStateReader() {}

    public static PlayerSnapshot readPlayer(UUID playerId) {
        Objects.requireNonNull(playerId, "playerId");
        Map<String, Object> result = invokeStateOp("playerStateGet", playerId, "unknown_player");
        if (result == null) {
            return NmsHostBinding.require().player(playerId);
        }
        return toPlayerSnapshot(result, tryFallbackPlayer(playerId));
    }

    public static EntitySnapshot readEntity(UUID entityId) {
        Objects.requireNonNull(entityId, "entityId");
        Map<String, Object> result = invokeStateOp("entityStateGet", entityId, "unknown_entity");
        if (result == null) {
            return NmsHostBinding.require().entity(entityId);
        }
        return toEntitySnapshot(result, tryFallbackEntity(entityId));
    }

    public static PlayerSnapshot toPlayerSnapshot(Map<String, Object> result, PlayerSnapshot fallback) {
        UUID id = UUID.fromString(requireString(result, "uuid"));
        String name = requireString(result, "name");
        UUID worldId = resolveWorldId(requireString(result, "world"));
        double x = requireDouble(result, "x");
        double y = requireDouble(result, "y");
        double z = requireDouble(result, "z");
        float yaw = requireFloat(result, "yaw");
        float pitch = requireFloat(result, "pitch");
        String gameMode = requireString(result, "game_mode");
        float health = requireFloat(result, "health");
        int foodLevel = requireInt(result, "food_level");

        EntityState entityState = fallback == null ? null : fallback.entityState();
        if (entityState == null && result.get("network_id") instanceof Number num) {
            int networkId = Math.max(0, num.intValue());
            entityState = new EntityState(
                    networkId, false, 1, -1, 0.0F, true, 0, "STANDING");
        }
        PlayerState playerState = fallback == null ? null : fallback.state();
        return new PlayerSnapshot(
                id,
                name,
                worldId,
                x,
                y,
                z,
                yaw,
                pitch,
                gameMode,
                health,
                foodLevel,
                entityState,
                playerState);
    }

    public static EntitySnapshot toEntitySnapshot(Map<String, Object> result, EntitySnapshot fallback) {
        UUID id = UUID.fromString(requireString(result, "uuid"));
        String typeKey = requireString(result, "type");
        UUID worldId = resolveWorldId(requireString(result, "world"));
        double x = requireDouble(result, "x");
        double y = requireDouble(result, "y");
        double z = requireDouble(result, "z");
        float yaw = requireFloat(result, "yaw");
        float pitch = requireFloat(result, "pitch");
        double vx = optDouble(result, "vx", 0.0);
        double vy = optDouble(result, "vy", 0.0);
        double vz = optDouble(result, "vz", 0.0);

        UUID vehicleId = null;
        if (result.get("vehicle") instanceof String s && !s.isBlank()) {
            vehicleId = UUID.fromString(s);
        } else if (fallback != null) {
            vehicleId = fallback.vehicleId();
        }

        List<UUID> passengers = List.of();
        if (result.get("passengers") instanceof List<?> rawList && !rawList.isEmpty()) {
            List<UUID> parsed = new ArrayList<>(rawList.size());
            for (Object item : rawList) {
                if (item instanceof String s && !s.isBlank()) {
                    parsed.add(UUID.fromString(s));
                }
            }
            passengers = parsed;
        } else if (fallback != null && !fallback.passengers().isEmpty()) {
            passengers = fallback.passengers();
        }

        Map<String, String> parsedEquipment = parseEquipment(result);
        EntityState entityState = fallback == null ? null : fallback.state();
        if (entityState == null) {
            int networkId = result.get("network_id") instanceof Number num
                    ? Math.max(0, num.intValue())
                    : 0;
            String customName = result.get("custom_name") instanceof String s ? s : null;
            entityState = new EntityState(
                    networkId,
                    false,
                    1,
                    -1,
                    0.0F,
                    true,
                    0,
                    "STANDING",
                    customName,
                    customName != null,
                    false,
                    false,
                    false,
                    false,
                    parsedEquipment);
        } else if (!parsedEquipment.isEmpty()) {
            entityState = new EntityState(
                    entityState.networkId(),
                    entityState.removed(),
                    entityState.ticksLived(),
                    entityState.fireTicks(),
                    entityState.fallDistance(),
                    entityState.onGround(),
                    entityState.portalCooldown(),
                    entityState.pose(),
                    entityState.customName(),
                    entityState.customNameVisible(),
                    entityState.silent(),
                    entityState.noGravity(),
                    entityState.glowing(),
                    entityState.invisible(),
                    parsedEquipment);
        }

        return new EntitySnapshot(
                id,
                typeKey,
                worldId,
                x,
                y,
                z,
                yaw,
                pitch,
                vx,
                vy,
                vz,
                vehicleId,
                passengers,
                entityState);
    }

    private static Map<String, String> parseEquipment(Map<String, Object> result) {
        if (!(result.get("equipment") instanceof Map<?, ?> rawEq) || rawEq.isEmpty()) {
            return Map.of();
        }
        Map<String, String> parsed = new java.util.LinkedHashMap<>();
        for (Map.Entry<?, ?> entry : rawEq.entrySet()) {
            if (!(entry.getKey() instanceof String slotKey) || !(entry.getValue() instanceof Map<?, ?> item)) {
                continue;
            }
            if (!(item.get("id") instanceof String itemId) || itemId.isBlank()) {
                continue;
            }
            try {
                parsed.put(EntityState.normalizeEquipmentSlot(slotKey), itemId);
            } catch (RuntimeException ignored) {
                // Ignore unknown slots from newer/custom sources.
            }
        }
        return parsed.isEmpty() ? Map.of() : Map.copyOf(parsed);
    }

    private static PlayerSnapshot tryFallbackPlayer(UUID playerId) {
        try {
            NmsHost host = NmsHostBinding.require();
            return host.player(playerId);
        } catch (IllegalStateException | NmsHostException ignored) {
            return null;
        }
    }

    private static EntitySnapshot tryFallbackEntity(UUID entityId) {
        try {
            NmsHost host = NmsHostBinding.require();
            return host.entity(entityId);
        } catch (IllegalStateException | NmsHostException ignored) {
            return null;
        }
    }

    private static Map<String, Object> invokeStateOp(
            String bootstrapMethod, UUID id, String notFoundCode) {
        String rawResponse;
        try {
            Class<?> bootstrap = Class.forName(BOOTSTRAP_CLASS);
            Method method = bootstrap.getMethod(bootstrapMethod, String.class);
            Object invoked = method.invoke(null, "{\"uuid\":\"" + id + "\"}");
            if (!(invoked instanceof String response) || response.isBlank()) {
                return null;
            }
            rawResponse = response;
        } catch (ClassNotFoundException | NoSuchMethodException | IllegalAccessException unavailable) {
            return null;
        } catch (InvocationTargetException failure) {
            return null;
        }

        Map<String, Object> envelope = parseJsonObject(rawResponse);
        Object okVal = envelope.get("ok");
        if (Boolean.TRUE.equals(okVal)) {
            Object result = envelope.get("result");
            if (result instanceof Map<?, ?> map) {
                @SuppressWarnings("unchecked")
                Map<String, Object> typed = (Map<String, Object>) map;
                return typed;
            }
            throw new NmsHostException("invalid_response", "Host returned non-object state result");
        }

        Object errorObj = envelope.get("error");
        if (errorObj instanceof Map<?, ?> errorMap) {
            String code = errorMap.get("code") instanceof String c ? c : "host_error";
            String message = errorMap.get("message") instanceof String m ? m : code;
            if ("state_unavailable".equals(code)
                    || "unsupported".equals(code)
                    || "java_exception".equals(code)) {
                return null;
            }
            if ("player_not_found".equals(code) || "entity_not_found".equals(code)) {
                throw new NmsHostException(notFoundCode, message);
            }
            throw new NmsHostException(code, message);
        }
        return null;
    }

    static UUID resolveWorldId(String worldKey) {
        if (Bukkit.getServer() != null) {
            World.Environment expectedEnv = switch (worldKey) {
                case "minecraft:overworld" -> World.Environment.NORMAL;
                case "minecraft:the_nether" -> World.Environment.NETHER;
                case "minecraft:the_end" -> World.Environment.THE_END;
                default -> null;
            };
            for (World world : Bukkit.getWorlds()) {
                if (worldKey.equals(world.getName())) {
                    return world.getUID();
                }
                if (expectedEnv != null && world.getEnvironment() == expectedEnv) {
                    return world.getUID();
                }
            }
        }
        return UUID.nameUUIDFromBytes(
                ("ferrite:world:" + worldKey).getBytes(StandardCharsets.UTF_8));
    }

    private static String requireString(Map<String, Object> map, String key) {
        Object value = map.get(key);
        if (value instanceof String s && !s.isBlank()) {
            return s;
        }
        throw new NmsHostException("invalid_response", "Missing or blank string field: " + key);
    }

    private static double requireDouble(Map<String, Object> map, String key) {
        Object value = map.get(key);
        if (value instanceof Number n) {
            return n.doubleValue();
        }
        throw new NmsHostException("invalid_response", "Missing numeric field: " + key);
    }

    private static double optDouble(Map<String, Object> map, String key, double fallback) {
        Object value = map.get(key);
        return value instanceof Number n ? n.doubleValue() : fallback;
    }

    private static float requireFloat(Map<String, Object> map, String key) {
        return (float) requireDouble(map, key);
    }

    private static int requireInt(Map<String, Object> map, String key) {
        Object value = map.get(key);
        if (value instanceof Number n) {
            return n.intValue();
        }
        throw new NmsHostException("invalid_response", "Missing integer field: " + key);
    }

    @SuppressWarnings("unchecked")
    public static Map<String, Object> parseJsonObject(String json) {
        Object parsed = new JsonParser(json).parseRoot();
        if (parsed instanceof Map<?, ?> map) {
            return (Map<String, Object>) map;
        }
        throw new NmsHostException("invalid_response", "Expected JSON object");
    }

    private static final class JsonParser {
        private final String input;
        private int pos;

        JsonParser(String input) {
            this.input = Objects.requireNonNull(input, "json");
        }

        Object parseRoot() {
            skipWhitespace();
            Object value = parseValue();
            skipWhitespace();
            return value;
        }

        private Object parseValue() {
            skipWhitespace();
            if (pos >= input.length()) {
                throw new NmsHostException("invalid_response", "Unexpected end of JSON");
            }
            char ch = input.charAt(pos);
            return switch (ch) {
                case '{' -> parseObject();
                case '[' -> parseArray();
                case '"' -> parseString();
                case 't' -> parseLiteral("true", Boolean.TRUE);
                case 'f' -> parseLiteral("false", Boolean.FALSE);
                case 'n' -> parseLiteral("null", null);
                default -> parseNumber();
            };
        }

        private Map<String, Object> parseObject() {
            expect('{');
            Map<String, Object> map = new LinkedHashMap<>();
            skipWhitespace();
            if (peek('}')) {
                pos++;
                return map;
            }
            while (true) {
                skipWhitespace();
                String key = parseString();
                skipWhitespace();
                expect(':');
                Object value = parseValue();
                map.put(key, value);
                skipWhitespace();
                if (peek(',')) {
                    pos++;
                } else if (peek('}')) {
                    pos++;
                    return map;
                } else {
                    throw new NmsHostException("invalid_response", "Expected ',' or '}' in JSON object");
                }
            }
        }

        private List<Object> parseArray() {
            expect('[');
            List<Object> list = new ArrayList<>();
            skipWhitespace();
            if (peek(']')) {
                pos++;
                return list;
            }
            while (true) {
                list.add(parseValue());
                skipWhitespace();
                if (peek(',')) {
                    pos++;
                } else if (peek(']')) {
                    pos++;
                    return list;
                } else {
                    throw new NmsHostException("invalid_response", "Expected ',' or ']' in JSON array");
                }
            }
        }

        private String parseString() {
            expect('"');
            StringBuilder sb = new StringBuilder();
            while (pos < input.length()) {
                char ch = input.charAt(pos++);
                if (ch == '"') {
                    return sb.toString();
                }
                if (ch == '\\') {
                    if (pos >= input.length()) {
                        throw new NmsHostException("invalid_response", "Unterminated JSON escape");
                    }
                    char esc = input.charAt(pos++);
                    switch (esc) {
                        case '"', '\\', '/' -> sb.append(esc);
                        case 'b' -> sb.append('\b');
                        case 'f' -> sb.append('\f');
                        case 'n' -> sb.append('\n');
                        case 'r' -> sb.append('\r');
                        case 't' -> sb.append('\t');
                        case 'u' -> {
                            String hex = input.substring(pos, Math.min(pos + 4, input.length()));
                            if (hex.length() != 4) {
                                throw new NmsHostException("invalid_response", "Invalid unicode escape");
                            }
                            sb.append((char) Integer.parseInt(hex, 16));
                            pos += 4;
                        }
                        default -> throw new NmsHostException("invalid_response", "Invalid JSON escape: \\" + esc);
                    }
                } else {
                    sb.append(ch);
                }
            }
            throw new NmsHostException("invalid_response", "Unterminated JSON string");
        }

        private Object parseLiteral(String literal, Object value) {
            if (!input.startsWith(literal, pos)) {
                throw new NmsHostException("invalid_response", "Expected " + literal);
            }
            pos += literal.length();
            return value;
        }

        private Number parseNumber() {
            int start = pos;
            if (peek('-')) {
                pos++;
            }
            while (pos < input.length() && Character.isDigit(input.charAt(pos))) {
                pos++;
            }
            boolean isFloat = false;
            if (pos < input.length() && input.charAt(pos) == '.') {
                isFloat = true;
                pos++;
                while (pos < input.length() && Character.isDigit(input.charAt(pos))) {
                    pos++;
                }
            }
            if (pos < input.length() && (input.charAt(pos) == 'e' || input.charAt(pos) == 'E')) {
                isFloat = true;
                pos++;
                if (pos < input.length() && (input.charAt(pos) == '+' || input.charAt(pos) == '-')) {
                    pos++;
                }
                while (pos < input.length() && Character.isDigit(input.charAt(pos))) {
                    pos++;
                }
            }
            String token = input.substring(start, pos);
            try {
                if (isFloat) {
                    return Double.parseDouble(token);
                }
                long l = Long.parseLong(token);
                return l;
            } catch (NumberFormatException err) {
                throw new NmsHostException("invalid_response", "Invalid JSON number: " + token, err);
            }
        }

        private void skipWhitespace() {
            while (pos < input.length() && Character.isWhitespace(input.charAt(pos))) {
                pos++;
            }
        }

        private boolean peek(char expected) {
            return pos < input.length() && input.charAt(pos) == expected;
        }

        private void expect(char expected) {
            if (pos >= input.length() || input.charAt(pos) != expected) {
                throw new NmsHostException("invalid_response", "Expected '" + expected + "'");
            }
            pos++;
        }
    }
}
