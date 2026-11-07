package net.minecraft.network.protocol.game;

/** Health, food and saturation as the client shows them. */
public record ClientboundSetHealthPacket(float health, int food, float saturation) implements net.minecraft.network.protocol.Packet<Object> {
    public float getHealth() { return health; }
    public int getFood() { return food; }
    public float getSaturation() { return saturation; }
}
