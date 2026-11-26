// Clean-room Ferrite NMS test. No Mojang code or assets.
package dev.ferrite.nms;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.serialization.JsonOps;
import dev.ferrite.nms.host.NmsHostBinding;
import dev.ferrite.nms.test.FakeNmsHost;
import java.util.UUID;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.SectionPos;
import net.minecraft.core.Vec3i;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.ListTag;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.StringTag;
import net.minecraft.network.chat.Component;
import net.minecraft.resources.Identifier;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.util.UUIDUtil;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import org.bukkit.Material;
import org.junit.jupiter.api.Test;

class WorldEditSurfaceTest {

    @Test
    void blockPosDirectionSectionPosAndUuidUtil() {
        BlockPos pos = new BlockPos(17, 65, -33);
        assertEquals(new BlockPos(17, 66, -33), pos.above());
        assertEquals(new BlockPos(18, 65, -33), pos.relative(Direction.EAST));
        BlockPos.MutableBlockPos mutable = pos.mutable().set(1, 2, 3);
        BlockPos frozen = mutable.immutable();
        mutable.set(9, 9, 9);
        assertEquals(new BlockPos(1, 2, 3), frozen);

        long packed = pos.asLong();
        assertEquals(pos.getX(), BlockPos.getX(packed));
        assertEquals(pos.getY(), BlockPos.getY(packed));
        assertEquals(pos.getZ(), BlockPos.getZ(packed));

        assertEquals(Direction.NORTH, Direction.SOUTH.getOpposite());
        assertEquals(Direction.Axis.Z, Direction.NORTH.getAxis());
        assertEquals(Direction.WEST, Direction.byName("west"));

        SectionPos sec = SectionPos.of(pos);
        assertEquals(1, sec.x());
        assertEquals(4, sec.y());
        assertEquals(-3, sec.z());
        assertEquals(new Vec3i(16, 64, -48), sec.origin());

        UUID uuid = UUIDUtil.createOfflinePlayerUUID("Builder");
        int[] bits = UUIDUtil.uuidToIntArray(uuid);
        assertEquals(uuid, UUIDUtil.uuidFromIntArray(bits));
        var encodedUuid = UUIDUtil.CODEC.encodeStart(JsonOps.INSTANCE, uuid).getOrThrow();
        assertEquals(uuid, UUIDUtil.CODEC.parse(JsonOps.INSTANCE, encodedUuid).getOrThrow());
    }

    @Test
    void identifierNbtAndComponentRoundTrip() {
        Identifier id = Identifier.parse("stone");
        assertEquals("minecraft", id.getNamespace());
        assertEquals("stone", id.getPath());
        assertEquals("ferrite:wand", Identifier.fromNamespaceAndPath("ferrite", "wand").toString());
        assertNull(Identifier.tryParse("INVALID:PATH"));

        CompoundTag tag = new CompoundTag();
        tag.putString("id", "minecraft:chest");
        tag.putInt("x", 10);
        tag.putBoolean("locked", true);
        tag.putIntArray("arr", new int[] {1, 2, 3});
        ListTag list = new ListTag();
        list.add(StringTag.valueOf("alpha"));
        tag.put("items", list);

        CompoundTag copy = tag.copy();
        copy.putInt("x", 99);
        assertEquals(10, tag.getInt("x"));
        assertTrue(tag.getBoolean("locked"));
        assertArrayEquals(new int[] {1, 2, 3}, tag.getIntArray("arr"));
        assertEquals("alpha", tag.getList("items").getString(0));

        // Round-trip through NbtOps <-> JsonOps.
        var json = NbtOps.INSTANCE.convertTo(JsonOps.INSTANCE, tag);
        var back = JsonOps.INSTANCE.convertTo(NbtOps.INSTANCE, json);
        assertTrue(back instanceof CompoundTag);
        assertEquals("minecraft:chest", ((CompoundTag) back).getString("id"));

        Component text = Component.literal("Set 12 blocks");
        assertEquals("Set 12 blocks", text.getString());
        assertEquals("", Component.empty().getString());
    }

    @Test
    void serverLevelBoundsBlockEntityAndThreeArgSetBlockRefusal() {
        FakeNmsHost host = new FakeNmsHost();
        NmsHostBinding.install(host);
        try {
            ServerLevel level = ServerLevel.of(FakeNmsHost.WORLD_ID);
            assertEquals(-64, level.getMinBuildHeight());
            assertEquals(320, level.getMaxBuildHeight());
            assertEquals(384, level.getHeight());
            assertTrue(level.isInWorldBounds(new BlockPos(0, 64, 0)));
            assertFalse(level.isInWorldBounds(new BlockPos(0, 400, 0)));
            assertNull(level.getBlockEntity(new BlockPos(0, 64, 0)));

            BlockState stone = new BlockState(new Block(Material.STONE), "minecraft:stone", 1);
            assertEquals(1, Block.getId(stone));
            assertThrows(UnsupportedOperationException.class,
                    () -> level.setBlock(new BlockPos(0, 65, 0), stone, 3));
        } finally {
            NmsHostBinding.uninstall(host);
            host.close();
        }
    }
}
