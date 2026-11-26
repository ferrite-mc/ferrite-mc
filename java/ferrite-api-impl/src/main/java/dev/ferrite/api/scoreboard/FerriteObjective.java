package dev.ferrite.api.scoreboard;

import io.papermc.paper.scoreboard.numbers.NumberFormat;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer;
import org.bukkit.OfflinePlayer;
import org.bukkit.entity.Entity;
import org.bukkit.scoreboard.Criteria;
import org.bukkit.scoreboard.DisplaySlot;
import org.bukkit.scoreboard.Objective;
import org.bukkit.scoreboard.RenderType;
import org.bukkit.scoreboard.Score;
import org.bukkit.scoreboard.Scoreboard;

/** State-backed {@link Objective} implementation for {@link FerriteScoreboard}. */
public final class FerriteObjective implements Objective {

    private static final class EntryData {
        int score;
        boolean set;
        boolean triggerable;
        Component customName;
        NumberFormat numberFormat;
    }

    private final FerriteScoreboard scoreboard;
    private final String name;
    private final Criteria criteria;
    private final Map<String, EntryData> entries = new LinkedHashMap<>();

    private Component displayName;
    private DisplaySlot displaySlot;
    private RenderType renderType;
    private boolean autoUpdateDisplay;
    private NumberFormat numberFormat;
    private boolean registered = true;

    FerriteObjective(
            FerriteScoreboard scoreboard,
            String name,
            Criteria criteria,
            Component displayName,
            RenderType renderType) {
        this.scoreboard = Objects.requireNonNull(scoreboard, "scoreboard");
        this.name = Objects.requireNonNull(name, "name");
        this.criteria = Objects.requireNonNull(criteria, "criteria");
        this.displayName = Objects.requireNonNull(displayName, "displayName");
        this.renderType = Objects.requireNonNull(renderType, "renderType");
    }

    @Override
    public String getName() {
        checkState();
        return name;
    }

    @Override
    public Component displayName() {
        checkState();
        return displayName;
    }

    @Override
    public void displayName(Component displayName) {
        checkState();
        if (displayName == null) {
            throw new IllegalArgumentException("Display name cannot be null");
        }
        this.displayName = displayName;
        scoreboard.broadcastObjective(this, 2);
    }

    @Override
    @SuppressWarnings("deprecation")
    public String getDisplayName() {
        checkState();
        return LegacyComponentSerializer.legacySection().serialize(displayName);
    }

    @Override
    @SuppressWarnings("deprecation")
    public void setDisplayName(String displayName) {
        checkState();
        if (displayName == null) {
            throw new IllegalArgumentException("Display name cannot be null");
        }
        this.displayName = LegacyComponentSerializer.legacySection().deserialize(displayName);
        scoreboard.broadcastObjective(this, 2);
    }

    @Override
    @SuppressWarnings("deprecation")
    public String getCriteria() {
        checkState();
        return criteria.getName();
    }

    @Override
    public Criteria getTrackedCriteria() {
        checkState();
        return criteria;
    }

    @Override
    public boolean isModifiable() {
        checkState();
        return !criteria.isReadOnly();
    }

    @Override
    public Scoreboard getScoreboard() {
        return registered ? scoreboard : null;
    }

    @Override
    public void unregister() {
        checkState();
        if (displaySlot != null) {
            scoreboard.clearObjectiveSlot(this, displaySlot);
            displaySlot = null;
        }
        scoreboard.removeObjective(this);
        registered = false;
        entries.clear();
    }

    String rawName() {
        return name;
    }

    String rawDisplayName() {
        return LegacyComponentSerializer.legacySection().serialize(displayName);
    }

    RenderType rawRenderType() {
        return renderType;
    }

    synchronized Map<String, Integer> snapshotScores() {
        Map<String, Integer> result = new LinkedHashMap<>();
        for (Map.Entry<String, EntryData> entry : entries.entrySet()) {
            if (entry.getValue().set) {
                result.put(entry.getKey(), entry.getValue().score);
            }
        }
        return result;
    }

