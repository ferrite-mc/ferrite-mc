package dev.ferrite.api.entity;

import dev.ferrite.api.world.FerriteWorld;
import java.util.List;
import java.util.Objects;
import java.util.UUID;
import java.util.concurrent.CopyOnWriteArrayList;
import org.bukkit.GameMode;
import org.bukkit.Particle;
import org.bukkit.SoundCategory;
import org.bukkit.scoreboard.Scoreboard;

/** Mutable server-side state behind {@link FerritePlayer}. */
public final class PlayerState {

    public record TitleRecord(String title, String subtitle, int fadeIn, int stay, int fadeOut) {}

    public record SoundRecord(
            String sound,
            SoundCategory category,
            float volume,
            float pitch,
            long seed,
            double x,
            double y,
            double z) {}

    public record ParticleRecord(
            Particle particle,
            double x,
            double y,
            double z,
            int count,
            double offsetX,
            double offsetY,
            double offsetZ,
            double extraX,
            double extraY,
            double extraZ,
            Object data,
            boolean force,
            Particle.RandomizationType randomizationType) {}

    private final String name;
    private final UUID uniqueId;
    private final List<String> messages = new CopyOnWriteArrayList<>();
    private final List<TitleRecord> titleHistory = new CopyOnWriteArrayList<>();
    private final List<String> actionBars = new CopyOnWriteArrayList<>();
    private final List<SoundRecord> playedSounds = new CopyOnWriteArrayList<>();
    private final List<ParticleRecord> spawnedParticles = new CopyOnWriteArrayList<>();

    private boolean op;
    private GameMode gameMode;
    private double health = 20.0;
    private int foodLevel = 20;
    private FerriteWorld world;
    private double x;
    private double y;
    private double z;
    private float yaw;
    private float pitch;
    private volatile boolean online;
    private boolean kicked;
    private String kickMessage;
    private FerritePlayerInventory inventory;
    private Scoreboard scoreboard;
    private TitleRecord lastTitle;
    private String pendingSubtitle = "";
    private int titleFadeIn = 10;
    private int titleStay = 70;
    private int titleFadeOut = 20;
    private String lastActionBar;
    private boolean allowFlight;
    private boolean flying;
    private float flySpeed = 0.1f;
    private float walkSpeed = 0.2f;
    private String playerListHeader = "";
    private String playerListFooter = "";
    private int level;
    private float exp;
    private int totalExperience;
    private double vx;
    private double vy;
    private double vz;
    private org.bukkit.Location compassTarget;
    private boolean sneaking;
    private boolean sprinting;
    private int ping;
    private String displayName;
    private String playerListName;
    private float saturation = 5.0f;
    private float exhaustion;
    private int fireTicks;
    private int remainingAir = 300;
    private String locale = "en_us";
    private final long firstPlayed = System.currentTimeMillis();
    private long lastPlayed = System.currentTimeMillis();
    private final java.util.Map<String, Integer> cooldowns = new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.Set<UUID> hiddenEntities = java.util.concurrent.ConcurrentHashMap.newKeySet();
    private final java.util.Map<org.bukkit.potion.PotionEffectType, org.bukkit.potion.PotionEffect> potionEffects =
            new java.util.concurrent.ConcurrentHashMap<>();

    public PlayerState(
            String name,
            UUID uniqueId,
            boolean op,
            GameMode gameMode,
            FerriteWorld world,
            double x,
            double y,
            double z) {
        this.name = Objects.requireNonNull(name, "name");
        this.uniqueId = Objects.requireNonNull(uniqueId, "uniqueId");
        this.op = op;
        this.gameMode = Objects.requireNonNull(gameMode, "gameMode");
        this.world = Objects.requireNonNull(world, "world");
        this.x = x;
        this.y = y;
        this.z = z;
    }

    public String name() {
        return name;
    }

    public UUID uniqueId() {
        return uniqueId;
    }

    public boolean op() {
        return op;
    }

    public void setOp(boolean op) {
        this.op = op;
    }

    public GameMode gameMode() {
        return gameMode;
    }

    public void setGameMode(GameMode gameMode) {
        this.gameMode = Objects.requireNonNull(gameMode, "gameMode");
    }

