package dev.ferrite.server;

import java.util.ArrayList;
import java.util.Collection;
import java.util.List;
import java.util.Map;
import java.util.Random;
import org.bukkit.Location;
import org.bukkit.RegionAccessor;
import org.bukkit.block.BlockState;
import org.bukkit.block.data.BlockData;
import org.bukkit.block.structure.Mirror;
import org.bukkit.block.structure.StructureRotation;
import org.bukkit.entity.Entity;
import org.bukkit.generator.LimitedRegion;
import org.bukkit.persistence.PersistentDataContainer;
import org.bukkit.structure.Palette;
import org.bukkit.structure.Structure;
import org.bukkit.util.BlockTransformer;
import org.bukkit.util.BlockVector;
import org.bukkit.util.EntityTransformer;

/**
 * A structure held in memory: block positions relative to its origin and a vanilla block-state
 * string for each. {@link #fill} copies a world region into it, {@link #place} writes it back
 * with rotation, mirroring and integrity, and {@link StructureNbt} saves and loads the file.
 *
 * <p>shortcut: entities are not captured (plugins cannot list non-player entities yet) and
 * block entity data goes through the host. Rotation and mirroring turn facing, axis, sign
 * rotation, stair/rail shapes, door hinges and the four horizontal connection keys. Other
 * directional properties are preserved; block transformers can adjust the final state.
 */
final class CapturedStructure implements Structure {
    /** A block of the structure: its offset from the origin and its vanilla state string. */
    record Cell(int x, int y, int z, String state, Map<String, Object> nbt) {
        Cell(int x, int y, int z, String state) { this(x, y, z, state, null); }
    }

    private BlockVector size;
    private List<List<Cell>> palettes;
    private final PersistentDataContainer data = new FerriteDataContainer();

    CapturedStructure() { this(new BlockVector(0, 0, 0), List.of()); }

    CapturedStructure(BlockVector size, List<List<Cell>> palettes) {
        this.size = size;
        this.palettes = new ArrayList<>(palettes);
    }

    List<List<Cell>> cellsByPalette() { return palettes; }

    CapturedStructure copy() {
        var copy = new ArrayList<List<Cell>>();
        for (var palette : palettes) copy.add(new ArrayList<>(palette));
        return new CapturedStructure(new BlockVector(size.getBlockX(), size.getBlockY(), size.getBlockZ()), copy);
    }

    @Override public BlockVector getSize() { return new BlockVector(size.getBlockX(), size.getBlockY(), size.getBlockZ()); }

    @Override public List<Palette> getPalettes() {
        var result = new ArrayList<Palette>();
        for (var palette : palettes) result.add(new BlockPalette(palette));
        return result;
    }

    @Override public int getPaletteCount() { return palettes.size(); }
    @Override public List<Entity> getEntities() { return List.of(); }
    @Override public int getEntityCount() { return 0; }
    @Override public PersistentDataContainer getPersistentDataContainer() { return data; }

    @Override public void fill(Location corner1, Location corner2, boolean includeEntities) {
        var world = java.util.Objects.requireNonNull(corner1.getWorld(), "corner1 world");
        int x0 = Math.min(corner1.getBlockX(), corner2.getBlockX());
        int y0 = Math.min(corner1.getBlockY(), corner2.getBlockY());
        int z0 = Math.min(corner1.getBlockZ(), corner2.getBlockZ());
        int x1 = Math.max(corner1.getBlockX(), corner2.getBlockX());
        int y1 = Math.max(corner1.getBlockY(), corner2.getBlockY());
        int z1 = Math.max(corner1.getBlockZ(), corner2.getBlockZ());
        capture(world, x0, y0, z0, x1 - x0 + 1, y1 - y0 + 1, z1 - z0 + 1);
    }

    @Override public void fill(Location corner, BlockVector size, boolean includeEntities) {
        var world = java.util.Objects.requireNonNull(corner.getWorld(), "corner world");
        if (size.getBlockX() < 1 || size.getBlockY() < 1 || size.getBlockZ() < 1) throw new IllegalArgumentException("Structure size must be positive: " + size);
        capture(world, corner.getBlockX(), corner.getBlockY(), corner.getBlockZ(), size.getBlockX(), size.getBlockY(), size.getBlockZ());
    }

