package dev.ferrite.server;

import java.util.*;

/**
 * Player API that Ferrite answers from Java-side state: conversations, attributes, statistics,
 * discovered recipes, per-player time and weather, vehicle/leash facts, pose and body size.
 *
 * <p>Values describe the state Ferrite actually has: a player never rides or leashes anything
 * here, so those queries answer "no", and settings the client cannot be told about yet are
 * remembered so plugins read back what they wrote.
 */
@SuppressWarnings({"deprecation", "unchecked", "rawtypes"})
public abstract class PlayerExtras extends PlayerSight {
    // ---- conversations -------------------------------------------------------------------
    private final Deque<org.bukkit.conversations.Conversation> conversations = new ArrayDeque<>();

    @Override public synchronized boolean beginConversation(org.bukkit.conversations.Conversation conversation) {
        Objects.requireNonNull(conversation, "conversation");
        if (conversations.contains(conversation)) return false;
        conversations.addLast(conversation);
        if (conversations.size() == 1) {
            conversation.begin();
            conversation.outputNextPrompt();
        }
        return true;
    }
    @Override public synchronized boolean isConversing() { return !conversations.isEmpty(); }
    @Override public void abandonConversation(org.bukkit.conversations.Conversation conversation) {
        abandonConversation(conversation, new org.bukkit.conversations.ConversationAbandonedEvent(conversation));
    }
    @Override public void abandonConversation(org.bukkit.conversations.Conversation conversation, org.bukkit.conversations.ConversationAbandonedEvent details) {
        org.bukkit.conversations.Conversation next = null;
        synchronized (this) {
            if (conversations.isEmpty() || conversations.getFirst() != conversation) return;
            conversations.removeFirst();
            next = conversations.peekFirst();
        }
        conversation.abandon(details);
        if (next != null) {
            next.begin();
            next.outputNextPrompt();
        }
    }
    @Override public void acceptConversationInput(String input) {
        org.bukkit.conversations.Conversation current;
        synchronized (this) { current = conversations.peekFirst(); }
        if (current != null) current.acceptInput(input);
    }
    @Override public void sendRawMessage(String message) { sendMessage(message); }
    @Override public void sendRawMessage(UUID sender, String message) { sendMessage(message); }

    // ---- attributes ------------------------------------------------------------------------
    private final Map<org.bukkit.attribute.Attribute, ValueAttribute> attributes = new HashMap<>();

    private static double defaultOf(org.bukkit.attribute.Attribute attribute) {
        return switch (attribute.getKey().getKey()) {
            case "max_health" -> 20.0;
            case "movement_speed" -> 0.1;
            case "attack_damage" -> 1.0;
            case "attack_speed" -> 4.0;
            case "luck" -> 0.0;
            case "armor", "armor_toughness", "knockback_resistance", "attack_knockback", "absorption",
                 "max_absorption", "burning_time", "explosion_knockback_resistance", "fall_damage_multiplier",
                 "movement_efficiency", "oxygen_bonus", "sneaking_speed", "submerged_mining_speed",
                 "sweeping_damage_ratio", "water_movement_efficiency", "mining_efficiency", "spawn_reinforcements" -> 0.0;
            case "follow_range" -> 32.0;
            case "flying_speed" -> 0.4;
            case "step_height" -> 0.6;
            case "gravity" -> 0.08;
            case "jump_strength" -> 0.42;
            case "safe_fall_distance" -> 3.0;
            case "scale", "tempt_range" -> attribute.getKey().getKey().equals("scale") ? 1.0 : 10.0;
            case "block_interaction_range" -> 4.5;
            case "entity_interaction_range" -> 3.0;
            case "block_break_speed" -> 1.0;
            default -> 0.0;
        };
    }

    private static final class ValueAttribute implements org.bukkit.attribute.AttributeInstance {
        private final org.bukkit.attribute.Attribute attribute;
        private double base;
        private final Map<net.kyori.adventure.key.Key, org.bukkit.attribute.AttributeModifier> modifiers = new LinkedHashMap<>();

