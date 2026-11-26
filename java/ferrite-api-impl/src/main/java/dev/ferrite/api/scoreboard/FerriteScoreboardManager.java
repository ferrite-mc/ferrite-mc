package dev.ferrite.api.scoreboard;

import org.bukkit.scoreboard.Scoreboard;
import org.bukkit.scoreboard.ScoreboardManager;

/** {@link ScoreboardManager} owning the main scoreboard and creating per-plugin scoreboards. */
public final class FerriteScoreboardManager implements ScoreboardManager {

    private final FerriteScoreboard mainScoreboard = new FerriteScoreboard();

    @Override
    public Scoreboard getMainScoreboard() {
        return mainScoreboard;
    }

    @Override
    public Scoreboard getNewScoreboard() {
        return new FerriteScoreboard();
    }
}
