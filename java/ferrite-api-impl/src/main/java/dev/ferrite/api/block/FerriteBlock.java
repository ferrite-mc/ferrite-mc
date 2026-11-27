package dev.ferrite.api.block;

import dev.ferrite.api.world.FerriteWorld;
import java.util.Objects;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.World;
import org.bukkit.block.Block;
import org.bukkit.block.BlockFace;
import org.bukkit.block.BlockState;
import org.bukkit.block.data.BlockData;

/** A live block handle for the supported material read/write slice. */
public final class FerriteBlock extends UnsupportedBlockBase {

    private final FerriteWorld world;
    private final int x;
    private final int y;
    private final int z;

    public FerriteBlock(FerriteWorld world, int x, int y, int z) {
        this.world = world;
        this.x = x;
        this.y = y;
        this.z = z;
    }

    @Override
    public Material getType() {
        return world.blockStore().getType(x, y, z);
    }

    @Override
    public void setType(Material type) {
        world.blockStore().setType(x, y, z, type);
    }

    @Override
    public void setType(Material type, boolean applyPhysics) {
        setType(type);
    }

    @Override
    public org.bukkit.Chunk getChunk() {
        return world.getChunkAt(x >> 4, z >> 4);
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
        loc.setYaw(0.0f);
        loc.setPitch(0.0f);
        return loc;
    }

    @Override
    public BlockFace getFace(Block block) {
        if (block == null || block.getWorld() != world) {
            return null;
        }
        int dx = block.getX() - x;
        int dy = block.getY() - y;
        int dz = block.getZ() - z;
        for (BlockFace face : BlockFace.values()) {
            if (face.getModX() == dx && face.getModY() == dy && face.getModZ() == dz) {
                return face;
            }
        }
        return null;
    }

    @Override
    public byte getLightLevel() {
        return (byte) Math.max(getLightFromSky(), getLightFromBlocks());
    }

    @Override
    public byte getLightFromSky() {
        return isEmpty() ? (byte) 15 : (byte) 0;
    }

    @Override
    public byte getLightFromBlocks() {
        Material m = getType();
        if (m == Material.TORCH || m == Material.WALL_TORCH || m == Material.GLOWSTONE
                || m == Material.SEA_LANTERN || m == Material.LANTERN || m == Material.LAVA
                || m == Material.SHROOMLIGHT || m == Material.JACK_O_LANTERN) {
            return 15;
        }
        return 0;
    }

    @Override
    public boolean isBlockPowered() {
        return getType() == Material.REDSTONE_BLOCK || getType() == Material.REDSTONE_TORCH;
    }

    @Override
    public boolean isBlockIndirectlyPowered() {
        if (isBlockPowered()) {
            return true;
        }
        for (BlockFace f : new BlockFace[] {
            BlockFace.NORTH, BlockFace.SOUTH, BlockFace.EAST, BlockFace.WEST, BlockFace.UP, BlockFace.DOWN
        }) {
            if (getRelative(f).isBlockPowered()) {
                return true;
            }
        }
        return false;
    }

    @Override
    public boolean isBlockFacePowered(BlockFace face) {
        Objects.requireNonNull(face, "face");
        return getRelative(face).isBlockPowered();
    }

    @Override
    public boolean isBlockFaceIndirectlyPowered(BlockFace face) {
        Objects.requireNonNull(face, "face");
        return getRelative(face).isBlockIndirectlyPowered();
    }

    @Override
    public int getBlockPower(BlockFace face) {
        Objects.requireNonNull(face, "face");
        return isBlockFacePowered(face) ? 15 : 0;
    }

    /** Vanilla-passable blocks this facade models (air, fluids, plants, rails, cobwebs). */
    public static boolean isPassableMaterial(Material m) {
        return m == Material.AIR
                || m == Material.CAVE_AIR
                || m == Material.VOID_AIR
                || m == Material.WATER
                || m == Material.LAVA
                || m == Material.TORCH
                || m == Material.WALL_TORCH
                || m == Material.SOUL_TORCH
                || m == Material.SOUL_WALL_TORCH
                || m == Material.REDSTONE_TORCH
                || m == Material.REDSTONE_WALL_TORCH
                || m == Material.FIRE
                || m == Material.SOUL_FIRE
                || m == Material.SHORT_GRASS
                || m == Material.TALL_GRASS
                || m == Material.FERN
                || m == Material.DANDELION
                || m == Material.POPPY
                || m == Material.VINE
                || m == Material.RAIL
                || m == Material.POWERED_RAIL
                || m == Material.DETECTOR_RAIL
                || m == Material.ACTIVATOR_RAIL
                || m == Material.COBWEB;
    }

