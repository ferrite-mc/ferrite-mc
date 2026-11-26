// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.core;

/** Immutable 16x16x16 section coordinate matching 26.3 {@code net.minecraft.core.SectionPos}. */
public final class SectionPos extends Vec3i {
    private SectionPos(int x, int y, int z) {
        super(x, y, z);
    }

    public static SectionPos of(int x, int y, int z) {
        return new SectionPos(x, y, z);
    }

    public static SectionPos of(BlockPos pos) {
        return new SectionPos(
                blockToSectionCoord(pos.getX()),
                blockToSectionCoord(pos.getY()),
                blockToSectionCoord(pos.getZ()));
    }

    public static int blockToSectionCoord(int coord) {
        return coord >> 4;
    }

    public static int sectionToBlockCoord(int sectionCoord) {
        return sectionCoord << 4;
    }

    public static int sectionToBlockCoord(int sectionCoord, int offset) {
        return (sectionCoord << 4) + offset;
    }

    public int x() {
        return getX();
    }

    public int y() {
        return getY();
    }

    public int z() {
        return getZ();
    }

    public int minBlockX() {
        return sectionToBlockCoord(getX());
    }

    public int minBlockY() {
        return sectionToBlockCoord(getY());
    }

    public int minBlockZ() {
        return sectionToBlockCoord(getZ());
    }

    public BlockPos origin() {
        return new BlockPos(minBlockX(), minBlockY(), minBlockZ());
    }

    public long asLong() {
        return asLong(getX(), getY(), getZ());
    }

    public static long asLong(int x, int y, int z) {
        return ((long) (x & 0x3FFFFF) << 42) | ((long) (z & 0x3FFFFF) << 20) | (y & 0xFFFFFL);
    }
}
