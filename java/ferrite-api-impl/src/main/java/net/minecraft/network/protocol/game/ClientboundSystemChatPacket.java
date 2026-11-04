package net.minecraft.network.protocol.game;

/** Chat or action-bar line sent by the server. */
public record ClientboundSystemChatPacket(net.minecraft.network.chat.Component content, boolean overlay) implements net.minecraft.network.protocol.Packet<Object> {}
