package net.minecraft.network.protocol.game;

/** The action-bar line. */
public record ClientboundSetActionBarTextPacket(net.minecraft.network.chat.Component text) implements net.minecraft.network.protocol.Packet<Object> {}
