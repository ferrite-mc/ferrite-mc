package dev.ferrite.server;

/** Spigot's extra world surface; lightning is the world's own strike path, silence is not reported by the host. */
final class FerriteWorldSpigot extends org.bukkit.World.Spigot {
    private final org.bukkit.World world;

    FerriteWorldSpigot(org.bukkit.World world) {
        this.world = java.util.Objects.requireNonNull(world, "world");
    }

    @Override public org.bukkit.entity.LightningStrike strikeLightning(org.bukkit.Location location, boolean isSilent) {
        return world.strikeLightning(location);
    }

    @Override public org.bukkit.entity.LightningStrike strikeLightningEffect(org.bukkit.Location location, boolean isSilent) {
        return world.strikeLightningEffect(location);
    }
}
