package dev.ferrite.api.server;

import io.papermc.paper.ServerBuildInfo;
import java.time.Instant;
import java.util.Optional;
import java.util.OptionalInt;
import net.kyori.adventure.key.Key;

/**
 * Supplies Paper's {@link ServerBuildInfo} service so {@link org.bukkit.Bukkit#setServer}
 * works without a CraftBukkit/Paper server jar.
 *
 * <p>The API's build-info lookup is a {@link java.util.ServiceLoader} service. This
 * provider reports Ferrite's real identity and 26.3 target; it never claims to be Paper.
 */
public final class FerriteServerBuildInfo implements ServerBuildInfo {

    /** Ferrite's brand key, distinct from Paper's {@code paper} brand. */
    public static final Key BRAND_FERRITE = Key.key("ferrite", "ferrite");

    private static final Instant BUILD_TIME = Instant.now();

    @Override
    public Key brandId() {
        return BRAND_FERRITE;
    }

    @Override
    public boolean isBrandCompatible(Key other) {
        return BRAND_FERRITE.equals(other);
    }

    @Override
    public String brandName() {
        return "Ferrite";
    }

    @Override
    public String minecraftVersionId() {
        return "26.3";
    }

    @Override
    public String minecraftVersionName() {
        return "26.3";
    }

    @Override
    public OptionalInt buildNumber() {
        return OptionalInt.empty();
    }

    @Override
    public Instant buildTime() {
        return BUILD_TIME;
    }

    @Override
    public Optional<String> gitBranch() {
        return Optional.empty();
    }

    @Override
    public Optional<String> gitCommit() {
        return Optional.empty();
    }

    @Override
    public String asString(StringRepresentation representation) {
        return switch (representation) {
            case VERSION_SIMPLE -> "26.3";
            case VERSION_FULL -> "Ferrite 0.1.0 (MC: 26.3)";
        };
    }

    @Override
    public String toString() {
        return "FerriteServerBuildInfo{26.3}";
    }
}
