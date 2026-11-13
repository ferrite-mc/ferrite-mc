// Original adapter: decode the user's generated data through the official codec, without a world/server.
import com.google.gson.*;
import com.mojang.serialization.JsonOps;
import java.nio.file.*;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.damagesource.*;

public final class DamageTypeExtractor {
    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        var root = new JsonObject();
        root.addProperty("version", SharedConstants.getCurrentVersion().name());
        var defaults = new DamageType("ferrite_probe", DamageScaling.NEVER, 0);
        var defaultJson = new JsonObject();
        defaultJson.addProperty("effects", defaults.effects().getSerializedName());
        defaultJson.addProperty("death_message_type", defaults.deathMessageType().getSerializedName());
        root.add("defaults", defaultJson);
        var effects = new JsonObject();
        for (var effect : DamageEffects.values())
            effects.addProperty(effect.getSerializedName(), BuiltInRegistries.SOUND_EVENT.getKey(effect.sound()).toString());
        root.add("effects", effects);
        var types = new JsonObject();
        Path directory = Path.of(args[0]);
        try (var files = Files.walk(directory)) {
            for (var file : files.filter(p -> p.toString().endsWith(".json")).sorted().toList()) {
                var input = JsonParser.parseString(Files.readString(file));
                var type = DamageType.DIRECT_CODEC.parse(JsonOps.INSTANCE, input).getOrThrow();
                var json = new JsonObject();
                json.addProperty("message_id", type.msgId());
                json.addProperty("exhaustion", type.exhaustion());
                json.addProperty("scaling", type.scaling().getSerializedName());
                json.addProperty("effects", type.effects().getSerializedName());
                json.addProperty("death_message_type", type.deathMessageType().getSerializedName());
                var key = "minecraft:" + directory.relativize(file).toString().replace('\\', '/').replaceAll("\\.json$", "");
                types.add(key, json);
            }
        }
        if (types.size() == 0) throw new IllegalStateException("No generated damage types");
        root.add("types", types);
        var output = Path.of(args[1]);
        Files.createDirectories(output.toAbsolutePath().getParent());
        Files.writeString(output, new GsonBuilder().setPrettyPrinting().create().toJson(root) + "\n");
    }
}
