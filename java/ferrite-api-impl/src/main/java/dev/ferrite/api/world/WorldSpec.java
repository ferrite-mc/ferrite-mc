package dev.ferrite.api.world;

import java.util.Objects;
import java.util.UUID;
import org.bukkit.Material;
import org.bukkit.World;

/** Immutable description of a world the facade exposes. */
public final class WorldSpec {

    private final String name;
    private final UUID uniqueId;
    private final World.Environment environment;
    private final double spawnX;
    private final double spawnY;
    private final double spawnZ;
    private final long time;
    private final Material defaultBlock;

    private WorldSpec(
            String name,
            UUID uniqueId,
            World.Environment environment,
            double spawnX,
            double spawnY,
            double spawnZ,
            long time,
            Material defaultBlock) {
        this.name = Objects.requireNonNull(name, "name");
        this.uniqueId = Objects.requireNonNull(uniqueId, "uniqueId");
        this.environment = Objects.requireNonNull(environment, "environment");
        this.spawnX = spawnX;
        this.spawnY = spawnY;
        this.spawnZ = spawnZ;
        this.time = time;
        this.defaultBlock = Objects.requireNonNull(defaultBlock, "defaultBlock");
    }

    public static WorldSpec of(String name, World.Environment environment) {
        return new WorldSpec(
                name,
                UUID.nameUUIDFromBytes(("ferrite:world:" + name).getBytes(java.nio.charset.StandardCharsets.UTF_8)),
                environment,
                0.5,
                65.0,
                0.5,
                6000L,
                Material.AIR);
    }

    public WorldSpec spawn(double x, double y, double z) {
        return new WorldSpec(name, uniqueId, environment, x, y, z, time, defaultBlock);
    }

    public WorldSpec time(long ticks) {
        return new WorldSpec(name, uniqueId, environment, spawnX, spawnY, spawnZ, ticks, defaultBlock);
    }

    public WorldSpec defaultBlock(Material material) {
        return new WorldSpec(name, uniqueId, environment, spawnX, spawnY, spawnZ, time, material);
    }

    public String name() {
        return name;
    }

    public UUID uniqueId() {
        return uniqueId;
    }

    public World.Environment environment() {
        return environment;
    }

    public double spawnX() {
        return spawnX;
    }

    public double spawnY() {
        return spawnY;
    }

    public double spawnZ() {
        return spawnZ;
    }

    public long time() {
        return time;
    }

    public Material defaultBlock() {
        return defaultBlock;
    }
}
