package dev.ferrite.worldgen;

import com.mojang.brigadier.exceptions.CommandSyntaxException;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.TimeUnit;
import java.util.function.Supplier;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.BlockPos;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.LevelChunk;
import net.minecraft.world.level.chunk.LevelChunkSection;
import net.minecraft.world.level.chunk.status.ChunkStatus;

/**
 * Runs the jar's own {@code /place feature|jigsaw|structure} command on the host's private world
 * and reports which blocks it changed. This class contains no placement logic: the command and
 * everything behind it is the user's jar. Ferrite applies the returned block list to its own
 * world.
 *
 * <p>Argument: {@code kind;a[;b;depth];x;y;z} where kind is {@code feature}, {@code structure} or
 * {@code jigsaw} (for jigsaw: {@code pool;target;depth}). Reply: the first line is {@code ok} or
 * {@code fail <message>}; every further line is {@code x y z block[prop=value,...]}.
 */
final class Placer {
    private Placer() {}

    private static final String[] DIMENSIONS = {"minecraft:overworld", "minecraft:the_nether", "minecraft:the_end"};

    /** A before-image of one chunk: a copy of every section's block states. */
    private record Snapshot(int chunkX, int chunkZ, LevelChunk chunk, BlockState[][] sections) {}

    static byte[] place(MinecraftServer server, ServerLevel level, int dimension, String argument)
            throws Exception {
        String[] parts = argument.split(";");
        String kind = parts[0];
        int n = parts.length;
        int x = Integer.parseInt(parts[n - 3]);
        int y = Integer.parseInt(parts[n - 2]);
        int z = Integer.parseInt(parts[n - 1]);
        String command;
        int radius;
        switch (kind) {
            case "feature" -> {
                command = "place feature " + parts[1] + " " + x + " " + y + " " + z;
                radius = 1;
            }
            case "structure" -> {
                command = "place structure " + parts[1] + " " + x + " " + y + " " + z;
                radius = 4;
            }
            case "jigsaw" -> {
                int depth = Integer.parseInt(parts[3]);
                command = "place jigsaw " + parts[1] + " " + parts[2] + " " + depth + " " + x + " " + y + " " + z;
                radius = Math.min(8, 1 + depth);
            }
            default -> throw new IllegalArgumentException("unknown place kind " + kind);
        }
        String full = "execute in " + DIMENSIONS[dimension] + " run " + command;
        int centerX = x >> 4;
        int centerZ = z >> 4;

        // Chunks must be fully generated and loaded before the command runs (vanilla checks it),
        // and must stay loaded while it runs, so they are force-loaded for the duration.
        forceChunks(server, level, centerX, centerZ, radius, true);
        try {
            return run(server, level, full, centerX, centerZ, radius);
        } finally {
            forceChunks(server, level, centerX, centerZ, radius, false);
        }
    }

    private static void forceChunks(
            MinecraftServer server, ServerLevel level, int centerX, int centerZ, int radius, boolean on)
            throws Exception {
        Supplier<Boolean> change = () -> {
            for (int cx = centerX - radius; cx <= centerX + radius; cx++) {
                for (int cz = centerZ - radius; cz <= centerZ + radius; cz++) {
                    level.setChunkForced(cx, cz, on);
                }
            }
            return true;
        };
        server.submit(change).get(120, TimeUnit.SECONDS);
        if (on) {
            for (int cx = centerX - radius; cx <= centerX + radius; cx++) {
                for (int cz = centerZ - radius; cz <= centerZ + radius; cz++) {
                    level.getChunkSource().getChunk(cx, cz, ChunkStatus.FULL, true);
                }
            }
        }
    }