    public double health() {
        return health;
    }

    public void setHealth(double health) {
        this.health = health;
    }

    public int foodLevel() {
        return foodLevel;
    }

    public void setFoodLevel(int foodLevel) {
        this.foodLevel = foodLevel;
    }

    public FerriteWorld world() {
        return world;
    }

    public void setWorld(FerriteWorld world) {
        this.world = Objects.requireNonNull(world, "world");
    }

    public double x() {
        return x;
    }

    public double y() {
        return y;
    }

    public double z() {
        return z;
    }

    public void setPosition(double x, double y, double z, float yaw, float pitch) {
        this.x = x;
        this.y = y;
        this.z = z;
        this.yaw = yaw;
        this.pitch = pitch;
    }

    public float yaw() {
        return yaw;
    }

    public float pitch() {
        return pitch;
    }

    public boolean online() {
        return online;
    }

    public void setOnline(boolean online) {
        this.online = online;
    }

    public boolean kicked() {
        return kicked;
    }

    public String kickMessage() {
        return kickMessage;
    }

    public void kick(String message) {
        this.kicked = true;
        this.kickMessage = message;
    }

    public List<String> messages() {
        return messages;
    }

    public FerritePlayerInventory inventory() {
        return inventory;
    }

    void setInventory(FerritePlayerInventory inventory) {
        this.inventory = inventory;
    }

    public Scoreboard scoreboard() {
        return scoreboard;
    }

    public void setScoreboard(Scoreboard scoreboard) {
        this.scoreboard = Objects.requireNonNull(scoreboard, "scoreboard");
    }

    public TitleRecord lastTitle() {
        return lastTitle;
    }

    public List<TitleRecord> titleHistory() {
        return titleHistory;
    }

    public String pendingSubtitle() {
        return pendingSubtitle;
    }

    public int titleFadeIn() {
        return titleFadeIn;
    }

    public int titleStay() {
        return titleStay;
    }

    public int titleFadeOut() {
        return titleFadeOut;
    }

    public void setTitleTimes(int fadeIn, int stay, int fadeOut) {
        this.titleFadeIn = fadeIn;
        this.titleStay = stay;
        this.titleFadeOut = fadeOut;
    }

    public void setPendingSubtitle(String subtitle) {
        this.pendingSubtitle = subtitle == null ? "" : subtitle;
    }

    public void recordTitle(String title, String subtitle, int fadeIn, int stay, int fadeOut) {
        TitleRecord record = new TitleRecord(
                title == null ? "" : title,
                subtitle == null ? "" : subtitle,
                fadeIn,
                stay,
                fadeOut);
        this.lastTitle = record;
        this.titleHistory.add(record);
    }

    public void clearTitle() {
        this.lastTitle = null;
        this.pendingSubtitle = "";
    }

    public void resetTitle() {
        clearTitle();
        this.titleFadeIn = 10;
        this.titleStay = 70;
        this.titleFadeOut = 20;
    }

    public String lastActionBar() {
        return lastActionBar;
    }

    public List<String> actionBars() {
        return actionBars;
    }

    public void recordActionBar(String message) {
        String value = message == null ? "" : message;
        this.lastActionBar = value;
        this.actionBars.add(value);
    }

    public List<SoundRecord> playedSounds() {
        return playedSounds;
    }

    public void recordSound(
            String sound,
            SoundCategory category,
            float volume,
            float pitch,
            long seed,
            double x,
            double y,
            double z) {
        this.playedSounds.add(new SoundRecord(
                sound,
                category == null ? SoundCategory.MASTER : category,
                volume,
                pitch,
                seed,
                x,
                y,
                z));
    }

    public List<ParticleRecord> spawnedParticles() {
        return spawnedParticles;
    }

    public void recordParticle(
            Particle particle,
            double x,
            double y,
            double z,
            int count,
            double offsetX,
            double offsetY,
            double offsetZ,
            double extraX,
            double extraY,
            double extraZ,
            Object data,
            boolean force,
            Particle.RandomizationType randomizationType) {
        this.spawnedParticles.add(new ParticleRecord(
                particle,
                x,
                y,
                z,
                count,
                offsetX,
                offsetY,
                offsetZ,
                extraX,
                extraY,
                extraZ,
                data,
                force,
                randomizationType));
    }

