// Original adapter around the user's official server jar. Output is local generated data only.
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.nio.file.Files;
import java.nio.file.Path;
import java.lang.reflect.Modifier;
import java.util.TreeMap;
import net.minecraft.SharedConstants;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.gamerules.GameRule;
import net.minecraft.world.level.gamerules.GameRules;

/** Bootstrap registries only: never constructs a server, world, or EULA object. */
public final class GameRuleExtractor {
    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        var entries = new TreeMap<String, GameRule<?>>();
        if (args.length == 2 && args[1].equals("--fields")) {
            // Independent verification path: enumerate actual public GameRules constants,
            // rather than using the registry iterator that supplies production output.
            for (var field : GameRules.class.getFields()) {
                if (Modifier.isStatic(field.getModifiers()) && field.getType() == GameRule.class) {
                    var rule = (GameRule<?>) field.get(null);
                    if (entries.put(rule.getIdentifier().toString(), rule) != null)
                        throw new IllegalStateException("Duplicate official constant " + field);
                }
            }
        } else {
            for (var rule : BuiltInRegistries.GAME_RULE)
                entries.put(BuiltInRegistries.GAME_RULE.getKey(rule).toString(), rule);
        }
        if (entries.isEmpty()) throw new IllegalStateException("No official game rules");
        var rules = new JsonObject();
        for (var entry : entries.entrySet()) {
            var rule = entry.getValue();
            var json = new JsonObject();
            json.addProperty("type", rule.valueClass().getName());
            Object value = rule.defaultValue();
            if (value instanceof Boolean flag) json.addProperty("default", flag);
            else if (value instanceof Integer number) json.addProperty("default", number);
            else throw new IllegalStateException("Unsupported official rule type " + rule.valueClass());
            json.addProperty("translation_key", rule.getDescriptionId());
            json.addProperty("protocol_id", BuiltInRegistries.GAME_RULE.getId(rule));
            var features = new JsonArray();
            FeatureFlags.REGISTRY.toNames(rule.requiredFeatures()).stream().map(Object::toString).sorted().forEach(features::add);
            json.add("required_features", features);
            rules.add(entry.getKey(), json);
        }
        var root = new JsonObject();
        root.addProperty("version", SharedConstants.getCurrentVersion().name());
        root.add("rules", rules);
        var output = Path.of(args[0]);
        Files.createDirectories(output.toAbsolutePath().getParent());
        Files.writeString(output, new GsonBuilder().setPrettyPrinting().create().toJson(root) + "\n");
    }
}
