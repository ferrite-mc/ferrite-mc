package dev.ferrite.server;

import io.papermc.paper.block.fluid.FluidData;
import io.papermc.paper.block.fluid.type.FlowingFluidData;
import java.util.HashMap;
import java.util.Map;
import java.util.Objects;
import org.bukkit.Fluid;
import org.bukkit.Location;
import org.bukkit.util.Vector;

/**
 * The fluid of one block, derived from its vanilla state string the way {@code BlockState.getFluidState()} does:
 * a {@code water}/{@code lava} block is a source at {@code level=0}, flows with amount {@code 8 - level} for levels
 * 1..7 and is a falling column of amount 8 from level 8; a {@code waterlogged=true} block and the always-wet plants
 * ({@code kelp}, {@code kelp_plant}, {@code seagrass}, {@code tall_seagrass}, {@code bubble_column}) hold a source of
 * water; everything else holds {@link Fluid#EMPTY}.
 *
 * <p>{@code getLevel} reports the vanilla fluid amount (8 for a source or a falling column, 0 for no fluid). The flow
 * direction and the surface height depend on the neighbouring blocks, which this value does not hold, so those two
 * refuse instead of answering a flat guess.
 */
final class FerriteFluidData implements FlowingFluidData {
    private static final java.util.Set<String> ALWAYS_WET =
        java.util.Set.of("minecraft:kelp", "minecraft:kelp_plant", "minecraft:seagrass", "minecraft:tall_seagrass", "minecraft:bubble_column");

    private final Fluid type;
    private final int amount;
    private final boolean source;
    private final boolean falling;

    private FerriteFluidData(Fluid type, int amount, boolean source, boolean falling) {
        this.type = type;
        this.amount = amount;
        this.source = source;
        this.falling = falling;
    }

    /** Fluid data for a state such as {@code minecraft:water[level=3]} (the namespace is optional). */
    static FluidData of(String state) {
        var text = state.indexOf(':') < 0 ? "minecraft:" + state : state;
        var open = text.indexOf('[');
        var name = open < 0 ? text : text.substring(0, open);
        Map<String, String> properties = new HashMap<>();
        if (open >= 0 && text.endsWith("]")) {
            for (var pair : text.substring(open + 1, text.length() - 1).split(",")) {
                int eq = pair.indexOf('=');
                if (eq > 0) properties.put(pair.substring(0, eq), pair.substring(eq + 1));
            }
        }
        if (name.equals("minecraft:water") || name.equals("minecraft:lava")) {
            boolean water = name.equals("minecraft:water");
            int level = Integer.parseInt(properties.getOrDefault("level", "0"));
            if (level == 0) return new FerriteFluidData(water ? Fluid.WATER : Fluid.LAVA, 8, true, false);
            boolean falling = level >= 8;
            return new FerriteFluidData(water ? Fluid.FLOWING_WATER : Fluid.FLOWING_LAVA, falling ? 8 : 8 - level, false, falling);
        }
        if ("true".equals(properties.get("waterlogged")) || ALWAYS_WET.contains(name)) {
            return new FerriteFluidData(Fluid.WATER, 8, true, false);
        }
        return new Empty();
    }

    @Override public Fluid getFluidType() { return type; }
    @Override public int getLevel() { return amount; }
    @Override public boolean isSource() { return source; }
    @Override public boolean isFalling() { return falling; }
    @Override public FluidData clone() { return this; }
    @Override public Vector computeFlowDirection(Location location) { throw refused("flow direction"); }
    @Override public float computeHeight(Location location) { throw refused("height"); }

    private static UnsupportedOperationException refused(String what) {
        return new UnsupportedOperationException("Ferrite: the " + what + " of a fluid depends on the neighbouring blocks, which a FluidData does not hold");
    }

    @Override public boolean equals(Object other) {
        return other instanceof FerriteFluidData data && data.type == type && data.amount == amount && data.source == source && data.falling == falling;
    }
    @Override public int hashCode() { return Objects.hash(type, amount, source, falling); }
    @Override public String toString() { return "FluidData{" + type + ", level=" + amount + (source ? ", source" : "") + (falling ? ", falling" : "") + "}"; }

    /** A block with no fluid: a plain {@link FluidData}, not a flowing one. */
    private static final class Empty implements FluidData {
        @Override public Fluid getFluidType() { return Fluid.EMPTY; }
        @Override public int getLevel() { return 0; }
        @Override public boolean isSource() { return false; }
        @Override public FluidData clone() { return this; }
        @Override public Vector computeFlowDirection(Location location) { throw refused("flow direction"); }
        @Override public float computeHeight(Location location) { throw refused("height"); }
        @Override public boolean equals(Object other) { return other instanceof Empty; }
        @Override public int hashCode() { return 0; }
        @Override public String toString() { return "FluidData{EMPTY}"; }
    }
}
