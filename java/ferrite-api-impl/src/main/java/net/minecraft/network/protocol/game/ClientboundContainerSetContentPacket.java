package net.minecraft.network.protocol.game;

/** Replaces every slot in a container and the carried stack. */
public record ClientboundContainerSetContentPacket(int containerId, int stateId, java.util.List<net.minecraft.world.item.ItemStack> items, net.minecraft.world.item.ItemStack carriedItem) implements net.minecraft.network.protocol.Packet<Object> {
    public ClientboundContainerSetContentPacket {
        items = java.util.List.copyOf(items);
        carriedItem = carriedItem == null ? net.minecraft.world.item.ItemStack.EMPTY : carriedItem;
    }
}
