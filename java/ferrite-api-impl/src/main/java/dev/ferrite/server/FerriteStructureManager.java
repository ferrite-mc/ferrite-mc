package dev.ferrite.server;

import java.io.File;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Random;
import java.util.concurrent.ConcurrentHashMap;
import org.bukkit.Location;
import org.bukkit.NamespacedKey;
import org.bukkit.RegionAccessor;
import org.bukkit.block.structure.Mirror;
import org.bukkit.block.structure.StructureRotation;
import org.bukkit.entity.Entity;
import org.bukkit.persistence.PersistentDataContainer;
import org.bukkit.structure.Palette;
import org.bukkit.structure.Structure;
import org.bukkit.structure.StructureManager;
import org.bukkit.util.BlockTransformer;
import org.bukkit.util.BlockVector;
import org.bukkit.util.EntityTransformer;

/**
 * Structures plugins register by name (Server#getStructureManager), kept in memory.
 *
 * <p>The registered structures are plugin-owned objects; reading or writing structure files and
 * placing blocks still belongs to the Rust world.
 */
final class FerriteStructureManager implements StructureManager {
    private final Map<NamespacedKey, Structure> structures = new ConcurrentHashMap<>();

    @Override public Map<NamespacedKey, Structure> getStructures() { return Map.copyOf(structures); }

    @Override public Structure getStructure(NamespacedKey key) {
        Objects.requireNonNull(key, "key");
        return structures.get(key);
    }

    @Override public Structure registerStructure(NamespacedKey key, Structure structure) {
        Objects.requireNonNull(key, "key");
        Objects.requireNonNull(structure, "structure");
        return structures.put(key, structure);
    }

    @Override public Structure unregisterStructure(NamespacedKey key) {
        Objects.requireNonNull(key, "key");
        return structures.remove(key);
    }

    @Override public Structure createStructure() { return new EmptyStructure(); }

    @Override public Structure loadStructure(NamespacedKey key, boolean register) { throw notImplemented("loadStructure"); }
    @Override public Structure loadStructure(NamespacedKey key) { throw notImplemented("loadStructure"); }
    @Override public void saveStructure(NamespacedKey key) { throw notImplemented("saveStructure"); }
    @Override public void saveStructure(NamespacedKey key, Structure structure) throws IOException { throw notImplemented("saveStructure"); }
    @Override public void deleteStructure(NamespacedKey key) throws IOException { throw notImplemented("deleteStructure"); }
    @Override public void deleteStructure(NamespacedKey key, boolean unregister) throws IOException { throw notImplemented("deleteStructure"); }
    @Override public File getStructureFile(NamespacedKey key) { throw notImplemented("getStructureFile"); }
    @Override public Structure loadStructure(File file) throws IOException { throw notImplemented("loadStructure"); }
    @Override public Structure loadStructure(InputStream inputStream) throws IOException { throw notImplemented("loadStructure"); }
    @Override public void saveStructure(File file, Structure structure) throws IOException { throw notImplemented("saveStructure"); }
    @Override public void saveStructure(OutputStream outputStream, Structure structure) throws IOException { throw notImplemented("saveStructure"); }
    @Override public Structure copy(Structure structure) { throw notImplemented("copy"); }

    private static UnsupportedOperationException notImplemented(String method) {
        return new UnsupportedOperationException("Ferrite: not implemented: StructureManager#" + method);
    }

    /** An empty structure: no palettes, no entities and a zero size until the world fills it. */
    private static final class EmptyStructure implements Structure {
        private final PersistentDataContainer data = new FerriteDataContainer();

        @Override public BlockVector getSize() { return new BlockVector(0, 0, 0); }
        @Override public List<Palette> getPalettes() { return List.of(); }
        @Override public int getPaletteCount() { return 0; }
        @Override public List<Entity> getEntities() { return List.of(); }
        @Override public int getEntityCount() { return 0; }
        @Override public PersistentDataContainer getPersistentDataContainer() { return data; }

        @Override public void place(Location location, boolean includeEntities, StructureRotation rotation, Mirror mirror, int palette, float integrity, Random random) { throw notImplemented("place"); }
        @Override public void place(Location location, boolean includeEntities, StructureRotation rotation, Mirror mirror, int palette, float integrity, Random random, java.util.Collection<BlockTransformer> blockTransformers, java.util.Collection<EntityTransformer> entityTransformers) { throw notImplemented("place"); }
        @Override public void place(RegionAccessor region, BlockVector location, boolean includeEntities, StructureRotation rotation, Mirror mirror, int palette, float integrity, Random random) { throw notImplemented("place"); }
        @Override public void place(RegionAccessor region, BlockVector location, boolean includeEntities, StructureRotation rotation, Mirror mirror, int palette, float integrity, Random random, java.util.Collection<BlockTransformer> blockTransformers, java.util.Collection<EntityTransformer> entityTransformers) { throw notImplemented("place"); }
        @Override public void fill(Location corner1, Location corner2, boolean includeEntities) { throw notImplemented("fill"); }
        @Override public void fill(Location corner, BlockVector size, boolean includeEntities) { throw notImplemented("fill"); }

        private static UnsupportedOperationException notImplemented(String method) {
            return new UnsupportedOperationException("Ferrite: not implemented: Structure#" + method);
        }
    }
}