    private void capture(org.bukkit.World world, int x0, int y0, int z0, int sx, int sy, int sz) {
        var cells = new ArrayList<Cell>();
        for (int y = 0; y < sy; y++) for (int z = 0; z < sz; z++) for (int x = 0; x < sx; x++) {
            var block = world.getBlockAt(x0 + x, y0 + y, z0 + z);
            cells.add(new Cell(x, y, z, block.getBlockData().getAsString()));
        }
        // One host call per capture brings the block entities in the box (chests, signs, ...).
        // Without a host (unit tests, standalone bridge) the blocks are still captured; with a
        // host, an unknown world is an explicit failure instead of silently losing block data.
        List<String> entities;
        if (!HostBridge.available()) {
            entities = List.of();
        } else {
            @SuppressWarnings("unchecked") var reply = (List<String>) HostBridge.call("World", world.getUID(), "ferriteBlockEntities(int,int,int,int,int,int)", List.class,
                x0, y0, z0, x0 + sx - 1, y0 + sy - 1, z0 + sz - 1);
            if (reply == null) {
                throw new UnsupportedOperationException("Ferrite: block entity data is unavailable for world " + world.getName() + " (" + world.getUID() + ")");
            }
            entities = reply;
        }
        for (var item : entities) {
            var parts = item.split(",", 4);
            if (parts.length != 4) continue;
            int x = Integer.parseInt(parts[0]) - x0, y = Integer.parseInt(parts[1]) - y0, z = Integer.parseInt(parts[2]) - z0;
            if (x < 0 || y < 0 || z < 0 || x >= sx || y >= sy || z >= sz) continue;
            int index = (y * sz + z) * sx + x;
            var nbt = StructureNbt.compound(parts[3]);
            if (nbt == null) continue;
            nbt.remove("x"); nbt.remove("y"); nbt.remove("z");
            var cell = cells.get(index);
            cells.set(index, new Cell(cell.x(), cell.y(), cell.z(), cell.state(), nbt));
        }
        size = new BlockVector(sx, sy, sz);
        palettes = new ArrayList<>(List.of(cells));
    }

    @Override public void place(Location location, boolean includeEntities, StructureRotation rotation, Mirror mirror, int palette, float integrity, Random random) {
        write(location.getWorld(), location.getBlockX(), location.getBlockY(), location.getBlockZ(), rotation, mirror, palette, integrity, random);
    }

    @Override public void place(Location location, boolean includeEntities, StructureRotation rotation, Mirror mirror, int palette, float integrity, Random random, Collection<BlockTransformer> blockTransformers, Collection<EntityTransformer> entityTransformers) {
        write(location.getWorld(), location.getBlockX(), location.getBlockY(), location.getBlockZ(), rotation, mirror, palette, integrity, random, blockTransformers, entityTransformers);
    }

    @Override public void place(RegionAccessor region, BlockVector location, boolean includeEntities, StructureRotation rotation, Mirror mirror, int palette, float integrity, Random random) {
        if (!(region instanceof org.bukkit.World world)) throw notImplemented("place into a non-world region");
        write(world, location.getBlockX(), location.getBlockY(), location.getBlockZ(), rotation, mirror, palette, integrity, random);
    }

    @Override public void place(RegionAccessor region, BlockVector location, boolean includeEntities, StructureRotation rotation, Mirror mirror, int palette, float integrity, Random random, Collection<BlockTransformer> blockTransformers, Collection<EntityTransformer> entityTransformers) {
        java.util.Objects.requireNonNull(blockTransformers, "blockTransformers");
        java.util.Objects.requireNonNull(entityTransformers, "entityTransformers");
        if (!entityTransformers.isEmpty()) throw entityTransformersNotSupported();
        if (!(region instanceof org.bukkit.World world)) throw notImplemented("place into a non-world region");
        write(world, location.getBlockX(), location.getBlockY(), location.getBlockZ(), rotation, mirror, palette, integrity, random, blockTransformers);
    }

    private void write(org.bukkit.World world, int ox, int oy, int oz, StructureRotation rotation, Mirror mirror, int palette, float integrity, Random random) {
        write(world, ox, oy, oz, rotation, mirror, palette, integrity, random, List.of());
    }

    private void write(org.bukkit.World world, int ox, int oy, int oz, StructureRotation rotation, Mirror mirror, int palette, float integrity, Random random, Collection<BlockTransformer> blockTransformers, Collection<EntityTransformer> entityTransformers) {
        java.util.Objects.requireNonNull(blockTransformers, "blockTransformers");
        java.util.Objects.requireNonNull(entityTransformers, "entityTransformers");
        if (!entityTransformers.isEmpty()) throw entityTransformersNotSupported();
        write(world, ox, oy, oz, rotation, mirror, palette, integrity, random, blockTransformers);
    }

