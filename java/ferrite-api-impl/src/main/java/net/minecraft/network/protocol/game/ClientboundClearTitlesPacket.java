package net.minecraft.network.protocol.game;

/** Clears the title, and the timings too when resetTimes is set. */
public record ClientboundClearTitlesPacket(boolean resetTimes) implements net.minecraft.network.protocol.Packet<Object> {}
