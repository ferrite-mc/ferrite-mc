package dev.ferrite.worldgen;

import java.io.BufferedOutputStream;
import java.io.ByteArrayOutputStream;
import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.IOException;
import java.lang.reflect.Field;
import java.net.InetAddress;
import java.net.Socket;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicReference;
import java.util.function.Predicate;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderSet;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.NbtIo;
import net.minecraft.resources.Identifier;
import net.minecraft.tags.TagKey;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.chunk.storage.SerializableChunkData;
import net.minecraft.world.level.levelgen.Heightmap;

/**
 * Ferrite's world generation host.
 *
 * <p>Boots the Minecraft server jar the user downloaded (it is on the class path; nothing of it
 * is bundled here), then answers chunk requests from Ferrite over a loopback socket. Chunks are
 * produced by the jar's own generator and serialized by the jar's own chunk serializer. This
 * class contains no generation logic.
 *
 * <p>Arguments: {@code --port N} (Ferrite's listener on 127.0.0.1), {@code --game-port N} (a free
 * loopback port for the jar's own, locked-down game listener), {@code --seed S},
 * {@code --threads T}. The working directory holds the host's private world, properties and
 * {@code eula.txt}.
 */
public final class Host {
    private static final int OP_GENERATE = 1;
    private static final int OP_SPAWN_HEIGHT = 2;
    private static final int OP_SHUTDOWN = 3;
    /** Text-argument ops: the request carries a UTF-8 string after the coordinates. */
    private static final int OP_BIOME_AT = 4;
    private static final int OP_LOCATE_BIOME = 5;
    private static final int OP_LOCATE_STRUCTURE = 6;
    private static final int OP_STRUCTURE_AT = 7;
    private static final int OP_PLACE = 8;
    private static final int OP_LOCATE_POI = 9;
    private static final int OP_STRUCTURE_STARTS = 10;

    private Host() {}

    public static void main(String[] args) throws Exception {
        int port = 0;
        String seed = "0";
        int threads = 4;
        int gamePort = 0;
        for (int i = 0; i + 1 < args.length; i += 2) {
            switch (args[i]) {
                case "--port" -> port = Integer.parseInt(args[i + 1]);
                case "--game-port" -> gamePort = Integer.parseInt(args[i + 1]);
                case "--seed" -> seed = args[i + 1];
                case "--threads" -> threads = Integer.parseInt(args[i + 1]);
                default -> throw new IllegalArgumentException("unknown argument " + args[i]);
            }
        }
        if (port <= 0 || gamePort <= 0) {
            throw new IllegalArgumentException("--port and --game-port are required");
        }
        writeProperties(seed, gamePort);
        MinecraftServer server = boot();
        try (Socket socket = new Socket(InetAddress.getLoopbackAddress(), port)) {
            socket.setTcpNoDelay(true);
            serve(server, socket, threads);
        }
        server.halt(false);
        System.exit(0);
    }

    /**
     * Settings that make the jar behave as a chunk factory; the rest is the jar's defaults.
     *
     * <p>The jar still opens its own game port until the patcher removes it, so it is locked down:
     * loopback only, on a port Ferrite picked, with an empty enforced whitelist and no player
     * slots, so nobody can log in even from this machine. Status, query, RCON, JMX and the
     * management server are off, and the server never pauses while nobody is online (a paused
     * server could stall chunk requests).
     */
    private static void writeProperties(String seed, int gamePort) throws IOException {
        Files.writeString(
                Path.of("server.properties"),
                String.join(
                        "\n",
                        "server-ip=127.0.0.1",
                        "server-port=" + gamePort,
                        "online-mode=false",
                        "white-list=true",
                        "enforce-whitelist=true",
                        "max-players=0",
                        "enable-status=false",
                        "enable-rcon=false",
                        "enable-query=false",
                        "enable-jmx-monitoring=false",
                        "management-server-enabled=false",
                        "accepts-transfers=false",
                        "pause-when-empty-seconds=-1",
                        "max-tick-time=-1",
                        "level-name=world",
                        "level-seed=" + seed,
                        "view-distance=2",
                        "simulation-distance=2",
                        "sync-chunk-writes=false",
                        "spawn-protection=0",
                        ""));
    }

