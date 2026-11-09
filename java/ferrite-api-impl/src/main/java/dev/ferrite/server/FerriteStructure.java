package dev.ferrite.server;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.concurrent.ConcurrentHashMap;
import java.util.regex.Pattern;
import org.bukkit.NamespacedKey;
import org.bukkit.generator.structure.StructureType;

/** A vanilla structure identified by its registry key; placement queries go to the world generation host. */
final class FerriteStructure extends org.bukkit.generator.structure.Structure {
    private static final Pattern TYPE = Pattern.compile("\"type\"\\s*:\\s*\"([^\"]+)\"");
    private static final ConcurrentHashMap<NamespacedKey, StructureType> TYPES = new ConcurrentHashMap<>();
    private final NamespacedKey key;

    FerriteStructure(NamespacedKey key) { this.key = key; }

    /** The structure type is the {@code type} field of the vanilla worldgen/structure JSON. */
    @Override public StructureType getStructureType() {
        var cached = TYPES.get(key);
        if (cached != null) return cached;
        var file = Bootstrap.dataFile("data/" + key.getNamespace() + "/worldgen/structure/" + key.getKey() + ".json");
        if (file == null || !file.isFile()) throw new UnsupportedOperationException("Ferrite: no structure data for " + key);
        try {
            var matcher = TYPE.matcher(Files.readString(file.toPath(), StandardCharsets.UTF_8));
            if (!matcher.find()) throw new UnsupportedOperationException("Ferrite: structure " + key + " has no type");
            var type = type(matcher.group(1));
            var previous = TYPES.putIfAbsent(key, type);
            return previous == null ? type : previous;
        } catch (IOException e) {
            throw new java.io.UncheckedIOException(e);
        }
    }

    private static StructureType type(String id) {
        return switch (id) {
            case "minecraft:jigsaw" -> StructureType.JIGSAW;
            case "minecraft:buried_treasure" -> StructureType.BURIED_TREASURE;
            case "minecraft:desert_pyramid" -> StructureType.DESERT_PYRAMID;
            case "minecraft:end_city" -> StructureType.END_CITY;
            case "minecraft:fortress" -> StructureType.FORTRESS;
            case "minecraft:igloo" -> StructureType.IGLOO;
            case "minecraft:jungle_temple" -> StructureType.JUNGLE_TEMPLE;
            case "minecraft:mineshaft" -> StructureType.MINESHAFT;
            case "minecraft:nether_fossil" -> StructureType.NETHER_FOSSIL;
            case "minecraft:ocean_monument" -> StructureType.OCEAN_MONUMENT;
            case "minecraft:ocean_ruin" -> StructureType.OCEAN_RUIN;
            case "minecraft:shipwreck" -> StructureType.SHIPWRECK;
            case "minecraft:stronghold" -> StructureType.STRONGHOLD;
            case "minecraft:swamp_hut" -> StructureType.SWAMP_HUT;
            case "minecraft:woodland_mansion" -> StructureType.WOODLAND_MANSION;
            default -> throw new UnsupportedOperationException("Ferrite: unknown structure type " + id);
        };
    }

    @Override public NamespacedKey getKey() { return key; }
    @Override public int hashCode() { return key.hashCode(); }
    @Override public boolean equals(Object other) { return other instanceof FerriteStructure s && s.key.equals(key); }
    @Override public String toString() { return key.toString(); }
}
