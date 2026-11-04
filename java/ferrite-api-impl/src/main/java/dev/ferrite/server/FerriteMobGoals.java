package dev.ferrite.server;

import com.destroystokyo.paper.entity.ai.Goal;
import com.destroystokyo.paper.entity.ai.GoalKey;
import com.destroystokyo.paper.entity.ai.GoalType;
import com.destroystokyo.paper.entity.ai.MobGoals;
import java.util.ArrayList;
import java.util.Collection;
import java.util.List;
import java.util.Objects;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.Predicate;
import org.bukkit.entity.Mob;

/**
 * Custom mob goals (Server#getMobGoals), kept per mob in memory and ordered by priority.
 *
 * <p>The Rust host owns mob AI, so added goals are registered and queryable here but are not
 * ticked by the Java side; the running-goal views therefore stay empty.
 */
final class FerriteMobGoals implements MobGoals {
    private record GoalEntry(int priority, Goal<?> goal) {}

    private final ConcurrentHashMap<UUID, List<GoalEntry>> goals = new ConcurrentHashMap<>();

    private List<GoalEntry> goalsOf(Mob mob) {
        return goals.getOrDefault(mob.getUniqueId(), List.of());
    }

    @Override public <T extends Mob> void addGoal(T mob, int priority, Goal<T> goal) {
        Objects.requireNonNull(mob, "mob");
        Objects.requireNonNull(goal, "goal");
        if (priority < 0) throw new IllegalArgumentException("Goal priority must not be negative");
        goals.compute(mob.getUniqueId(), (id, current) -> {
            var updated = current == null ? new ArrayList<GoalEntry>() : new ArrayList<>(current);
            updated.add(new GoalEntry(priority, goal));
            updated.sort(java.util.Comparator.comparingInt(GoalEntry::priority));
            return List.copyOf(updated);
        });
    }

    @Override public <T extends Mob> void removeGoal(T mob, Goal<T> goal) {
        Objects.requireNonNull(mob, "mob");
        Objects.requireNonNull(goal, "goal");
        goals.computeIfPresent(mob.getUniqueId(), (id, current) -> {
            var updated = new ArrayList<GoalEntry>();
            boolean removed = false;
            for (var entry : current) {
                if (!removed && entry.goal() == goal) { removed = true; continue; }
                updated.add(entry);
            }
            return updated.isEmpty() ? null : List.copyOf(updated);
        });
    }

    @Override public <T extends Mob> void removeGoal(T mob, GoalKey<T> key) {
        Objects.requireNonNull(mob, "mob");
        Objects.requireNonNull(key, "key");
        goals.computeIfPresent(mob.getUniqueId(), (id, current) -> {
            var updated = current.stream().filter(entry -> !entry.goal().getKey().equals(key)).toList();
            return updated.isEmpty() ? null : updated;
        });
    }

    @Override public <T extends Mob> void removeAllGoals(T mob) {
        Objects.requireNonNull(mob, "mob");
        goals.remove(mob.getUniqueId());
    }

    @Override public <T extends Mob> void removeAllGoals(T mob, GoalType type) {
        Objects.requireNonNull(mob, "mob");
        Objects.requireNonNull(type, "type");
        goals.computeIfPresent(mob.getUniqueId(), (id, current) -> {
            var updated = current.stream().filter(entry -> !entry.goal().getTypes().contains(type)).toList();
            return updated.isEmpty() ? null : updated;
        });
    }

    @Override public <T extends Mob> boolean hasGoal(T mob, GoalKey<T> key) {
        Objects.requireNonNull(mob, "mob");
        Objects.requireNonNull(key, "key");
        return goalsOf(mob).stream().anyMatch(entry -> entry.goal().getKey().equals(key));
    }

    @Override public <T extends Mob> Goal<T> getGoal(T mob, GoalKey<T> key) {
        Objects.requireNonNull(mob, "mob");
        Objects.requireNonNull(key, "key");
        List<Goal<T>> matches = matching(mob, entry -> entry.goal().getKey().equals(key));
        return matches.isEmpty() ? null : matches.get(0);
    }

    @Override public <T extends Mob> Collection<Goal<T>> getGoals(T mob, GoalKey<T> key) {
        Objects.requireNonNull(mob, "mob");
        Objects.requireNonNull(key, "key");
        return matching(mob, entry -> entry.goal().getKey().equals(key));
    }

    @Override public <T extends Mob> Collection<Goal<T>> getAllGoals(T mob) {
        Objects.requireNonNull(mob, "mob");
        return matching(mob, entry -> true);
    }

    @Override public <T extends Mob> Collection<Goal<T>> getAllGoals(T mob, GoalType type) {
        Objects.requireNonNull(mob, "mob");
        Objects.requireNonNull(type, "type");
        return matching(mob, entry -> entry.goal().getTypes().contains(type));
    }

    @Override public <T extends Mob> Collection<Goal<T>> getAllGoalsWithout(T mob, GoalType type) {
        Objects.requireNonNull(mob, "mob");
        Objects.requireNonNull(type, "type");
        return matching(mob, entry -> !entry.goal().getTypes().contains(type));
    }

    /** Nothing ticks on the Java side, so no registered goal is running. */
    @Override public <T extends Mob> Collection<Goal<T>> getRunningGoals(T mob) {
        Objects.requireNonNull(mob, "mob");
        return List.of();
    }

    @Override public <T extends Mob> Collection<Goal<T>> getRunningGoals(T mob, GoalType type) {
        Objects.requireNonNull(mob, "mob");
        Objects.requireNonNull(type, "type");
        return List.of();
    }

    @Override public <T extends Mob> Collection<Goal<T>> getRunningGoalsWithout(T mob, GoalType type) {
        Objects.requireNonNull(mob, "mob");
        Objects.requireNonNull(type, "type");
        return List.of();
    }

    @SuppressWarnings("unchecked")
    private <T extends Mob> List<Goal<T>> matching(Mob mob, Predicate<GoalEntry> keep) {
        var out = new ArrayList<Goal<T>>();
        for (var entry : goalsOf(mob)) if (keep.test(entry)) out.add((Goal<T>) entry.goal());
        return List.copyOf(out);
    }
}