        private final Runnable changed;

        ValueAttribute(org.bukkit.attribute.Attribute attribute, Runnable changed) { this.attribute = attribute; this.base = defaultOf(attribute); this.changed = changed; }
        @Override public org.bukkit.attribute.Attribute getAttribute() { return attribute; }
        @Override public double getBaseValue() { return base; }
        @Override public void setBaseValue(double value) {
            if (!Double.isFinite(value)) throw new IllegalArgumentException("value must be finite");
            base = value;
            changed.run();
        }
        @Override public synchronized Collection<org.bukkit.attribute.AttributeModifier> getModifiers() { return List.copyOf(modifiers.values()); }
        @Override public synchronized org.bukkit.attribute.AttributeModifier getModifier(net.kyori.adventure.key.Key key) { return modifiers.get(key); }
        @Override public synchronized org.bukkit.attribute.AttributeModifier getModifier(UUID id) {
            for (var modifier : modifiers.values()) if (modifier.getUniqueId().equals(id)) return modifier;
            return null;
        }
        @Override public synchronized void removeModifier(net.kyori.adventure.key.Key key) { modifiers.remove(key); changed.run(); }
        @Override public synchronized void removeModifier(UUID id) { modifiers.values().removeIf(m -> m.getUniqueId().equals(id)); changed.run(); }
        @Override public synchronized void removeModifier(org.bukkit.attribute.AttributeModifier modifier) { modifiers.remove(modifier.getKey()); changed.run(); }
        @Override public synchronized void addModifier(org.bukkit.attribute.AttributeModifier modifier) {
            Objects.requireNonNull(modifier, "modifier");
            if (modifiers.putIfAbsent(modifier.getKey(), modifier) != null)
                throw new IllegalArgumentException("Modifier is already applied on this attribute!");
            changed.run();
        }
        @Override public void addTransientModifier(org.bukkit.attribute.AttributeModifier modifier) { addModifier(modifier); }
        @Override public synchronized double getValue() {
            double added = base, scalar = 0, product = 1;
            for (var m : modifiers.values()) {
                switch (m.getOperation()) {
                    case ADD_NUMBER -> added += m.getAmount();
                    case ADD_SCALAR -> scalar += m.getAmount();
                    case MULTIPLY_SCALAR_1 -> product *= 1 + m.getAmount();
                }
            }
            return added * (1 + scalar) * product;
        }
        @Override public double getDefaultValue() { return defaultOf(attribute); }
    }

    @Override public synchronized org.bukkit.attribute.AttributeInstance getAttribute(org.bukkit.attribute.Attribute attribute) {
        Objects.requireNonNull(attribute, "attribute");
        return attributes.computeIfAbsent(attribute, key -> {
            if (!key.getKey().getKey().equals("max_health")) return new ValueAttribute(key, () -> { });
            // The game keeps the player's health cap and tells the client, so changes are passed on.
            var holder = new ValueAttribute[1];
            holder[0] = new ValueAttribute(key, () -> { if (holder[0] != null) tellHost("setMaxHealth(double)", Math.max(1.0, Math.min(1024.0, holder[0].getValue()))); });
            return holder[0];
        });
    }
    @Override public void registerAttribute(org.bukkit.attribute.Attribute attribute) { getAttribute(attribute); }

    // ---- statistics ------------------------------------------------------------------------
    private final Map<String, Integer> statistics = new HashMap<>();

