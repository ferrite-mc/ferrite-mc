package dev.ferrite.server;

import java.util.ArrayList;
import java.util.Collection;
import java.util.Date;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import org.bukkit.advancement.Advancement;
import org.bukkit.advancement.AdvancementProgress;

/** Progress one player made on one advancement (Player#getAdvancementProgress), held in memory. */
final class FerriteAdvancementProgress implements AdvancementProgress {
    private record ProgressKey(UUID player, org.bukkit.NamespacedKey advancement) {}
    private static final ConcurrentHashMap<ProgressKey, FerriteAdvancementProgress> progress = new ConcurrentHashMap<>();

    static AdvancementProgress of(UUID player, Advancement advancement) {
        Objects.requireNonNull(player, "player");
        Objects.requireNonNull(advancement, "advancement");
        return progress.computeIfAbsent(new ProgressKey(player, advancement.getKey()),
                key -> new FerriteAdvancementProgress(advancement));
    }

    private final Advancement advancement;
    private final Map<String, Date> awarded = new LinkedHashMap<>();

    private FerriteAdvancementProgress(Advancement advancement) { this.advancement = advancement; }

    @Override public Advancement getAdvancement() { return advancement; }

    /** Done when every requirement group has at least one awarded criterion. */
    @Override public synchronized boolean isDone() {
        for (var requirement : advancement.getRequirements().getRequirements()) {
            boolean satisfied = false;
            for (var criterion : requirement.getRequiredCriteria())
                if (awarded.containsKey(criterion)) { satisfied = true; break; }
            if (!satisfied) return false;
        }
        return true;
    }

    @Override public synchronized boolean awardCriteria(String criteria) {
        requireKnown(criteria);
        return awarded.putIfAbsent(criteria, new Date()) == null;
    }

    @Override public synchronized boolean revokeCriteria(String criteria) {
        requireKnown(criteria);
        return awarded.remove(criteria) != null;
    }

    @Override public synchronized Date getDateAwarded(String criteria) {
        requireKnown(criteria);
        var date = awarded.get(criteria);
        return date == null ? null : (Date) date.clone();
    }

    @Override public synchronized Collection<String> getRemainingCriteria() {
        var remaining = new ArrayList<String>();
        for (var criterion : advancement.getCriteria()) if (!awarded.containsKey(criterion)) remaining.add(criterion);
        return List.copyOf(remaining);
    }

    @Override public synchronized Collection<String> getAwardedCriteria() {
        var done = new ArrayList<String>();
        for (var criterion : advancement.getCriteria()) if (awarded.containsKey(criterion)) done.add(criterion);
        return List.copyOf(done);
    }

    private void requireKnown(String criteria) {
        Objects.requireNonNull(criteria, "criteria");
        if (!advancement.getCriteria().contains(criteria)) throw new IllegalArgumentException("Unknown criteria: " + criteria);
    }
}