    private void write(org.bukkit.World world, int ox, int oy, int oz, StructureRotation rotation, Mirror mirror, int palette, float integrity, Random random, Collection<BlockTransformer> blockTransformers) {
        java.util.Objects.requireNonNull(world, "location world");
        java.util.Objects.requireNonNull(blockTransformers, "blockTransformers");
        if (integrity < 0f || integrity > 1f) throw new IllegalArgumentException("Integrity must be between 0 and 1: " + integrity);
        if (palettes.isEmpty()) return;
        if (palette < -1 || palette >= palettes.size()) throw new IllegalArgumentException("Palette index out of range: " + palette);
        // -1 picks a palette at random, as vanilla does for multi-palette templates.
        var cells = palettes.get(palette < 0 ? random.nextInt(palettes.size()) : palette);
        var region = blockTransformers.isEmpty() ? null : region(world, ox, oz);
        for (var cell : cells) {
            if (integrity < 1f && random.nextFloat() > integrity) continue;
            int x = cell.x(), z = cell.z();
            if (mirror == Mirror.LEFT_RIGHT) z = -z;
            else if (mirror == Mirror.FRONT_BACK) x = -x;
            int rx = x, rz = z;
            switch (rotation) {
                case CLOCKWISE_90 -> { rx = -z; rz = x; }
                case CLOCKWISE_180 -> { rx = -x; rz = -z; }
                case COUNTERCLOCKWISE_90 -> { rx = z; rz = -x; }
                default -> { }
            }
            int wx = ox + rx, wy = oy + cell.y(), wz = oz + rz;
            var state = turn(cell.state(), rotation, mirror);
            if (region != null) state = transform(world, region, wx, wy, wz, cell.state(), state, blockTransformers);
            world.getBlockAt(wx, wy, wz).setBlockData(FerriteBlockData.create(state));
            if (cell.nbt() != null) HostBridge.callVoid("World", world.getUID(), "ferriteDataMerge(int,int,int,String)", wx, wy, wz, Snbt.of(cell.nbt()));
        }
    }

    /**
     * Turns the supported directional properties of a vanilla state string the way the blocks
     * move. {@code Mirror.LEFT_RIGHT} flips Z and {@code FRONT_BACK} flips X, matching the
     * position transform and Bukkit's mirror names.
     */
    static String turn(String state, StructureRotation rotation, Mirror mirror) {
        int open = state.indexOf('[');
        if (open < 0 || (rotation == StructureRotation.NONE && mirror == Mirror.NONE)) return state;
        var props = new java.util.LinkedHashMap<String, String>();
        var body = state.substring(open + 1, state.length() - 1);
        if (!body.isEmpty()) for (var pair : body.split(",")) {
            int eq = pair.indexOf('=');
            if (eq < 0) throw new IllegalArgumentException("Bad block state: " + state);
            props.put(pair.substring(0, eq), pair.substring(eq + 1));
        }
        var originalFacing = props.get("facing");
        if (mirror != Mirror.NONE) {
            if (originalFacing != null) props.put("facing", mirrorDirection(originalFacing, mirror));
            if (props.containsKey("hinge")) props.put("hinge", mirrorHinge(props.get("hinge")));
            if (props.containsKey("rotation")) props.put("rotation", mirrorRotation(props.get("rotation"), mirror));
            if (props.containsKey("shape")) props.put("shape", mirrorShape(props.get("shape"), originalFacing, mirror));
            props = remapDirections(props, d -> mirrorDirection(d, mirror));
        }
        if (rotation != StructureRotation.NONE) {
            if (props.containsKey("facing")) props.put("facing", rotateDirection(props.get("facing"), rotation));
            if (props.containsKey("axis")) props.put("axis", rotateAxis(props.get("axis"), rotation));
            if (props.containsKey("rotation")) props.put("rotation", rotateRotation(props.get("rotation"), rotation));
            if (props.containsKey("shape")) props.put("shape", rotateShape(props.get("shape"), rotation));
            props = remapDirections(props, d -> rotateDirection(d, rotation));
        }
        var out = new StringBuilder(state.substring(0, open + 1));
        boolean first = true;
        for (var entry : props.entrySet()) {
            if (!first) out.append(',');
            first = false;
            out.append(entry.getKey()).append('=').append(entry.getValue());
        }
        return out.append(']').toString();
    }