    private static String statKey(org.bukkit.Statistic statistic, Object qualifier) {
        Objects.requireNonNull(statistic, "statistic");
        boolean typed = statistic.getType() != org.bukkit.Statistic.Type.UNTYPED;
        if (typed && qualifier == null) throw new IllegalArgumentException("Statistic " + statistic + " needs a qualifier");
        if (!typed && qualifier != null) throw new IllegalArgumentException("Statistic " + statistic + " takes no qualifier");
        return statistic.name() + (qualifier == null ? "" : ":" + qualifier);
    }
    private synchronized int stat(String key) { return statistics.getOrDefault(key, 0); }
    private synchronized void putStat(String key, int value) {
        if (value < 0) throw new IllegalArgumentException("Value must be non-negative");
        statistics.put(key, value);
    }
    @Override public int getStatistic(org.bukkit.Statistic s) { return stat(statKey(s, null)); }
    @Override public int getStatistic(org.bukkit.Statistic s, org.bukkit.Material m) { return stat(statKey(s, Objects.requireNonNull(m, "material"))); }
    @Override public int getStatistic(org.bukkit.Statistic s, org.bukkit.entity.EntityType e) { return stat(statKey(s, Objects.requireNonNull(e, "entityType"))); }
    @Override public void setStatistic(org.bukkit.Statistic s, int v) { putStat(statKey(s, null), v); }
    @Override public void setStatistic(org.bukkit.Statistic s, org.bukkit.Material m, int v) { putStat(statKey(s, Objects.requireNonNull(m, "material")), v); }
    @Override public void setStatistic(org.bukkit.Statistic s, org.bukkit.entity.EntityType e, int v) { putStat(statKey(s, Objects.requireNonNull(e, "entityType")), v); }
    @Override public void incrementStatistic(org.bukkit.Statistic s) { incrementStatistic(s, 1); }
    @Override public void incrementStatistic(org.bukkit.Statistic s, int amount) { addStat(statKey(s, null), amount); }
    @Override public void incrementStatistic(org.bukkit.Statistic s, org.bukkit.Material m) { incrementStatistic(s, m, 1); }
    @Override public void incrementStatistic(org.bukkit.Statistic s, org.bukkit.Material m, int amount) { addStat(statKey(s, Objects.requireNonNull(m, "material")), amount); }
    @Override public void incrementStatistic(org.bukkit.Statistic s, org.bukkit.entity.EntityType e) { incrementStatistic(s, e, 1); }
    @Override public void incrementStatistic(org.bukkit.Statistic s, org.bukkit.entity.EntityType e, int amount) { addStat(statKey(s, Objects.requireNonNull(e, "entityType")), amount); }
    @Override public void decrementStatistic(org.bukkit.Statistic s) { decrementStatistic(s, 1); }
    @Override public void decrementStatistic(org.bukkit.Statistic s, int amount) { addStat(statKey(s, null), -amount); }
    @Override public void decrementStatistic(org.bukkit.Statistic s, org.bukkit.Material m) { decrementStatistic(s, m, 1); }
    @Override public void decrementStatistic(org.bukkit.Statistic s, org.bukkit.Material m, int amount) { addStat(statKey(s, Objects.requireNonNull(m, "material")), -amount); }
    @Override public void decrementStatistic(org.bukkit.Statistic s, org.bukkit.entity.EntityType e) { decrementStatistic(s, e, 1); }
    @Override public void decrementStatistic(org.bukkit.Statistic s, org.bukkit.entity.EntityType e, int amount) { addStat(statKey(s, Objects.requireNonNull(e, "entityType")), -amount); }
    private synchronized void addStat(String key, int delta) {
        long next = (long) stat(key) + delta;
        if (next < 0) throw new IllegalArgumentException("Value must be non-negative");
        statistics.put(key, (int) Math.min(next, Integer.MAX_VALUE));
    }

    // ---- recipe book -----------------------------------------------------------------------
    private final Set<org.bukkit.NamespacedKey> discovered = new LinkedHashSet<>();

    @Override public synchronized int discoverRecipes(Collection<org.bukkit.NamespacedKey> recipes) {
        int added = 0;
        for (var key : recipes) if (FerriteRecipes.get(key) != null && discovered.add(key)) added++;
        return added;
    }
    @Override public synchronized int undiscoverRecipes(Collection<org.bukkit.NamespacedKey> recipes) {
        int removed = 0;
        for (var key : recipes) if (discovered.remove(key)) removed++;
        return removed;
    }
    @Override public synchronized boolean hasDiscoveredRecipe(org.bukkit.NamespacedKey recipe) { return discovered.contains(Objects.requireNonNull(recipe, "recipe")); }
    @Override public synchronized Set<org.bukkit.NamespacedKey> getDiscoveredRecipes() { return Set.copyOf(discovered); }

