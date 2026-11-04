package dev.ferrite.server;

/** Isolated in-memory scoreboards; team and entry state is local to each board. */
final class FerriteScoreboards implements org.bukkit.scoreboard.ScoreboardManager {
    private final org.bukkit.scoreboard.Scoreboard main = newBoard();
    public org.bukkit.scoreboard.Scoreboard getMainScoreboard() { return main; }
    public org.bukkit.scoreboard.Scoreboard getNewScoreboard() { return newBoard(); }
    private static org.bukkit.scoreboard.Scoreboard newBoard() {
        var teams = new java.util.concurrent.ConcurrentHashMap<String, TeamState>();
        java.lang.reflect.InvocationHandler board = (proxy, method, args) -> switch (method.getName()) {
            case "getTeam" -> { var team = teams.get((String) args[0]); yield team == null ? null : team.proxy; }
            case "registerNewTeam" -> {
                String name = (String) args[0];
                var state = new TeamState(name, (org.bukkit.scoreboard.Scoreboard) proxy, teams);
                if (teams.putIfAbsent(name, state) != null) throw new IllegalArgumentException("Team exists: " + name);
                yield state.proxy;
            }
            case "getTeams" -> teams.values().stream().map(t -> t.proxy).collect(java.util.stream.Collectors.toUnmodifiableSet());
            case "getEntryTeam" -> teams.values().stream().filter(t -> t.entries.contains((String) args[0])).map(t -> t.proxy).findFirst().orElse(null);
            case "getEntries" -> { var entries = new java.util.HashSet<String>(); teams.values().forEach(t -> entries.addAll(t.entries)); yield java.util.Set.copyOf(entries); }
            case "hashCode" -> System.identityHashCode(proxy);
            case "equals" -> proxy == args[0];
            case "toString" -> "FerriteScoreboard";
            default -> HostBridge.dispatch("Scoreboard", "", method, args);
        };
        return (org.bukkit.scoreboard.Scoreboard) java.lang.reflect.Proxy.newProxyInstance(FerriteScoreboards.class.getClassLoader(), new Class<?>[]{org.bukkit.scoreboard.Scoreboard.class}, board);
    }
    private static final class TeamState implements java.lang.reflect.InvocationHandler {
        final String name;
        final org.bukkit.scoreboard.Scoreboard board;
        final java.util.Map<String, TeamState> teams;
        final java.util.Set<String> entries = java.util.concurrent.ConcurrentHashMap.newKeySet();
        final org.bukkit.scoreboard.Team proxy;
        TeamState(String name, org.bukkit.scoreboard.Scoreboard board, java.util.Map<String, TeamState> teams) {
            this.name = name; this.board = board; this.teams = teams;
            proxy = (org.bukkit.scoreboard.Team) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.scoreboard.Team.class}, this);
        }
        public Object invoke(Object self, java.lang.reflect.Method method, Object[] args) {
            return switch (method.getName()) {
                case "getName" -> name;
                case "getScoreboard" -> teams.get(name) == this ? board : null;
                case "addEntry" -> { String entry = (String) args[0]; teams.values().forEach(t -> t.entries.remove(entry)); entries.add(entry); yield null; }
                case "removeEntry" -> entries.remove((String) args[0]);
                case "hasEntry" -> entries.contains((String) args[0]);
                case "getEntries" -> java.util.Set.copyOf(entries);
                case "getSize" -> entries.size();
                case "unregister" -> { if (!teams.remove(name, this)) throw new IllegalStateException("Team is unregistered"); yield null; }
                case "hashCode" -> System.identityHashCode(self);
                case "equals" -> self == args[0];
                case "toString" -> "Team[" + name + "]";
                default -> HostBridge.dispatch("Team", "", method, args);
            };
        }
    }
}
