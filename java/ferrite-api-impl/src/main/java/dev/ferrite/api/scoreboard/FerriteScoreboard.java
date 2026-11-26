package dev.ferrite.api.scoreboard;

import com.google.gson.Gson;
import java.util.EnumMap;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.UUID;
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

    private static final Gson HOST_GSON = new Gson();

    private final Map<String, FerriteObjective> objectives = new LinkedHashMap<>();
    private final Map<DisplaySlot, FerriteObjective> displaySlots = new EnumMap<>(DisplaySlot.class);
    private final Map<String, FerriteTeam> teams = new LinkedHashMap<>();
    private final Map<String, FerriteTeam> entryTeams = new LinkedHashMap<>();
    private final Set<UUID> viewers = new LinkedHashSet<>();

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
        broadcastObjective(objective, 0);
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
        broadcastTeam(team, 0, List.of());
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
        broadcastDisplay(slot, "");
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
        broadcastDisplay(slot, objective.rawName());
    }

    synchronized void clearObjectiveSlot(FerriteObjective objective, DisplaySlot slot) {
        if (displaySlots.remove(slot, objective)) {
            broadcastDisplay(slot, "");
        }
    }

    synchronized void removeObjective(FerriteObjective objective) {
        if (objectives.remove(objective.rawName(), objective)) {
            broadcastObjective(objective, 1);
        }
    }

    synchronized void assignEntryToTeam(String entry, FerriteTeam team) {
        FerriteTeam existing = entryTeams.get(entry);
        if (existing == team) {
            return;
        }
        if (existing != null) {
            existing.removeEntryInternal(entry);
            broadcastTeam(existing, 4, List.of(entry));
        }
        entryTeams.put(entry, team);
        team.addEntryInternal(entry);
        broadcastTeam(team, 3, List.of(entry));
    }

    synchronized boolean removeEntryFromTeam(String entry, FerriteTeam team) {
        if (entryTeams.get(entry) != team) {
            return false;
        }
        entryTeams.remove(entry);
        boolean removed = team.removeEntryInternal(entry);
        if (removed) {
            broadcastTeam(team, 4, List.of(entry));
        }
        return removed;
    }

    synchronized void removeTeam(FerriteTeam team) {
        if (teams.remove(team.rawName(), team)) {
            broadcastTeam(team, 1, List.of());
        }
        for (String entry : team.snapshotEntries()) {
            entryTeams.remove(entry, team);
        }
    }

    /** Attaches a player viewer and sends the full current scoreboard state to their client. */
    public synchronized void attachViewer(UUID playerId) {
        if (playerId == null) {
            return;
        }
        viewers.add(playerId);
        for (FerriteObjective objective : objectives.values()) {
            sendObjective(playerId, objective, 0);
            for (Map.Entry<String, Integer> entry : objective.snapshotScores().entrySet()) {
                sendScore(playerId, objective.rawName(), entry.getKey(), entry.getValue());
            }
        }
        for (Map.Entry<DisplaySlot, FerriteObjective> entry : displaySlots.entrySet()) {
            sendDisplay(playerId, entry.getKey(), entry.getValue().rawName());
        }
        for (FerriteTeam team : teams.values()) {
            sendTeam(playerId, team, 0, List.copyOf(team.snapshotEntries()));
        }
    }

    /** Detaches a player viewer and clears this scoreboard's objectives/teams from their client. */
    public synchronized void detachViewer(UUID playerId) {
        if (playerId == null || !viewers.remove(playerId)) {
            return;
        }
        for (DisplaySlot slot : displaySlots.keySet()) {
            sendDisplay(playerId, slot, "");
        }
        for (FerriteObjective objective : objectives.values()) {
            sendObjective(playerId, objective, 1);
        }
        for (FerriteTeam team : teams.values()) {
            sendTeam(playerId, team, 1, List.of());
        }
    }

    synchronized void broadcastObjective(FerriteObjective objective, int method) {
        if (viewers.isEmpty()) {
            return;
        }
        for (UUID viewer : viewers) {
            sendObjective(viewer, objective, method);
        }
    }

    synchronized void broadcastDisplay(DisplaySlot slot, String objectiveName) {
        if (viewers.isEmpty()) {
            return;
        }
        for (UUID viewer : viewers) {
            sendDisplay(viewer, slot, objectiveName);
        }
    }

    synchronized void broadcastScore(String objectiveName, String entry, int score) {
        if (viewers.isEmpty()) {
            return;
        }
        for (UUID viewer : viewers) {
            sendScore(viewer, objectiveName, entry, score);
        }
    }

    synchronized void broadcastResetScore(String objectiveName, String entry) {
        if (viewers.isEmpty()) {
            return;
        }
        for (UUID viewer : viewers) {
            sendResetScore(viewer, objectiveName, entry);
        }
    }

    synchronized void broadcastTeam(FerriteTeam team, int method, List<String> players) {
        if (viewers.isEmpty()) {
            return;
        }
        for (UUID viewer : viewers) {
            sendTeam(viewer, team, method, players);
        }
    }

    private static void sendObjective(UUID playerId, FerriteObjective objective, int method) {
        Map<String, Object> req = new LinkedHashMap<>();
        req.put("player_uuid", playerId.toString());
        req.put("kind", "objective");
        req.put("name", objective.rawName());
        req.put("method", method);
        req.put("display", objective.rawDisplayName());
        req.put("hearts", objective.rawRenderType() == RenderType.HEARTS);
        sendToHost(req);
    }

    private static void sendDisplay(UUID playerId, DisplaySlot slot, String objectiveName) {
        Map<String, Object> req = new LinkedHashMap<>();
        req.put("player_uuid", playerId.toString());
        req.put("kind", "display");
        req.put("slot", displaySlotToProtocol(slot));
        req.put("name", objectiveName);
        sendToHost(req);
    }

    private static void sendScore(UUID playerId, String objectiveName, String owner, int score) {
        Map<String, Object> req = new LinkedHashMap<>();
        req.put("player_uuid", playerId.toString());
        req.put("kind", "score");
        req.put("name", objectiveName);
        req.put("owner", owner);
        req.put("score", score);
        sendToHost(req);
    }

    private static void sendResetScore(UUID playerId, String objectiveName, String owner) {
        Map<String, Object> req = new LinkedHashMap<>();
        req.put("player_uuid", playerId.toString());
        req.put("kind", "reset");
        req.put("name", objectiveName);
        req.put("owner", owner);
        sendToHost(req);
    }

    private static void sendTeam(UUID playerId, FerriteTeam team, int method, List<String> players) {
        Map<String, Object> req = new LinkedHashMap<>();
        req.put("player_uuid", playerId.toString());
        req.put("kind", "team");
        req.put("name", team.rawName());
        req.put("method", method);
        if (method == 0 || method == 2) {
            req.put("params", team.protocolParams());
        }
        if (method == 0 || method == 3 || method == 4) {
            req.put("players", players);
        }
        sendToHost(req);
    }

    private static int displaySlotToProtocol(DisplaySlot slot) {
        if (slot == null) {
            return 1;
        }
        return switch (slot) {
            case PLAYER_LIST -> 0;
            case SIDEBAR -> 1;
            case BELOW_NAME -> 2;
            default -> 1;
        };
    }

    private static void sendToHost(Map<String, Object> req) {
        try {
            dev.ferrite.host.Bootstrap.playerScoreboardUpdate(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
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
