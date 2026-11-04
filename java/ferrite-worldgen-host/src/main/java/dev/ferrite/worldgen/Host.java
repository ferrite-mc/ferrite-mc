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
import net.minecraft.nbt.NbtIo;
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
            if (op == OP_SHUTDOWN) {
                break;
            }
            pool.execute(() -> {
                byte[] payload;
                int status = 0;
                try {
                    payload = handle(server, op, dim, x, z);
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

    private static byte[] handle(MinecraftServer server, int op, int dim, int x, int z)
            throws IOException {
        ServerLevel level = server.getLevel(dimension(dim));
        if (level == null) {
            throw new IllegalStateException("dimension " + dim + " is not loaded");
        }
        switch (op) {
            case OP_GENERATE -> {
                ChunkAccess chunk =
                        level.getChunkSource().getChunk(x, z, ChunkStatus.FULL, true);
                var tag = SerializableChunkData.copyOf(level, chunk).write();
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
            default -> throw new IllegalArgumentException("unknown op " + op);
        }
    }
}
