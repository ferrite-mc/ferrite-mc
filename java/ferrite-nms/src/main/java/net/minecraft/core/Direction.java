// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.core;

import java.util.Locale;

/** Cardinal 3D direction matching 26.3 {@code net.minecraft.core.Direction}. */
public enum Direction {
    DOWN(0, 1, -1, "down", AxisDirection.NEGATIVE, Axis.Y, new Vec3i(0, -1, 0)),
    UP(1, 0, -1, "up", AxisDirection.POSITIVE, Axis.Y, new Vec3i(0, 1, 0)),
    NORTH(2, 3, 2, "north", AxisDirection.NEGATIVE, Axis.Z, new Vec3i(0, 0, -1)),
    SOUTH(3, 2, 0, "south", AxisDirection.POSITIVE, Axis.Z, new Vec3i(0, 0, 1)),
    WEST(4, 5, 1, "west", AxisDirection.NEGATIVE, Axis.X, new Vec3i(-1, 0, 0)),
    EAST(5, 4, 3, "east", AxisDirection.POSITIVE, Axis.X, new Vec3i(1, 0, 0));

    private static final Direction[] BY_3D_DATA = {DOWN, UP, NORTH, SOUTH, WEST, EAST};
    private static final Direction[] BY_2D_DATA = {SOUTH, WEST, NORTH, EAST};

    private final int data3d;
    private final int oppositeIndex;
    private final int data2d;
    private final String name;
    private final AxisDirection axisDirection;
    private final Axis axis;
    private final Vec3i normal;

    Direction(
            int data3d,
            int oppositeIndex,
            int data2d,
            String name,
            AxisDirection axisDirection,
            Axis axis,
            Vec3i normal) {
        this.data3d = data3d;
        this.oppositeIndex = oppositeIndex;
        this.data2d = data2d;
        this.name = name;
        this.axisDirection = axisDirection;
        this.axis = axis;
        this.normal = normal;
    }

    public int get3DDataValue() {
        return data3d;
    }

    public int get2DDataValue() {
        return data2d;
    }

    public Direction getOpposite() {
        return BY_3D_DATA[oppositeIndex];
    }

    public int getStepX() {
        return normal.getX();
    }

    public int getStepY() {
        return normal.getY();
    }

    public int getStepZ() {
        return normal.getZ();
    }

    public String getName() {
        return name;
    }

    public Axis getAxis() {
        return axis;
    }

    public AxisDirection getAxisDirection() {
        return axisDirection;
    }

    public Vec3i getUnitVec3i() {
        return normal;
    }

    public static Direction from3DDataValue(int index) {
        return BY_3D_DATA[Math.floorMod(index, BY_3D_DATA.length)];
    }

    public static Direction from2DDataValue(int horizontalIndex) {
        return BY_2D_DATA[Math.floorMod(horizontalIndex, BY_2D_DATA.length)];
    }

    public static Direction fromYRot(double yRot) {
        return from2DDataValue((int) Math.floor(yRot / 90.0 + 0.5) & 3);
    }

    public static Direction byName(String name) {
        if (name == null) {
            return null;
        }
        String norm = name.toLowerCase(Locale.ROOT);
        for (Direction d : values()) {
            if (d.name.equals(norm)) {
                return d;
            }
        }
        return null;
    }

    public enum Axis {
        X("x"),
        Y("y"),
        Z("z");

        private final String name;

        Axis(String name) {
            this.name = name;
        }

        public String getName() {
            return name;
        }

        public boolean isVertical() {
            return this == Y;
        }

        public boolean isHorizontal() {
            return this == X || this == Z;
        }
    }

    public enum AxisDirection {
        POSITIVE(1, "Towards positive"),
        NEGATIVE(-1, "Towards negative");

        private final int step;
        private final String name;

        AxisDirection(int step, String name) {
            this.step = step;
            this.name = name;
        }

        public int getStep() {
            return step;
        }

        public String getName() {
            return name;
        }
    }
}
