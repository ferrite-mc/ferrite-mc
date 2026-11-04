package dev.ferrite.server;

import java.util.*;
import java.util.function.Predicate;

/**
 * Player API answered from Java-side state: ally rules, tab-list and visibility choices, the
 * ender chest, the entity scheduler, titles, batched block changes and idle time.
 */
@SuppressWarnings({"deprecation", "unchecked", "rawtypes"})
public abstract class PlayerMisc extends PlayerStubs {
    // ---- allied rules ------------------------------------------------------------------------
    private final Map<net.kyori.adventure.key.Key, Predicate<org.bukkit.entity.Entity>> alliedRules = new LinkedHashMap<>();

    @Override public synchronized void addAdditionalAlliedRule(net.kyori.adventure.key.Key key, Predicate<org.bukkit.entity.Entity> rule) {
        alliedRules.put(Objects.requireNonNull(key, "key"), Objects.requireNonNull(rule, "rule"));
    }
    @Override public synchronized void removeAdditionalAlliedRule(net.kyori.adventure.key.Key key) { alliedRules.remove(key); }
    @Override public synchronized void removeAdditionalAlliedRule(net.kyori.adventure.key.Namespaced key) {
        if (key instanceof net.kyori.adventure.key.Key exact) alliedRules.remove(exact);
        else alliedRules.keySet().removeIf(k -> k.namespace().equals(key.namespace()));
    }
    @Override public synchronized Map<net.kyori.adventure.key.Key, Predicate<org.bukkit.entity.Entity>> getAdditionalAlliedRules() { return Map.copyOf(alliedRules); }
    @Override public boolean isAlliedTo(org.bukkit.entity.Entity entity) {
        Objects.requireNonNull(entity, "entity");
        if (entity.equals(this)) return true;
        List<Predicate<org.bukkit.entity.Entity>> rules;
        synchronized (this) { rules = List.copyOf(alliedRules.values()); }
        for (var rule : rules) if (rule.test(entity)) return true;
        if (entity instanceof org.bukkit.entity.Player other) {
            var mine = getScoreboard().getEntryTeam(getName());
            return mine != null && mine.equals(other.getScoreboard().getEntryTeam(other.getName())) && mine.allowFriendlyFire() == false;
        }
        return false;
    }

    // ---- chat completions --------------------------------------------------------------------
    @Override public void addAdditionalChatCompletions(Collection<String> completions) { addCustomChatCompletions(completions); }
    @Override public void removeAdditionalChatCompletions(Collection<String> completions) { removeCustomChatCompletions(completions); }

    // ---- settings the server only remembers -----------------------------------------------------------
    // These are kept for the plugin that set them and read back; the client is not told and no server
    // rule consults them (yet), so they have no in-game effect.
    private String customName;
    private boolean customNameVisible, sleepingIgnored, healthScaled, affectsSpawning = true, collidable = true, canPickupItems = true, persistent = true, ai = true;
    private double healthScale = 20.0;

    @Override public synchronized String getCustomName() { return customName; }
    @Override public synchronized void setCustomName(String name) { customName = name; }
    @Override public synchronized boolean isCustomNameVisible() { return customNameVisible; }
    @Override public synchronized void setCustomNameVisible(boolean flag) { customNameVisible = flag; }
    @Override public synchronized boolean isSleepingIgnored() { return sleepingIgnored; }
    @Override public synchronized void setSleepingIgnored(boolean isSleeping) { sleepingIgnored = isSleeping; }
    @Override public synchronized boolean isCollidable() { return collidable; }
    @Override public synchronized void setCollidable(boolean collidable) { this.collidable = collidable; }
    @Override public synchronized boolean getAffectsSpawning() { return affectsSpawning; }
    @Override public synchronized void setAffectsSpawning(boolean affects) { affectsSpawning = affects; }
    @Override public synchronized boolean getCanPickupItems() { return canPickupItems; }
    @Override public synchronized void setCanPickupItems(boolean pickup) { canPickupItems = pickup; }
    @Override public synchronized boolean isPersistent() { return persistent; }
    @Override public synchronized void setPersistent(boolean persistent) { this.persistent = persistent; }
    @Override public synchronized boolean hasAI() { return ai; }
    @Override public synchronized void setAI(boolean ai) { this.ai = ai; }
    @Override public synchronized boolean isHealthScaled() { return healthScaled; }
    @Override public synchronized void setHealthScaled(boolean scale) { healthScaled = scale; }
    @Override public synchronized double getHealthScale() { return healthScale; }
    @Override public synchronized void setHealthScale(double scale) {
        if (!(scale > 0) || Double.isInfinite(scale)) throw new IllegalArgumentException("scale must be greater than 0 and finite");
        healthScale = scale;
    }