    @Override
    public void setDisplaySlot(DisplaySlot slot) {
        checkState();
        if (Objects.equals(this.displaySlot, slot)) {
            return;
        }
        if (this.displaySlot != null) {
            scoreboard.clearObjectiveSlot(this, this.displaySlot);
        }
        this.displaySlot = slot;
        if (slot != null) {
            scoreboard.assignDisplaySlot(slot, this);
        }
    }

    @Override
    public DisplaySlot getDisplaySlot() {
        checkState();
        return displaySlot;
    }

    @Override
    public void setRenderType(RenderType renderType) {
        checkState();
        if (renderType == null) {
            throw new IllegalArgumentException("RenderType cannot be null");
        }
        this.renderType = renderType;
        scoreboard.broadcastObjective(this, 2);
    }

    @Override
    public RenderType getRenderType() {
        checkState();
        return renderType;
    }

    @Override
    @SuppressWarnings("deprecation")
    public Score getScore(OfflinePlayer player) {
        checkState();
        if (player == null) {
            throw new IllegalArgumentException("Player cannot be null");
        }
        return getScore(FerriteScoreboard.entryName(player));
    }

    @Override
    public Score getScore(String entry) {
        checkState();
        if (entry == null) {
            throw new IllegalArgumentException("Entry cannot be null");
        }
        return new FerriteScore(this, entry);
    }

    @Override
    public Score getScoreFor(Entity entity) {
        checkState();
        if (entity == null) {
            throw new IllegalArgumentException("Entity cannot be null");
        }
        return getScore(FerriteScoreboard.entityEntryName(entity));
    }

    @Override
    public boolean willAutoUpdateDisplay() {
        checkState();
        return autoUpdateDisplay;
    }

    @Override
    public void setAutoUpdateDisplay(boolean autoUpdateDisplay) {
        checkState();
        this.autoUpdateDisplay = autoUpdateDisplay;
    }

    @Override
    public NumberFormat numberFormat() {
        checkState();
        return numberFormat;
    }

    @Override
    public void numberFormat(NumberFormat format) {
        checkState();
        this.numberFormat = format;
    }

    void clearDisplaySlotField() {
        this.displaySlot = null;
    }

    void checkState() {
        if (!registered) {
            throw new IllegalStateException("Unregistered scoreboard component");
        }
    }

    synchronized int scoreValue(String entry) {
        checkState();
        EntryData data = entries.get(entry);
        return data != null && data.set ? data.score : 0;
    }

    synchronized void setScoreValue(String entry, int score) {
        checkState();
        EntryData data = entries.computeIfAbsent(entry, ignored -> new EntryData());
        data.score = score;
        data.set = true;
        scoreboard.broadcastScore(name, entry, score);
    }

    synchronized boolean hasScore(String entry) {
        checkState();
        EntryData data = entries.get(entry);
        return data != null && data.set;
    }

    synchronized void resetScoreValue(String entry) {
        checkState();
        if (entries.remove(entry) != null) {
            scoreboard.broadcastResetScore(name, entry);
        }
    }

    synchronized boolean isTriggerable(String entry) {
        checkState();
        EntryData data = entries.get(entry);
        return data != null && data.triggerable;
    }

    synchronized void setTriggerable(String entry, boolean trigger) {
        checkState();
        EntryData data = entries.computeIfAbsent(entry, ignored -> new EntryData());
        data.triggerable = trigger;
    }

    synchronized Component scoreCustomName(String entry) {
        checkState();
        EntryData data = entries.get(entry);
        return data == null ? null : data.customName;
    }

    synchronized void setScoreCustomName(String entry, Component customName) {
        checkState();
        EntryData data = entries.computeIfAbsent(entry, ignored -> new EntryData());
        data.customName = customName;
    }

    synchronized NumberFormat scoreNumberFormat(String entry) {
        checkState();
        EntryData data = entries.get(entry);
        return data == null ? null : data.numberFormat;
    }

    synchronized void setScoreNumberFormat(String entry, NumberFormat format) {
        checkState();
        EntryData data = entries.computeIfAbsent(entry, ignored -> new EntryData());
        data.numberFormat = format;
    }

    synchronized Set<String> scoredEntries() {
        Set<String> result = new LinkedHashSet<>();
        for (Map.Entry<String, EntryData> entry : entries.entrySet()) {
            if (entry.getValue().set) {
                result.add(entry.getKey());
            }
        }
        return result;
    }
}
