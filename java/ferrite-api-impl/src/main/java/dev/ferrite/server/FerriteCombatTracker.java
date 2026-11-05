package dev.ferrite.server;

import io.papermc.paper.world.damagesource.CombatEntry;
import io.papermc.paper.world.damagesource.FallLocationType;
import java.util.ArrayList;
import java.util.List;

/** The damage a player took lately, kept the way vanilla's CombatTracker does (times in ticks of 50 ms). */
final class FerriteCombatTracker implements io.papermc.paper.world.damagesource.CombatTracker {
    /** Vanilla: a fight lasts 300 ticks after the last hit, and "taking damage" 100. */
    private static final int COMBAT_TICKS = 300, TAKING_DAMAGE_TICKS = 100;

    record Entry(org.bukkit.damage.DamageSource damageSource, float damage, FallLocationType fallLocationType, float fallDistance, long tick) implements CombatEntry {
        @Override public org.bukkit.damage.DamageSource getDamageSource() { return damageSource; }
        @Override public float getDamage() { return damage; }
        @Override public FallLocationType getFallLocationType() { return fallLocationType; }
        @Override public float getFallDistance() { return fallDistance; }
    }

    private final org.bukkit.entity.LivingEntity entity;
    private final List<Entry> entries = new ArrayList<>();
    private long firstTick = -1;

    FerriteCombatTracker(org.bukkit.entity.LivingEntity entity) { this.entity = entity; }

    static long now() { return System.nanoTime() / 50_000_000L; }

    synchronized void record(org.bukkit.damage.DamageSource source, float damage) {
        if (firstTick < 0 || !isInCombat()) firstTick = now();
        entries.add(new Entry(source, damage, FallLocationType.GENERIC, 0f, now()));
    }

    @Override public org.bukkit.entity.LivingEntity getEntity() { return entity; }
    @Override public synchronized List<CombatEntry> getCombatEntries() { return List.copyOf(entries); }
    @Override public synchronized void setCombatEntries(List<CombatEntry> list) {
        entries.clear();
        for (var entry : list) entries.add(new Entry(entry.getDamageSource(), entry.getDamage(), entry.getFallLocationType(), entry.getFallDistance(), now()));
    }
    @Override public synchronized CombatEntry computeMostSignificantFall() {
        Entry best = null;
        for (var entry : entries) if (entry.fallDistance() > 0 && (best == null || entry.damage() > best.damage())) best = entry;
        return best;
    }
    @Override public synchronized boolean isInCombat() { return !entries.isEmpty() && now() - entries.get(entries.size() - 1).tick() <= COMBAT_TICKS; }
    @Override public synchronized boolean isTakingDamage() { return !entries.isEmpty() && now() - entries.get(entries.size() - 1).tick() <= TAKING_DAMAGE_TICKS; }
    @Override public synchronized int getCombatDuration() { return entries.isEmpty() ? 0 : (int) (entries.get(entries.size() - 1).tick() - firstTick); }
    @Override public synchronized void addCombatEntry(CombatEntry entry) { setLast(entry); }
    private void setLast(CombatEntry entry) {
        if (firstTick < 0 || !isInCombat()) firstTick = now();
        entries.add(new Entry(entry.getDamageSource(), entry.getDamage(), entry.getFallLocationType(), entry.getFallDistance(), now()));
    }
    @Override public net.kyori.adventure.text.Component getDeathMessage() {
        return net.kyori.adventure.text.Component.translatable("death.attack.generic", net.kyori.adventure.text.Component.text(entity.getName()));
    }
    @Override public synchronized void resetCombatState() { entries.clear(); firstTick = -1; }
    @Override public FallLocationType calculateFallLocationType() { return FallLocationType.GENERIC; }
    @Override public synchronized int getLastDamageTime() { return entries.isEmpty() ? 0 : (int) entries.get(entries.size() - 1).tick(); }
}