    // ---- per-player time and weather ---------------------------------------------------------
    private Long fixedTime;
    private long timeOffset;
    private boolean timeRelative = true;
    private org.bukkit.WeatherType weather;

    @Override public void setPlayerTime(long time, boolean relative) {
        if (relative) { fixedTime = null; timeOffset = time; } else { fixedTime = time; timeOffset = 0; }
        timeRelative = relative;
    }
    @Override public long getPlayerTime() {
        if (!timeRelative && fixedTime != null) return fixedTime;
        return getWorld().getFullTime() + timeOffset;
    }
    @Override public long getPlayerTimeOffset() {
        return timeRelative ? timeOffset : (fixedTime == null ? 0 : fixedTime - getWorld().getFullTime());
    }
    @Override public boolean isPlayerTimeRelative() { return timeRelative; }
    @Override public void resetPlayerTime() { fixedTime = null; timeOffset = 0; timeRelative = true; }
    @Override public void setPlayerWeather(org.bukkit.WeatherType type) { weather = Objects.requireNonNull(type, "type"); }
    @Override public org.bukkit.WeatherType getPlayerWeather() { return weather; }
    @Override public void resetPlayerWeather() { weather = null; }

    // ---- vehicles, passengers and leashes ------------------------------------------------------
    private String rideAsk(String method) {
        try { return (String) HostBridge.call("Player", getUniqueId(), method, String.class); }
        catch (UnsupportedOperationException | IllegalStateException noHost) { return null; }
    }
    /** The entity with this id: an online player, or a handle built from what the host knows of it. */
    private org.bukkit.entity.Entity entityById(String id) {
        var uuid = java.util.UUID.fromString(id);
        var online = org.bukkit.Bukkit.getPlayer(uuid);
        if (online != null) return online;
        String kind;
        try { kind = (String) HostBridge.call("Entity", uuid, "getEntityType()", String.class); }
        catch (UnsupportedOperationException | IllegalStateException unknown) { return null; }
        var type = kind == null || kind.isEmpty() ? null : org.bukkit.Registry.ENTITY_TYPE.get(org.bukkit.NamespacedKey.fromString(kind));
        if (type == null) return null;
        return SpawnedEntity.of(type, getLocation(), getWorld(), uuid);
    }
    @Override public org.bukkit.entity.Entity getVehicle() { var id = rideAsk("getVehicle()"); return id == null || id.isEmpty() ? null : entityById(id); }
    @Override public boolean isInsideVehicle() { return "true".equals(rideAsk("isInsideVehicle()")); }
    @Override public boolean leaveVehicle() { return "true".equals(rideAsk("leaveVehicle()")); }
    @Override public List<org.bukkit.entity.Entity> getPassengers() {
        var ids = rideAsk("getPassengers()");
        var result = new java.util.ArrayList<org.bukkit.entity.Entity>();
        if (ids == null || ids.isEmpty()) return result;
        for (var id : ids.split(String.valueOf(HostBridge.ITEM))) { var entity = entityById(id); if (entity != null) result.add(entity); }
        return result;
    }
    @Override public org.bukkit.entity.Entity getPassenger() { var all = getPassengers(); return all.isEmpty() ? null : all.get(0); }
    @Override public boolean isEmpty() { var answer = rideAsk("isEmpty()"); return answer == null || Boolean.parseBoolean(answer); }
    @Override public boolean eject() { return "true".equals(rideAsk("eject()")); }
    @Override public boolean removePassenger(org.bukkit.entity.Entity passenger) {
        Objects.requireNonNull(passenger, "passenger");
        return HostBridge.callBoolean("Player", getUniqueId(), "removePassenger(Entity)", passenger);
    }
    @Override public boolean isLeashed() { return false; }
    @Override public org.bukkit.entity.Entity getLeashHolder() { throw new IllegalStateException("Entity not leashed"); }