    private static byte[] run(
            MinecraftServer server, ServerLevel level, String full, int centerX, int centerZ, int radius)
            throws Exception {
        List<Snapshot> before = new ArrayList<>();
        for (int cx = centerX - radius; cx <= centerX + radius; cx++) {
            for (int cz = centerZ - radius; cz <= centerZ + radius; cz++) {
                LevelChunk chunk = (LevelChunk) level.getChunkSource().getChunk(cx, cz, ChunkStatus.FULL, true);
                before.add(snapshot(cx, cz, chunk));
            }
        }

        Supplier<String> run = () -> {
            try {
                server.getCommands().getDispatcher().execute(full, server.createCommandSourceStack());
                return null;
            } catch (CommandSyntaxException error) {
                return error.getMessage();
            } catch (RuntimeException error) {
                return String.valueOf(error);
            }
        };
        String failure = server.submit(run).get(120, TimeUnit.SECONDS);

        StringBuilder out = new StringBuilder();
        if (failure != null) {
            out.append("fail ").append(failure.replace('\n', ' ')).append('\n');
            return out.toString().getBytes(StandardCharsets.UTF_8);
        }
        out.append("ok\n");
        StringBuilder entities = new StringBuilder();
        int changed = 0;
        for (Snapshot snap : before) {
            LevelChunk chunk = snap.chunk();
            LevelChunkSection[] sections = chunk.getSections();
            for (int s = 0; s < sections.length; s++) {
                LevelChunkSection section = sections[s];
                BlockState[] old = snap.sections()[s];
                if (old == null && section.hasOnlyAir()) {
                    continue;
                }
                int baseY = (chunk.getMinSectionY() + s) << 4;
                for (int i = 0; i < 4096; i++) {
                    int lx = i & 15;
                    int lz = (i >> 4) & 15;
                    int ly = i >> 8;
                    BlockState now = section.getBlockState(lx, ly, lz);
                    BlockState was = old == null ? null : old[i];
                    if (was == null ? now.isAir() : was == now) {
                        continue;
                    }
                    out.append((snap.chunkX() << 4) + lx).append(' ')
                            .append(baseY + ly).append(' ')
                            .append((snap.chunkZ() << 4) + lz).append(' ')
                            .append(describe(now)).append('\n');
                    changed++;
                    // Chest contents, signs and the like travel as `be x y z <snbt>` lines.
                    BlockPos at = new BlockPos(
                            (snap.chunkX() << 4) + lx, baseY + ly, (snap.chunkZ() << 4) + lz);
                    BlockEntity entity = chunk.getBlockEntity(at);
                    if (entity != null) {
                        entities.append("be ").append(at.getX()).append(' ').append(at.getY())
                                .append(' ').append(at.getZ()).append(' ')
                                .append(entity.saveWithFullMetadata(level.registryAccess()).toString()
                                        .replace('\n', ' '))
                                .append('\n');
                    }
                }
            }
        }
        if (changed == 0) {
            return "fail nothing changed\n".getBytes(StandardCharsets.UTF_8);
        }
        out.append(entities);
        return out.toString().getBytes(StandardCharsets.UTF_8);
    }

    private static Snapshot snapshot(int cx, int cz, LevelChunk chunk) {
        LevelChunkSection[] sections = chunk.getSections();
        BlockState[][] copy = new BlockState[sections.length][];
        for (int s = 0; s < sections.length; s++) {
            LevelChunkSection section = sections[s];
            if (section.hasOnlyAir()) {
                continue;
            }
            BlockState[] states = new BlockState[4096];
            for (int i = 0; i < 4096; i++) {
                states[i] = section.getBlockState(i & 15, i >> 8, (i >> 4) & 15);
            }
            copy[s] = states;
        }
        return new Snapshot(cx, cz, chunk, copy);
    }

    /** {@code namespace:name[prop=value,...]} for a block state. */
    private static String describe(BlockState state) {
        StringBuilder text = new StringBuilder(BuiltInRegistries.BLOCK.getKey(state.getBlock()).toString());
        List<String> properties = new ArrayList<>();
        state.getValues().forEach(value -> properties.add(value.toString()));
        if (!properties.isEmpty()) {
            text.append('[').append(String.join(",", properties)).append(']');
        }
        return text.toString();
    }
}