    private static MinecraftServer boot() throws Exception {
        net.minecraft.server.Main.main(new String[] {"--nogui", "--universe", "."});
        Object found = null;
        for (int attempt = 0; attempt < 1200 && found == null; attempt++) {
            found = findServer();
            if (found == null) {
                Thread.sleep(50);
            }
        }
        if (!(found instanceof MinecraftServer server)) {
            throw new IllegalStateException(
                    "the server did not start (is eula=true set in eula.txt?)");
        }
        while (!server.isReady()) {
            Thread.sleep(50);
        }
        return server;
    }

    /**
     * Finds the running server through the "Server thread" task. Needs
     * {@code --add-opens java.base/java.lang=ALL-UNNAMED}.
     */
    private static Object findServer() throws Exception {
        for (Thread thread : Thread.getAllStackTraces().keySet()) {
            if (!thread.getName().equals("Server thread")) {
                continue;
            }
            Field holderField = Thread.class.getDeclaredField("holder");
            holderField.setAccessible(true);
            Object holder = holderField.get(thread);
            Field taskField = holder.getClass().getDeclaredField("task");
            taskField.setAccessible(true);
            Object task = taskField.get(holder);
            for (Field field : task.getClass().getDeclaredFields()) {
                field.setAccessible(true);
                if (field.get(task) instanceof AtomicReference<?> reference
                        && reference.get() != null) {
                    return reference.get();
                }
            }
        }
        return null;
    }

    /**
     * A fully generated chunk, held by a ticket until {@link #release}. {@code
     * ServerChunkCache.getChunk} called off the server thread parks the main thread in {@code
     * managedBlock} until the chunk is ready, so concurrent requests nest inside each other there
     * and finish in stack order. This only schedules the request and waits on the calling thread;
     * the ticket keeps the chunk from being dropped while the main thread keeps running.
     */
    private static ChunkAccess fullChunk(MinecraftServer server, ServerLevel level, int x, int z)
            throws Exception {
        var pos = new net.minecraft.world.level.ChunkPos(x, z);
        server.submit(() -> level.getChunkSource().addTicketWithRadius(
                net.minecraft.server.level.TicketType.FORCED, pos, 0)).get();
        return level.getChunkSource()
                .getChunkFuture(x, z, ChunkStatus.FULL, true)
                .join()
                .orElseThrow(() -> new IllegalStateException("chunk " + x + "," + z + " unavailable"));
    }

    private static void release(MinecraftServer server, ServerLevel level, int x, int z) {
        var pos = new net.minecraft.world.level.ChunkPos(x, z);
        server.execute(() -> level.getChunkSource().removeTicketWithRadius(
                net.minecraft.server.level.TicketType.FORCED, pos, 0));
    }

    private static ResourceKey<Level> dimension(int id) {
        return switch (id) {
            case 0 -> Level.OVERWORLD;
            case 1 -> Level.NETHER;
            case 2 -> Level.END;
            default -> throw new IllegalArgumentException("unknown dimension " + id);
        };
    }

    private static void serve(MinecraftServer server, Socket socket, int threads)
            throws IOException {
        DataInputStream in = new DataInputStream(socket.getInputStream());
        DataOutputStream out =
                new DataOutputStream(new BufferedOutputStream(socket.getOutputStream()));
        ExecutorService pool = Executors.newFixedThreadPool(threads, runnable -> {
            Thread thread = new Thread(runnable, "ferrite-worldgen-request");
            thread.setDaemon(true);
            return thread;
        });
        // Tell Ferrite the host is ready.
        synchronized (out) {
            out.writeInt(0);
            out.writeByte(0);
            out.writeInt(0);
            out.flush();
        }
        while (true) {
            int id;
            try {
                id = in.readInt();
            } catch (java.io.EOFException closed) {
                break;
            }
            int op = in.readUnsignedByte();
            int dim = in.readUnsignedByte();
            int x = in.readInt();
            int z = in.readInt();
            byte[] raw = new byte[in.readUnsignedShort()];
            in.readFully(raw);
            String argument = new String(raw, StandardCharsets.UTF_8);
            if (op == OP_SHUTDOWN) {
                break;
            }
            pool.execute(() -> {
                byte[] payload;
                int status = 0;
                try {
                    payload = handle(server, op, dim, x, z, argument);
                } catch (Throwable error) {
                    status = 1;
                    payload = String.valueOf(error).getBytes(StandardCharsets.UTF_8);
                }
                synchronized (out) {
                    try {
                        out.writeInt(id);
                        out.writeByte(status);
                        out.writeInt(payload.length);
                        out.write(payload);
                        out.flush();
                    } catch (IOException ignored) {
                        // Ferrite went away; the read loop ends the process.
                    }
                }
            });
        }
        pool.shutdownNow();
    }

