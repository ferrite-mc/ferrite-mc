package dev.ferrite.api.block;

import dev.ferrite.api.world.FerriteWorld;
import java.util.ArrayList;
import java.util.Collection;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import org.bukkit.Bukkit;
import org.bukkit.Chunk;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.World;
import org.bukkit.block.Block;
import org.bukkit.block.BlockState;
import org.bukkit.block.data.BlockData;
import org.bukkit.entity.Entity;
import org.bukkit.inventory.ItemStack;
import org.bukkit.metadata.MetadataValue;
import org.bukkit.plugin.Plugin;

/** Snapshot {@link BlockState} implementation backed by {@link FerriteBlock}. */
@SuppressWarnings({"deprecation", "removal"})
public final class FerriteBlockState implements BlockState {

    private final FerriteWorld world;
    private final int x;
    private final int y;
    private final int z;
    private BlockData blockData;
    private byte rawData;
    private Map<String, List<MetadataValue>> metadata;

    public FerriteBlockState(FerriteBlock block) {
        Objects.requireNonNull(block, "block");
        this.world = (FerriteWorld) block.getWorld();
        this.x = block.getX();
        this.y = block.getY();
        this.z = block.getZ();
        this.blockData = block.getBlockData().clone();
    }

    public FerriteBlockState(Material material, BlockData blockData) {
        Objects.requireNonNull(material, "material");
        this.world = null;
        this.x = 0;
        this.y = 0;
        this.z = 0;
        this.blockData = blockData != null ? blockData.clone() : Bukkit.createBlockData(material);
    }

    private FerriteBlockState(
            FerriteWorld world,
            int x,
            int y,
            int z,
            BlockData blockData,
            byte rawData) {
        this.world = world;
        this.x = x;
        this.y = y;
        this.z = z;
        this.blockData = blockData;
        this.rawData = rawData;
    }

    @Override
    public Block getBlock() {
        requirePlaced();
        return world.getBlockAt(x, y, z);
    }

    @Override
    @SuppressWarnings("deprecation")
    public org.bukkit.material.MaterialData getData() {
        return new org.bukkit.material.MaterialData(getType(), rawData);
    }

    @Override
    public BlockData getBlockData() {
        return blockData.clone();
    }

    @Override
    public BlockState copy() {
        return new FerriteBlockState(null, x, y, z, blockData.clone(), rawData);
    }

    @Override
    public BlockState copy(Location location) {
        Objects.requireNonNull(location, "location");
        FerriteWorld targetWorld = null;
        if (location.getWorld() != null) {
            if (!(location.getWorld() instanceof FerriteWorld fw)) {
                throw new IllegalArgumentException(
                        "location belongs to another world implementation: " + location.getWorld().getName());
            }
            targetWorld = fw;
        }
        return new FerriteBlockState(
                targetWorld,
                location.getBlockX(),
                location.getBlockY(),
                location.getBlockZ(),
                blockData.clone(),
                rawData);
    }

    @Override
    public Material getType() {
        return blockData.getMaterial();
    }

    @Override
    public byte getLightLevel() {
        return 0;
    }

    @Override
    public World getWorld() {
        requirePlaced();
        return world;
    }

    @Override
    public int getX() {
        return x;
    }

    @Override
    public int getY() {
        return y;
    }

    @Override
    public int getZ() {
        return z;
    }

    @Override
    public Location getLocation() {
        return new Location(world, x, y, z);
    }

    @Override
    public Location getLocation(Location loc) {
        if (loc == null) {
            return null;
        }
        loc.setWorld(world);
        loc.setX(x);
        loc.setY(y);
        loc.setZ(z);
        return loc;
    }

    @Override
    public Chunk getChunk() {
        requirePlaced();
        return world.getChunkAt(Math.floorDiv(x, 16), Math.floorDiv(z, 16));
    }

    @Override
    @SuppressWarnings("deprecation")
    public void setData(org.bukkit.material.MaterialData data) {
        Objects.requireNonNull(data, "data");
        setType(data.getItemType());
        this.rawData = data.getData();
    }

