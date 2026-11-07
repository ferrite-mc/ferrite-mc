package dev.ferrite.server;

import java.util.ArrayList;
import java.util.Collection;
import java.util.List;
import java.util.Random;
import org.bukkit.Location;
import org.bukkit.RegionAccessor;
import org.bukkit.block.BlockState;
import org.bukkit.block.structure.Mirror;
import org.bukkit.block.structure.StructureRotation;
import org.bukkit.entity.Entity;
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
 * <p>shortcut: entities and block entity data are not captured, and rotating or mirroring turns
 * only the {@code facing} and {@code axis} properties (not stair shapes, rails or hinges).
 */
final class CapturedStructure implements Structure {
    /** A block of the structure: its offset from the origin and its vanilla state string. */
    record Cell(int x, int y, int z, String state) {}

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
        size = new BlockVector(sx, sy, sz);
        palettes = new ArrayList<>(List.of(cells));
    }

    @Override public void place(Location location, boolean includeEntities, StructureRotation rotation, Mirror mirror, int palette, float integrity, Random random) {
        write(location.getWorld(), location.getBlockX(), location.getBlockY(), location.getBlockZ(), rotation, mirror, palette, integrity, random);
    }

    @Override public void place(Location location, boolean includeEntities, StructureRotation rotation, Mirror mirror, int palette, float integrity, Random random, Collection<BlockTransformer> blockTransformers, Collection<EntityTransformer> entityTransformers) {
        if (!blockTransformers.isEmpty() || !entityTransformers.isEmpty()) throw notImplemented("place with transformers");
        place(location, includeEntities, rotation, mirror, palette, integrity, random);
    }

    @Override public void place(RegionAccessor region, BlockVector location, boolean includeEntities, StructureRotation rotation, Mirror mirror, int palette, float integrity, Random random) {
        if (!(region instanceof org.bukkit.World world)) throw notImplemented("place into a non-world region");
        write(world, location.getBlockX(), location.getBlockY(), location.getBlockZ(), rotation, mirror, palette, integrity, random);
    }

    @Override public void place(RegionAccessor region, BlockVector location, boolean includeEntities, StructureRotation rotation, Mirror mirror, int palette, float integrity, Random random, Collection<BlockTransformer> blockTransformers, Collection<EntityTransformer> entityTransformers) {
        if (!blockTransformers.isEmpty() || !entityTransformers.isEmpty()) throw notImplemented("place with transformers");
        place(region, location, includeEntities, rotation, mirror, palette, integrity, random);
    }

    private void write(org.bukkit.World world, int ox, int oy, int oz, StructureRotation rotation, Mirror mirror, int palette, float integrity, Random random) {
        java.util.Objects.requireNonNull(world, "location world");
        if (integrity < 0f || integrity > 1f) throw new IllegalArgumentException("Integrity must be between 0 and 1: " + integrity);
        if (palettes.isEmpty()) return;
        if (palette < -1 || palette >= palettes.size()) throw new IllegalArgumentException("Palette index out of range: " + palette);
        // -1 picks a palette at random, as vanilla does for multi-palette templates.
        var cells = palettes.get(palette < 0 ? random.nextInt(palettes.size()) : palette);
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
            world.getBlockAt(ox + rx, oy + cell.y(), oz + rz)
                .setBlockData(FerriteBlockData.create(turn(cell.state(), rotation, mirror)));
        }
    }

    /** Turns the {@code facing} and {@code axis} properties of a state string the way the blocks move. */
    static String turn(String state, StructureRotation rotation, Mirror mirror) {
        int open = state.indexOf('[');
        if (open < 0 || (rotation == StructureRotation.NONE && mirror == Mirror.NONE)) return state;
        var out = new StringBuilder(state.substring(0, open + 1));
        var pairs = state.substring(open + 1, state.length() - 1).split(",");
        for (int i = 0; i < pairs.length; i++) {
            int eq = pairs[i].indexOf('=');
            var name = pairs[i].substring(0, eq);
            var value = pairs[i].substring(eq + 1);
            if (name.equals("facing")) value = facing(value, rotation, mirror);
            else if (name.equals("axis") && (rotation == StructureRotation.CLOCKWISE_90 || rotation == StructureRotation.COUNTERCLOCKWISE_90)) {
                value = value.equals("x") ? "z" : value.equals("z") ? "x" : value;
            }
            if (i > 0) out.append(',');
            out.append(name).append('=').append(value);
        }
        return out.append(']').toString();
    }

    private static String facing(String value, StructureRotation rotation, Mirror mirror) {
        var order = List.of("north", "east", "south", "west");
        int i = order.indexOf(value);
        if (i < 0) return value; // up and down do not turn
        if (mirror == Mirror.LEFT_RIGHT && i % 2 == 0) i = (i + 2) % 4;
        else if (mirror == Mirror.FRONT_BACK && i % 2 == 1) i = (i + 2) % 4;
        i = switch (rotation) {
            case CLOCKWISE_90 -> i + 1;
            case CLOCKWISE_180 -> i + 2;
            case COUNTERCLOCKWISE_90 -> i + 3;
            default -> i;
        };
        return order.get(i % 4);
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