    private static byte[] text(String value) {
        return value.getBytes(StandardCharsets.UTF_8);
    }

    private static Predicate<Holder<Biome>> biomeMatcher(String spec) {
        if (spec.startsWith("#")) {
            TagKey<Biome> tag = TagKey.create(Registries.BIOME, Identifier.parse(spec.substring(1)));
            return holder -> holder.is(tag);
        }
        Identifier id = Identifier.parse(spec);
        return holder -> holder.is(id);
    }

    private static String biomeName(Holder<Biome> holder) {
        return holder.unwrapKey().map(key -> key.identifier().toString()).orElse("");
    }

    /** {@code spec;radius;x;y;z}: the nearest matching biome, or an empty reply. */
    private static byte[] locateBiome(ServerLevel level, String argument) {
        String[] parts = argument.split(";");
        BlockPos origin = new BlockPos(
                Integer.parseInt(parts[2]), Integer.parseInt(parts[3]), Integer.parseInt(parts[4]));
        var found = level.findClosestBiome3d(
                biomeMatcher(parts[0]), origin, Integer.parseInt(parts[1]), 32, 64);
        if (found == null) {
            return new byte[0];
        }
        BlockPos at = found.getFirst();
        return text(at.getX() + " " + at.getY() + " " + at.getZ() + " " + biomeName(found.getSecond()));
    }

    /**
     * {@code spec;radius;x;y;z}: the nearest point of interest of that type id or tag, or an empty
     * reply. Like the jar's own /locate poi it only knows points of interest of chunks that were
     * generated and stored.
     */
    private static byte[] locatePoi(MinecraftServer server, ServerLevel level, String argument)
            throws Exception {
        String[] parts = argument.split(";");
        BlockPos origin = new BlockPos(
                Integer.parseInt(parts[2]), Integer.parseInt(parts[3]), Integer.parseInt(parts[4]));
        int radius = Integer.parseInt(parts[1]);
        java.util.function.Predicate<Holder<net.minecraft.world.entity.ai.village.poi.PoiType>> match;
        if (parts[0].startsWith("#")) {
            TagKey<net.minecraft.world.entity.ai.village.poi.PoiType> tag = TagKey.create(
                    Registries.POINT_OF_INTEREST_TYPE, Identifier.parse(parts[0].substring(1)));
            match = holder -> holder.is(tag);
        } else {
            Identifier id = Identifier.parse(parts[0]);
            match = holder -> holder.is(id);
        }
        var finalMatch = match;
        java.util.function.Supplier<byte[]> search = () -> {
            var found = level.getPoiManager().findClosestWithType(
                    finalMatch, origin, radius,
                    net.minecraft.world.entity.ai.village.poi.PoiManager.Occupancy.ANY);
            if (found.isEmpty()) {
                return new byte[0];
            }
            BlockPos at = found.get().getSecond();
            return text(at.getX() + " " + at.getY() + " " + at.getZ());
        };
        return server.submit(search).get(120, java.util.concurrent.TimeUnit.SECONDS);
    }

    /** {@code spec;radius;x;z}: the nearest structure of that id or tag, or an empty reply. */
    private static byte[] locateStructure(ServerLevel level, String argument) {
        String[] parts = argument.split(";");
        int radius = Integer.parseInt(parts[1]);
        BlockPos origin = new BlockPos(Integer.parseInt(parts[2]), 64, Integer.parseInt(parts[3]));
        BlockPos at;
        if (parts[0].startsWith("#")) {
            TagKey<Structure> tag =
                    TagKey.create(Registries.STRUCTURE, Identifier.parse(parts[0].substring(1)));
            at = level.findNearestMapStructure(tag, origin, radius, false);
        } else {
            var registry = level.registryAccess().lookupOrThrow(Registries.STRUCTURE);
            Holder<Structure> holder = registry.get(Identifier.parse(parts[0]))
                    .orElseThrow(() -> new IllegalArgumentException("unknown structure " + parts[0]));
            var found = level.getChunkSource().getGenerator()
                    .findNearestMapStructure(level, HolderSet.direct(holder), origin, radius, false);
            at = found == null ? null : found.getFirst();
        }
        return at == null ? new byte[0] : text(at.getX() + " " + at.getY() + " " + at.getZ());
    }

