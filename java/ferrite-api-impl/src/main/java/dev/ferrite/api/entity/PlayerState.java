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

    private static final java.util.concurrent.atomic.AtomicInteger NEXT_ENTITY_ID =
            new java.util.concurrent.atomic.AtomicInteger(1000);

    private final int entityId = NEXT_ENTITY_ID.incrementAndGet();
    private final String name;
    private final UUID uniqueId;
    private final List<String> messages = new CopyOnWriteArrayList<>();
    private final List<TitleRecord> titleHistory = new CopyOnWriteArrayList<>();
    private final List<String> actionBars = new CopyOnWriteArrayList<>();
    private final List<SoundRecord> playedSounds = new CopyOnWriteArrayList<>();
    private final List<ParticleRecord> spawnedParticles = new CopyOnWriteArrayList<>();

    private boolean op;
    private GameMode gameMode;
    private GameMode previousGameMode;
    private double health = 20.0;
    private double absorptionAmount;
    private boolean healthScaled;
    private double healthScale = 20.0;
    private int foodLevel = 20;
    private FerriteWorld world;
    private double x;
    private double y;
    private double z;
    private float yaw;
    private float bodyYaw;
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
    private int playerListOrder;
    private int level;
    private float exp;
    private int totalExperience;
    private int expCooldown;
    private double vx;
    private double vy;
    private double vz;
    private org.bukkit.Location compassTarget;
    private boolean sneaking;
    private boolean sprinting;
    private boolean gliding;
    private boolean swimming;
    private boolean riptiding;
    private boolean jumping;
    private boolean sleepingIgnored;
    private boolean invisible;
    private boolean glowing;
    private boolean silent;
    private boolean invulnerable;
    private boolean gravity = true;
    private boolean collidable = true;
    private boolean noPhysics;
    private boolean persistent;
    private boolean canPickupItems = true;
    private boolean removeWhenFarAway;
    private boolean ai = true;
    private boolean affectsSpawning = true;
    private boolean customNameVisible;
    private boolean hasSeenWinScreen;
    private boolean visibleByDefault = true;
    private boolean freezeTicksLocked;
    private float fallDistance;
    private int ticksLived = 1;
    private int portalCooldown;
    private int freezeTicks;
    private int maximumAir = 300;
    private int maximumNoDamageTicks = 20;
    private int noDamageTicks;
    private int noActionTicks;
    private double lastDamage;
    private float hurtDirection;
    private org.bukkit.event.entity.EntityDamageEvent lastDamageCause;
    private org.bukkit.entity.Player killer;
    private int saturatedRegenRate = 10;
    private int unsaturatedRegenRate = 80;
    private int starvationRate = 80;
    private int arrowCooldown;
    private int arrowsInBody;
    private int beeStingerCooldown;
    private int beeStingersInBody;
    private int wardenWarningCooldown;
    private int wardenTimeSinceLastWarning;
    private int wardenWarningLevel;
    private int deathScreenScore;
    private int enchantmentSeed;
    private int viewDistance = 10;
    private int simulationDistance = 10;
    private int sendViewDistance = 10;
    private int ping;
    private String displayName;
    private String playerListName;
    private String customName;
    private float saturation = 5.0f;
    private float exhaustion;
    private int fireTicks;
    private net.kyori.adventure.util.TriState visualFire = net.kyori.adventure.util.TriState.NOT_SET;
    private net.kyori.adventure.util.TriState flyingFallDamage = net.kyori.adventure.util.TriState.NOT_SET;
    private net.kyori.adventure.util.TriState frictionState = net.kyori.adventure.util.TriState.NOT_SET;
    private int remainingAir = 300;
    private String locale = "en_us";
    private org.bukkit.inventory.MainHand mainHand = org.bukkit.inventory.MainHand.RIGHT;
    private org.bukkit.WorldBorder worldBorder;
    private org.bukkit.entity.Pose fixedPose;
    private org.bukkit.entity.Entity spectatorTarget;
    private org.bukkit.Location lastDeathLocation;
    private Long playerTimeOffset;
    private boolean playerTimeRelative = true;
    private org.bukkit.WeatherType playerWeather;
    private org.bukkit.inventory.ItemStack cursorItem;
    private org.bukkit.inventory.EquipmentSlot activeItemHand;
    private int activeItemRemainingTime;
    private int activeItemUseTicks;
    private long lastActivityMillis = System.currentTimeMillis();
    private long lastAttackMillis = System.currentTimeMillis();
    private final long firstPlayed = System.currentTimeMillis();
    private long lastPlayed = System.currentTimeMillis();
    private final java.util.Map<String, Integer> cooldowns = new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.Set<UUID> hiddenEntities = java.util.concurrent.ConcurrentHashMap.newKeySet();
    private final java.util.Set<UUID> collidableExemptions = java.util.concurrent.ConcurrentHashMap.newKeySet();
    private final java.util.Set<org.bukkit.NamespacedKey> discoveredRecipes =
            java.util.concurrent.ConcurrentHashMap.newKeySet();
    private final java.util.Set<UUID> unlistedPlayers = java.util.concurrent.ConcurrentHashMap.newKeySet();
    private final java.util.Set<String> customChatCompletions =
            java.util.concurrent.ConcurrentHashMap.newKeySet();
    private final java.util.Set<String> additionalChatCompletions =
            java.util.concurrent.ConcurrentHashMap.newKeySet();
    private final java.util.Map<org.bukkit.NamespacedKey, byte[]> cookies =
            new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.Map<net.kyori.adventure.key.Key, java.util.function.Predicate<org.bukkit.entity.Entity>>
            alliedRules = new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.Set<net.kyori.adventure.bossbar.BossBar> activeAdventureBossBars =
            java.util.Collections.newSetFromMap(new java.util.concurrent.ConcurrentHashMap<>());
    private final java.util.Map<org.bukkit.potion.PotionEffectType, org.bukkit.potion.PotionEffect> potionEffects =
            new java.util.concurrent.ConcurrentHashMap<>();
    private FerritePlayerProfile playerProfile;
    private final FerriteSkinParts skinParts = FerriteSkinParts.allParts();

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

    public int entityId() {
        return entityId;
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

    public GameMode previousGameMode() {
        return previousGameMode;
    }

    public void setGameMode(GameMode gameMode) {
        Objects.requireNonNull(gameMode, "gameMode");
        if (this.gameMode != gameMode) {
            this.previousGameMode = this.gameMode;
        }
        this.gameMode = gameMode;
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
        this.bodyYaw = yaw;
        this.pitch = pitch;
    }

    public void setRotation(float yaw, float pitch) {
        this.yaw = yaw;
        this.bodyYaw = yaw;
        this.pitch = Math.max(-90.0f, Math.min(90.0f, pitch));
    }

    public float yaw() {
        return yaw;
    }

    public float bodyYaw() {
        return bodyYaw;
    }

    public void setBodyYaw(float bodyYaw) {
        this.bodyYaw = bodyYaw;
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

    public int expCooldown() {
        return expCooldown;
    }

    public void setExpCooldown(int expCooldown) {
        this.expCooldown = Math.max(0, expCooldown);
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

    public double absorptionAmount() {
        return absorptionAmount;
    }

    public void setAbsorptionAmount(double absorptionAmount) {
        this.absorptionAmount = absorptionAmount;
    }

    public boolean healthScaled() {
        return healthScaled;
    }

    public void setHealthScaled(boolean healthScaled) {
        this.healthScaled = healthScaled;
    }

    public double healthScale() {
        return healthScale;
    }

    public void setHealthScale(double healthScale) {
        this.healthScale = healthScale;
    }

    public int playerListOrder() {
        return playerListOrder;
    }

    public void setPlayerListOrder(int playerListOrder) {
        this.playerListOrder = playerListOrder;
    }

    public boolean gliding() {
        return gliding;
    }

    public void setGliding(boolean gliding) {
        this.gliding = gliding;
    }

    public boolean swimming() {
        return swimming;
    }

    public void setSwimming(boolean swimming) {
        this.swimming = swimming;
    }

    public boolean riptiding() {
        return riptiding;
    }

    public void setRiptiding(boolean riptiding) {
        this.riptiding = riptiding;
    }

    public boolean jumping() {
        return jumping;
    }

    public void setJumping(boolean jumping) {
        this.jumping = jumping;
    }

    public boolean sleepingIgnored() {
        return sleepingIgnored;
    }

    public void setSleepingIgnored(boolean sleepingIgnored) {
        this.sleepingIgnored = sleepingIgnored;
    }

    public boolean invisible() {
        return invisible;
    }

    public void setInvisible(boolean invisible) {
        this.invisible = invisible;
    }

    public boolean glowing() {
        return glowing;
    }

    public void setGlowing(boolean glowing) {
        this.glowing = glowing;
    }

    public boolean silent() {
        return silent;
    }

    public void setSilent(boolean silent) {
        this.silent = silent;
    }

    public boolean invulnerable() {
        return invulnerable;
    }

    public void setInvulnerable(boolean invulnerable) {
        this.invulnerable = invulnerable;
    }

    public boolean gravity() {
        return gravity;
    }

    public void setGravity(boolean gravity) {
        this.gravity = gravity;
    }

    public boolean collidable() {
        return collidable;
    }

    public void setCollidable(boolean collidable) {
        this.collidable = collidable;
    }

    public java.util.Set<UUID> collidableExemptions() {
        return collidableExemptions;
    }

    public boolean noPhysics() {
        return noPhysics;
    }

    public void setNoPhysics(boolean noPhysics) {
        this.noPhysics = noPhysics;
    }

    public boolean persistent() {
        return persistent;
    }

    public void setPersistent(boolean persistent) {
        this.persistent = persistent;
    }

    public boolean canPickupItems() {
        return canPickupItems;
    }

    public void setCanPickupItems(boolean canPickupItems) {
        this.canPickupItems = canPickupItems;
    }

    public boolean removeWhenFarAway() {
        return removeWhenFarAway;
    }

    public void setRemoveWhenFarAway(boolean removeWhenFarAway) {
        this.removeWhenFarAway = removeWhenFarAway;
    }

    public boolean ai() {
        return ai;
    }

    public void setAi(boolean ai) {
        this.ai = ai;
    }

    public boolean affectsSpawning() {
        return affectsSpawning;
    }

    public void setAffectsSpawning(boolean affectsSpawning) {
        this.affectsSpawning = affectsSpawning;
    }

    public boolean customNameVisible() {
        return customNameVisible;
    }

    public void setCustomNameVisible(boolean customNameVisible) {
        this.customNameVisible = customNameVisible;
    }

    public String customName() {
        return customName;
    }

    public void setCustomName(String customName) {
        this.customName = customName;
    }

    public boolean hasSeenWinScreen() {
        return hasSeenWinScreen;
    }

    public void setHasSeenWinScreen(boolean hasSeenWinScreen) {
        this.hasSeenWinScreen = hasSeenWinScreen;
    }

    public boolean visibleByDefault() {
        return visibleByDefault;
    }

    public void setVisibleByDefault(boolean visibleByDefault) {
        this.visibleByDefault = visibleByDefault;
    }

    public float fallDistance() {
        return fallDistance;
    }

    public void setFallDistance(float fallDistance) {
        this.fallDistance = fallDistance;
    }

    public int ticksLived() {
        return ticksLived;
    }

    public void setTicksLived(int ticksLived) {
        this.ticksLived = ticksLived;
    }

    public int portalCooldown() {
        return portalCooldown;
    }

    public void setPortalCooldown(int portalCooldown) {
        this.portalCooldown = portalCooldown;
    }

    public int freezeTicks() {
        return freezeTicks;
    }

    public void setFreezeTicks(int freezeTicks) {
        this.freezeTicks = freezeTicks;
    }

    public boolean freezeTicksLocked() {
        return freezeTicksLocked;
    }

    public void setFreezeTicksLocked(boolean freezeTicksLocked) {
        this.freezeTicksLocked = freezeTicksLocked;
    }

    public int maximumAir() {
        return maximumAir;
    }

    public void setMaximumAir(int maximumAir) {
        this.maximumAir = maximumAir;
    }

    public int maximumNoDamageTicks() {
        return maximumNoDamageTicks;
    }

    public void setMaximumNoDamageTicks(int maximumNoDamageTicks) {
        this.maximumNoDamageTicks = maximumNoDamageTicks;
    }

    public int noDamageTicks() {
        return noDamageTicks;
    }

    public void setNoDamageTicks(int noDamageTicks) {
        this.noDamageTicks = noDamageTicks;
    }

    public int noActionTicks() {
        return noActionTicks;
    }

    public void setNoActionTicks(int noActionTicks) {
        this.noActionTicks = noActionTicks;
    }

    public double lastDamage() {
        return lastDamage;
    }

    public void setLastDamage(double lastDamage) {
        this.lastDamage = lastDamage;
    }

    public float hurtDirection() {
        return hurtDirection;
    }

    public void setHurtDirection(float hurtDirection) {
        this.hurtDirection = hurtDirection;
    }

    public org.bukkit.event.entity.EntityDamageEvent lastDamageCause() {
        return lastDamageCause;
    }

    public void setLastDamageCause(org.bukkit.event.entity.EntityDamageEvent lastDamageCause) {
        this.lastDamageCause = lastDamageCause;
    }

    public org.bukkit.entity.Player killer() {
        return killer;
    }

    public void setKiller(org.bukkit.entity.Player killer) {
        this.killer = killer;
    }

    public int saturatedRegenRate() {
        return saturatedRegenRate;
    }

    public void setSaturatedRegenRate(int saturatedRegenRate) {
        this.saturatedRegenRate = saturatedRegenRate;
    }

    public int unsaturatedRegenRate() {
        return unsaturatedRegenRate;
    }

    public void setUnsaturatedRegenRate(int unsaturatedRegenRate) {
        this.unsaturatedRegenRate = unsaturatedRegenRate;
    }

    public int starvationRate() {
        return starvationRate;
    }

    public void setStarvationRate(int starvationRate) {
        this.starvationRate = starvationRate;
    }

    public int arrowCooldown() {
        return arrowCooldown;
    }

    public void setArrowCooldown(int arrowCooldown) {
        this.arrowCooldown = arrowCooldown;
    }

    public int arrowsInBody() {
        return arrowsInBody;
    }

    public void setArrowsInBody(int arrowsInBody) {
        this.arrowsInBody = Math.max(0, arrowsInBody);
    }

    public int beeStingerCooldown() {
        return beeStingerCooldown;
    }

    public void setBeeStingerCooldown(int beeStingerCooldown) {
        this.beeStingerCooldown = beeStingerCooldown;
    }

    public int beeStingersInBody() {
        return beeStingersInBody;
    }

    public void setBeeStingersInBody(int beeStingersInBody) {
        this.beeStingersInBody = Math.max(0, beeStingersInBody);
    }

    public int wardenWarningCooldown() {
        return wardenWarningCooldown;
    }

    public void setWardenWarningCooldown(int wardenWarningCooldown) {
        this.wardenWarningCooldown = wardenWarningCooldown;
    }

    public int wardenTimeSinceLastWarning() {
        return wardenTimeSinceLastWarning;
    }

    public void setWardenTimeSinceLastWarning(int wardenTimeSinceLastWarning) {
        this.wardenTimeSinceLastWarning = wardenTimeSinceLastWarning;
    }

    public int wardenWarningLevel() {
        return wardenWarningLevel;
    }

    public void setWardenWarningLevel(int wardenWarningLevel) {
        this.wardenWarningLevel = Math.max(0, Math.min(4, wardenWarningLevel));
    }

    public int deathScreenScore() {
        return deathScreenScore;
    }

    public void setDeathScreenScore(int deathScreenScore) {
        this.deathScreenScore = deathScreenScore;
    }

    public int enchantmentSeed() {
        return enchantmentSeed;
    }

    public void setEnchantmentSeed(int enchantmentSeed) {
        this.enchantmentSeed = enchantmentSeed;
    }

    public int viewDistance() {
        return viewDistance;
    }

    public void setViewDistance(int viewDistance) {
        this.viewDistance = viewDistance;
    }

    public int simulationDistance() {
        return simulationDistance;
    }

    public void setSimulationDistance(int simulationDistance) {
        this.simulationDistance = simulationDistance;
    }

    public int sendViewDistance() {
        return sendViewDistance;
    }

    public void setSendViewDistance(int sendViewDistance) {
        this.sendViewDistance = sendViewDistance;
    }

    public net.kyori.adventure.util.TriState visualFire() {
        return visualFire;
    }

    public void setVisualFire(net.kyori.adventure.util.TriState visualFire) {
        this.visualFire = visualFire == null ? net.kyori.adventure.util.TriState.NOT_SET : visualFire;
    }

    public net.kyori.adventure.util.TriState flyingFallDamage() {
        return flyingFallDamage;
    }

    public void setFlyingFallDamage(net.kyori.adventure.util.TriState flyingFallDamage) {
        this.flyingFallDamage =
                flyingFallDamage == null ? net.kyori.adventure.util.TriState.NOT_SET : flyingFallDamage;
    }

    public net.kyori.adventure.util.TriState frictionState() {
        return frictionState;
    }

    public void setFrictionState(net.kyori.adventure.util.TriState frictionState) {
        this.frictionState = frictionState == null ? net.kyori.adventure.util.TriState.NOT_SET : frictionState;
    }

    public org.bukkit.inventory.MainHand mainHand() {
        return mainHand;
    }

    public void setMainHand(org.bukkit.inventory.MainHand mainHand) {
        this.mainHand = Objects.requireNonNull(mainHand, "mainHand");
    }

    public org.bukkit.WorldBorder worldBorder() {
        return worldBorder;
    }

    public void setWorldBorder(org.bukkit.WorldBorder worldBorder) {
        this.worldBorder = worldBorder;
    }

    public org.bukkit.entity.Pose fixedPose() {
        return fixedPose;
    }

    public void setFixedPose(org.bukkit.entity.Pose fixedPose) {
        this.fixedPose = fixedPose;
    }

    public org.bukkit.entity.Entity spectatorTarget() {
        return spectatorTarget;
    }

    public void setSpectatorTarget(org.bukkit.entity.Entity spectatorTarget) {
        this.spectatorTarget = spectatorTarget;
    }

    public org.bukkit.Location lastDeathLocation() {
        return lastDeathLocation == null ? null : lastDeathLocation.clone();
    }

    public void setLastDeathLocation(org.bukkit.Location lastDeathLocation) {
        this.lastDeathLocation = lastDeathLocation == null ? null : lastDeathLocation.clone();
    }

    public Long playerTimeOffset() {
        return playerTimeOffset;
    }

    public void setPlayerTime(long time, boolean relative) {
        this.playerTimeOffset = relative ? time : time - world.getTime();
        this.playerTimeRelative = relative;
    }

    public void resetPlayerTime() {
        this.playerTimeOffset = null;
        this.playerTimeRelative = true;
    }

    public boolean playerTimeRelative() {
        return playerTimeRelative;
    }

    public org.bukkit.WeatherType playerWeather() {
        return playerWeather;
    }

    public void setPlayerWeather(org.bukkit.WeatherType playerWeather) {
        this.playerWeather = playerWeather;
    }

    public org.bukkit.inventory.ItemStack cursorItem() {
        return cursorItem == null ? null : cursorItem.clone();
    }

    public void setCursorItem(org.bukkit.inventory.ItemStack cursorItem) {
        this.cursorItem = (cursorItem == null || cursorItem.getType().isAir() || cursorItem.getAmount() <= 0)
                ? null
                : cursorItem.clone();
    }

    public org.bukkit.inventory.EquipmentSlot activeItemHand() {
        return activeItemHand;
    }

    public void setActiveItemHand(org.bukkit.inventory.EquipmentSlot activeItemHand) {
        this.activeItemHand = activeItemHand;
    }

    public int activeItemRemainingTime() {
        return activeItemRemainingTime;
    }

    public void setActiveItemRemainingTime(int activeItemRemainingTime) {
        this.activeItemRemainingTime = Math.max(0, activeItemRemainingTime);
    }

    public int activeItemUseTicks() {
        return activeItemUseTicks;
    }

    public void setActiveItemUseTicks(int activeItemUseTicks) {
        this.activeItemUseTicks = Math.max(0, activeItemUseTicks);
    }

    public long lastActivityMillis() {
        return lastActivityMillis;
    }

    public void resetIdleDuration() {
        this.lastActivityMillis = System.currentTimeMillis();
    }

    public long lastAttackMillis() {
        return lastAttackMillis;
    }

    public void resetAttackCooldown() {
        this.lastAttackMillis = System.currentTimeMillis();
    }

    public java.util.Set<org.bukkit.NamespacedKey> discoveredRecipes() {
        return discoveredRecipes;
    }

    public java.util.Set<UUID> unlistedPlayers() {
        return unlistedPlayers;
    }

    public java.util.Set<String> customChatCompletions() {
        return customChatCompletions;
    }

    public java.util.Set<String> additionalChatCompletions() {
        return additionalChatCompletions;
    }

    public java.util.Map<org.bukkit.NamespacedKey, byte[]> cookies() {
        return cookies;
    }

    public java.util.Map<net.kyori.adventure.key.Key, java.util.function.Predicate<org.bukkit.entity.Entity>>
            alliedRules() {
        return alliedRules;
    }

    public java.util.Set<net.kyori.adventure.bossbar.BossBar> activeAdventureBossBars() {
        return activeAdventureBossBars;
    }

    public FerritePlayerProfile playerProfile() {
        if (playerProfile == null) {
            playerProfile = new FerritePlayerProfile(uniqueId, name);
        }
        return playerProfile;
    }

    public void setPlayerProfile(FerritePlayerProfile profile) {
        this.playerProfile = profile;
    }

    public FerriteSkinParts skinParts() {
        return skinParts;
    }
}