    // ---- tab list and visibility ---------------------------------------------------------------
    private final Set<UUID> unlisted = new HashSet<>();
    private final Set<UUID> hidden = new HashSet<>();

    @Override public synchronized boolean isListed(org.bukkit.entity.Player other) { return !unlisted.contains(Objects.requireNonNull(other, "player").getUniqueId()); }
    @Override public synchronized boolean unlistPlayer(org.bukkit.entity.Player other) { return unlisted.add(Objects.requireNonNull(other, "player").getUniqueId()); }
    @Override public synchronized boolean listPlayer(org.bukkit.entity.Player other) { return unlisted.remove(Objects.requireNonNull(other, "player").getUniqueId()); }
    /** Visibility is remembered here either way; with no host attached (plain Java) only the memory applies. */
    private void tellHost(String method, Object... args) {
        try { HostBridge.callVoid("Player", getUniqueId(), method, args); } catch (UnsupportedOperationException offline) { /* no host */ }
    }
    @Override public void sendSignChange(org.bukkit.Location loc, String[] lines, org.bukkit.DyeColor dyeColor, boolean hasGlowingText) {
        java.util.List<String> text = new java.util.ArrayList<>();
        if (lines != null) for (int i = 0; i < Math.min(4, lines.length); i++) text.add(lines[i]);
        HostBridge.callVoid("Player", getUniqueId(), "sendSignChange(Location,String[],DyeColor,boolean)", loc, text, dyeColor == null ? org.bukkit.DyeColor.BLACK : dyeColor, hasGlowingText);
    }
    @Override public void sendSignChange(org.bukkit.Location loc, java.util.List<? extends net.kyori.adventure.text.Component> lines, org.bukkit.DyeColor dyeColor, boolean hasGlowingText) {
        String[] text = new String[4];
        if (lines != null) for (int i = 0; i < Math.min(4, lines.size()); i++) text[i] = net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(lines.get(i));
        sendSignChange(loc, text, dyeColor, hasGlowingText);
    }
    @Override public void playNote(org.bukkit.Location loc, org.bukkit.Instrument instrument, org.bukkit.Note note) {
        HostBridge.callVoid("Player", getUniqueId(), "playNote(Location,Instrument,int)", loc, instrument, (int) note.getId());
    }
    @Override public void sendLinks(org.bukkit.ServerLinks links) {
        java.util.List<String> entries = new java.util.ArrayList<>();
        for (var link : links.getLinks()) {
            var type = link.getType();
            String head = type != null ? "KNOWN:" + type.ordinal() : "CUSTOM:" + link.getDisplayName();
            entries.add(head + "	" + link.getUrl());
        }
        HostBridge.callVoid("Player", getUniqueId(), "sendLinks(ServerLinks)", entries);
    }
    /** Sends the property to the open menu; false when the value does not fit the client's 16-bit field. */
    @Override public boolean setWindowProperty(org.bukkit.inventory.InventoryView.Property prop, int value) {
        Objects.requireNonNull(prop, "prop");
        if (value < Short.MIN_VALUE || value > Short.MAX_VALUE) return false;
        tellHost("setWindowProperty(int,int)", prop.getId(), value);
        return true;
    }
    @Override public synchronized void hidePlayer(org.bukkit.entity.Player other) { hidden.add(Objects.requireNonNull(other, "player").getUniqueId()); tellHost("hidePlayer(Player)", other); }
    @Override public synchronized void showPlayer(org.bukkit.entity.Player other) { hidden.remove(Objects.requireNonNull(other, "player").getUniqueId()); tellHost("showPlayer(Player)", other); }
    @Override public synchronized void hideEntity(org.bukkit.plugin.Plugin plugin, org.bukkit.entity.Entity entity) {
        Objects.requireNonNull(plugin, "plugin");
        hidden.add(Objects.requireNonNull(entity, "entity").getUniqueId());
        tellHost("hideEntity(Plugin,Entity)", plugin, entity);
    }
    @Override public synchronized void showEntity(org.bukkit.plugin.Plugin plugin, org.bukkit.entity.Entity entity) {
        Objects.requireNonNull(plugin, "plugin");
        hidden.remove(Objects.requireNonNull(entity, "entity").getUniqueId());
        tellHost("showEntity(Plugin,Entity)", plugin, entity);
    }
    @Override public synchronized boolean canSee(org.bukkit.entity.Entity entity) { return !hidden.contains(Objects.requireNonNull(entity, "entity").getUniqueId()); }
    @Override public boolean canSee(org.bukkit.entity.Player other) { return canSee((org.bukkit.entity.Entity) other); }
    @Override public String getPlayerListName() {
        return net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer.legacySection().serialize(playerListName());
    }

