package net.minecraft.network.protocol.game;

/** Title timings in ticks. */
public record ClientboundSetTitlesAnimationPacket(int fadeIn, int stay, int fadeOut) implements net.minecraft.network.protocol.Packet<Object> {}
