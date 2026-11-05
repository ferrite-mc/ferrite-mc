package dev.ferrite.server;

import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.format.NamedTextColor;
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer;
import org.bukkit.scoreboard.Criteria;
import org.bukkit.scoreboard.DisplaySlot;
import org.bukkit.scoreboard.RenderType;

/**
 * In-memory scoreboards: objectives, scores, display slots and teams with their options. State is
 * local to each board, so a plugin reads back exactly what it set; showing a board to players is a
 * separate step the host does not take yet.
 */
final class FerriteScoreboards implements org.bukkit.scoreboard.ScoreboardManager {
    private static final LegacyComponentSerializer LEGACY = LegacyComponentSerializer.legacySection();
    private final org.bukkit.scoreboard.Scoreboard main = newBoard();
    public org.bukkit.scoreboard.Scoreboard getMainScoreboard() { return main; }
    public org.bukkit.scoreboard.Scoreboard getNewScoreboard() { return newBoard(); }

    private static Component component(Object value) {
        return value instanceof Component c ? c : value == null ? Component.empty() : LEGACY.deserialize(value.toString());
    }

    private static final class BoardState implements java.lang.reflect.InvocationHandler {
        final java.util.Map<String, TeamState> teams = new java.util.concurrent.ConcurrentHashMap<>();
        final java.util.Map<String, ObjectiveState> objectives = new java.util.LinkedHashMap<>();
        final java.util.Map<DisplaySlot, ObjectiveState> slots = new java.util.EnumMap<>(DisplaySlot.class);
        org.bukkit.scoreboard.Scoreboard proxy;
        final java.util.List<Runnable> listeners = new java.util.concurrent.CopyOnWriteArrayList<>();

        /** Tells everyone showing this board that something on it changed. */
        void touch() { for (var listener : listeners) listener.run(); }

        synchronized ObjectiveState register(Object[] args) {
            String name = (String) args[0];
            if (objectives.containsKey(name)) throw new IllegalArgumentException("An objective of name '" + name + "' already exists");
            String criteria = args[1] instanceof Criteria c ? c.getName() : (String) args[1];
            Object display = args.length > 2 ? args[2] : name;
            RenderType render = args.length > 3 && args[3] != null ? (RenderType) args[3] : RenderType.INTEGER;
            var state = new ObjectiveState(this, name, criteria, component(display), render);
            objectives.put(name, state);
            return state;
        }

        public synchronized Object invoke(Object self, java.lang.reflect.Method method, Object[] args) {
            var result = call(self, method, args);
            switch (method.getName()) {
                case "registerNewObjective", "registerNewTeam", "clearSlot", "resetScores" -> touch();
                default -> { }
            }
            return result;
        }