    /**
     * Whether the material has a collision, without going through {@link Material#isSolid()}:
     * the block-type registry proxy uses this helper while the API itself is initializing.
     */
    public static boolean isSolidMaterial(Material m) {
        return m != null && !isPassableMaterial(m);
    }

    /** Whether the solid material hides light and suffocates (vanilla `BlockType#isOccluding`). */
    public static boolean isOccludingMaterial(Material m) {
        if (!isSolidMaterial(m)) {
            return false;
        }
        String name = m.name();
        return !(name.contains("GLASS")
                || name.endsWith("_LEAVES")
                || name.equals("ICE")
                || name.equals("FROSTED_ICE")
                || name.equals("SLIME_BLOCK")
                || name.equals("HONEY_BLOCK")
                || name.equals("SPAWNER")
                || name.equals("BEACON"));
    }

    @Override
    public boolean isPassable() {
        return isPassableMaterial(getType());
    }

    @Override
    public boolean isSolid() {
        return isSolidMaterial(getType());
    }

    @Override
    public boolean isCollidable() {
        return !isPassable();
    }

    @Override
    public boolean isBuildable() {
        return isSolid();
    }

    @Override
    public boolean isBurnable() {
        return getType().isBurnable();
    }

    @Override
    public boolean isReplaceable() {
        Material m = getType();
        return m.isAir() || m == Material.WATER || m == Material.LAVA;
    }

    @Override
    public org.bukkit.block.Biome getBiome() {
        return world.getBiome(x, y, z);
    }

    @Override
    public org.bukkit.block.Biome getComputedBiome() {
        return getBiome();
    }

    @Override
    public java.util.Collection<org.bukkit.inventory.ItemStack> getDrops() {
        return getDrops(null, null);
    }

    @Override
    public java.util.Collection<org.bukkit.inventory.ItemStack> getDrops(org.bukkit.inventory.ItemStack tool) {
        return getDrops(tool, null);
    }

    @Override
    public java.util.Collection<org.bukkit.inventory.ItemStack> getDrops(
            org.bukkit.inventory.ItemStack tool, org.bukkit.entity.Entity entity) {
        Material m = getType();
        if (m.isAir() || !m.isItem()) {
            return java.util.List.of();
        }
        return java.util.List.of(new org.bukkit.inventory.ItemStack(m, 1));
    }

