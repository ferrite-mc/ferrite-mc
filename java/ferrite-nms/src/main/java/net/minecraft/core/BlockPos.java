// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.core;

import com.mojang.serialization.Codec;
import java.util.List;

/** Immutable block position matching 26.3 {@code net.minecraft.core.BlockPos}. */
public class BlockPos extends Vec3i {
    public static final BlockPos ZERO = new BlockPos(0, 0, 0);

    public static final Codec<BlockPos> CODEC = Codec.INT.listOf().comapFlatMap(
            list -> list.size() == 3
                    ? com.mojang.serialization.DataResult.success(
                            new BlockPos(list.get(0), list.get(1), list.get(2)))
                    : com.mojang.serialization.DataResult.error(
                            () -> "Expected 3 ints for BlockPos, got " + list.size()),
            pos -> List.of(pos.getX(), pos.getY(), pos.getZ()));

    public final int x;
    public final int y;
    public final int z;

    public BlockPos(int x, int y, int z) {
        super(x, y, z);
        this.x = x;
        this.y = y;
        this.z = z;
    }

    public BlockPos(Vec3i vec) {
        this(vec.getX(), vec.getY(), vec.getZ());
    }

    public static BlockPos containing(double x, double y, double z) {
        return new BlockPos((int) Math.floor(x), (int) Math.floor(y), (int) Math.floor(z));
    }

    public BlockPos offset(int dx, int dy, int dz) {
        return (dx == 0 && dy == 0 && dz == 0)
                ? this
                : new BlockPos(getX() + dx, getY() + dy, getZ() + dz);
    }

    public BlockPos offset(Vec3i vec) {
        return offset(vec.getX(), vec.getY(), vec.getZ());
    }

    public BlockPos above() {
        return offset(0, 1, 0);
    }

    public BlockPos above(int distance) {
        return offset(0, distance, 0);
    }

    public BlockPos below() {
        return offset(0, -1, 0);
    }

    public BlockPos below(int distance) {
        return offset(0, -distance, 0);
    }

    public BlockPos north() {
        return offset(0, 0, -1);
    }

    public BlockPos south() {
        return offset(0, 0, 1);
    }

    public BlockPos west() {
        return offset(-1, 0, 0);
    }

    public BlockPos east() {
        return offset(1, 0, 0);
    }

    public BlockPos relative(Direction direction) {
        return offset(direction.getStepX(), direction.getStepY(), direction.getStepZ());
    }

    public BlockPos relative(Direction direction, int distance) {
        return distance == 0
                ? this
                : offset(
                        direction.getStepX() * distance,
                        direction.getStepY() * distance,
                        direction.getStepZ() * distance);
    }

    public BlockPos immutable() {
        return this;
    }

    public MutableBlockPos mutable() {
        return new MutableBlockPos(getX(), getY(), getZ());
    }

    public long asLong() {
        return asLong(getX(), getY(), getZ());
    }

    public static long asLong(int x, int y, int z) {
        return ((long) (x & 0x3FFFFFF) << 38) | ((long) (z & 0x3FFFFFF) << 12) | (y & 0xFFF);
    }

    public static int getX(long packed) {
        return (int) (packed >> 38);
    }

    public static int getY(long packed) {
        return (int) (packed << 52 >> 52);
    }

    public static int getZ(long packed) {
        return (int) (packed << 26 >> 38);
    }

    @Override
    public String toString() {
        return "BlockPos{x=" + getX() + ", y=" + getY() + ", z=" + getZ() + "}";
    }

    /** Mutable cursor position; {@link #immutable()} returns a detached immutable {@link BlockPos}. */
    public static final class MutableBlockPos extends BlockPos {
        public MutableBlockPos() {
            this(0, 0, 0);
        }

        public MutableBlockPos(int x, int y, int z) {
            super(x, y, z);
        }

        public MutableBlockPos set(int x, int y, int z) {
            setX(x);
            setY(y);
            setZ(z);
            return this;
        }

        public MutableBlockPos set(Vec3i vec) {
            return set(vec.getX(), vec.getY(), vec.getZ());
        }

        @Override
        public BlockPos immutable() {
            return new BlockPos(getX(), getY(), getZ());
        }
    }
}