    @Override
    public void setBlockData(BlockData data) {
        Objects.requireNonNull(data, "data");
        this.blockData = data.clone();
    }

    @Override
    public void setType(Material type) {
        Objects.requireNonNull(type, "type");
        if (type != blockData.getMaterial()) {
            this.blockData = Bukkit.createBlockData(type);
        }
    }

    @Override
    public boolean update() {
        return update(false, true);
    }

    @Override
    public boolean update(boolean force) {
        return update(force, true);
    }

    @Override
    public boolean update(boolean force, boolean applyPhysics) {
        if (!isPlaced()) {
            return true;
        }
        Block block = getBlock();
        if (block.getType() != getType() && !force) {
            return false;
        }
        block.setBlockData(blockData, applyPhysics);
        return true;
    }

    @Override
    @SuppressWarnings("deprecation")
    public byte getRawData() {
        return rawData;
    }

    @Override
    @SuppressWarnings("deprecation")
    public void setRawData(byte data) {
        this.rawData = data;
    }

    @Override
    public boolean isPlaced() {
        return world != null;
    }

    @Override
    public boolean isCollidable() {
        return FerriteBlock.isSolidMaterial(getType());
    }

    @Override
    public Collection<ItemStack> getDrops(ItemStack tool, Entity entity) {
        return List.of();
    }

    @Override
    public boolean isSuffocating() {
        return FerriteBlock.isOccludingMaterial(getType());
    }

    @Override
    public void setMetadata(String metadataKey, MetadataValue newMetadataValue) {
        Objects.requireNonNull(metadataKey, "metadataKey");
        Objects.requireNonNull(newMetadataValue, "newMetadataValue");
        if (metadata == null) {
            metadata = new HashMap<>();
        }
        List<MetadataValue> values = metadata.computeIfAbsent(metadataKey, ignored -> new ArrayList<>());
        Plugin owner = newMetadataValue.getOwningPlugin();
        values.removeIf(existing -> Objects.equals(existing.getOwningPlugin(), owner));
        values.add(newMetadataValue);
    }

    @Override
    public List<MetadataValue> getMetadata(String metadataKey) {
        Objects.requireNonNull(metadataKey, "metadataKey");
        if (metadata == null) {
            return List.of();
        }
        List<MetadataValue> values = metadata.get(metadataKey);
        return values == null ? List.of() : List.copyOf(values);
    }

    @Override
    public boolean hasMetadata(String metadataKey) {
        Objects.requireNonNull(metadataKey, "metadataKey");
        return metadata != null
                && metadata.containsKey(metadataKey)
                && !metadata.get(metadataKey).isEmpty();
    }

    @Override
    public void removeMetadata(String metadataKey, Plugin owningPlugin) {
        Objects.requireNonNull(metadataKey, "metadataKey");
        Objects.requireNonNull(owningPlugin, "owningPlugin");
        if (metadata == null) {
            return;
        }
        List<MetadataValue> values = metadata.get(metadataKey);
        if (values != null) {
            values.removeIf(value -> Objects.equals(value.getOwningPlugin(), owningPlugin));
            if (values.isEmpty()) {
                metadata.remove(metadataKey);
            }
        }
    }

    private void requirePlaced() {
        if (!isPlaced()) {
            throw new IllegalStateException("The blockState must be placed to call this method");
        }
    }

    @Override
    public boolean equals(Object object) {
        return object instanceof FerriteBlockState other
                && Objects.equals(world, other.world)
                && x == other.x
                && y == other.y
                && z == other.z
                && Objects.equals(blockData, other.blockData);
    }

    @Override
    public int hashCode() {
        return Objects.hash(world, x, y, z, blockData);
    }

    @Override
    public String toString() {
        return "FerriteBlockState{world="
                + (world == null ? "null" : world.getName())
                + ",x="
                + x
                + ",y="
                + y
                + ",z="
                + z
                + ",type="
                + getType()
                + "}";
    }
}