    @Override
    public byte getData() {
        return 0;
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
    public World getWorld() {
        return world;
    }

    @Override
    public Location getLocation() {
        return new Location(world, x, y, z);
    }

    @Override
    public BlockData getBlockData() {
        return org.bukkit.Bukkit.createBlockData(getType());
    }

    @Override
    public void setBlockData(BlockData data) {
        setBlockData(data, true);
    }

    @Override
    public void setBlockData(BlockData data, boolean applyPhysics) {
        Objects.requireNonNull(data, "data");
        setType(data.getMaterial());
    }

    @Override
    public Block getRelative(int modX, int modY, int modZ) {
        return new FerriteBlock(world, x + modX, y + modY, z + modZ);
    }

    @Override
    public Block getRelative(BlockFace face) {
        return getRelative(face, 1);
    }

    @Override
    public Block getRelative(BlockFace face, int distance) {
        Objects.requireNonNull(face, "face");
        return getRelative(face.getModX() * distance, face.getModY() * distance, face.getModZ() * distance);
    }

    @Override
    public boolean isEmpty() {
        return getType().isAir();
    }

    @Override
    public boolean isLiquid() {
        Material type = getType();
        return type == Material.WATER || type == Material.LAVA;
    }

    @Override
    public BlockState getState() {
        return getState(true);
    }

    @Override
    public BlockState getState(boolean useSnapshot) {
        return new FerriteBlockState(this);
    }

    @Override
    public boolean breakNaturally() {
        return breakNaturally(null);
    }

    @Override
    public boolean breakNaturally(org.bukkit.inventory.ItemStack tool) {
        return breakNaturally(tool, true, true, false);
    }

    @Override
    public boolean breakNaturally(boolean triggerEffect, boolean dropExperience) {
        return breakNaturally(null, triggerEffect, dropExperience, false);
    }

    @Override
    public boolean breakNaturally(
            org.bukkit.inventory.ItemStack tool, boolean triggerEffect, boolean dropExperience) {
        return breakNaturally(tool, triggerEffect, dropExperience, false);
    }

    @Override
    public boolean breakNaturally(
            org.bukkit.inventory.ItemStack tool,
            boolean triggerEffect,
            boolean dropExperience,
            boolean forceEffect) {
        Material mat = getType();
        if (mat.isAir()) {
            return false;
        }
        setType(Material.AIR);
        if (mat.isItem()) {
            world.dropItemNaturally(getLocation().add(0.5, 0.5, 0.5), new org.bukkit.inventory.ItemStack(mat, 1));
        }
        return true;
    }

    @Override
    public boolean isValidTool(org.bukkit.inventory.ItemStack itemStack) {
        Material mat = getType();
        if (mat.isAir() || mat == Material.BEDROCK || mat == Material.BARRIER) {
            return false;
        }
        if (isPreferredTool(itemStack)) {
            return true;
        }
        String name = mat.name();
        boolean needsPickaxe = name.contains("_ORE")
                || name.endsWith("_BLOCK")
                || name.equals("STONE")
                || name.equals("COBBLESTONE")
                || name.equals("OBSIDIAN")
                || name.equals("DEEPSLATE")
                || name.equals("NETHERRACK");
        return !needsPickaxe;
    }

    @Override
    public boolean isPreferredTool(org.bukkit.inventory.ItemStack tool) {
        if (tool == null || tool.isEmpty()) {
            return false;
        }
        String blockName = getType().name();
        String toolName = tool.getType().name();
        if (toolName.endsWith("_PICKAXE")) {
            return blockName.contains("STONE")
                    || blockName.contains("_ORE")
                    || blockName.contains("IRON")
                    || blockName.contains("GOLD")
                    || blockName.contains("DIAMOND")
                    || blockName.contains("EMERALD")
                    || blockName.contains("NETHERITE")
                    || blockName.contains("COPPER")
                    || blockName.equals("OBSIDIAN")
                    || blockName.equals("NETHERRACK")
                    || blockName.equals("DEEPSLATE")
                    || blockName.contains("BRICK")
                    || blockName.contains("TERRACOTTA");
        }
        if (toolName.endsWith("_AXE")) {
            return blockName.contains("LOG")
                    || blockName.contains("WOOD")
                    || blockName.contains("PLANKS")
                    || blockName.contains("CHEST")
                    || blockName.equals("CRAFTING_TABLE");
        }
        if (toolName.endsWith("_SHOVEL")) {
            return blockName.equals("DIRT")
                    || blockName.equals("GRASS_BLOCK")
                    || blockName.contains("SAND")
                    || blockName.equals("GRAVEL")
                    || blockName.contains("SNOW")
                    || blockName.equals("CLAY");
        }
        if (toolName.equals("SHEARS")) {
            return blockName.endsWith("_WOOL") || blockName.endsWith("_LEAVES") || blockName.equals("COBWEB");
        }
        return false;
    }

    @Override
    public float getBreakSpeed(org.bukkit.entity.Player player) {
        Objects.requireNonNull(player, "player");
        Material mat = getType();
        if (mat.isAir() || mat == Material.BEDROCK || mat == Material.BARRIER) {
            return 0.0f;
        }
        org.bukkit.inventory.ItemStack hand = player.getInventory().getItemInMainHand();
        float speed = 1.0f;
        if (hand != null && !hand.isEmpty() && isPreferredTool(hand)) {
            String tn = hand.getType().name();
            if (tn.startsWith("NETHERITE_")) speed = 9.0f;
            else if (tn.startsWith("DIAMOND_")) speed = 8.0f;
            else if (tn.startsWith("IRON_")) speed = 6.0f;
            else if (tn.startsWith("GOLDEN_")) speed = 12.0f;
            else if (tn.startsWith("STONE_")) speed = 4.0f;
            else if (tn.startsWith("WOODEN_")) speed = 2.0f;
        }
        float divisor = isValidTool(hand) ? 30.0f : 100.0f;
        return speed / divisor;
    }

    @Override
    public double getTemperature() {
        return world.getTemperature(x, y, z);
    }

    @Override
    public double getHumidity() {
        return world.getHumidity(x, y, z);
    }

    @Override
    public org.bukkit.block.PistonMoveReaction getPistonMoveReaction() {
        Material m = getType();
        if (m == Material.BEDROCK
                || m == Material.OBSIDIAN
                || m == Material.CRYING_OBSIDIAN
                || m == Material.BARRIER
                || m == Material.REINFORCED_DEEPSLATE
                || m == Material.CHEST
                || m == Material.TRAPPED_CHEST
                || m == Material.ENDER_CHEST
                || m == Material.FURNACE
                || m == Material.SPAWNER) {
            return org.bukkit.block.PistonMoveReaction.BLOCK;
        }
        if (m.name().endsWith("_BED")
                || m == Material.TORCH
                || m == Material.REDSTONE_TORCH
                || m.name().contains("FLOWER")
                || m == Material.WATER
                || m == Material.LAVA) {
            return org.bukkit.block.PistonMoveReaction.BREAK;
        }
        return org.bukkit.block.PistonMoveReaction.MOVE;
    }

    @Override
    public void tick() {
    }

    @Override
    public void fluidTick() {
    }

    @Override
    public void randomTick() {
    }

    @Override
    public boolean applyBoneMeal(BlockFace face) {
        Objects.requireNonNull(face, "face");
        return false;
    }

    @Override
    public org.bukkit.util.BoundingBox getBoundingBox() {
        if (isEmpty()) {
            return new org.bukkit.util.BoundingBox(x, y, z, x, y, z);
        }
        return new org.bukkit.util.BoundingBox(x, y, z, x + 1.0, y + 1.0, z + 1.0);
    }

    @Override
    public org.bukkit.util.VoxelShape getCollisionShape() {
        final java.util.Collection<org.bukkit.util.BoundingBox> boxes =
                isPassable() ? java.util.List.of() : java.util.List.of(getBoundingBox());
        return new org.bukkit.util.VoxelShape() {
            @Override
            public java.util.Collection<org.bukkit.util.BoundingBox> getBoundingBoxes() {
                return boxes;
            }

            @Override
            public boolean overlaps(org.bukkit.util.BoundingBox other) {
                Objects.requireNonNull(other, "other");
                for (org.bukkit.util.BoundingBox box : boxes) {
                    if (box.overlaps(other)) {
                        return true;
                    }
                }
                return false;
            }
        };
    }

    @Override
    public org.bukkit.util.RayTraceResult rayTrace(
            Location start,
            org.bukkit.util.Vector direction,
            double maxDistance,
            org.bukkit.FluidCollisionMode fluidCollisionMode) {
        Objects.requireNonNull(start, "start");
        Objects.requireNonNull(direction, "direction");
        if (isEmpty()) {
            return null;
        }
        if (isLiquid() && fluidCollisionMode == org.bukkit.FluidCollisionMode.NEVER) {
            return null;
        }
        org.bukkit.util.RayTraceResult boxHit =
                getBoundingBox().rayTrace(start.toVector(), direction, maxDistance);
        if (boxHit == null) {
            return null;
        }
        return new org.bukkit.util.RayTraceResult(
                boxHit.getHitPosition(), this, boxHit.getHitBlockFace());
    }

    @Override
    public boolean canPlace(BlockData data) {
        Objects.requireNonNull(data, "data");
        return y >= world.getMinHeight() && y < world.getMaxHeight() && data.getMaterial().isBlock();
    }

    @Override
    @SuppressWarnings("removal")
    public com.destroystokyo.paper.block.BlockSoundGroup getSoundGroup() {
        org.bukkit.SoundGroup group = getBlockSoundGroup();
        return new com.destroystokyo.paper.block.BlockSoundGroup() {
            @Override
            public org.bukkit.Sound getBreakSound() {
                return group.getBreakSound();
            }

            @Override
            public org.bukkit.Sound getStepSound() {
                return group.getStepSound();
            }

            @Override
            public org.bukkit.Sound getPlaceSound() {
                return group.getPlaceSound();
            }

            @Override
            public org.bukkit.Sound getHitSound() {
                return group.getHitSound();
            }

            @Override
            public org.bukkit.Sound getFallSound() {
                return group.getFallSound();
            }
        };
    }

    @Override
    public org.bukkit.SoundGroup getBlockSoundGroup() {
        String name = getType().name();
        final org.bukkit.Sound b, s, p, h, f;
        if (name.contains("WOOD") || name.contains("LOG") || name.contains("PLANKS")) {
            b = org.bukkit.Sound.BLOCK_WOOD_BREAK;
            s = org.bukkit.Sound.BLOCK_WOOD_STEP;
            p = org.bukkit.Sound.BLOCK_WOOD_PLACE;
            h = org.bukkit.Sound.BLOCK_WOOD_HIT;
            f = org.bukkit.Sound.BLOCK_WOOD_FALL;
        } else if (name.contains("GRASS") || name.contains("DIRT") || name.contains("LEAVES")) {
            b = org.bukkit.Sound.BLOCK_GRASS_BREAK;
            s = org.bukkit.Sound.BLOCK_GRASS_STEP;
            p = org.bukkit.Sound.BLOCK_GRASS_PLACE;
            h = org.bukkit.Sound.BLOCK_GRASS_HIT;
            f = org.bukkit.Sound.BLOCK_GRASS_FALL;
        } else if (name.contains("SAND") || name.contains("GRAVEL")) {
            b = org.bukkit.Sound.BLOCK_SAND_BREAK;
            s = org.bukkit.Sound.BLOCK_SAND_STEP;
            p = org.bukkit.Sound.BLOCK_SAND_PLACE;
            h = org.bukkit.Sound.BLOCK_SAND_HIT;
            f = org.bukkit.Sound.BLOCK_SAND_FALL;
        } else if (name.contains("GLASS") || name.contains("ICE")) {
            b = org.bukkit.Sound.BLOCK_GLASS_BREAK;
            s = org.bukkit.Sound.BLOCK_GLASS_STEP;
            p = org.bukkit.Sound.BLOCK_GLASS_PLACE;
            h = org.bukkit.Sound.BLOCK_GLASS_HIT;
            f = org.bukkit.Sound.BLOCK_GLASS_FALL;
        } else if (name.contains("IRON") || name.contains("GOLD") || name.contains("NETHERITE") || name.contains("COPPER")) {
            b = org.bukkit.Sound.BLOCK_METAL_BREAK;
            s = org.bukkit.Sound.BLOCK_METAL_STEP;
            p = org.bukkit.Sound.BLOCK_METAL_PLACE;
            h = org.bukkit.Sound.BLOCK_METAL_HIT;
            f = org.bukkit.Sound.BLOCK_METAL_FALL;
        } else {
            b = org.bukkit.Sound.BLOCK_STONE_BREAK;
            s = org.bukkit.Sound.BLOCK_STONE_STEP;
            p = org.bukkit.Sound.BLOCK_STONE_PLACE;
            h = org.bukkit.Sound.BLOCK_STONE_HIT;
            f = org.bukkit.Sound.BLOCK_STONE_FALL;
        }
        return new org.bukkit.SoundGroup() {
            @Override
            public float getVolume() {
                return 1.0f;
            }

            @Override
            public float getPitch() {
                return 1.0f;
            }

            @Override
            public org.bukkit.Sound getBreakSound() {
                return b;
            }

            @Override
            public org.bukkit.Sound getStepSound() {
                return s;
            }

            @Override
            public org.bukkit.Sound getPlaceSound() {
                return p;
            }

            @Override
            public org.bukkit.Sound getHitSound() {
                return h;
            }

            @Override
            public org.bukkit.Sound getFallSound() {
                return f;
            }
        };
    }

    @Override
    @SuppressWarnings("deprecation")
    public String getTranslationKey() {
        return translationKey();
    }

    @Override
    public String translationKey() {
        return "block.minecraft." + getType().getKey().getKey();
    }

    @Override
    public boolean isSuffocating() {
        return isOccludingMaterial(getType());
    }

    @Override
    public void setMetadata(String metadataKey, org.bukkit.metadata.MetadataValue newMetadataValue) {
        world.setBlockMetadata(x, y, z, metadataKey, newMetadataValue);
    }

    @Override
    public java.util.List<org.bukkit.metadata.MetadataValue> getMetadata(String metadataKey) {
        return world.getBlockMetadata(x, y, z, metadataKey);
    }

    @Override
    public boolean hasMetadata(String metadataKey) {
        return world.hasBlockMetadata(x, y, z, metadataKey);
    }

    @Override
    public void removeMetadata(String metadataKey, org.bukkit.plugin.Plugin owningPlugin) {
        world.removeBlockMetadata(x, y, z, metadataKey, owningPlugin);
    }

    @Override
    public boolean equals(Object object) {
        return object instanceof FerriteBlock other
                && other.world == world
                && other.x == x
                && other.y == y
                && other.z == z;
    }

    @Override
    public int hashCode() {
        return (world.getName().hashCode() * 31 + x) * 31 * 31 + y * 31 + z;
    }

    @Override
    public String toString() {
        return "FerriteBlock{" + world.getName() + ":" + x + "," + y + "," + z + "}";
    }
}
