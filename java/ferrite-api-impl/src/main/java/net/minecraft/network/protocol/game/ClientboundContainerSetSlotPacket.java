package net.minecraft.network.protocol.game;

/** Replaces one container slot, or the carried stack when the slot is -1. */
public final class ClientboundContainerSetSlotPacket implements net.minecraft.network.protocol.Packet<Object> {
    private final int containerId;
    private final int stateId;
    private final int slot;
    private final net.minecraft.world.item.ItemStack item;

    public ClientboundContainerSetSlotPacket(int containerId, int stateId, int slot, net.minecraft.world.item.ItemStack item) {
        this.containerId = containerId;
        this.stateId = stateId;
        this.slot = slot;
        this.item = item == null ? net.minecraft.world.item.ItemStack.EMPTY : item;
    }

    public int getContainerId() { return containerId; }
    public int getStateId() { return stateId; }
    public int getSlot() { return slot; }
    public net.minecraft.world.item.ItemStack getItem() { return item; }
}
