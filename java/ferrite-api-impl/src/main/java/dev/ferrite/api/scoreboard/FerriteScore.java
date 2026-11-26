package dev.ferrite.api.scoreboard;

import io.papermc.paper.scoreboard.numbers.NumberFormat;
import java.util.Objects;
import net.kyori.adventure.text.Component;
import org.bukkit.OfflinePlayer;
import org.bukkit.scoreboard.Objective;
import org.bukkit.scoreboard.Score;
import org.bukkit.scoreboard.Scoreboard;

/** State-backed {@link Score} implementation for {@link FerriteObjective}. */
public final class FerriteScore implements Score {

    private final FerriteObjective objective;
    private final String entry;

    FerriteScore(FerriteObjective objective, String entry) {
        this.objective = Objects.requireNonNull(objective, "objective");
        this.entry = Objects.requireNonNull(entry, "entry");
    }

    @Override
    @SuppressWarnings("deprecation")
    public OfflinePlayer getPlayer() {
        return FerriteOfflinePlayer.resolve(entry);
    }

    @Override
    public String getEntry() {
        return entry;
    }

    @Override
    public Objective getObjective() {
        return objective;
    }

    @Override
    public int getScore() {
        return objective.scoreValue(entry);
    }

    @Override
    public void setScore(int score) {
        objective.setScoreValue(entry, score);
    }

    @Override
    public boolean isScoreSet() {
        return objective.hasScore(entry);
    }

    @Override
    public Scoreboard getScoreboard() {
        return objective.getScoreboard();
    }

    @Override
    public void resetScore() {
        objective.resetScoreValue(entry);
    }

    @Override
    public boolean isTriggerable() {
        return objective.isTriggerable(entry);
    }

    @Override
    public void setTriggerable(boolean trigger) {
        objective.setTriggerable(entry, trigger);
    }

    @Override
    public Component customName() {
        return objective.scoreCustomName(entry);
    }

    @Override
    public void customName(Component customName) {
        objective.setScoreCustomName(entry, customName);
    }

    @Override
    public NumberFormat numberFormat() {
        return objective.scoreNumberFormat(entry);
    }

    @Override
    public void numberFormat(NumberFormat numberFormat) {
        objective.setScoreNumberFormat(entry, numberFormat);
    }

    @Override
    public boolean equals(Object object) {
        return object instanceof FerriteScore other
                && objective.equals(other.objective)
                && entry.equals(other.entry);
    }

    @Override
    public int hashCode() {
        return Objects.hash(objective, entry);
    }
}