    public boolean allowFlight() {
        return allowFlight;
    }

    public void setAllowFlight(boolean allowFlight) {
        this.allowFlight = allowFlight;
    }

    public boolean flying() {
        return flying;
    }

    public void setFlying(boolean flying) {
        this.flying = flying;
    }

    public float flySpeed() {
        return flySpeed;
    }

    public void setFlySpeed(float flySpeed) {
        this.flySpeed = flySpeed;
    }

    public float walkSpeed() {
        return walkSpeed;
    }

    public void setWalkSpeed(float walkSpeed) {
        this.walkSpeed = walkSpeed;
    }

    public String playerListHeader() {
        return playerListHeader;
    }

    public void setPlayerListHeader(String header) {
        this.playerListHeader = header == null ? "" : header;
    }

    public String playerListFooter() {
        return playerListFooter;
    }

    public void setPlayerListFooter(String footer) {
        this.playerListFooter = footer == null ? "" : footer;
    }

    public int level() {
        return level;
    }

    public void setLevel(int level) {
        this.level = Math.max(0, level);
    }

    public float exp() {
        return exp;
    }

    public void setExp(float exp) {
        this.exp = Math.max(0.0f, Math.min(1.0f, exp));
    }

    public int totalExperience() {
        return totalExperience;
    }

    public void setTotalExperience(int totalExperience) {
        this.totalExperience = Math.max(0, totalExperience);
    }

    public java.util.Map<org.bukkit.potion.PotionEffectType, org.bukkit.potion.PotionEffect> potionEffects() {
        return potionEffects;
    }

    public org.bukkit.util.Vector velocity() {
        return new org.bukkit.util.Vector(vx, vy, vz);
    }

    public void setVelocity(double vx, double vy, double vz) {
        this.vx = vx;
        this.vy = vy;
        this.vz = vz;
    }

    public org.bukkit.Location compassTarget() {
        return compassTarget == null ? world.getSpawnLocation() : compassTarget.clone();
    }

    public void setCompassTarget(org.bukkit.Location target) {
        this.compassTarget = target == null ? null : target.clone();
    }

    public boolean sneaking() {
        return sneaking;
    }

    public void setSneaking(boolean sneaking) {
        this.sneaking = sneaking;
    }

    public boolean sprinting() {
        return sprinting;
    }

    public void setSprinting(boolean sprinting) {
        this.sprinting = sprinting;
    }

    public int ping() {
        return ping;
    }

    public void setPing(int ping) {
        this.ping = Math.max(0, ping);
    }

    public java.util.Set<UUID> hiddenEntities() {
        return hiddenEntities;
    }

    public String displayName() {
        return displayName == null ? name : displayName;
    }

    public void setDisplayName(String displayName) {
        this.displayName = displayName;
    }

    public String playerListName() {
        return playerListName == null ? name : playerListName;
    }

    public void setPlayerListName(String playerListName) {
        this.playerListName = playerListName;
    }

    public float saturation() {
        return saturation;
    }

    public void setSaturation(float saturation) {
        this.saturation = Math.max(0.0f, saturation);
    }

    public float exhaustion() {
        return exhaustion;
    }

    public void setExhaustion(float exhaustion) {
        this.exhaustion = Math.max(0.0f, exhaustion);
    }

    public int fireTicks() {
        return fireTicks;
    }

    public void setFireTicks(int fireTicks) {
        this.fireTicks = fireTicks;
    }

    public int remainingAir() {
        return remainingAir;
    }

    public void setRemainingAir(int remainingAir) {
        this.remainingAir = remainingAir;
    }

    public String locale() {
        return locale;
    }

    public void setLocale(String locale) {
        this.locale = locale == null ? "en_us" : locale;
    }

    public long firstPlayed() {
        return firstPlayed;
    }

    public long lastPlayed() {
        return lastPlayed;
    }

    public void touchLastPlayed() {
        this.lastPlayed = System.currentTimeMillis();
    }

    public java.util.Map<String, Integer> cooldowns() {
        return cooldowns;
    }
}
