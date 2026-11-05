// Extract piston properties locally from the user's server jar. Generated data is gitignored.
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import net.minecraft.SharedConstants;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;

public final class PistonExtractor {
    public static void main(String[] args) throws Exception {
        if (args.length != 1) throw new IllegalArgumentException("expected output file");
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        StringBuilder json = new StringBuilder("[\n");
        int count = Block.BLOCK_STATE_REGISTRY.size();
        for (int id = 0; id < count; id++) {
            BlockState state = Block.BLOCK_STATE_REGISTRY.byId(id);
            if (id != 0) json.append(",\n");
            json.append("{\"reaction\":\"").append(state.getPistonPushReaction().name())
                .append("\",\"blockEntity\":").append(state.hasBlockEntity()).append('}');
        }
        json.append("\n]\n");
        Files.writeString(Path.of(args[0]), json, StandardCharsets.UTF_8);
    }
}
