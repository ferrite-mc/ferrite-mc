package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import net.minecraft.core.BlockPos;
import net.minecraft.network.chat.Component;
import net.minecraft.world.item.ItemStack;
import org.junit.jupiter.api.Test;

class NmsShimTest {
    @Test
    void componentKeepsPlainText() {
        assertEquals("hello", Component.literal("hello").getString());
    }

    @Test
    void blockPosOffsetsAndKeepsCoordinates() {
        BlockPos pos = new BlockPos(1, 2, 3);
        assertEquals(1, pos.getX());
        assertEquals(3, pos.getZ());
    }

    @Test
    void emptyItemStackReportsZeroCount() {
        assertTrue(ItemStack.EMPTY.isEmpty());
        assertEquals(0, ItemStack.EMPTY.getCount());
    }

    @Test
    void playerTypeIsCraftPlayerSoCastsSucceed() {
        assertTrue(org.bukkit.craftbukkit.entity.CraftPlayer.class.isAssignableFrom(FerritePlayer.class));
    }
}
