package dev.ferrite.server;

import java.io.DataInputStream;
import java.io.File;
import java.io.IOException;
import java.util.Collection;
import java.util.List;
import java.util.Random;
import java.util.zip.GZIPInputStream;
import org.bukkit.Location;
import org.bukkit.NamespacedKey;
import org.bukkit.RegionAccessor;
import org.bukkit.block.structure.Mirror;
import org.bukkit.block.structure.StructureRotation;
import org.bukkit.entity.Entity;
import org.bukkit.persistence.PersistentDataContainer;
import org.bukkit.structure.Palette;
import org.bukkit.structure.Structure;
import org.bukkit.util.BlockTransformer;
import org.bukkit.util.BlockVector;
import org.bukkit.util.EntityTransformer;

/**
 * A structure template stored as a vanilla {@code .nbt} file. Placing it asks the server to run
 * {@code /place template}, so rotation, mirroring, integrity and every block state are applied by
 * the Rust world; the palette itself stays in the file.
 */
final class FerriteTemplate implements Structure {
    private final NamespacedKey key;
    private final BlockVector size;
    private final PersistentDataContainer data = new FerriteDataContainer();

    private FerriteTemplate(NamespacedKey key, BlockVector size) { this.key = key; this.size = size; }

    /** The template file for a key, or null when the data directory has none. */
    static File file(NamespacedKey key) {
        return Bootstrap.dataFile("data/" + key.getNamespace() + "/structure/" + key.getKey() + ".nbt");
    }

    static FerriteTemplate load(NamespacedKey key) throws IOException {
        var file = file(key);
        if (file == null || !file.isFile()) throw new IOException("No structure template " + key);
        try (var in = new DataInputStream(new GZIPInputStream(new java.io.BufferedInputStream(new java.io.FileInputStream(file))))) {
            return new FerriteTemplate(key, readSize(in));
        }
    }

    /** Reads the root compound until its {@code size} list of three ints. */
    static BlockVector readSize(DataInputStream in) throws IOException {
        if (in.readByte() != 10) throw new IOException("Not a compound");
        in.skipBytes(in.readUnsignedShort());
        while (true) {
            int type = in.readByte();
            if (type == 0) throw new IOException("Template has no size");
            var name = in.readUTF();
            if (name.equals("size") && type == 9) {
                int elements = in.readByte();
                int length = in.readInt();
                if (elements != 3 || length != 3) throw new IOException("Bad size list");
                return new BlockVector(in.readInt(), in.readInt(), in.readInt());
            }
            skip(in, type);
        }
    }

    private static void skip(DataInputStream in, int type) throws IOException {
        switch (type) {
            case 1 -> in.skipBytes(1);
            case 2 -> in.skipBytes(2);
            case 3, 5 -> in.skipBytes(4);
            case 4, 6 -> in.skipBytes(8);
            case 7 -> in.skipBytes(in.readInt());
            case 8 -> in.skipBytes(in.readUnsignedShort());
            case 9 -> {
                int elements = in.readByte();
                int length = in.readInt();
                for (int i = 0; i < length; i++) skip(in, elements);
            }
            case 10 -> {
                while (true) {
                    int inner = in.readByte();
                    if (inner == 0) break;
                    in.skipBytes(in.readUnsignedShort());
                    skip(in, inner);
                }
            }
            case 11 -> in.skipBytes(in.readInt() * 4);
            case 12 -> in.skipBytes(in.readInt() * 8);
            default -> throw new IOException("Unknown tag " + type);
        }
    }

    @Override public BlockVector getSize() { return new BlockVector(size.getBlockX(), size.getBlockY(), size.getBlockZ()); }
    @Override public List<Palette> getPalettes() { throw notImplemented("getPalettes"); }
    @Override public int getPaletteCount() { throw notImplemented("getPaletteCount"); }
    @Override public List<Entity> getEntities() { return List.of(); }
    @Override public int getEntityCount() { return 0; }
    @Override public PersistentDataContainer getPersistentDataContainer() { return data; }

    @Override public void place(Location location, boolean includeEntities, StructureRotation rotation, Mirror mirror, int palette, float integrity, Random random) {
        placeAt(location, rotation, mirror, integrity, random);
    }
    @Override public void place(Location location, boolean includeEntities, StructureRotation rotation, Mirror mirror, int palette, float integrity, Random random, Collection<BlockTransformer> blockTransformers, Collection<EntityTransformer> entityTransformers) {
        if (!blockTransformers.isEmpty() || !entityTransformers.isEmpty()) throw notImplemented("place with transformers");
        placeAt(location, rotation, mirror, integrity, random);
    }
    @Override public void place(RegionAccessor region, BlockVector location, boolean includeEntities, StructureRotation rotation, Mirror mirror, int palette, float integrity, Random random) {
        placeAt(at(region, location), rotation, mirror, integrity, random);
    }
    @Override public void place(RegionAccessor region, BlockVector location, boolean includeEntities, StructureRotation rotation, Mirror mirror, int palette, float integrity, Random random, Collection<BlockTransformer> blockTransformers, Collection<EntityTransformer> entityTransformers) {
        if (!blockTransformers.isEmpty() || !entityTransformers.isEmpty()) throw notImplemented("place with transformers");
        placeAt(at(region, location), rotation, mirror, integrity, random);
    }
    @Override public void fill(Location corner1, Location corner2, boolean includeEntities) { throw notImplemented("fill"); }
    @Override public void fill(Location corner, BlockVector size, boolean includeEntities) { throw notImplemented("fill"); }

    private static Location at(RegionAccessor region, BlockVector location) {
        if (!(region instanceof org.bukkit.World world)) throw notImplemented("place into a non-world region");
        return new Location(world, location.getX(), location.getY(), location.getZ());
    }

    /** A template file has one palette, so the palette index is not used. */
    private void placeAt(Location at, StructureRotation rotation, Mirror mirror, float integrity, Random random) {
        var world = java.util.Objects.requireNonNull(at.getWorld(), "location world");
        HostBridge.callVoid("World", world.getUID().toString(), "placeTemplate(Location,String,String,String,String,String)",
            at, key.toString(), rotationName(rotation), mirrorName(mirror),
            String.valueOf(Math.max(0f, Math.min(1f, integrity))), String.valueOf(random.nextInt()));
    }

    static String rotationName(StructureRotation rotation) {
        return switch (rotation) {
            case NONE -> "none";
            case CLOCKWISE_90 -> "clockwise_90";
            case CLOCKWISE_180 -> "180";
            case COUNTERCLOCKWISE_90 -> "counterclockwise_90";
        };
    }

    static String mirrorName(Mirror mirror) {
        return switch (mirror) {
            case NONE -> "none";
            case LEFT_RIGHT -> "left_right";
            case FRONT_BACK -> "front_back";
        };
    }

    private static UnsupportedOperationException notImplemented(String method) {
        return new UnsupportedOperationException("Ferrite: not implemented: Structure#" + method);
    }
}
