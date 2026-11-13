package dev.ferrite.server;

import com.google.gson.JsonElement;
import com.google.gson.JsonParser;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.util.Map;
import java.util.Set;
import org.bukkit.FeatureFlag;
import org.bukkit.NamespacedKey;

/** Strict immutable definitions extracted at build time; no server/world or native host dependency. */
final class FerriteGameRuleData {
    record Definition(NamespacedKey key, Class<?> type, Object defaultValue, String translationKey, Set<FeatureFlag> features) {}

    static Map<NamespacedKey, Definition> load(InputStream input, Set<String> expectedKeys) {
        if (input == null) throw new IllegalStateException("Missing generated game-rule defaults");
        try (var reader = new InputStreamReader(input, StandardCharsets.UTF_8)) {
            var root = JsonParser.parseReader(reader).getAsJsonObject();
            if (!"26.3".equals(string(root.get("version")))) throw new IllegalArgumentException("Wrong game-rule data version");
            var rules = root.getAsJsonObject("rules");
            if (!rules.keySet().equals(expectedKeys) || rules.size() == 0)
                throw new IllegalArgumentException("Game-rule keys differ from generated official registry");
            var definitions = new java.util.HashMap<NamespacedKey, Definition>();
            for (var entry : rules.entrySet()) {
                var key = NamespacedKey.fromString(entry.getKey());
                if (key == null || !key.toString().equals(entry.getKey())) throw new IllegalArgumentException("Invalid game-rule key " + entry.getKey());
                var value = entry.getValue().getAsJsonObject();
                var defaultJson = value.getAsJsonPrimitive("default");
                Class<?> type;
                Object defaultValue;
                switch (string(value.get("type"))) {
                    case "java.lang.Boolean" -> {
                        if (!defaultJson.isBoolean()) throw new IllegalArgumentException("Expected Boolean for " + key);
                        type = Boolean.class;
                        defaultValue = defaultJson.getAsBoolean();
                    }
                    case "java.lang.Integer" -> {
                        if (!defaultJson.isNumber()) throw new IllegalArgumentException("Expected Integer for " + key);
                        type = Integer.class;
                        defaultValue = defaultJson.getAsBigDecimal().intValueExact();
                    }
                    default -> throw new IllegalArgumentException("Unsupported game-rule type for " + key);
                }
                var features = new java.util.HashSet<FeatureFlag>();
                for (var feature : value.getAsJsonArray("required_features")) {
                    var featureKey = NamespacedKey.fromString(string(feature));
                    var flag = featureKey == null ? null : FeatureFlag.ALL_FLAGS.value(featureKey);
                    if (flag == null || !features.add(flag)) throw new IllegalArgumentException("Invalid/duplicate feature for " + key);
                }
                definitions.put(key, new Definition(key, type, defaultValue, string(value.get("translation_key")), Set.copyOf(features)));
            }
            return Map.copyOf(definitions);
        } catch (Exception error) {
            throw new IllegalStateException("Cannot read generated game-rule defaults: " + error.getMessage(), error);
        }
    }

    private static String string(JsonElement element) {
        if (element == null || !element.isJsonPrimitive() || !element.getAsJsonPrimitive().isString() || element.getAsString().isEmpty())
            throw new IllegalArgumentException("Expected nonempty game-rule string");
        return element.getAsString();
    }
}
