package dev.ferrite.api.scoreboard;

import java.util.EnumMap;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.Map;
import java.util.Set;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer;
import org.bukkit.OfflinePlayer;
import org.bukkit.entity.Entity;
import org.bukkit.entity.Player;
import org.bukkit.scoreboard.Criteria;
import org.bukkit.scoreboard.DisplaySlot;
import org.bukkit.scoreboard.Objective;
import org.bukkit.scoreboard.RenderType;
import org.bukkit.scoreboard.Score;
import org.bukkit.scoreboard.Scoreboard;
import org.bukkit.scoreboard.Team;

/** State-backed {@link Scoreboard} implementation for {@link FerriteScoreboardManager}. */
@SuppressWarnings("deprecation")
public final class FerriteScoreboard implements Scoreboard {

    private final Map<String, FerriteObjective> objectives = new LinkedHashMap<>();
    private final Map<DisplaySlot, FerriteObjective> displaySlots = new EnumMap<>(DisplaySlot.class);
    private final Map<String, FerriteTeam> teams = new LinkedHashMap<>();
    private final Map<String, FerriteTeam> entryTeams = new LinkedHashMap<>();

    @Override
    public Objective registerNewObjective(
            String name, String criteria, Component displayName, RenderType renderType) {
        if (criteria == null) {
            throw new IllegalArgumentException("Criteria cannot be null");
        }
        return registerNewObjective(name, FerriteCriteria.getOrCreate(criteria), displayName, renderType);
    }

    @Override
    public synchronized Objective registerNewObjective(
            String name, Criteria criteria, Component displayName, RenderType renderType) {
        if (name == null) {
            throw new IllegalArgumentException("Objective name cannot be null");
        }
        if (criteria == null) {
            throw new IllegalArgumentException("Criteria cannot be null");
        }
        if (displayName == null) {
            throw new IllegalArgumentException("Display name cannot be null");
        }
        if (renderType == null) {
            throw new IllegalArgumentException("RenderType cannot be null");
        }
        if (objectives.containsKey(name)) {
            throw new IllegalArgumentException("An objective of name '" + name + "' already exists");
        }
        FerriteObjective objective = new FerriteObjective(this, name, criteria, displayName, renderType);
        objectives.put(name, objective);
        return objective;
    }

    @Override
    public Objective registerNewObjective(
            String name, String criteria, String displayName, RenderType renderType) {
        if (displayName == null) {
            throw new IllegalArgumentException("Display name cannot be null");
        }
        return registerNewObjective(
                name,
                criteria,
                LegacyComponentSerializer.legacySection().deserialize(displayName),
                renderType);
    }

    @Override
    public synchronized Objective getObjective(String name) {
        if (name == null) {
            throw new IllegalArgumentException("Name cannot be null");
        }
        return objectives.get(name);
    }

    @Override
    public synchronized Set<Objective> getObjectivesByCriteria(String criteria) {
        if (criteria == null) {
            throw new IllegalArgumentException("Criteria cannot be null");
        }
        Set<Objective> matching = new LinkedHashSet<>();
        for (FerriteObjective objective : objectives.values()) {
            if (objective.getCriteria().equals(criteria)) {
                matching.add(objective);
            }
        }
        return Set.copyOf(matching);
    }

    @Override
    public synchronized Set<Objective> getObjectivesByCriteria(Criteria criteria) {
        if (criteria == null) {
            throw new IllegalArgumentException("Criteria cannot be null");
        }
        Set<Objective> matching = new LinkedHashSet<>();
        for (FerriteObjective objective : objectives.values()) {
            if (objective.getTrackedCriteria().equals(criteria)) {
                matching.add(objective);
            }
        }
        return Set.copyOf(matching);
    }

    @Override
    public synchronized Set<Objective> getObjectives() {
        return Set.copyOf(objectives.values());
    }

    @Override
    public synchronized Objective getObjective(DisplaySlot slot) {
        if (slot == null) {
            throw new IllegalArgumentException("DisplaySlot cannot be null");
        }
        return displaySlots.get(slot);
    }

    @Override
    public Set<Score> getScores(OfflinePlayer player) {
        if (player == null) {
            throw new IllegalArgumentException("OfflinePlayer cannot be null");
        }
        return getScores(entryName(player));
    }

    @Override
    public synchronized Set<Score> getScores(String entry) {
        if (entry == null) {
            throw new IllegalArgumentException("Entry cannot be null");
        }
        Set<Score> scores = new LinkedHashSet<>();
        for (FerriteObjective objective : objectives.values()) {
            scores.add(objective.getScore(entry));
        }
        return Set.copyOf(scores);
    }

