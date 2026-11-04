package net.minecraft.network.protocol.game;

/** Experience bar, level and total as the client shows them. */
public record ClientboundSetExperiencePacket(float experienceProgress, int experienceLevel, int totalExperience) implements net.minecraft.network.protocol.Packet<Object> {}
