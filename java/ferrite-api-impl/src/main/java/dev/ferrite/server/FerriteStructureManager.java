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

    @Override public Structure createStructure() { return new CapturedStructure(); }

    /** Reads a vanilla template file from the data directory; placing it runs {@code /place template}; a missing file gives null. */
    @Override public Structure loadStructure(NamespacedKey key, boolean register) {
        Objects.requireNonNull(key, "key");
        var file = FerriteTemplate.file(key);
        if (file == null || !file.isFile()) return null;
        try {
            var structure = FerriteTemplate.load(key);
            if (register) structures.put(key, structure);
            return structure;
        } catch (IOException e) {
            throw new java.io.UncheckedIOException(e);
        }
    }
    @Override public Structure loadStructure(NamespacedKey key) { return loadStructure(key, true); }
    @Override public void saveStructure(NamespacedKey key) {
        var structure = structures.get(Objects.requireNonNull(key, "key"));
        if (structure == null) throw new IllegalStateException("Structure " + key + " is not registered");
        try {
            saveStructure(key, structure);
        } catch (IOException e) {
            throw new java.io.UncheckedIOException(e);
        }
    }
    @Override public void saveStructure(NamespacedKey key, Structure structure) throws IOException {
        saveStructure(getStructureFile(key), structure);
    }
    @Override public void deleteStructure(NamespacedKey key) throws IOException { deleteStructure(key, true); }
    @Override public void deleteStructure(NamespacedKey key, boolean unregister) throws IOException {
        var file = getStructureFile(key);
        if (unregister) structures.remove(key);
        java.nio.file.Files.deleteIfExists(file.toPath());
    }
    /** {@code <data>/data/<namespace>/structure/<key>.nbt}, where vanilla looks for templates. */
    @Override public File getStructureFile(NamespacedKey key) {
        Objects.requireNonNull(key, "key");
        var file = FerriteTemplate.file(key);
        if (file == null) throw new IllegalStateException("Ferrite: no data directory to keep structure " + key);
        return file;
    }
    @Override public Structure loadStructure(File file) throws IOException {
        try (var in = new java.io.FileInputStream(Objects.requireNonNull(file, "file"))) {
            return StructureNbt.read(in);
        }
    }
    @Override public Structure loadStructure(InputStream inputStream) throws IOException {
        return StructureNbt.read(Objects.requireNonNull(inputStream, "inputStream"));
    }
    @Override public void saveStructure(File file, Structure structure) throws IOException {
        Objects.requireNonNull(file, "file");
        var parent = file.getParentFile();
        if (parent != null) java.nio.file.Files.createDirectories(parent.toPath());
        try (var out = new java.io.FileOutputStream(file)) {
            saveStructure(out, structure);
        }
    }
    @Override public void saveStructure(OutputStream outputStream, Structure structure) throws IOException {
        StructureNbt.write(Objects.requireNonNull(outputStream, "outputStream"), captured(structure));
    }
    @Override public Structure copy(Structure structure) { return captured(structure).copy(); }

    /** The in-memory form of a structure this implementation made or loaded. */
    private static CapturedStructure captured(Structure structure) {
        Objects.requireNonNull(structure, "structure");
        if (structure instanceof CapturedStructure captured) return captured;
        if (structure instanceof FerriteTemplate template) return template.captured();
        throw notImplemented("a structure from another implementation");
    }

    private static UnsupportedOperationException notImplemented(String method) {
        return new UnsupportedOperationException("Ferrite: not implemented: StructureManager#" + method);
    }
}