    @Override
    public void resetScores(OfflinePlayer player) {
        if (player == null) {
            throw new IllegalArgumentException("OfflinePlayer cannot be null");
        }
        resetScores(entryName(player));
    }

    @Override
    public synchronized void resetScores(String entry) {
        if (entry == null) {
            throw new IllegalArgumentException("Entry cannot be null");
        }
        for (FerriteObjective objective : objectives.values()) {
            objective.resetScoreValue(entry);
        }
    }

    @Override
    public Team getPlayerTeam(OfflinePlayer player) {
        if (player == null) {
            throw new IllegalArgumentException("OfflinePlayer cannot be null");
        }
        return getEntryTeam(entryName(player));
    }

    @Override
    public synchronized Team getEntryTeam(String entry) {
        if (entry == null) {
            throw new IllegalArgumentException("Entry cannot be null");
        }
        return entryTeams.get(entry);
    }

    @Override
    public synchronized Team getTeam(String teamName) {
        if (teamName == null) {
            throw new IllegalArgumentException("Team name cannot be null");
        }
        return teams.get(teamName);
    }

    @Override
    public synchronized Set<Team> getTeams() {
        return Set.copyOf(teams.values());
    }

    @Override
    public synchronized Team registerNewTeam(String name) {
        if (name == null) {
            throw new IllegalArgumentException("Team name cannot be null");
        }
        if (teams.containsKey(name)) {
            throw new IllegalArgumentException("Team name '" + name + "' is already in use");
        }
        FerriteTeam team = new FerriteTeam(this, name);
        teams.put(name, team);
        return team;
    }

    @Override
    public Set<OfflinePlayer> getPlayers() {
        Set<OfflinePlayer> players = new LinkedHashSet<>();
        for (String entry : getEntries()) {
            players.add(FerriteOfflinePlayer.resolve(entry));
        }
        return Set.copyOf(players);
    }

    @Override
    public synchronized Set<String> getEntries() {
        Set<String> all = new LinkedHashSet<>();
        for (FerriteObjective objective : objectives.values()) {
            all.addAll(objective.scoredEntries());
        }
        return Set.copyOf(all);
    }

    @Override
    public synchronized void clearSlot(DisplaySlot slot) {
        if (slot == null) {
            throw new IllegalArgumentException("Slot cannot be null");
        }
        FerriteObjective previous = displaySlots.remove(slot);
        if (previous != null) {
            previous.clearDisplaySlotField();
        }
    }

    @Override
    public Set<Score> getScoresFor(Entity entity) {
        if (entity == null) {
            throw new IllegalArgumentException("Entity cannot be null");
        }
        return getScores(entityEntryName(entity));
    }

    @Override
    public void resetScoresFor(Entity entity) {
        if (entity == null) {
            throw new IllegalArgumentException("Entity cannot be null");
        }
        resetScores(entityEntryName(entity));
    }

    @Override
    public Team getEntityTeam(Entity entity) {
        if (entity == null) {
            throw new IllegalArgumentException("Entity cannot be null");
        }
        return getEntryTeam(entityEntryName(entity));
    }

    synchronized void assignDisplaySlot(DisplaySlot slot, FerriteObjective objective) {
        FerriteObjective previous = displaySlots.put(slot, objective);
        if (previous != null && previous != objective) {
            previous.clearDisplaySlotField();
        }
    }

    synchronized void clearObjectiveSlot(FerriteObjective objective, DisplaySlot slot) {
        displaySlots.remove(slot, objective);
    }

    synchronized void removeObjective(FerriteObjective objective) {
        objectives.remove(objective.rawName(), objective);
    }

    synchronized void assignEntryToTeam(String entry, FerriteTeam team) {
        FerriteTeam existing = entryTeams.get(entry);
        if (existing == team) {
            return;
        }
        if (existing != null) {
            existing.removeEntryInternal(entry);
        }
        entryTeams.put(entry, team);
        team.addEntryInternal(entry);
    }

    synchronized boolean removeEntryFromTeam(String entry, FerriteTeam team) {
        if (entryTeams.get(entry) != team) {
            return false;
        }
        entryTeams.remove(entry);
        return team.removeEntryInternal(entry);
    }

    synchronized void removeTeam(FerriteTeam team) {
        teams.remove(team.rawName(), team);
        for (String entry : team.snapshotEntries()) {
            entryTeams.remove(entry, team);
        }
    }

    static String entryName(OfflinePlayer player) {
        String name = player.getName();
        return name != null ? name : player.getUniqueId().toString();
    }

    static String entityEntryName(Entity entity) {
        if (entity instanceof Player player) {
            return player.getName();
        }
        return entity.getUniqueId().toString();
    }
}