    // ---- body, pose and damage bookkeeping ------------------------------------------------------
    private org.bukkit.entity.Pose pose = org.bukkit.entity.Pose.STANDING;
    private boolean fixedPose;
    private int arrowsInBody, beeStingersInBody, arrowCooldown, beeStingerCooldown, expCooldown;
    private int noDamageTicks, maximumNoDamageTicks = 20;
    private double lastDamage;
    private float fallDistance;
    private double absorption;
    private boolean freezeLocked;

    @Override public org.bukkit.entity.Pose getPose() {
        if (fixedPose) return pose;
        if (isSneaking()) return org.bukkit.entity.Pose.SNEAKING;
        if (isSwimming()) return org.bukkit.entity.Pose.SWIMMING;
        if (isSleeping()) return org.bukkit.entity.Pose.SLEEPING;
        return org.bukkit.entity.Pose.STANDING;
    }
    @Override public void setPose(org.bukkit.entity.Pose value, boolean fixed) { pose = Objects.requireNonNull(value, "pose"); fixedPose = fixed; }
    @Override public boolean hasFixedPose() { return fixedPose; }
    @Override public void unsetFixedPose() { fixedPose = false; }
    @Override public double getHeight() {
        return switch (getPose()) {
            case SNEAKING -> 1.5;
            case SLEEPING -> 0.2;
            case SWIMMING, FALL_FLYING, SPIN_ATTACK -> 0.6;
            default -> 1.8;
        };
    }
    @Override public double getWidth() { return getPose() == org.bukkit.entity.Pose.SLEEPING ? 0.2 : 0.6; }
    @Override public org.bukkit.util.BoundingBox getBoundingBox() {
        var l = getLocation();
        double half = getWidth() / 2;
        return new org.bukkit.util.BoundingBox(l.getX() - half, l.getY(), l.getZ() - half, l.getX() + half, l.getY() + getHeight(), l.getZ() + half);
    }
    @Override public org.bukkit.block.BlockFace getFacing() {
        float yaw = (getLocation().getYaw() % 360 + 360) % 360;
        if (yaw >= 315 || yaw < 45) return org.bukkit.block.BlockFace.SOUTH;
        if (yaw < 135) return org.bukkit.block.BlockFace.WEST;
        if (yaw < 225) return org.bukkit.block.BlockFace.NORTH;
        return org.bukkit.block.BlockFace.EAST;
    }
    @Override public boolean isInvulnerableTo(org.bukkit.damage.DamageSource source) {
        Objects.requireNonNull(source, "source");
        return isInvulnerable() || getGameMode() == org.bukkit.GameMode.CREATIVE || getGameMode() == org.bukkit.GameMode.SPECTATOR;
    }
    @Override public boolean isMarkedInvulnerable() { return isInvulnerable(); }
    @Override public boolean isInWater() { return getLocation().getBlock().getType() == org.bukkit.Material.WATER; }
    @Override public boolean isInLava() { return getLocation().getBlock().getType() == org.bukkit.Material.LAVA; }
    @Override public boolean isInPowderedSnow() { return getLocation().getBlock().getType() == org.bukkit.Material.POWDER_SNOW; }
    @Override public boolean isUnderWater() { return getEyeLocation().getBlock().getType() == org.bukkit.Material.WATER; }
    @Override public boolean isInRain() { return false; }
    @Override public boolean isFrozen() { return getFreezeTicks() >= getMaxFreezeTicks(); }
    @Override public int getMaxFreezeTicks() { return 140; }
    @Override public int getMaxFireTicks() { return 20; }
    @Override public boolean isFreezeTickingLocked() { return freezeLocked; }
    @Override public void lockFreezeTicks(boolean locked) { freezeLocked = locked; }
    @Override public int getArrowsInBody() { return arrowsInBody; }
    @Override public void setArrowsInBody(int count, boolean fireEvent) {
        if (count < 0) throw new IllegalArgumentException("New arrow amount must be >= 0");
        arrowsInBody = count;
    }
    @Override public int getBeeStingersInBody() { return beeStingersInBody; }
    @Override public void setBeeStingersInBody(int count) {
        if (count < 0) throw new IllegalArgumentException("New bee stinger amount must be >= 0");
        beeStingersInBody = count;
    }
    @Override public int getArrowCooldown() { return arrowCooldown; }
    @Override public void setArrowCooldown(int ticks) { arrowCooldown = ticks; }
    @Override public int getBeeStingerCooldown() { return beeStingerCooldown; }
    @Override public void setBeeStingerCooldown(int ticks) { beeStingerCooldown = ticks; }
    @Override public int getExpCooldown() { return expCooldown; }
    @Override public void setExpCooldown(int ticks) { expCooldown = ticks; }
    @Override public int getNoDamageTicks() { return noDamageTicks; }
    @Override public void setNoDamageTicks(int ticks) { noDamageTicks = ticks; }
    @Override public int getMaximumNoDamageTicks() { return maximumNoDamageTicks; }
    @Override public void setMaximumNoDamageTicks(int ticks) { maximumNoDamageTicks = ticks; }
    @Override public double getLastDamage() { return lastDamage; }
    @Override public void setLastDamage(double damage) { lastDamage = damage; }
    @Override public float getFallDistance() { return fallDistance; }
    @Override public void setFallDistance(float distance) { fallDistance = distance; super.setFallDistance(distance); }
    @Override public double getAbsorptionAmount() { return absorption; }
    @Override public void setAbsorptionAmount(double amount) { absorption = amount; super.setAbsorptionAmount(amount); }
    @Override public boolean canBreatheUnderwater() { return false; }
    @Override public boolean isRiptiding() { return false; }
    @Override public org.bukkit.inventory.ItemStack getActiveItem() {
        if (!hasActiveItem()) return org.bukkit.inventory.ItemStack.empty();
        var item = getActiveItemHand() == org.bukkit.inventory.EquipmentSlot.OFF_HAND ? getInventory().getItemInOffHand() : getInventory().getItemInMainHand();
        return item == null ? org.bukkit.inventory.ItemStack.empty() : item;
    }
    @Override public org.bukkit.entity.FishHook getFishHook() { return null; }
    // Shoulder parrots live in the game: the host owns the table (saved with the player), a write is applied by the
    // player's own region, and a read returns a detached parrot handle carrying the seated parrot's uuid.
    private org.bukkit.entity.Entity shoulder(String side) {
        if (!HostBridge.available()) return null; // no host, so nothing can be seated
        var reply = (String) HostBridge.call("Player", getUniqueId(), "getShoulder(String)", String.class, side);
        if (reply == null || reply.isEmpty()) return null;
        return SpawnedEntity.of(org.bukkit.entity.EntityType.PARROT, getLocation(), getWorld(), java.util.UUID.fromString(reply.split(",", 2)[0]));
    }
    @Override public org.bukkit.entity.Entity getShoulderEntityRight() { return shoulder("RIGHT"); }
    @Override public org.bukkit.entity.Entity getShoulderEntityLeft() { return shoulder("LEFT"); }
    /** null empties the shoulder, a parrot is seated there (a live one leaves the world), anything else is refused. */
    private void seat(String side, org.bukkit.entity.Entity entity) {
        if (entity != null && entity.getType() != org.bukkit.entity.EntityType.PARROT) throw new IllegalArgumentException("Only parrots can sit on a shoulder, not " + entity.getType());
        HostBridge.callVoid("Player", getUniqueId(), "shoulder(String,String)", side, entity == null ? "CLEAR" : "SEAT:" + entity.getUniqueId());
    }
    @Override public void setShoulderEntityLeft(org.bukkit.entity.Entity entity) { seat("LEFT", entity); }
    @Override public void setShoulderEntityRight(org.bukkit.entity.Entity entity) { seat("RIGHT", entity); }
    @Override public org.bukkit.entity.Entity releaseLeftShoulderEntity() { return releaseShoulder("LEFT"); }
    @Override public org.bukkit.entity.Entity releaseRightShoulderEntity() { return releaseShoulder("RIGHT"); }
    /** The parrot that was on the shoulder, now standing in the world beside the player; null when the shoulder was empty. */
    private org.bukkit.entity.Entity releaseShoulder(String side) {
        var parrot = shoulder(side);
        if (parrot == null) return null;
        HostBridge.callVoid("Player", getUniqueId(), "shoulder(String,String)", side, "RELEASE");
        return parrot;
    }
    @Override public double getDefaultGravity() { return 0.08; }
    @Override public float getAirDrag() { return 0.02f; }
    @Override public org.bukkit.entity.EntityCategory getCategory() { return org.bukkit.entity.EntityCategory.NONE; }
    @Override public org.bukkit.entity.SpawnCategory getSpawnCategory() { return org.bukkit.entity.SpawnCategory.MISC; }
    @Override public org.bukkit.block.PistonMoveReaction getPistonMoveReaction() { return org.bukkit.block.PistonMoveReaction.IGNORE; }
    /** What the client said in its settings; right-handed until it has said anything. */
    private String clientSetting(String method, String fallback) {
        try { return HostBridge.call("Player", getUniqueId(), method, String.class) instanceof String text ? text : fallback; }
        catch (UnsupportedOperationException | IllegalStateException none) { return fallback; }
    }
    @Override public org.bukkit.inventory.MainHand getMainHand() { return "LEFT".equals(clientSetting("getMainHand()", "RIGHT")) ? org.bukkit.inventory.MainHand.LEFT : org.bukkit.inventory.MainHand.RIGHT; }
    @Override public int getClientViewDistance() { try { return Integer.parseInt(clientSetting("getClientViewDistance()", "10")); } catch (NumberFormatException e) { return 10; } }
    @Override @SuppressWarnings("unchecked")
    public <T> T getClientOption(com.destroystokyo.paper.ClientOption<T> option) {
        Objects.requireNonNull(option, "option");
        if (option == com.destroystokyo.paper.ClientOption.LOCALE) return (T) getLocale();
        if (option == com.destroystokyo.paper.ClientOption.MAIN_HAND) return (T) getMainHand();
        if (option == com.destroystokyo.paper.ClientOption.VIEW_DISTANCE) return (T) Integer.valueOf(getClientViewDistance());
        if (option == com.destroystokyo.paper.ClientOption.CHAT_COLORS_ENABLED) return (T) Boolean.valueOf(clientSetting("getClientChatColors()", "true"));
        if (option == com.destroystokyo.paper.ClientOption.TEXT_FILTERING_ENABLED) return (T) Boolean.valueOf(clientSetting("getClientTextFiltering()", "false"));
        if (option == com.destroystokyo.paper.ClientOption.ALLOW_SERVER_LISTINGS) return (T) Boolean.valueOf(isAllowingServerListings());
        if (option == com.destroystokyo.paper.ClientOption.CHAT_VISIBILITY) {
            return (T) switch (clientSetting("getClientChatMode()", "0")) {
                case "0" -> com.destroystokyo.paper.ClientOption.ChatVisibility.FULL;
                case "1" -> com.destroystokyo.paper.ClientOption.ChatVisibility.SYSTEM;
                case "2" -> com.destroystokyo.paper.ClientOption.ChatVisibility.HIDDEN;
                default -> com.destroystokyo.paper.ClientOption.ChatVisibility.UNKNOWN;
            };
        }
        if (option == com.destroystokyo.paper.ClientOption.PARTICLE_VISIBILITY) {
            return (T) switch (clientSetting("getClientParticleStatus()", "0")) {
                case "1" -> com.destroystokyo.paper.ClientOption.ParticleVisibility.DECREASED;
                case "2" -> com.destroystokyo.paper.ClientOption.ParticleVisibility.MINIMAL;
                default -> com.destroystokyo.paper.ClientOption.ParticleVisibility.ALL;
            };
        }
        if (option == com.destroystokyo.paper.ClientOption.SKIN_PARTS) {
            int raw;
            try { raw = Integer.parseInt(clientSetting("getClientSkinParts()", "127")); } catch (NumberFormatException e) { raw = 127; }
            return (T) new ClientSkinParts(raw);
        }
        throw new IllegalArgumentException("Unknown client option " + option);
    }
    /** The model parts a client shows, as the raw settings bits. */
    static final class ClientSkinParts implements com.destroystokyo.paper.SkinParts.Mutable {
        private int raw;
        ClientSkinParts(int raw) { this.raw = raw & 0x7f; }
        private boolean bit(int mask) { return (raw & mask) != 0; }
        private void bit(int mask, boolean on) { raw = on ? raw | mask : raw & ~mask; }
        @Override public boolean hasCapeEnabled() { return bit(1); }
        @Override public boolean hasJacketEnabled() { return bit(2); }
        @Override public boolean hasLeftSleeveEnabled() { return bit(4); }
        @Override public boolean hasRightSleeveEnabled() { return bit(8); }
        @Override public boolean hasLeftPantsEnabled() { return bit(16); }
        @Override public boolean hasRightPantsEnabled() { return bit(32); }
        @Override public boolean hasHatsEnabled() { return bit(64); }
        @Override public int getRaw() { return raw; }
        @Override public void setCapeEnabled(boolean on) { bit(1, on); }
        @Override public void setJacketEnabled(boolean on) { bit(2, on); }
        @Override public void setLeftSleeveEnabled(boolean on) { bit(4, on); }
        @Override public void setRightSleeveEnabled(boolean on) { bit(8, on); }
        @Override public void setLeftPantsEnabled(boolean on) { bit(16, on); }
        @Override public void setRightPantsEnabled(boolean on) { bit(32, on); }
        @Override public void setHatsEnabled(boolean on) { bit(64, on); }
        @Override public com.destroystokyo.paper.SkinParts.Mutable mutableCopy() { return new ClientSkinParts(raw); }
        @Override public com.destroystokyo.paper.SkinParts immutableCopy() { return new ClientSkinParts(raw); }
    }
    @Override public org.bukkit.SoundCategory getSoundCategory() { return org.bukkit.SoundCategory.PLAYERS; }
    @Override public String getLocale() { return clientSetting("getLocale()", "en_us"); }
    @Override public Locale locale() { return Locale.forLanguageTag(getLocale().replace('_', '-')); }
    @Override public int getDeathScreenScore() { return getTotalExperience(); }
    @Override public boolean getAffectsSpawning() { return true; }
    @Override public boolean getRemoveWhenFarAway() { return false; }
    @Override public int getSaturatedRegenRate() { return 10; }
    @Override public org.bukkit.Location getOrigin() { return null; }

    // ---- nearby players ---------------------------------------------------------------------
    /** Players inside the box around this player; non-player entities are owned by their regions. */
    @Override public List<org.bukkit.entity.Entity> getNearbyEntities(double x, double y, double z) {
        var here = getLocation();
        var out = new ArrayList<org.bukkit.entity.Entity>();
        for (var other : getServer().getOnlinePlayers()) {
            if (other.equals(this) || !Objects.equals(other.getWorld(), getWorld())) continue;
            var at = other.getLocation();
            if (Math.abs(at.getX() - here.getX()) <= x && Math.abs(at.getY() - here.getY()) <= y && Math.abs(at.getZ() - here.getZ()) <= z) out.add(other);
        }
        return out;
    }
}
