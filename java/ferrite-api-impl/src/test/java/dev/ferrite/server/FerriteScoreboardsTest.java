package dev.ferrite.server;

import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.format.NamedTextColor;
import org.bukkit.scoreboard.DisplaySlot;
import org.bukkit.scoreboard.Criteria;
import org.bukkit.scoreboard.Team;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class FerriteScoreboardsTest {
    @org.junit.jupiter.api.BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
    }
    private static org.bukkit.scoreboard.Scoreboard board() { return new FerriteScoreboards().getNewScoreboard(); }

    @Test void objectivesKeepScoresAndDisplaySlots() {
        var board = board();
        var objective = board.registerNewObjective("kills", Criteria.DUMMY, Component.text("Kills"));
        objective.setDisplaySlot(DisplaySlot.SIDEBAR);
        objective.getScore("Alex").setScore(7);
        assertEquals(7, objective.getScore("Alex").getScore());
        assertTrue(objective.getScore("Alex").isScoreSet());
        assertFalse(objective.getScore("Sam").isScoreSet());
        assertSame(objective, board.getObjective(DisplaySlot.SIDEBAR));
        assertEquals(DisplaySlot.SIDEBAR, objective.getDisplaySlot());
        assertEquals(1, board.getScores("Alex").size());
        board.resetScores("Alex");
        assertEquals(0, objective.getScore("Alex").getScore());
        assertThrows(IllegalArgumentException.class, () -> board.registerNewObjective("kills", Criteria.DUMMY, Component.text("x")));
        objective.unregister();
        assertNull(board.getObjective("kills"));
        assertNull(board.getObjective(DisplaySlot.SIDEBAR));
        assertThrows(IllegalStateException.class, () -> objective.getScore("Alex").setScore(1));
    }

    @Test void teamsKeepOptionsAndEntries() {
        var board = board();
        Team team = board.registerNewTeam("red");
        team.color(NamedTextColor.RED);
        team.prefix(Component.text("[R] "));
        team.setAllowFriendlyFire(false);
        team.setOption(Team.Option.NAME_TAG_VISIBILITY, Team.OptionStatus.NEVER);
        team.addEntry("Alex");
        assertEquals(NamedTextColor.RED, team.color());
        assertEquals("[R] ", net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(team.prefix()));
        assertFalse(team.allowFriendlyFire());
        assertEquals(Team.OptionStatus.NEVER, team.getOption(Team.Option.NAME_TAG_VISIBILITY));
        assertSame(team, board.getEntryTeam("Alex"));
        board.registerNewTeam("blue").addEntry("Alex");
        assertEquals(0, team.getSize(), "an entry belongs to one team");
        team.unregister();
        assertThrows(IllegalStateException.class, team::getSize);
    }
}