    private static final List<String> HORIZONTAL = List.of("north", "east", "south", "west");

    private static String mirrorDirection(String value, Mirror mirror) {
        return switch (mirror) {
            case LEFT_RIGHT -> value.equals("north") ? "south" : value.equals("south") ? "north" : value;
            case FRONT_BACK -> value.equals("east") ? "west" : value.equals("west") ? "east" : value;
            default -> value;
        };
    }

    private static String rotateDirection(String value, StructureRotation rotation) {
        int index = HORIZONTAL.indexOf(value);
        if (index < 0) return value; // up and down do not turn
        int step = switch (rotation) {
            case CLOCKWISE_90 -> 1;
            case CLOCKWISE_180 -> 2;
            case COUNTERCLOCKWISE_90 -> 3;
            default -> 0;
        };
        return HORIZONTAL.get((index + step) % 4);
    }

    private static java.util.LinkedHashMap<String, String> remapDirections(java.util.Map<String, String> props, java.util.function.UnaryOperator<String> mapper) {
        var result = new java.util.LinkedHashMap<String, String>();
        for (var entry : props.entrySet()) result.put(mapper.apply(entry.getKey()), entry.getValue());
        return result;
    }

    private static String mirrorHinge(String value) {
        return value.equals("left") ? "right" : value.equals("right") ? "left" : value;
    }

    private static String mirrorRotation(String value, Mirror mirror) {
        try {
            int rotation = Integer.parseInt(value);
            int half = 8;
            int adjusted = rotation > half ? rotation - 16 : rotation;
            return Integer.toString(switch (mirror) {
                case LEFT_RIGHT -> Math.floorMod(half - adjusted + 16, 16);
                case FRONT_BACK -> Math.floorMod(16 - adjusted, 16);
                default -> rotation;
            });
        } catch (NumberFormatException ignored) {
            return value;
        }
    }

    private static String rotateRotation(String value, StructureRotation rotation) {
        try {
            int degrees = Integer.parseInt(value);
            int step = switch (rotation) {
                case CLOCKWISE_90 -> 4;
                case CLOCKWISE_180 -> 8;
                case COUNTERCLOCKWISE_90 -> 12;
                default -> 0;
            };
            return Integer.toString(Math.floorMod(step + degrees, 16));
        } catch (NumberFormatException ignored) {
            return value;
        }
    }

    private static String rotateAxis(String value, StructureRotation rotation) {
        if (rotation != StructureRotation.CLOCKWISE_90 && rotation != StructureRotation.COUNTERCLOCKWISE_90) return value;
        return value.equals("x") ? "z" : value.equals("z") ? "x" : value;
    }

    private static boolean stairShape(String value) {
        return value.equals("straight") || value.equals("inner_left") || value.equals("inner_right")
            || value.equals("outer_left") || value.equals("outer_right");
    }

    private static String mirrorShape(String shape, String facing, Mirror mirror) {
        if (stairShape(shape)) {
            if (facing == null) return shape;
            boolean zAxis = facing.equals("north") || facing.equals("south");
            boolean xAxis = facing.equals("east") || facing.equals("west");
            if (mirror == Mirror.LEFT_RIGHT && zAxis) return switch (shape) {
                case "inner_left" -> "inner_right";
                case "inner_right" -> "inner_left";
                case "outer_left" -> "outer_right";
                case "outer_right" -> "outer_left";
                default -> shape;
            };
            if (mirror == Mirror.FRONT_BACK && xAxis) return switch (shape) {
                case "outer_left" -> "outer_right";
                case "outer_right" -> "outer_left";
                default -> shape;
            };
            return shape;
        }
        return switch (mirror) {
            case LEFT_RIGHT -> switch (shape) {
                case "ascending_north" -> "ascending_south";
                case "ascending_south" -> "ascending_north";
                case "south_east" -> "north_east";
                case "south_west" -> "north_west";
                case "north_west" -> "south_west";
                case "north_east" -> "south_east";
                default -> shape;
            };
            case FRONT_BACK -> switch (shape) {
                case "ascending_east" -> "ascending_west";
                case "ascending_west" -> "ascending_east";
                case "south_east" -> "south_west";
                case "south_west" -> "south_east";
                case "north_west" -> "north_east";
                case "north_east" -> "north_west";
                default -> shape;
            };
            default -> shape;
        };
    }

