package dev.ferrite.server;

import java.util.Objects;
import java.util.Set;
import java.util.function.Function;
import org.bukkit.FeatureFlag;
import org.bukkit.GameRule;
import org.bukkit.NamespacedKey;

/** Paper supplies the alias conversions; preserve them rather than inventing legacy names/defaults. */
final class FerriteLegacyGameRule<M, L> extends GameRule<L> {
    final GameRule<M> modern;
    final Function<L, M> toModern;
    final Function<M, L> toLegacy;
    private final Class<L> type;

    FerriteLegacyGameRule(GameRule<M> modern, Function<L, M> toModern, Function<M, L> toLegacy, Class<L> type) {
        this.modern = Objects.requireNonNull(modern, "modern");
        this.toModern = Objects.requireNonNull(toModern, "toModern");
        this.toLegacy = Objects.requireNonNull(toLegacy, "toLegacy");
        this.type = Objects.requireNonNull(type, "type");
    }
    @Override public NamespacedKey getKey() { return modern.getKey(); }
    @Override public String getName() { return modern.getName(); }
    @Override public Class<L> getType() { return type; }
    @Override public L getDefaultValue() { return type.cast(toLegacy.apply(modern.getDefaultValue())); }
    @Override public String translationKey() { return modern.translationKey(); }
    @Override public Set<FeatureFlag> requiredFeatures() { return modern.requiredFeatures(); }
}