        private Object call(Object self, java.lang.reflect.Method method, Object[] args) {
            return switch (method.getName()) {
                case "registerNewObjective" -> register(args).proxy;
                case "getObjective" -> {
                    var found = args[0] instanceof DisplaySlot slot ? slots.get(slot) : objectives.get((String) args[0]);
                    yield found == null ? null : found.proxy;
                }
                case "getObjectives" -> objectives.values().stream().map(o -> o.proxy).collect(java.util.stream.Collectors.toUnmodifiableSet());
                case "getObjectivesByCriteria" -> {
                    String wanted = args[0] instanceof Criteria c ? c.getName() : (String) args[0];
                    yield objectives.values().stream().filter(o -> o.criteria.equals(wanted)).map(o -> o.proxy).collect(java.util.stream.Collectors.toUnmodifiableSet());
                }
                case "clearSlot" -> { slots.remove((DisplaySlot) args[0]); yield null; }
                case "getScores" -> {
                    String entry = entryName(args[0]);
                    var found = new java.util.LinkedHashSet<org.bukkit.scoreboard.Score>();
                    for (var objective : objectives.values()) if (objective.scores.containsKey(entry)) found.add(objective.score(entry));
                    yield java.util.Collections.unmodifiableSet(found);
                }
                case "resetScores" -> { String entry = entryName(args[0]); objectives.values().forEach(o -> o.scores.remove(entry)); yield null; }
                case "getTeam" -> { var team = teams.get((String) args[0]); yield team == null ? null : team.proxy; }
                case "registerNewTeam" -> {
                    String name = (String) args[0];
                    var state = new TeamState(name, proxy, teams);
                    if (teams.putIfAbsent(name, state) != null) throw new IllegalArgumentException("Team exists: " + name);
                    yield state.proxy;
                }
                case "getTeams" -> teams.values().stream().map(t -> t.proxy).collect(java.util.stream.Collectors.toUnmodifiableSet());
                case "getEntryTeam" -> teams.values().stream().filter(t -> t.entries.contains(entryName(args[0]))).map(t -> t.proxy).findFirst().orElse(null);
                case "getPlayerTeam" -> teams.values().stream().filter(t -> t.entries.contains(entryName(args[0]))).map(t -> t.proxy).findFirst().orElse(null);
                case "getEntries" -> {
                    var entries = new java.util.HashSet<String>();
                    teams.values().forEach(t -> entries.addAll(t.entries));
                    objectives.values().forEach(o -> entries.addAll(o.scores.keySet()));
                    yield java.util.Set.copyOf(entries);
                }
                case "hashCode" -> System.identityHashCode(self);
                case "equals" -> self == args[0];
                case "toString" -> "FerriteScoreboard";
                default -> HostBridge.dispatch("Scoreboard", "", method, args);
            };
        }
    }

    private static String entryName(Object entry) {
        return entry instanceof org.bukkit.OfflinePlayer player ? java.util.Objects.requireNonNullElse(player.getName(), player.getUniqueId().toString()) : String.valueOf(entry);
    }

    private static final java.util.Map<org.bukkit.scoreboard.Scoreboard, BoardState> STATES = java.util.Collections.synchronizedMap(new java.util.WeakHashMap<>());

    private static org.bukkit.scoreboard.Scoreboard newBoard() {
        var state = new BoardState();
        state.proxy = (org.bukkit.scoreboard.Scoreboard) java.lang.reflect.Proxy.newProxyInstance(FerriteScoreboards.class.getClassLoader(), new Class<?>[]{org.bukkit.scoreboard.Scoreboard.class}, state);
        STATES.put(state.proxy, state);
        return state.proxy;
    }

    /** What a client needs to draw a board: each displayed objective with its slot and scores. */
    record Shown(String name, int slot, String title, boolean hearts, java.util.Map<String, Integer> scores) { }

    /** The objectives of the board that sit in a slot the client can draw (list, sidebar, below name). */
    static java.util.List<Shown> shown(org.bukkit.scoreboard.Scoreboard board) {
        var state = STATES.get(board);
        if (state == null) return java.util.List.of();
        synchronized (state) {
            var result = new java.util.ArrayList<Shown>();
            for (var entry : state.slots.entrySet()) {
                int slot = switch (entry.getKey()) {
                    case PLAYER_LIST -> 0;
                    case SIDEBAR -> 1;
                    case BELOW_NAME -> 2;
                    default -> -1;
                };
                if (slot < 0) continue;
                var objective = entry.getValue();
                result.add(new Shown(objective.name, slot, net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(objective.display), objective.render == RenderType.HEARTS, new java.util.LinkedHashMap<>(objective.scores)));
            }
            return result;
        }
    }

    /** A team as a client needs it: its settings as one text, and who is in it. */
    record TeamShown(String name, String params, java.util.Set<String> entries) { }

    static java.util.List<TeamShown> teams(org.bukkit.scoreboard.Scoreboard board) {
        var state = STATES.get(board);
        if (state == null) return java.util.List.of();
        var result = new java.util.ArrayList<TeamShown>();
        for (var team : state.teams.values()) result.add(new TeamShown(team.name, team.clientParams(), new java.util.TreeSet<>(team.entries)));
        return result;
    }