    private static String rotateShape(String shape, StructureRotation rotation) {
        if (stairShape(shape)) return shape; // stairs turn FACING, not SHAPE
        return switch (rotation) {
            case CLOCKWISE_90 -> switch (shape) {
                case "north_south", "east_west" -> shape.equals("north_south") ? "east_west" : "north_south";
                case "ascending_east" -> "ascending_south";
                case "ascending_west" -> "ascending_north";
                case "ascending_north" -> "ascending_east";
                case "ascending_south" -> "ascending_west";
                case "south_east" -> "south_west";
                case "south_west" -> "north_west";
                case "north_west" -> "north_east";
                case "north_east" -> "south_east";
                default -> shape;
            };
            case COUNTERCLOCKWISE_90 -> switch (shape) {
                case "north_south", "east_west" -> shape.equals("north_south") ? "east_west" : "north_south";
                case "ascending_east" -> "ascending_north";
                case "ascending_west" -> "ascending_south";
                case "ascending_north" -> "ascending_west";
                case "ascending_south" -> "ascending_east";
                case "south_east" -> "north_east";
                case "south_west" -> "south_east";
                case "north_west" -> "south_west";
                case "north_east" -> "north_west";
                default -> shape;
            };
            case CLOCKWISE_180 -> switch (shape) {
                case "ascending_east" -> "ascending_west";
                case "ascending_west" -> "ascending_east";
                case "ascending_north" -> "ascending_south";
                case "ascending_south" -> "ascending_north";
                case "south_east" -> "north_west";
                case "south_west" -> "north_east";
                case "north_west" -> "south_east";
                case "north_east" -> "south_west";
                default -> shape;
            };
            default -> shape;
        };
    }

    private static String transform(org.bukkit.World world, LimitedRegion region, int x, int y, int z, String original, String current, Collection<BlockTransformer> transformers) {
        var state = new Transformation(world, x, y, z, original);
        var block = blockState(world, x, y, z, FerriteBlockData.create(current));
        for (var transformer : transformers) {
            if (transformer == null) throw new IllegalArgumentException("BlockTransformer must not be null");
            block = transformer.transform(region, x, y, z, block, state);
            if (block == null) throw new IllegalArgumentException("BlockTransformer returned null: " + transformer);
            if (block.getBlockData() == null) throw new IllegalArgumentException("BlockTransformer returned a state without block data: " + transformer);
        }
        return block.getBlockData().getAsString();
    }

    private static final class Transformation implements BlockTransformer.TransformationState {
        private final org.bukkit.World world;
        private final int x, y, z;
        private final String original;
        private String worldState;

        Transformation(org.bukkit.World world, int x, int y, int z, String original) {
            this.world = world;
            this.x = x;
            this.y = y;
            this.z = z;
            this.original = original;
        }

        @Override public BlockState getOriginal() { return blockState(world, x, y, z, FerriteBlockData.create(original)); }

        @Override public BlockState getWorld() {
            if (worldState == null) worldState = world.getBlockAt(x, y, z).getBlockData().getAsString();
            return blockState(world, x, y, z, FerriteBlockData.create(worldState));
        }
    }

    private static BlockState blockState(org.bukkit.World world, int x, int y, int z, BlockData initial) {
        var data = new java.util.concurrent.atomic.AtomicReference<>(java.util.Objects.requireNonNull(initial, "initial"));
        return (BlockState) java.lang.reflect.Proxy.newProxyInstance(CapturedStructure.class.getClassLoader(), new Class<?>[]{BlockState.class}, (self, method, args) -> switch (method.getName()) {
            case "getX" -> x;
            case "getY" -> y;
            case "getZ" -> z;
            case "getWorld" -> world;
            case "getLocation" -> new Location(world, x, y, z);
            case "getBlock" -> world.getBlockAt(x, y, z);
            case "getType" -> data.get().getMaterial();
            case "getBlockData" -> data.get();
            case "setBlockData" -> { data.set(java.util.Objects.requireNonNull((BlockData) args[0], "data")); yield null; }
            case "setType" -> { data.set(FerriteBlockData.create((org.bukkit.Material) args[0])); yield null; }
            case "copy" -> args == null || args.length == 0
                ? blockState(world, x, y, z, data.get())
                : blockState(java.util.Objects.requireNonNull(((Location) args[0]).getWorld(), "location world"), ((Location) args[0]).getBlockX(), ((Location) args[0]).getBlockY(), ((Location) args[0]).getBlockZ(), data.get());
            case "isPlaced" -> false;
            case "hashCode" -> java.util.Objects.hash(world.getUID(), x, y, z, data.get().getAsString());
            case "equals" -> self == args[0];
            case "toString" -> "BlockState[" + world.getName() + ":" + x + "," + y + "," + z + "=" + data.get().getAsString() + "]";
            default -> throw new UnsupportedOperationException("Ferrite: BlockState#" + method.getName() + " is not implemented for a structure transformer");
        });
    }

