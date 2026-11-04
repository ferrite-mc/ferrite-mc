package net.minecraft.network.protocol.game;

/** The main line of a title; shown together with the stored subtitle and timings. */
public record ClientboundSetTitleTextPacket(net.minecraft.network.chat.Component text) implements net.minecraft.network.protocol.Packet<Object> {}