    // ---- containers and equipment --------------------------------------------------------------
    private org.bukkit.inventory.Inventory enderChest;

    @Override public synchronized org.bukkit.inventory.Inventory getEnderChest() {
        if (enderChest == null) enderChest = getServer().createInventory(this, org.bukkit.event.inventory.InventoryType.ENDER_CHEST);
        return enderChest;
    }
    @Override public org.bukkit.inventory.EntityEquipment getEquipment() {
        var inventory = getInventory();
        return (org.bukkit.inventory.EntityEquipment) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.inventory.EntityEquipment.class}, (proxy, method, args) -> {
            try {
                return inventory.getClass().getMethod(method.getName(), method.getParameterTypes()).invoke(inventory, args);
            } catch (NoSuchMethodException e) {
                throw new UnsupportedOperationException("Ferrite: not implemented: EntityEquipment#" + method.getName());
            } catch (java.lang.reflect.InvocationTargetException e) {
                throw e.getCause();
            }
        });
    }
    @Override public org.bukkit.inventory.ItemStack getItemOnCursor() { return getOpenInventory().getCursor(); }
    /** The client keeps the cookie under the key; a cookie is at most 5120 bytes. */
    @Override public void storeCookie(org.bukkit.NamespacedKey key, byte[] value) {
        Objects.requireNonNull(key, "key");
        Objects.requireNonNull(value, "value");
        if (value.length > 5120) throw new IllegalArgumentException("Cookie payload too large (" + value.length + " > 5120)");
        HostBridge.callVoid("Player", getUniqueId(), "storeCookie(NamespacedKey,String)", key, java.util.Base64.getEncoder().encodeToString(value));
    }
    /** Opens the sign editor for the given side; only a sign block in the player's world can be edited. */
    @Override public void openSign(org.bukkit.block.Sign sign, org.bukkit.block.sign.Side side) {
        Objects.requireNonNull(sign, "sign");
        Objects.requireNonNull(side, "side");
        HostBridge.callVoid("Player", getUniqueId(), "openSign(Location,Side)", sign.getLocation(), side);
    }
    /** Opens a trading screen for the merchant's recipes. Uses are tracked by the open screen only. */
    @Override public org.bukkit.inventory.InventoryView openMerchant(org.bukkit.inventory.Merchant merchant, boolean force) {
        Objects.requireNonNull(merchant, "merchant");
        var item = (char) 0x1c; var offer = (char) 0x1e;
        var offers = new StringBuilder();
        for (var recipe : merchant.getRecipes()) {
            var ingredients = recipe.getIngredients();
            if (ingredients.isEmpty() || ingredients.size() > 2) continue;
            if (offers.length() > 0) offers.append(offer);
            offers.append(HostBridge.encode(recipe.getResult())).append(item)
                  .append(HostBridge.encode(ingredients.get(0))).append(item)
                  .append(ingredients.size() > 1 ? HostBridge.encode(ingredients.get(1)) : HostBridge.NULL).append(item)
                  .append(recipe.getUses()).append(item).append(recipe.getMaxUses()).append(item)
                  .append(recipe.getVillagerExperience()).append(item).append(recipe.getPriceMultiplier());
        }
        if (offers.length() == 0) return null;
        String title = merchant instanceof FerriteMerchant own && own.getTitle() != null ? own.getTitle() : "Villager";
        HostBridge.callVoid("Player", getUniqueId(), "openMerchant(String,String)", title, offers.toString());
        try { return getOpenInventory(); } catch (UnsupportedOperationException | IllegalStateException unavailable) { return null; }
    }
    /** The movement keys the client last reported (`player_input`); all false until it sends any. */
    @Override public org.bukkit.Input getCurrentInput() {
        int flags = HostBridge.callInt("Player", getUniqueId(), "getInputFlags()");
        return new org.bukkit.Input() {
            @Override public boolean isForward() { return (flags & 1) != 0; }
            @Override public boolean isBackward() { return (flags & 2) != 0; }
            @Override public boolean isLeft() { return (flags & 4) != 0; }
            @Override public boolean isRight() { return (flags & 8) != 0; }
            @Override public boolean isJump() { return (flags & 16) != 0; }
            @Override public boolean isSneak() { return (flags & 32) != 0; }
            @Override public boolean isSprint() { return (flags & 64) != 0; }
        };
    }
    /** The client's latest answer to a pushed resource pack, or null before it has sent any. */
    @Override public org.bukkit.event.player.PlayerResourcePackStatusEvent.Status getResourcePackStatus() {
        try { return super.getResourcePackStatus(); } catch (UnsupportedOperationException none) { return null; }
    }
    @Override public org.bukkit.inventory.ItemStack getItemInHand() { return getInventory().getItemInMainHand(); }
    @Override public void setItemInHand(org.bukkit.inventory.ItemStack item) { getInventory().setItemInMainHand(item); }
    @Override public void setItemOnCursor(org.bukkit.inventory.ItemStack item) { getOpenInventory().setCursor(item); }
    @Override public org.bukkit.inventory.ItemStack getWeapon() { return getInventory().getItemInMainHand(); }
    @Override public org.bukkit.inventory.ItemStack getPickItemStack() { return null; }

    // ---- scheduler and view ranges ---------------------------------------------------------------
    private io.papermc.paper.threadedregions.scheduler.EntityScheduler scheduler;

    @Override public synchronized io.papermc.paper.threadedregions.scheduler.EntityScheduler getScheduler() {
        if (scheduler == null) scheduler = ((FerriteServer) getServer()).entityScheduler(getUniqueId(), this::isOnline);
        return scheduler;
    }
    @Override public int getViewDistance() { return getServer().getViewDistance(); }
    @Override public int getSendViewDistance() { return getServer().getViewDistance(); }
    @Override public int getSimulationDistance() { return getServer().getSimulationDistance(); }

    // ---- entity data that players do not carry -----------------------------------------------------
    @Override public <T> T getMemory(org.bukkit.entity.memory.MemoryKey<T> key) { Objects.requireNonNull(key, "key"); return null; }
    @Override public <T> void setMemory(org.bukkit.entity.memory.MemoryKey<T> key, T value) { Objects.requireNonNull(key, "key"); }
    @Override public <T> T getData(io.papermc.paper.datacomponent.DataComponentType.Valued<T> type) { Objects.requireNonNull(type, "type"); return null; }
    @Override public boolean hasData(io.papermc.paper.datacomponent.DataComponentType type) { Objects.requireNonNull(type, "type"); return false; }
    @Override public <T> T getDataOrDefault(io.papermc.paper.datacomponent.DataComponentType.Valued<? extends T> type, T fallback) { Objects.requireNonNull(type, "type"); return fallback; }
    @Override public org.bukkit.event.entity.CreatureSpawnEvent.SpawnReason getEntitySpawnReason() { return org.bukkit.event.entity.CreatureSpawnEvent.SpawnReason.DEFAULT; }
    @Override public boolean fromMobSpawner() { return false; }
    @Override public org.bukkit.event.entity.EntityDamageEvent getLastDamageCause() { return lastCause; }
    @Override public void setLastDamageCause(org.bukkit.event.entity.EntityDamageEvent event) { lastCause = event; }
    private org.bukkit.event.entity.EntityDamageEvent lastCause;
    @Override public java.util.Set<UUID> getCollidableExemptions() { return exemptions; }
    private final Set<UUID> exemptions = new java.util.concurrent.CopyOnWriteArraySet<>();
    @Override public void resetMaxHealth() { getAttribute(org.bukkit.attribute.Attribute.MAX_HEALTH).setBaseValue(20.0); }

    // ---- idle time ---------------------------------------------------------------------------------
    private volatile long lastActive = System.nanoTime();

    /** Called when the player moves, chats or acts. */
    public void touch() { lastActive = System.nanoTime(); }
    @Override public java.time.Duration getIdleDuration() { return java.time.Duration.ofNanos(System.nanoTime() - lastActive); }
    @Override public void resetIdleDuration() { touch(); }

    // ---- respawn point -----------------------------------------------------------------------------
    private org.bukkit.Location respawn;

    @Override public void setRespawnLocation(org.bukkit.Location location, boolean force) { respawn = location == null ? null : location.clone(); super.setRespawnLocation(location, force); }
    @Override public org.bukkit.Location getRespawnLocation(boolean validate) { return respawn == null ? null : respawn.clone(); }
    @Override public org.bukkit.Location getBedLocation() {
        if (respawn == null) throw new IllegalStateException("Player has not slept in a bed");
        return respawn.clone();
    }

    // ---- titles and batched block updates ----------------------------------------------------------
    @Override public void sendTitle(com.destroystokyo.paper.Title title) { sendTitle(net.md_5.bungee.api.chat.TextComponent.toLegacyText(title.getTitle()), net.md_5.bungee.api.chat.TextComponent.toLegacyText(title.getSubtitle()), title.getFadeIn(), title.getStay(), title.getFadeOut()); }
    @Override public void updateTitle(com.destroystokyo.paper.Title title) { sendTitle(title); }
    @Override public void sendHurtAnimation(float yaw) { playHurtAnimation(yaw); }
    @Override public void sendBlockDamage(org.bukkit.Location location, float progress, org.bukkit.entity.Entity source) {
        sendBlockDamage(location, progress, Objects.requireNonNull(source, "source").getEntityId());
    }
    @Override public void sendBlockChanges(Collection<org.bukkit.block.BlockState> states) {
        for (var state : Objects.requireNonNull(states, "blocks")) sendBlockChange(state.getLocation(), state.getBlockData());
    }
    @Override public void sendMultiBlockChange(Map<? extends io.papermc.paper.math.Position, org.bukkit.block.data.BlockData> changes) {
        for (var change : Objects.requireNonNull(changes, "blockChanges").entrySet())
            sendBlockChange(change.getKey().toLocation(getWorld()), change.getValue());
    }

    // ---- world effects -------------------------------------------------------------------------------
    @Override public void playEffect(org.bukkit.Location location, org.bukkit.Effect effect, int data) {
        Objects.requireNonNull(location, "location");
        Objects.requireNonNull(effect, "effect");
        HostBridge.callVoid("Player", getUniqueId(), "playLevelEvent(Location,int,int)", location, effect.getId(), data);
    }
    @Override public <T> void playEffect(org.bukkit.Location location, org.bukkit.Effect effect, T data) {
        Objects.requireNonNull(effect, "effect");
        if (data == null) { playEffect(location, effect, 0); return; }
        if (data instanceof Number number) { playEffect(location, effect, number.intValue()); return; }
        throw new UnsupportedOperationException("Ferrite: not implemented: Player#playEffect with " + data.getClass().getSimpleName() + " data");
    }

    // ---- looking, tracking and profile ----------------------------------------------------------------
    @Override public void lookAt(org.bukkit.entity.Entity target, io.papermc.paper.entity.LookAnchor playerAnchor, io.papermc.paper.entity.LookAnchor targetAnchor) {
        Objects.requireNonNull(target, "entity");
        var at = target instanceof org.bukkit.entity.LivingEntity living && targetAnchor == io.papermc.paper.entity.LookAnchor.EYES ? living.getEyeLocation() : target.getLocation();
        lookAt(at.getX(), at.getY(), at.getZ(), playerAnchor);
    }
    @Override public void setRotation(io.papermc.paper.math.Angle yaw, io.papermc.paper.math.Angle pitch) {
        var here = getLocation();
        setRotation(yaw.relative() ? here.getYaw() + yaw.degrees() : yaw.degrees(), pitch.relative() ? here.getPitch() + pitch.degrees() : pitch.degrees());
    }
    /** Players in the same world within the server view distance, the ones whose client tracks this player. */
    @Override public Set<org.bukkit.entity.Player> getTrackedBy() {
        var out = new LinkedHashSet<org.bukkit.entity.Player>();
        var here = getLocation();
        int range = getServer().getViewDistance() * 16;
        for (var other : getServer().getOnlinePlayers()) {
            if (other.equals(this) || !Objects.equals(other.getWorld(), getWorld())) continue;
            var at = other.getLocation();
            if (Math.abs(at.getX() - here.getX()) <= range && Math.abs(at.getZ() - here.getZ()) <= range && other.canSee((org.bukkit.entity.Entity) this)) out.add(other);
        }
        return out;
    }
    @Override public Set<org.bukkit.entity.Player> getTrackedPlayers() { return getTrackedBy(); }
    @Override public boolean isTrackedBy(org.bukkit.entity.Player viewer) { return getTrackedBy().contains(Objects.requireNonNull(viewer, "player")); }
    @Override public com.destroystokyo.paper.profile.PlayerProfile getPlayerProfile() { return new FerriteProfile(getUniqueId(), getName()); }
    @Override public Set<String> getListeningPluginChannels() { return Set.of(); }
    @Override public void resetCooldown() { }
    @Override public void resetFlyingTicks() { }

    // ---- advancements ---------------------------------------------------------------------------------------
    @Override public org.bukkit.advancement.AdvancementProgress getAdvancementProgress(org.bukkit.advancement.Advancement advancement) {
        return FerriteAdvancementProgress.of(getUniqueId(), Objects.requireNonNull(advancement, "advancement"));
    }

    // ---- bans ----------------------------------------------------------------------------------------------
    private org.bukkit.BanEntry applyBan(String reason, java.util.Date expires, String source, boolean kick) {
        var list = (org.bukkit.ban.ProfileBanList) getServer().getBanList(org.bukkit.BanList.Type.PROFILE);
        var entry = list.addBan((org.bukkit.profile.PlayerProfile) getPlayerProfile(), reason, expires, source);
        if (kick) kickPlayer(reason);
        return entry;
    }
    @Override public <E extends org.bukkit.BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E ban(String reason, java.util.Date expires, String source, boolean kick) { return (E) applyBan(reason, expires, source, kick); }
    @Override public <E extends org.bukkit.BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E ban(String reason, java.util.Date expires, String source) { return (E) applyBan(reason, expires, source, true); }
    @Override public <E extends org.bukkit.BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E ban(String reason, java.time.Instant expires, String source, boolean kick) { return (E) applyBan(reason, expires == null ? null : java.util.Date.from(expires), source, kick); }
    @Override public <E extends org.bukkit.BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E ban(String reason, java.time.Instant expires, String source) { return (E) applyBan(reason, expires == null ? null : java.util.Date.from(expires), source, true); }
    @Override public <E extends org.bukkit.BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E ban(String reason, java.time.Duration duration, String source, boolean kick) { return (E) applyBan(reason, duration == null ? null : java.util.Date.from(java.time.Instant.now().plus(duration)), source, kick); }
    @Override public <E extends org.bukkit.BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E ban(String reason, java.time.Duration duration, String source) { return (E) applyBan(reason, duration == null ? null : java.util.Date.from(java.time.Instant.now().plus(duration)), source, true); }
    @Override public boolean isBanned() { return ((org.bukkit.ban.ProfileBanList) getServer().getBanList(org.bukkit.BanList.Type.PROFILE)).isBanned((org.bukkit.profile.PlayerProfile) getPlayerProfile()); }

    // ---- connection --------------------------------------------------------------------------------------------
    /** The connecting address; the host keeps the IP but not the client port, which is reported as 0. */
    @Override public java.net.InetSocketAddress getAddress() {
        String text = (String) HostBridge.call("Player", getUniqueId(), "getAddressText()", String.class);
        try {
            return new java.net.InetSocketAddress(java.net.InetAddress.getByName(text), 0);
        } catch (java.net.UnknownHostException e) {
            throw new IllegalStateException("Invalid player address " + text, e);
        }
    }
    @Override public java.net.SocketAddress getSocketAddress() { return getAddress(); }
    @Override public java.net.InetSocketAddress getVirtualHost() { return null; }
    @Override public java.net.InetSocketAddress getHAProxyAddress() { return null; }
}