    private static LimitedRegion region(org.bukkit.World world, int x, int z) {
        return (LimitedRegion) java.lang.reflect.Proxy.newProxyInstance(CapturedStructure.class.getClassLoader(), new Class<?>[]{LimitedRegion.class}, (self, method, args) -> {
            var values = args == null ? new Object[0] : args;
            return switch (method.getName()) {
                case "getWorld" -> world;
                case "getBlockData" -> blockAt(world, values).getBlockData();
                case "getBlockState" -> {
                    var block = blockAt(world, values);
                    yield blockState(world, block.getX(), block.getY(), block.getZ(), block.getBlockData());
                }
                case "getType" -> blockAt(world, values).getType();
                case "setBlockData" -> { blockAt(world, values).setBlockData((BlockData) values[values.length - 1]); yield null; }
                case "setType" -> { blockAt(world, values).setType((org.bukkit.Material) values[values.length - 1]); yield null; }
                case "setBlockState" -> { blockAt(world, values).setBlockData(((BlockState) values[values.length - 1]).getBlockData()); yield null; }
                case "getBuffer" -> 0;
                case "isInRegion" -> true;
                case "getCenterChunkX" -> x >> 4;
                case "getCenterChunkZ" -> z >> 4;
                case "getCenterBlockX" -> x;
                case "getCenterBlockZ" -> z;
                case "getKey" -> world.getKey();
                case "getMoonPhase" -> world.getMoonPhase();
                case "getHighestBlockYAt" -> {
                    if (values[0] instanceof Location location) yield world.getHighestBlockYAt(location.getBlockX(), location.getBlockZ());
                    yield world.getHighestBlockYAt((int) values[0], (int) values[1]);
                }
                case "getTileEntities" -> throw new UnsupportedOperationException("Ferrite: LimitedRegion#getTileEntities is not available for structure transformers; block entity data is merged after placement");
                case "hashCode" -> java.util.Objects.hash(world.getUID(), x, z);
                case "equals" -> self == values[0];
                case "toString" -> "LimitedRegion[" + world.getName() + "]";
                default -> throw new UnsupportedOperationException("Ferrite: LimitedRegion#" + method.getName() + " is not implemented for structure transformers");
            };
        });
    }

    private static org.bukkit.block.Block blockAt(org.bukkit.World world, Object[] values) {
        if (values[0] instanceof Location location) return world.getBlockAt(location);
        return world.getBlockAt((int) values[0], (int) values[1], (int) values[2]);
    }

    static UnsupportedOperationException entityTransformersNotSupported() {
        return new UnsupportedOperationException("Ferrite: not implemented: Structure#place with EntityTransformer; structures do not capture entities and there is no host enumeration of non-player entities for the callback");
    }

    private static UnsupportedOperationException notImplemented(String method) {
        return new UnsupportedOperationException("Ferrite: not implemented: Structure#" + method);
    }

    /** One palette: the blocks of the structure as block states with their relative positions. */
    private record BlockPalette(List<Cell> cells) implements Palette {
        @Override public List<BlockState> getBlocks() {
            var states = new ArrayList<BlockState>();
            for (var cell : cells) states.add(state(cell));
            return states;
        }

        @Override public int getBlockCount() { return cells.size(); }

        private static BlockState state(Cell cell) {
            var data = FerriteBlockData.create(cell.state());
            return (BlockState) java.lang.reflect.Proxy.newProxyInstance(CapturedStructure.class.getClassLoader(), new Class<?>[] {BlockState.class}, (self, method, args) -> switch (method.getName()) {
                case "getX" -> cell.x();
                case "getY" -> cell.y();
                case "getZ" -> cell.z();
                case "getType" -> data.getMaterial();
                case "getBlockData" -> data;
                case "getLocation" -> new Location(null, cell.x(), cell.y(), cell.z());
                case "isPlaced" -> false;
                case "hashCode" -> cell.hashCode();
                case "equals" -> self == args[0];
                case "toString" -> "BlockState[" + cell + "]";
                default -> throw new UnsupportedOperationException("Ferrite: BlockState#" + method.getName() + " is not implemented for a structure palette");
            });
        }
    }
}
