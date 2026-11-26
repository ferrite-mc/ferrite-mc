// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.world.level.block.entity;

import net.minecraft.core.BlockPos;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.level.block.state.BlockState;

/** Clean-room block entity handle matching 26.3 {@code net.minecraft.world.level.block.entity.BlockEntity}. */
public abstract class BlockEntity {
    private final BlockPos worldPosition;
    private final BlockState blockState;
    private ServerLevel level;

    protected BlockEntity(BlockPos pos, BlockState state) {
        this.worldPosition = pos;
        this.blockState = state;
    }

    public BlockPos getBlockPos() {
        return worldPosition;
    }

    public BlockState getBlockState() {
        return blockState;
    }

    public ServerLevel getLevel() {
        return level;
    }

    public void setLevel(ServerLevel level) {
        this.level = level;
    }
}
