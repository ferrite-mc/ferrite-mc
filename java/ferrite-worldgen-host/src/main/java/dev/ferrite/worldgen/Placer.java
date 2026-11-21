package dev.ferrite.worldgen;

import com.mojang.brigadier.exceptions.CommandSyntaxException;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Base64;
import java.util.List;
import java.util.concurrent.TimeUnit;
import java.util.function.Supplier;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.BlockPos;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.level.block.Block;
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

    private record SavedBlockEntity(BlockPos pos, BlockState state, CompoundTag tag) {}

    static byte[] place(MinecraftServer server, ServerLevel level, int dimension, String argument)
            throws Exception {
        String[] parts = argument.split(";");
        String kind = parts[0];
        if (kind.equals("tree")) {
            return placeTree(server, level, dimension, parts);
        }
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

    /**
     * Evaluates a tree feature against a local block snapshot from Ferrite and restores the host's
     * chunks afterwards so sapling growth never mutates the host's private world permanently.
     */
    private static byte[] placeTree(
            MinecraftServer server, ServerLevel level, int dimension, String[] parts)
            throws Exception {
        String feature = parts[1];
        int x = Integer.parseInt(parts[2]);
        int y = Integer.parseInt(parts[3]);
        int z = Integer.parseInt(parts[4]);
        String full = "execute in " + DIMENSIONS[dimension] + " run place feature " + feature + " " + x + " " + y + " " + z;
        int centerX = x >> 4;
        int centerZ = z >> 4;
        int radius = 1;
        forceChunks(server, level, centerX, centerZ, radius, true);
        try {
            return runTree(server, level, full, centerX, centerZ, radius, parts);
        } finally {
            forceChunks(server, level, centerX, centerZ, radius, false);
        }
    }

    private static byte[] runTree(
            MinecraftServer server,
            ServerLevel level,
            String full,
            int centerX,
            int centerZ,
            int radius,
            String[] parts)
            throws Exception {
        List<Snapshot> original = new ArrayList<>();
        for (int cx = centerX - radius; cx <= centerX + radius; cx++) {
            for (int cz = centerZ - radius; cz <= centerZ + radius; cz++) {
                LevelChunk chunk = (LevelChunk) level.getChunkSource().getChunk(cx, cz, ChunkStatus.FULL, true);
                original.add(snapshot(cx, cz, chunk));
            }
        }
        List<SavedBlockEntity> savedEntities = new ArrayList<>();
        List<Snapshot> preFeature = new ArrayList<>();
        String failure;
        try {
            Supplier<String> applyAndRun = () -> {
                if (parts.length >= 12) {
                    int minX = Integer.parseInt(parts[5]);
                    int minY = Integer.parseInt(parts[6]);
                    int minZ = Integer.parseInt(parts[7]);
                    int sx = Integer.parseInt(parts[8]);
                    int sy = Integer.parseInt(parts[9]);
                    int sz = Integer.parseInt(parts[10]);
                    byte[] encoded = Base64.getDecoder().decode(parts[11]);
                    int total = sx * sy * sz;
                    int[] states = decodeRle(encoded, total);
                    int idx = 0;
                    for (int dy = 0; dy < sy; dy++) {
                        for (int dz = 0; dz < sz; dz++) {
                            for (int dx = 0; dx < sx; dx++) {
                                BlockPos at = new BlockPos(minX + dx, minY + dy, minZ + dz);
                                BlockEntity existing = level.getBlockEntity(at);
                                if (existing != null) {
                                    savedEntities.add(new SavedBlockEntity(
                                            at,
                                            level.getBlockState(at),
                                            existing.saveWithFullMetadata(level.registryAccess())));
                                }
                                BlockState target = Block.stateById(states[idx++]);
                                level.setBlock(at, target, Block.UPDATE_CLIENTS | Block.UPDATE_KNOWN_SHAPE);
                            }
                        }
                    }
                }
                for (Snapshot snap : original) {
                    preFeature.add(snapshot(snap.chunkX(), snap.chunkZ(), snap.chunk()));
                }
                try {
                    server.getCommands().getDispatcher().execute(full, server.createCommandSourceStack());
                    return null;
                } catch (CommandSyntaxException error) {
                    return error.getMessage();
                } catch (RuntimeException error) {
                    return String.valueOf(error);
                }
            };
            failure = server.submit(applyAndRun).get(120, TimeUnit.SECONDS);
            return formatDiff(level, preFeature, failure);
        } finally {
            Supplier<Boolean> restore = () -> {
                for (Snapshot snap : original) {
                    LevelChunk chunk = snap.chunk();
                    LevelChunkSection[] sections = chunk.getSections();
                    for (int s = 0; s < sections.length; s++) {
                        LevelChunkSection section = sections[s];
                        BlockState[] old = snap.sections()[s];
                        int baseY = (chunk.getMinSectionY() + s) << 4;
                        for (int i = 0; i < 4096; i++) {
                            int lx = i & 15;
                            int lz = (i >> 4) & 15;
                            int ly = i >> 8;
                            BlockState now = section.getBlockState(lx, ly, lz);
                            BlockState was = old == null ? null : old[i];
                            if (was == null ? now.isAir() : now == was) {
                                continue;
                            }
                            BlockPos at = new BlockPos((snap.chunkX() << 4) + lx, baseY + ly, (snap.chunkZ() << 4) + lz);
                            BlockState target = was == null ? net.minecraft.world.level.block.Blocks.AIR.defaultBlockState() : was;
                            level.setBlock(at, target, Block.UPDATE_CLIENTS | Block.UPDATE_KNOWN_SHAPE);
                        }
                    }
                }
                for (SavedBlockEntity saved : savedEntities) {
                    BlockEntity entity = BlockEntity.loadStatic(saved.pos(), saved.state(), saved.tag(), level.registryAccess());
                    if (entity != null) {
                        level.setBlockEntity(entity);
                    }
                }
                return true;
            };
            server.submit(restore).get(120, TimeUnit.SECONDS);
        }
    }

    private static int[] decodeRle(byte[] encoded, int total) {
        int[] out = new int[total];
        int pos = 0;
        for (int i = 0; i + 5 < encoded.length && pos < total; i += 6) {
            int count = ((encoded[i] & 0xFF) << 8) | (encoded[i + 1] & 0xFF);
            int stateId = ((encoded[i + 2] & 0xFF) << 24)
                    | ((encoded[i + 3] & 0xFF) << 16)
                    | ((encoded[i + 4] & 0xFF) << 8)
                    | (encoded[i + 5] & 0xFF);
            for (int k = 0; k < count && pos < total; k++) {
                out[pos++] = stateId;
            }
        }
        return out;
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
        return formatDiff(level, before, failure);
    }

    private static byte[] formatDiff(ServerLevel level, List<Snapshot> before, String failure) {
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
