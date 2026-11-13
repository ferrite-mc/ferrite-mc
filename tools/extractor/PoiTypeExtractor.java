// Original adapter around static official definitions; no server/world or live occupancy is created.
import com.google.gson.*;
import java.nio.file.*;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.server.Bootstrap;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.world.level.block.Block;

public final class PoiTypeExtractor {
    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        var root = new JsonObject();
        root.addProperty("version", SharedConstants.getCurrentVersion().name());
        root.addProperty("state_count", Block.BLOCK_STATE_REGISTRY.size());
        var types = new TreeMap<String, JsonObject>();
        for (var type : BuiltInRegistries.POINT_OF_INTEREST_TYPE) {
            var expected = new TreeSet<Integer>();
            type.matchingStates().forEach(state -> expected.add(Block.BLOCK_STATE_REGISTRY.getId(state)));
            // Independent official predicate read verifies the extracted matching set exhaustively.
            var actual = new TreeSet<Integer>();
            for (var state : Block.BLOCK_STATE_REGISTRY)
                if (type.is(state)) actual.add(Block.BLOCK_STATE_REGISTRY.getId(state));
            if (!actual.equals(expected)) throw new IllegalStateException("Official POI predicate/set mismatch");
            var json = new JsonObject();
            json.addProperty("max_tickets", type.maxTickets());
            var states = new JsonArray();
            expected.forEach(states::add);
            json.add("states", states);
            types.put(BuiltInRegistries.POINT_OF_INTEREST_TYPE.getKey(type).toString(), json);
        }
        var jsonTypes = new JsonObject();
        types.forEach(jsonTypes::add);
        root.add("types", jsonTypes);
        var output = Path.of(args[0]);
        Files.createDirectories(output.toAbsolutePath().getParent());
        Files.writeString(output, new GsonBuilder().setPrettyPrinting().create().toJson(root) + "\n");
    }
}
