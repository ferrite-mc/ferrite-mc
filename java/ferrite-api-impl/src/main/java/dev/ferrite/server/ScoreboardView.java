package dev.ferrite.server;

import java.util.HashMap;
import java.util.Map;
import java.util.function.BiConsumer;

/**
 * What one client currently shows of a scoreboard. {@link #show} compares the board with what the
 * client has and sends only the difference, so each score change costs one packet.
 */
final class ScoreboardView {
    private record Known(int slot, String title, boolean hearts, Map<String, Integer> scores) { }

    private final BiConsumer<String, Object[]> sender;
    private final Map<String, Known> known = new HashMap<>();
    private record KnownTeam(String params, java.util.Set<String> entries) { }
    private final Map<String, KnownTeam> knownTeams = new HashMap<>();
    private Runnable stopWatching = () -> { };

    ScoreboardView(BiConsumer<String, Object[]> sender) { this.sender = sender; }

    private void send(String method, Object... args) { sender.accept(method, args); }

    /** Starts showing the board and keeps the client in step with it until another board is shown. */
    synchronized void attach(org.bukkit.scoreboard.Scoreboard board) {
        stopWatching.run();
        refresh(board);
        stopWatching = FerriteScoreboards.watch(board, () -> refresh(board));
    }

    private static final String TEAM = "sbTeam(String,int,String,String)";

    private static String joined(java.util.Collection<String> players) { return String.join(String.valueOf(HostBridge.ITEM), players); }

    private void refreshTeams(org.bukkit.scoreboard.Scoreboard board) {
        var wanted = new HashMap<String, FerriteScoreboards.TeamShown>();
        for (var team : FerriteScoreboards.teams(board)) wanted.put(team.name(), team);
        for (var name : new java.util.ArrayList<>(knownTeams.keySet())) {
            if (wanted.containsKey(name)) continue;
            send(TEAM, name, 1, "", "");
            knownTeams.remove(name);
        }
        for (var team : wanted.values()) {
            var old = knownTeams.get(team.name());
            if (old == null || !old.params().equals(team.params())) {
                // A changed team is rebuilt: removed, created again and refilled.
                if (old != null) send(TEAM, team.name(), 1, "", "");
                send(TEAM, team.name(), 0, team.params(), joined(team.entries()));
            } else {
                var joining = new java.util.TreeSet<>(team.entries());
                joining.removeAll(old.entries());
                var leaving = new java.util.TreeSet<>(old.entries());
                leaving.removeAll(team.entries());
                if (!joining.isEmpty()) send(TEAM, team.name(), 2, "", joined(joining));
                if (!leaving.isEmpty()) send(TEAM, team.name(), 3, "", joined(leaving));
            }
            knownTeams.put(team.name(), new KnownTeam(team.params(), new java.util.TreeSet<>(team.entries())));
        }
    }

    synchronized void refresh(org.bukkit.scoreboard.Scoreboard board) {
        refreshTeams(board);
        var wanted = new HashMap<String, FerriteScoreboards.Shown>();
        for (var shown : FerriteScoreboards.shown(board)) wanted.put(shown.name(), shown);
        for (var name : new java.util.ArrayList<>(known.keySet())) {
            if (wanted.containsKey(name)) continue;
            send("sbObjective(String,int,String,int)", name, 1, "", 0);
            known.remove(name);
        }
        for (var shown : wanted.values()) {
            var old = known.get(shown.name());
            if (old == null || old.slot() != shown.slot()) {
                if (old != null) send("sbObjective(String,int,String,int)", shown.name(), 1, "", 0);
                send("sbObjective(String,int,String,int)", shown.name(), 0, shown.title(), shown.hearts() ? 1 : 0);
                send("sbDisplay(int,String)", shown.slot(), shown.name());
                old = new Known(shown.slot(), shown.title(), shown.hearts(), new HashMap<>());
            } else if (!old.title().equals(shown.title()) || old.hearts() != shown.hearts()) {
                send("sbObjective(String,int,String,int)", shown.name(), 2, shown.title(), shown.hearts() ? 1 : 0);
                old = new Known(old.slot(), shown.title(), shown.hearts(), old.scores());
            }
            for (var entry : shown.scores().entrySet()) {
                if (!entry.getValue().equals(old.scores().get(entry.getKey()))) send("sbScore(String,String,int)", entry.getKey(), shown.name(), entry.getValue());
            }
            for (var entry : new java.util.ArrayList<>(old.scores().keySet())) {
                if (!shown.scores().containsKey(entry)) send("sbReset(String,String)", entry, shown.name());
            }
            old.scores().clear();
            old.scores().putAll(shown.scores());
            known.put(shown.name(), old);
        }
    }

    /** Takes down everything the client shows, for a player who leaves the board. */
    synchronized void clear() {
        stopWatching.run();
        stopWatching = () -> { };
        for (var name : known.keySet()) send("sbObjective(String,int,String,int)", name, 1, "", 0);
        known.clear();
        for (var name : knownTeams.keySet()) send(TEAM, name, 1, "", "");
        knownTeams.clear();
    }
}