    /** {@code id x y z}: whether a piece of that structure covers the block. */
    private static byte[] structureAt(ServerLevel level, String argument) {
        String[] parts = argument.split(" ");
        BlockPos pos = new BlockPos(
                Integer.parseInt(parts[1]), Integer.parseInt(parts[2]), Integer.parseInt(parts[3]));
        level.getChunkSource().getChunk(pos.getX() >> 4, pos.getZ() >> 4, ChunkStatus.FULL, true);
        var registry = level.registryAccess().lookupOrThrow(Registries.STRUCTURE);
        Holder<Structure> holder = registry.get(Identifier.parse(parts[0]))
                .orElseThrow(() -> new IllegalArgumentException("unknown structure " + parts[0]));
        var start = level.structureManager().getStructureWithPieceAt(pos.getX(), pos.getY(), pos.getZ(), holder.value());
        return text(start.isValid() ? "1" : "0");
    }

    /** One line per structure that starts in the chunk: `id minX minY minZ maxX maxY maxZ`. */
    private static byte[] structureStarts(ServerLevel level, int x, int z) {
        ChunkAccess chunk = level.getChunkSource().getChunk(x, z, ChunkStatus.FULL, true);
        var registry = level.registryAccess().lookupOrThrow(Registries.STRUCTURE);
        StringBuilder out = new StringBuilder();
        chunk.getAllStarts().forEach((structure, start) -> {
            if (!start.isValid()) return;
            var key = registry.getKey(structure);
            if (key == null) return;
            var box = start.getBoundingBox();
            out.append(key).append(' ')
                    .append(box.minX()).append(' ').append(box.minY()).append(' ').append(box.minZ()).append(' ')
                    .append(box.maxX()).append(' ').append(box.maxY()).append(' ').append(box.maxZ()).append('\n');
        });
        return text(out.toString());
    }

    private static byte[] handle(
            MinecraftServer server, int op, int dim, int x, int z, String argument)
            throws Exception {
        ServerLevel level = server.getLevel(dimension(dim));
        if (level == null) {
            throw new IllegalStateException("dimension " + dim + " is not loaded");
        }
        switch (op) {
            case OP_GENERATE -> {
                ChunkAccess chunk = fullChunk(server, level, x, z);
                net.minecraft.nbt.CompoundTag tag;
                try {
                    tag = SerializableChunkData.copyOf(level, chunk).write();
                } finally {
                    release(server, level, x, z);
                }
                ByteArrayOutputStream bytes = new ByteArrayOutputStream(48 * 1024);
                try (DataOutputStream data = new DataOutputStream(bytes)) {
                    NbtIo.write(tag, data);
                }
                return bytes.toByteArray();
            }
            case OP_SPAWN_HEIGHT -> {
                level.getChunkSource().getChunk(x >> 4, z >> 4, ChunkStatus.FULL, true);
                int y = level.getHeight(Heightmap.Types.MOTION_BLOCKING_NO_LEAVES, x, z);
                return new byte[] {(byte) (y >>> 24), (byte) (y >>> 16), (byte) (y >>> 8), (byte) y};
            }
            case OP_BIOME_AT -> {
                String[] parts = argument.split(" ");
                BlockPos pos = new BlockPos(
                        Integer.parseInt(parts[0]), Integer.parseInt(parts[1]), Integer.parseInt(parts[2]));
                level.getChunkSource().getChunk(pos.getX() >> 4, pos.getZ() >> 4, ChunkStatus.FULL, true);
                return text(biomeName(level.getBiome(pos)));
            }
            case OP_LOCATE_BIOME -> {
                return locateBiome(level, argument);
            }
            case OP_LOCATE_STRUCTURE -> {
                return locateStructure(level, argument);
            }
            case OP_STRUCTURE_AT -> {
                return structureAt(level, argument);
            }
            case OP_STRUCTURE_STARTS -> {
                return structureStarts(level, x, z);
            }
            case OP_LOCATE_POI -> {
                return locatePoi(server, level, argument);
            }
            case OP_PLACE -> {
                return Placer.place(server, level, dim, argument);
            }
            default -> throw new IllegalArgumentException("unknown op " + op);
        }
    }
}
