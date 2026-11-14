package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import io.papermc.paper.block.fluid.type.FallingFluidData;
import org.bukkit.Fluid;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

/** {@code World#getFluidData}: the fluid is read from the block's vanilla state string. */
class FerriteFluidDataTest {
    private static final String WORLD_ID = "00000000-0000-0000-0000-0000000000f1";
    private static org.bukkit.World world;

    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        Bootstrap.registerWorld(WORLD_ID, "fluid_world", 1, 0, 64, 0);
        world = org.bukkit.Bukkit.getWorld("fluid_world");
    }

    @AfterAll static void stop() {
        var existing = org.bukkit.Bukkit.getWorld("fluid_world");
        if (existing != null) ((FerriteServer) org.bukkit.Bukkit.getServer()).unregisterWorld(existing.getUID());
    }

    private static io.papermc.paper.block.fluid.FluidData at(int y, String state) {
        world.setBlockData(0, y, 0, FerriteBlockData.create(state));
        return world.getFluidData(0, y, 0);
    }

    @Test void aWaterSourceIsASourceOfLevelEight() {
        var data = at(70, "minecraft:water[level=0]");
        assertEquals(Fluid.WATER, data.getFluidType());
        assertTrue(data.isSource());
        assertEquals(8, data.getLevel());
        assertFalse(((FallingFluidData) data).isFalling());
    }

    @Test void flowingWaterAndLavaLoseOneLevelPerStep() {
        var water = at(71, "minecraft:water[level=3]");
        assertEquals(Fluid.FLOWING_WATER, water.getFluidType());
        assertFalse(water.isSource());
        assertEquals(5, water.getLevel());
        var lava = at(72, "minecraft:lava[level=7]");
        assertEquals(Fluid.FLOWING_LAVA, lava.getFluidType());
        assertEquals(1, lava.getLevel());
        assertEquals(Fluid.LAVA, at(73, "minecraft:lava").getFluidType());
    }

    @Test void aFallingColumnIsFullHeight() {
        var data = at(74, "minecraft:water[level=9]");
        assertEquals(Fluid.FLOWING_WATER, data.getFluidType());
        assertEquals(8, data.getLevel());
        assertTrue(((FallingFluidData) data).isFalling());
        assertFalse(data.isSource());
    }

    @Test void waterloggedBlocksAndWetPlantsHoldAWaterSource() {
        for (var state : new String[] {"minecraft:oak_stairs[waterlogged=true]", "minecraft:kelp", "minecraft:seagrass", "minecraft:bubble_column"}) {
            var data = at(75, state);
            assertEquals(Fluid.WATER, data.getFluidType(), state);
            assertTrue(data.isSource(), state);
        }
        assertEquals(Fluid.EMPTY, at(76, "minecraft:oak_stairs[waterlogged=false]").getFluidType());
    }

    @Test void aBlockWithoutFluidIsEmptyAndTheLocationOverloadAgrees() {
        var empty = at(77, "minecraft:stone");
        assertEquals(Fluid.EMPTY, empty.getFluidType());
        assertEquals(0, empty.getLevel());
        assertFalse(empty.isSource());
        assertFalse(empty instanceof FallingFluidData);
        at(78, "minecraft:water[level=0]");
        assertEquals(Fluid.WATER, world.getFluidData(new org.bukkit.Location(world, 0, 78, 0)).getFluidType());
    }

    @Test void flowDirectionAndHeightAreRefusedBecauseTheyNeedNeighbours() {
        var data = at(79, "minecraft:water[level=0]");
        var here = new org.bukkit.Location(world, 0, 79, 0);
        assertTrue(assertThrows(UnsupportedOperationException.class, () -> data.computeFlowDirection(here)).getMessage().contains("neighbouring"));
        assertThrows(UnsupportedOperationException.class, () -> data.computeHeight(here));
    }
}
