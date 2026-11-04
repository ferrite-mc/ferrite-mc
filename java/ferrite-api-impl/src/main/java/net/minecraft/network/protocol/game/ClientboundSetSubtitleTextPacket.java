package net.minecraft.network.protocol.game;

/** The subtitle line, kept until the next title packet shows it. */
public record ClientboundSetSubtitleTextPacket(net.minecraft.network.chat.Component text) implements net.minecraft.network.protocol.Packet<Object> {}