    /** Runs the action whenever the board changes; the returned handle stops it. */
    static Runnable watch(org.bukkit.scoreboard.Scoreboard board, Runnable action) {
        var state = STATES.get(board);
        if (state == null) return () -> { };
        state.listeners.add(action);
        return () -> state.listeners.remove(action);
    }

    private static final class ObjectiveState implements java.lang.reflect.InvocationHandler {
        final BoardState board;
        final String name;
        final String criteria;
        Component display;
        RenderType render;
        boolean registered = true;
        final java.util.Map<String, Integer> scores = new java.util.LinkedHashMap<>();
        final org.bukkit.scoreboard.Objective proxy;

        ObjectiveState(BoardState board, String name, String criteria, Component display, RenderType render) {
            this.board = board; this.name = name; this.criteria = criteria; this.display = display; this.render = render;
            proxy = (org.bukkit.scoreboard.Objective) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.scoreboard.Objective.class}, this);
        }

        org.bukkit.scoreboard.Score score(String entry) {
            return (org.bukkit.scoreboard.Score) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.scoreboard.Score.class}, (self, method, args) -> switch (method.getName()) {
                case "getEntry" -> entry;
                case "getObjective" -> proxy;
                case "getScoreboard" -> board.proxy;
                case "getScore" -> { synchronized (board) { yield scores.getOrDefault(entry, 0); } }
                case "setScore" -> { synchronized (board) { check(); scores.put(entry, (int) args[0]); board.touch(); } yield null; }
                case "isScoreSet" -> { synchronized (board) { yield scores.containsKey(entry); } }
                case "resetScore" -> { synchronized (board) { check(); scores.remove(entry); board.touch(); } yield null; }
                case "hashCode" -> java.util.Objects.hash(name, entry);
                case "equals" -> args[0] != null && java.lang.reflect.Proxy.isProxyClass(args[0].getClass()) && args[0].toString().equals(self.toString());
                case "toString" -> "Score[" + name + ":" + entry + "]";
                default -> HostBridge.dispatch("Score", entry, method, args);
            });
        }

        private void check() { if (!registered) throw new IllegalStateException("Unregistered scoreboard component"); }

        public Object invoke(Object self, java.lang.reflect.Method method, Object[] args) {
            synchronized (board) {
                return switch (method.getName()) {
                    case "getName" -> name;
                    case "getDisplayName" -> LEGACY.serialize(display);
                    case "setDisplayName" -> { check(); display = component(args[0]); board.touch(); yield null; }
                    case "displayName" -> { if (args == null || args.length == 0) yield display; check(); display = args[0] == null ? Component.empty() : (Component) args[0]; board.touch(); yield null; }
                    case "getCriteria" -> criteria;
                    case "getTrackedCriteria" -> Criteria.create(criteria);
                    case "getScoreboard" -> registered ? board.proxy : null;
                    case "isModifiable" -> !Criteria.create(criteria).isReadOnly();
                    case "getRenderType" -> render;
                    case "setRenderType" -> { check(); render = (RenderType) args[0]; board.touch(); yield null; }
                    case "getDisplaySlot" -> board.slots.entrySet().stream().filter(e -> e.getValue() == this).map(java.util.Map.Entry::getKey).findFirst().orElse(null);
                    case "setDisplaySlot" -> {
                        check();
                        board.slots.values().removeIf(o -> o == this);
                        if (args[0] != null) board.slots.put((DisplaySlot) args[0], this);
                        board.touch();
                        yield null;
                    }
                    case "getScore" -> { check(); yield score(entryName(args[0])); }
                    case "unregister" -> { check(); board.objectives.remove(name); board.slots.values().removeIf(o -> o == this); registered = false; board.touch(); yield null; }
                    case "hashCode" -> System.identityHashCode(self);
                    case "equals" -> self == args[0];
                    case "toString" -> "Objective[" + name + "]";
                    default -> HostBridge.dispatch("Objective", name, method, args);
                };
            }
        }
    }

    private static final class TeamState implements java.lang.reflect.InvocationHandler {
        final String name;
        final org.bukkit.scoreboard.Scoreboard board;
        final java.util.Map<String, TeamState> teams;
        final java.util.Set<String> entries = java.util.concurrent.ConcurrentHashMap.newKeySet();
        final java.util.Map<org.bukkit.scoreboard.Team.Option, org.bukkit.scoreboard.Team.OptionStatus> options = new java.util.EnumMap<>(org.bukkit.scoreboard.Team.Option.class);
        Component display, prefix = Component.empty(), suffix = Component.empty();
        NamedTextColor color;
        boolean friendlyFire = true, seeInvisible = true;
        final org.bukkit.scoreboard.Team proxy;

        TeamState(String name, org.bukkit.scoreboard.Scoreboard board, java.util.Map<String, TeamState> teams) {
            this.name = name; this.board = board; this.teams = teams; this.display = Component.text(name);
            proxy = (org.bukkit.scoreboard.Team) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.scoreboard.Team.class}, this);
        }

        private void check() { if (teams.get(name) != this) throw new IllegalStateException("Team is unregistered"); }

        private String status(org.bukkit.scoreboard.Team.Option option, String always, String never, String other, String own) {
            return switch (options.getOrDefault(option, org.bukkit.scoreboard.Team.OptionStatus.ALWAYS)) {
                case ALWAYS -> always;
                case NEVER -> never;
                case FOR_OTHER_TEAMS -> other;
                case FOR_OWN_TEAM -> own;
            };
        }

        /** The team's settings in the order the host expects, as plain text (formatting is dropped). */
        String clientParams() {
            var plain = net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText();
            int colorId = 21;
            if (color != null) {
                var names = java.util.List.of("black", "dark_blue", "dark_green", "dark_aqua", "dark_red", "dark_purple", "gold", "gray", "dark_gray", "blue", "green", "aqua", "red", "light_purple", "yellow", "white");
                int index = names.indexOf(NamedTextColor.NAMES.key(color));
                if (index >= 0) colorId = index;
            }
            String part = String.valueOf(HostBridge.ITEM);
            return String.join(part, plain.serialize(display), String.valueOf((friendlyFire ? 1 : 0) | (seeInvisible ? 2 : 0)),
                status(org.bukkit.scoreboard.Team.Option.NAME_TAG_VISIBILITY, "always", "never", "hideForOtherTeams", "hideForOwnTeam"),
                status(org.bukkit.scoreboard.Team.Option.COLLISION_RULE, "always", "never", "pushOtherTeams", "pushOwnTeam"),
                String.valueOf(colorId), plain.serialize(prefix), plain.serialize(suffix));
        }

        private static final java.util.Set<String> CHANGES = java.util.Set.of("prefix", "setPrefix", "suffix", "setSuffix", "displayName", "setDisplayName",
            "color", "setColor", "setAllowFriendlyFire", "setCanSeeFriendlyInvisibles", "setOption", "addEntry", "addPlayer", "addEntries", "addEntities",
            "removeEntry", "removePlayer", "removeEntries", "unregister");

        @Override
        public Object invoke(Object self, java.lang.reflect.Method method, Object[] args) {
            var result = call(self, method, args);
            boolean changes = CHANGES.contains(method.getName()) && (args != null && args.length > 0 || method.getName().equals("unregister"));
            if (changes) {
                var state = STATES.get(board);
                if (state != null) state.touch();
            }
            return result;
        }

        @SuppressWarnings("unchecked")
        private Object call(Object self, java.lang.reflect.Method method, Object[] args) {
            boolean none = args == null || args.length == 0;
            return switch (method.getName()) {
                case "getName" -> name;
                case "getScoreboard" -> teams.get(name) == this ? board : null;
                case "displayName" -> { if (none) { check(); yield display; } check(); display = (Component) args[0]; yield null; }
                case "getDisplayName" -> { check(); yield LEGACY.serialize(display); }
                case "setDisplayName" -> { check(); display = component(args[0]); yield null; }
                case "prefix" -> { if (none) { check(); yield prefix; } check(); prefix = (Component) args[0]; yield null; }
                case "getPrefix" -> { check(); yield LEGACY.serialize(prefix); }
                case "setPrefix" -> { check(); prefix = component(args[0]); yield null; }
                case "suffix" -> { if (none) { check(); yield suffix; } check(); suffix = (Component) args[0]; yield null; }
                case "getSuffix" -> { check(); yield LEGACY.serialize(suffix); }
                case "setSuffix" -> { check(); suffix = component(args[0]); yield null; }
                case "color" -> {
                    if (none) { check(); if (color == null) throw new IllegalStateException("Team does not have a color specified"); yield color; }
                    check(); color = (NamedTextColor) args[0]; yield null;
                }
                case "hasColor" -> { check(); yield color != null; }
                case "getColor" -> { check(); yield color == null ? org.bukkit.ChatColor.RESET : org.bukkit.ChatColor.valueOf(NamedTextColor.NAMES.key(color).toUpperCase(java.util.Locale.ROOT)); }
                case "setColor" -> { check(); var chat = (org.bukkit.ChatColor) args[0]; color = chat == org.bukkit.ChatColor.RESET ? null : NamedTextColor.NAMES.value(chat.name().toLowerCase(java.util.Locale.ROOT)); yield null; }
                case "allowFriendlyFire" -> { check(); yield friendlyFire; }
                case "setAllowFriendlyFire" -> { check(); friendlyFire = (boolean) args[0]; yield null; }
                case "canSeeFriendlyInvisibles" -> { check(); yield seeInvisible; }
                case "setCanSeeFriendlyInvisibles" -> { check(); seeInvisible = (boolean) args[0]; yield null; }
                case "getOption" -> { check(); yield options.getOrDefault((org.bukkit.scoreboard.Team.Option) args[0], org.bukkit.scoreboard.Team.OptionStatus.ALWAYS); }
                case "setOption" -> { check(); options.put((org.bukkit.scoreboard.Team.Option) args[0], (org.bukkit.scoreboard.Team.OptionStatus) args[1]); yield null; }
                case "addEntry" -> { check(); String entry = (String) args[0]; teams.values().forEach(t -> t.entries.remove(entry)); entries.add(entry); yield null; }
                case "addPlayer" -> { check(); String entry = entryName(args[0]); teams.values().forEach(t -> t.entries.remove(entry)); entries.add(entry); yield null; }
                case "addEntries" -> { check(); for (var entry : (java.util.Collection<String>) args[0]) { teams.values().forEach(t -> t.entries.remove(entry)); entries.add(entry); } yield null; }
                case "addEntities" -> { check(); for (var entity : (java.util.Collection<org.bukkit.entity.Entity>) args[0]) entries.add(entity instanceof org.bukkit.entity.Player p ? p.getName() : entity.getUniqueId().toString()); yield null; }
                case "removeEntry" -> { check(); yield entries.remove((String) args[0]); }
                case "removePlayer" -> { check(); yield entries.remove(entryName(args[0])); }
                case "removeEntries" -> { check(); yield entries.removeAll((java.util.Collection<String>) args[0]); }
                case "hasEntry" -> { check(); yield entries.contains((String) args[0]); }
                case "hasPlayer" -> { check(); yield entries.contains(entryName(args[0])); }
                case "getEntries" -> { check(); yield java.util.Set.copyOf(entries); }
                case "getSize" -> { check(); yield entries.size(); }
                case "unregister" -> { if (!teams.remove(name, this)) throw new IllegalStateException("Team is unregistered"); yield null; }
                case "hashCode" -> System.identityHashCode(self);
                case "equals" -> self == args[0];
                case "toString" -> "Team[" + name + "]";
                default -> HostBridge.dispatch("Team", name, method, args);
            };
        }
    }
}
