package dev.ferrite.server;

import com.google.gson.JsonParser;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.util.*;
import org.bukkit.NamespacedKey;

/** Immutable static POI definitions. Occupied seats/tickets in live worlds are not represented here. */
final class FerritePoiData {
    record Definition(int maxTickets, Set<Integer> states) {
        Definition { states = Set.copyOf(states); }
    }
    private static final class Holder {
        static final Map<NamespacedKey, Definition> DATA = load(FerritePoiData.class.getResourceAsStream("/ferrite/poi-types.json"), FerriteRegistryAccess.poiTypeKeys());
    }
    static Definition definition(NamespacedKey key) {
        return Objects.requireNonNull(Holder.DATA.get(key), "Missing generated POI " + key);
    }
    static Map<NamespacedKey, Definition> load(InputStream input, Set<String> expectedKeys) {
        if (input == null) throw new IllegalStateException("Missing generated POI definitions");
        try (var reader = new InputStreamReader(input, StandardCharsets.UTF_8)) {
            var root = JsonParser.parseReader(reader).getAsJsonObject();
            if (!root.getAsJsonPrimitive("version").isString() || !root.get("version").getAsString().equals("26.3")) throw new IllegalArgumentException("Wrong POI data version");
            int count = integer(root.get("state_count"));
            if (count <= 0) throw new IllegalArgumentException("Invalid POI block state count");
            var types = root.getAsJsonObject("types");
            if (types.size() == 0 || !types.keySet().equals(expectedKeys)) throw new IllegalArgumentException("POI keys differ from generated registry");
            var result = new HashMap<NamespacedKey, Definition>();
            for (var entry : types.entrySet()) {
                var key = NamespacedKey.fromString(entry.getKey());
                if (key == null || !key.toString().equals(entry.getKey())) throw new IllegalArgumentException("Invalid POI key");
                var value = entry.getValue().getAsJsonObject();
                int max = integer(value.get("max_tickets"));
                var states = new HashSet<Integer>();
                for (var state : value.getAsJsonArray("states")) {
                    int id = integer(state);
                    if (id < 0 || id >= count || !states.add(id)) throw new IllegalArgumentException("Invalid/duplicate POI state for " + key);
                }
                result.put(key, new Definition(max, states));
            }
            return Map.copyOf(result);
        } catch (Exception error) {
            throw new IllegalStateException("Invalid generated POI definitions: " + error.getMessage(), error);
        }
    }
    private static int integer(com.google.gson.JsonElement value) {
        if (value == null || !value.isJsonPrimitive() || !value.getAsJsonPrimitive().isNumber()) throw new IllegalArgumentException("Expected POI integer");
        return value.getAsBigDecimal().intValueExact();
    }
}
