package dev.ferrite.server;

import java.util.*;
import java.util.function.Predicate;

/**
 * Player API answered from Java-side state: ally rules, tab-list and visibility choices, the
 * ender chest, the entity scheduler, titles, batched block changes and idle time.
 */
@SuppressWarnings({"deprecation", "unchecked", "rawtypes"})
public abstract class PlayerMisc extends PlayerStubs {
    // ---- durability --------------------------------------------------------------------------
    /** Wears the item by `amount`; the result is empty when it breaks. Unbreaking is not modelled. */
    @Override public org.bukkit.inventory.ItemStack damageItemStack(org.bukkit.inventory.ItemStack stack, int amount) {
        java.util.Objects.requireNonNull(stack, "stack");
        int max = stack.getType().getMaxDurability();
        if (amount <= 0 || max <= 0 || !(stack.getItemMeta() instanceof org.bukkit.inventory.meta.Damageable meta) || meta.isUnbreakable()) return stack;
        int worn = meta.getDamage() + amount;
        if (worn >= max) return new org.bukkit.inventory.ItemStack(org.bukkit.Material.AIR);
        meta.setDamage(worn);
        var copy = stack.clone();
        copy.setItemMeta(meta);
        return copy;
    }
    @Override public void damageItemStack(org.bukkit.inventory.EquipmentSlot slot, int amount) {
        var equipment = getEquipment();
        var item = equipment.getItem(slot);
        if (item == null || item.getType().isAir()) return;
        equipment.setItem(slot, damageItemStack(item, amount));
    }
    /** XP mends the first worn item that has Mending and is damaged (2 durability per point); returns the XP left over. */
    @Override public int applyMending(int amount) {
        var equipment = getEquipment();
        for (var slot : new org.bukkit.inventory.EquipmentSlot[]{org.bukkit.inventory.EquipmentSlot.HAND, org.bukkit.inventory.EquipmentSlot.OFF_HAND, org.bukkit.inventory.EquipmentSlot.FEET,
                org.bukkit.inventory.EquipmentSlot.LEGS, org.bukkit.inventory.EquipmentSlot.CHEST, org.bukkit.inventory.EquipmentSlot.HEAD}) {
            if (amount <= 0) break;
            var item = equipment.getItem(slot);
            if (item == null || item.getType().isAir() || !(item.getItemMeta() instanceof org.bukkit.inventory.meta.Damageable meta)) continue;
            if (meta.getDamage() <= 0 || !meta.hasEnchant(org.bukkit.enchantments.Enchantment.MENDING)) continue;
            int repair = Math.min(amount * 2, meta.getDamage());
            meta.setDamage(meta.getDamage() - repair);
            var mended = item.clone();
            mended.setItemMeta(meta);
            equipment.setItem(slot, mended);
            amount -= (repair + 1) / 2;
        }
        return amount;
    }

    /** Whether the player's body, moved to the location, would overlap a solid block. */
    @Override public boolean collidesAt(org.bukkit.Location location) {
        return getWorld().hasCollisionsIn(getBoundingBox().shift(location.toVector().subtract(getLocation().toVector())));
    }
    @Override public boolean wouldCollideUsing(org.bukkit.util.BoundingBox box) { return getWorld().hasCollisionsIn(box); }

    /** Several slots at once are sent one slot at a time. */
    @Override public void sendEquipmentChange(org.bukkit.entity.LivingEntity entity, java.util.Map<org.bukkit.inventory.EquipmentSlot, org.bukkit.inventory.ItemStack> items) {
        java.util.Objects.requireNonNull(entity, "entity");
        for (var entry : items.entrySet()) sendEquipmentChange(entity, entry.getKey(), entry.getValue());
    }

    /** Sends the map's colours and visible cursors as its renderers paint them for this player. */
    @Override public void sendMap(org.bukkit.map.MapView map) {
        java.util.Objects.requireNonNull(map, "map");
        var rendered = map instanceof FerriteMapView ferrite
            ? ferrite.render(this)
            : new FerriteMapView.Rendered(new byte[128 * 128], "");
        HostBridge.callVoid("Player", getUniqueId(), "sendMapData(int,int,boolean,byte[],String)",
            map.getId(), map.getScale().getValue(), map.isLocked(), rendered.pixels(), rendered.decorations());
    }

    /** Shows a written book. Only the title, author and the plain text of each page reach the client. */
    @Override public void openBook(org.bukkit.inventory.ItemStack book) {
        java.util.Objects.requireNonNull(book, "book");
        if (book.getType() != org.bukkit.Material.WRITTEN_BOOK) throw new IllegalArgumentException("Book must be Material.WRITTEN_BOOK");
        var plain = net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText();
        String title = "", author = "";
        var pages = new java.util.ArrayList<String>();
        if (book.getItemMeta() instanceof org.bukkit.inventory.meta.BookMeta meta) {
            if (meta.title() != null) title = plain.serialize(meta.title());
            if (meta.author() != null) author = plain.serialize(meta.author());
            for (var page : meta.pages()) pages.add(plain.serialize(page));
        }
        if (pages.isEmpty()) pages.add("");
        String field = String.valueOf(''), part = String.valueOf(HostBridge.ITEM);
        HostBridge.callVoid("Player", getUniqueId(), "openBookContent(String)", title + field + author + field + String.join(part, pages));
    }

    /** The chunks the client currently has, as the host last recorded them. */
    @Override public java.util.Set<org.bukkit.Chunk> getSentChunks() {
        String text;
        try { text = (String) HostBridge.call("Player", getUniqueId(), "getSentChunkCoordinates()", String.class); }
        catch (UnsupportedOperationException | IllegalStateException noHost) { return new java.util.HashSet<>(); }
        var chunks = new java.util.LinkedHashSet<org.bukkit.Chunk>();
        if (text == null || text.isEmpty()) return chunks;
        for (var pair : text.split(String.valueOf(HostBridge.ITEM))) {
            var parts = pair.split(",");
            chunks.add(getWorld().getChunkAt(Integer.parseInt(parts[0]), Integer.parseInt(parts[1])));
        }
        return chunks;
    }

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

    // ---- values a plugin sets and reads back; the game does not use them yet --------------------
    private float bodyYawSet = Float.NaN;
    private int enchantmentSeed = new java.util.Random().nextInt();
    private net.kyori.adventure.util.TriState frictionState = net.kyori.adventure.util.TriState.NOT_SET;
    private net.kyori.adventure.util.TriState flyingFallDamage = net.kyori.adventure.util.TriState.NOT_SET;
    private float hurtDirection;
    private org.bukkit.Location lastDeathLocation;
    private int noActionTicks;
    private int starvationRate = 80;
    private int wardenWarningLevel;
    private boolean noPhysics;
    private boolean visibleByDefault = true;
    private org.bukkit.Color waypointColor;
    private net.kyori.adventure.key.Key waypointStyle = net.kyori.adventure.key.Key.key("minecraft", "default");

    @Override public synchronized float getBodyYaw() { return Float.isNaN(bodyYawSet) ? getLocation().getYaw() : bodyYawSet; }
    @Override public synchronized void setBodyYaw(float yaw) { bodyYawSet = yaw; }
    @Override public synchronized int getEnchantmentSeed() { return enchantmentSeed; }
    @Override public synchronized void setEnchantmentSeed(int seed) { enchantmentSeed = seed; }
    /** The game has no ender pearl entities yet, so none is ever in flight. */
    @Override public java.util.Collection<org.bukkit.entity.EnderPearl> getEnderPearls() { return java.util.List.of(); }
    @Override public synchronized net.kyori.adventure.util.TriState getFrictionState() { return frictionState; }
    @Override public synchronized void setFrictionState(net.kyori.adventure.util.TriState state) { frictionState = Objects.requireNonNull(state, "state"); }
    @Override public synchronized net.kyori.adventure.util.TriState hasFlyingFallDamage() { return flyingFallDamage; }
    @Override public synchronized void setFlyingFallDamage(net.kyori.adventure.util.TriState state) { flyingFallDamage = Objects.requireNonNull(state, "state"); }
    @Override public synchronized float getHurtDirection() { return hurtDirection; }
    @Override public synchronized void setHurtDirection(float direction) { hurtDirection = direction; }
    @Override public synchronized org.bukkit.Location getLastDeathLocation() { return lastDeathLocation == null ? null : lastDeathLocation.clone(); }
    @Override public synchronized void setLastDeathLocation(org.bukkit.Location location) { lastDeathLocation = location == null ? null : location.clone(); }
    @Override public synchronized int getNoActionTicks() { return noActionTicks; }
    @Override public synchronized void setNoActionTicks(int ticks) { noActionTicks = ticks; }
    @Override public synchronized int getStarvationRate() { return starvationRate; }
    @Override public synchronized void setStarvationRate(int ticks) { starvationRate = ticks; }
    @Override public synchronized int getWardenWarningLevel() { return wardenWarningLevel; }
    @Override public synchronized void setWardenWarningLevel(int level) { wardenWarningLevel = level; }
    @Override public synchronized void increaseWardenWarningLevel() { wardenWarningLevel++; }
    @Override public synchronized boolean hasNoPhysics() { return noPhysics; }
    @Override public synchronized void setNoPhysics(boolean value) { noPhysics = value; }
    @Override public synchronized boolean isVisibleByDefault() { return visibleByDefault; }
    @Override public synchronized void setVisibleByDefault(boolean visible) { visibleByDefault = visible; }
    @Override public synchronized org.bukkit.Color getWaypointColor() { return waypointColor; }
    @Override public synchronized void setWaypointColor(org.bukkit.Color color) { waypointColor = color; }
    @Override public synchronized net.kyori.adventure.key.Key getWaypointStyle() { return waypointStyle; }
    @Override public synchronized void setWaypointStyle(net.kyori.adventure.key.Key style) { waypointStyle = Objects.requireNonNull(style, "style"); }

    /** Players wear no body armor or saddle; every other slot is usable. */
    @Override public boolean canUseEquipmentSlot(org.bukkit.inventory.EquipmentSlot slot) {
        Objects.requireNonNull(slot, "slot");
        return slot != org.bukkit.inventory.EquipmentSlot.BODY && slot != org.bukkit.inventory.EquipmentSlot.SADDLE;
    }
    @Override public Map<String, Object> serialize() {
        var result = new LinkedHashMap<String, Object>();
        result.put("UUID", getUniqueId().toString());
        return result;
    }
    // ---- projectiles ----------------------------------------------------------------------------
    /**
     * Shoots an arrow, snowball, egg, ender pearl, experience bottle, splash or lingering potion, wind charge, small
     * fireball or fireball from the eyes, at the power vanilla gives the item that throws it. A trident flies at
     * vanilla's 2.5 and is owned by the player. A potion launches as a bare bottle because the call cannot carry
     * its contents, so a callback that tries to set them fails instead of being dropped.
     */
    @Override public <T extends org.bukkit.entity.Projectile> T launchProjectile(Class<? extends T> projectile, org.bukkit.util.Vector velocity, java.util.function.Consumer<? super T> callback) {
        Objects.requireNonNull(projectile, "projectile");
        org.bukkit.entity.EntityType thrown = null;
        double power = 1.5;
        if (org.bukkit.entity.Snowball.class.isAssignableFrom(projectile)) thrown = org.bukkit.entity.EntityType.SNOWBALL;
        else if (org.bukkit.entity.Egg.class.isAssignableFrom(projectile)) thrown = org.bukkit.entity.EntityType.EGG;
        else if (org.bukkit.entity.EnderPearl.class.isAssignableFrom(projectile)) thrown = org.bukkit.entity.EntityType.ENDER_PEARL;
        else if (org.bukkit.entity.SmallFireball.class.isAssignableFrom(projectile)) thrown = org.bukkit.entity.EntityType.SMALL_FIREBALL;
        else if (projectile == org.bukkit.entity.Fireball.class || org.bukkit.entity.LargeFireball.class.isAssignableFrom(projectile)) thrown = org.bukkit.entity.EntityType.FIREBALL;
        else if (org.bukkit.entity.ThrownExpBottle.class.isAssignableFrom(projectile)) { thrown = org.bukkit.entity.EntityType.EXPERIENCE_BOTTLE; power = 0.7; }
        else if (org.bukkit.entity.ThrownPotion.class.isAssignableFrom(projectile)) {
            thrown = org.bukkit.entity.LingeringPotion.class.isAssignableFrom(projectile) ? org.bukkit.entity.EntityType.LINGERING_POTION : org.bukkit.entity.EntityType.SPLASH_POTION;
            power = 0.5;
        }
        else if (org.bukkit.entity.WindCharge.class.isAssignableFrom(projectile)) thrown = org.bukkit.entity.EntityType.WIND_CHARGE;
        // Vanilla Trident.PROJECTILE_SHOOT_POWER; the host gives it the default trident item, the thrown-trident damage and the player as owner.
        else if (org.bukkit.entity.Trident.class.isAssignableFrom(projectile)) { thrown = org.bukkit.entity.EntityType.TRIDENT; power = 2.5; }
        else if (!projectile.isAssignableFrom(org.bukkit.entity.Arrow.class)) throw new UnsupportedOperationException("Ferrite: not implemented: Player#launchProjectile(" + projectile.getSimpleName() + ")");
        var eye = getEyeLocation();
        var direction = velocity != null ? velocity.clone() : eye.getDirection().multiply(thrown != null ? power : 3.0);
        var uuid = UUID.randomUUID();
        var type = thrown != null ? thrown : org.bukkit.entity.EntityType.ARROW;
        @SuppressWarnings("unchecked") var handle = (T) SpawnedEntity.of(type, eye, getWorld(), uuid);
        if (callback != null) callback.accept(handle);
        if (thrown != null) tellHost("launchThrown(Location,EntityType,double,double,double,String,String)", eye, thrown, direction.getX(), direction.getY(), direction.getZ(), getUniqueId().toString(), uuid.toString());
        else tellHost("spawnArrow(Location,double,double,double,String)", eye, direction.getX(), direction.getY(), direction.getZ(), uuid.toString());
        return handle;
    }

    /** Hits the entity as a bare-handed player would: one point of damage. */
    @Override public void attack(org.bukkit.entity.Entity target) {
        Objects.requireNonNull(target, "target");
        tellHost("attack(String)", target.getUniqueId().toString());
    }

    // ---- throwing items -------------------------------------------------------------------------
    /** Throws a stack that is no longer in the inventory, as the player would with the drop key. */
    private org.bukkit.entity.Item throwStack(org.bukkit.inventory.ItemStack stack, java.util.function.Consumer<org.bukkit.entity.Item> callback) {
        var handle = DroppedItem.of(getEyeLocation(), stack.clone(), getWorld(), UUID.randomUUID());
        if (callback != null) callback.accept(handle);
        tellHost("dropStack(ItemStack)", stack);
        return handle;
    }
    /** The drop key: one item, or the whole stack, of the selected slot. */
    @Override public boolean dropItem(boolean dropAll) {
        var held = getInventory().getItemInMainHand();
        if (held == null || held.getType().isAir() || held.getAmount() <= 0) return false;
        tellHost("dropSelected(boolean)", dropAll);
        return true;
    }
    @Override public org.bukkit.entity.Item dropItem(org.bukkit.inventory.ItemStack item, boolean throwRandomly, java.util.function.Consumer<org.bukkit.entity.Item> callback) {
        Objects.requireNonNull(item, "item");
        if (item.getType().isAir() || item.getAmount() <= 0) throw new IllegalArgumentException("Cannot drop air");
        return throwStack(item, callback);
    }
    @Override public org.bukkit.entity.Item dropItem(org.bukkit.inventory.EquipmentSlot slot, int amount, boolean throwRandomly, java.util.function.Consumer<org.bukkit.entity.Item> callback) {
        Objects.requireNonNull(slot, "slot");
        var inventory = getInventory();
        var held = inventory.getItem(slot);
        if (held == null || held.getType().isAir() || amount <= 0) return null;
        var thrown = held.clone();
        thrown.setAmount(Math.min(amount, held.getAmount()));
        var left = held.clone();
        left.setAmount(held.getAmount() - thrown.getAmount());
        inventory.setItem(slot, left.getAmount() > 0 ? left : null);
        return throwStack(thrown, callback);
    }
    @Override public org.bukkit.entity.Item dropItem(int slot, int amount, boolean throwRandomly, java.util.function.Consumer<org.bukkit.entity.Item> callback) {
        var inventory = getInventory();
        var held = inventory.getItem(slot);
        if (held == null || held.getType().isAir() || amount <= 0) return null;
        var thrown = held.clone();
        thrown.setAmount(Math.min(amount, held.getAmount()));
        var left = held.clone();
        left.setAmount(held.getAmount() - thrown.getAmount());
        inventory.setItem(slot, left.getAmount() > 0 ? left : null);
        return throwStack(thrown, callback);
    }
    /** Adds the items to the inventory; what does not fit is thrown at the player's feet or handed back. */
    @Override public io.papermc.paper.entity.PlayerGiveResult give(Collection<org.bukkit.inventory.ItemStack> items, boolean dropIfFull) {
        Objects.requireNonNull(items, "items");
        var left = getInventory().addItem(items.toArray(new org.bukkit.inventory.ItemStack[0]));
        var drops = new ArrayList<org.bukkit.entity.Item>();
        List<org.bukkit.inventory.ItemStack> leftovers;
        if (dropIfFull) {
            for (var rest : left.values()) drops.add(throwStack(rest, null));
            leftovers = List.of();
        } else {
            leftovers = List.copyOf(left.values());
        }
        var thrown = List.copyOf(drops);
        return new io.papermc.paper.entity.PlayerGiveResult() {
            @Override public Collection<org.bukkit.inventory.ItemStack> leftovers() { return leftovers; }
            @Override public Collection<org.bukkit.entity.Item> drops() { return thrown; }
        };
    }

    // ---- boss bars ----------------------------------------------------------------------------
    private final Map<net.kyori.adventure.bossbar.BossBar, BossShown> shownBars = new LinkedHashMap<>();
    private record BossShown(UUID id, net.kyori.adventure.bossbar.BossBar.Listener listener) {}

    private static String plain(net.kyori.adventure.text.Component name) {
        return net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(name);
    }
    private static int flagBits(Set<net.kyori.adventure.bossbar.BossBar.Flag> flags) {
        int bits = 0;
        if (flags.contains(net.kyori.adventure.bossbar.BossBar.Flag.DARKEN_SCREEN)) bits |= 1;
        if (flags.contains(net.kyori.adventure.bossbar.BossBar.Flag.PLAY_BOSS_MUSIC)) bits |= 2;
        if (flags.contains(net.kyori.adventure.bossbar.BossBar.Flag.CREATE_WORLD_FOG)) bits |= 4;
        return bits;
    }

    /** Shows the bar and keeps the client's copy in step with later changes to it. */
    @Override public void showBossBar(net.kyori.adventure.bossbar.BossBar bar) {
        Objects.requireNonNull(bar, "bar");
        UUID id;
        synchronized (this) {
            if (shownBars.containsKey(bar)) return;
            id = UUID.randomUUID();
            var barId = id;
            var listener = new net.kyori.adventure.bossbar.BossBar.Listener() {
                @Override public void bossBarNameChanged(net.kyori.adventure.bossbar.BossBar b, net.kyori.adventure.text.Component oldName, net.kyori.adventure.text.Component newName) { tellHost("bossBarName(String,String)", barId.toString(), plain(newName)); }
                @Override public void bossBarProgressChanged(net.kyori.adventure.bossbar.BossBar b, float oldProgress, float newProgress) { tellHost("bossBarProgress(String,float)", barId.toString(), newProgress); }
                @Override public void bossBarColorChanged(net.kyori.adventure.bossbar.BossBar b, net.kyori.adventure.bossbar.BossBar.Color oldColor, net.kyori.adventure.bossbar.BossBar.Color newColor) { tellHost("bossBarStyle(String,int,int)", barId.toString(), newColor.ordinal(), b.overlay().ordinal()); }
                @Override public void bossBarOverlayChanged(net.kyori.adventure.bossbar.BossBar b, net.kyori.adventure.bossbar.BossBar.Overlay oldOverlay, net.kyori.adventure.bossbar.BossBar.Overlay newOverlay) { tellHost("bossBarStyle(String,int,int)", barId.toString(), b.color().ordinal(), newOverlay.ordinal()); }
                @Override public void bossBarFlagsChanged(net.kyori.adventure.bossbar.BossBar b, Set<net.kyori.adventure.bossbar.BossBar.Flag> added, Set<net.kyori.adventure.bossbar.BossBar.Flag> removed) { tellHost("bossBarFlags(String,int)", barId.toString(), flagBits(b.flags())); }
            };
            shownBars.put(bar, new BossShown(id, listener));
            bar.addListener(listener);
        }
        tellHost("bossBarAdd(String,String,float,int,int,int)", id.toString(), plain(bar.name()), bar.progress(), bar.color().ordinal(), bar.overlay().ordinal(), flagBits(bar.flags()));
    }
    @Override public void hideBossBar(net.kyori.adventure.bossbar.BossBar bar) {
        Objects.requireNonNull(bar, "bar");
        BossShown shown;
        synchronized (this) { shown = shownBars.remove(bar); }
        if (shown == null) return;
        bar.removeListener(shown.listener());
        tellHost("bossBarRemove(String)", shown.id().toString());
    }
    @Override public synchronized Iterable<? extends net.kyori.adventure.bossbar.BossBar> activeBossBars() { return List.copyOf(shownBars.keySet()); }

    // ---- tab list and visibility ---------------------------------------------------------------
    private final Set<UUID> unlisted = new HashSet<>();
    private final Set<UUID> hidden = new HashSet<>();

    @Override public synchronized boolean isListed(org.bukkit.entity.Player other) { return !unlisted.contains(Objects.requireNonNull(other, "player").getUniqueId()); }
    @Override public synchronized boolean unlistPlayer(org.bukkit.entity.Player other) { return unlisted.add(Objects.requireNonNull(other, "player").getUniqueId()); }
    @Override public synchronized boolean listPlayer(org.bukkit.entity.Player other) { return unlisted.remove(Objects.requireNonNull(other, "player").getUniqueId()); }
    /** Visibility is remembered here either way; with no host attached (plain Java) only the memory applies. */
    protected void tellHost(String method, Object... args) {
        try { HostBridge.callVoid("Player", getUniqueId(), method, args); } catch (UnsupportedOperationException offline) { /* no host */ }
    }
    /**
     * A tile snapshot shown to this player only (never a world change). Signs and the block entity families the host
     * models (chest, trapped chest, barrel, shulker boxes, furnace, blast furnace, smoker) are sent; any other
     * {@code TileState} (beacon, ...) has no host model and is refused instead of being sent with invented NBT.
     */
    @Override public void sendBlockUpdate(org.bukkit.Location loc, org.bukkit.block.TileState tileState) {
        Objects.requireNonNull(loc, "loc");
        Objects.requireNonNull(tileState, "tileState");
        if (tileState instanceof org.bukkit.block.Sign sign) {
            var side = sign.getSide(org.bukkit.block.sign.Side.FRONT);
            var plain = net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText();
            var lines = new String[4];
            for (int i = 0; i < 4; i++) lines[i] = plain.serialize(side.line(i));
            sendSignChange(loc, lines, side.getColor(), side.isGlowingText());
            return;
        }
        var type = tileState.getType();
        if (tileState instanceof org.bukkit.block.Banner banner) {
            var entries = new java.util.ArrayList<String>();
            for (var layer : banner.getPatterns())
                entries.add("pattern," + layer.getPattern().getKey() + "," + layer.getColor().name().toLowerCase(java.util.Locale.ROOT));
            HostBridge.callVoid("Player", getUniqueId(), "sendTileSnapshot(Location,String,List)", loc, "minecraft:banner", entries);
            return;
        }
        if (tileState instanceof org.bukkit.block.Skull skull) {
            // SkullBlockEntity saves `profile`, `note_block_sound` and `custom_name`; only the profile's name and
            // uuid are modelled. Textures (profile properties) and a note block sound are refused, not dropped.
            if (skull.getNoteBlockSound() != null)
                throw new UnsupportedOperationException("Ferrite: Player#sendBlockUpdate(" + type + ") cannot carry a note block sound; the host snapshot encodes the profile name and uuid only");
            var entries = new java.util.ArrayList<String>();
            var profile = skull.getPlayerProfile();
            if (profile != null) {
                if (!profile.getProperties().isEmpty())
                    throw new UnsupportedOperationException("Ferrite: Player#sendBlockUpdate(" + type + ") cannot carry profile properties (textures); the host snapshot encodes the profile name and uuid only");
                var name = profile.getName();
                var id = profile.getUniqueId();
                entries.add("profile," + (name == null ? "" : name) + "," + (id == null ? "" : id));
            }
            HostBridge.callVoid("Player", getUniqueId(), "sendTileSnapshot(Location,String,List)", loc, "minecraft:skull", entries);
            return;
        }
        String entity = switch (type) {
            case CHEST -> "minecraft:chest";
            case TRAPPED_CHEST -> "minecraft:trapped_chest";
            case BARREL -> "minecraft:barrel";
            case FURNACE -> "minecraft:furnace";
            case BLAST_FURNACE -> "minecraft:blast_furnace";
            case SMOKER -> "minecraft:smoker";
            case DISPENSER -> "minecraft:dispenser";
            case DROPPER -> "minecraft:dropper";
            case HOPPER -> "minecraft:hopper";
            case BREWING_STAND -> "minecraft:brewing_stand";
            case CRAFTER -> "minecraft:crafter";
            default -> type.name().endsWith("SHULKER_BOX") ? "minecraft:shulker_box" : null;
        };
        if (entity == null || !(tileState instanceof org.bukkit.block.Container container))
            throw new UnsupportedOperationException("Ferrite: Player#sendBlockUpdate(" + type + ") is not implemented: the host models only signs, banners, skulls, chests, barrels, shulker boxes, furnaces, dispensers, droppers, hoppers, brewing stands and crafters as tile entities, and has no other block entity model to encode");
        var snapshot = container.getSnapshotInventory();
        var entries = new java.util.ArrayList<String>();
        for (int slot = 0; slot < snapshot.getSize(); slot++) {
            var item = snapshot.getItem(slot);
            if (item == null || item.getType().isAir() || item.getAmount() <= 0) continue;
            if (item.hasItemMeta())
                throw new UnsupportedOperationException("Ferrite: Player#sendBlockUpdate cannot carry item metadata (slot " + slot + " holds " + item.getType() + " with meta); the host snapshot encodes material and amount only");
            entries.add("item," + slot + "," + item.getType().getKey() + "," + item.getAmount());
        }
        if (tileState instanceof org.bukkit.block.Furnace furnace) {
            // Bukkit exposes the remaining burn time but not the fuel's total, so the total is sent as 0.
            entries.add("furnace," + furnace.getBurnTime() + ",0," + furnace.getCookTime() + "," + furnace.getCookTimeTotal());
        }
        HostBridge.callVoid("Player", getUniqueId(), "sendTileSnapshot(Location,String,List)", loc, entity, entries);
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
    private static final java.util.concurrent.ScheduledExecutorService COOKIE_POLLER = java.util.concurrent.Executors.newSingleThreadScheduledExecutor(task -> { var t = new Thread(task, "ferrite-cookie-poll"); t.setDaemon(true); return t; });
    /** Asks the client for the cookie; the future completes with it, or null when the client holds none. */
    @Override public java.util.concurrent.CompletableFuture<byte[]> retrieveCookie(org.bukkit.NamespacedKey key) {
        Objects.requireNonNull(key, "key");
        var result = new java.util.concurrent.CompletableFuture<byte[]>();
        HostBridge.callVoid("Player", getUniqueId(), "retrieveCookie(NamespacedKey)", key);
        var deadline = System.nanoTime() + java.util.concurrent.TimeUnit.SECONDS.toNanos(10);
        var poll = new Runnable() {
            @Override public void run() {
                try {
                    var reply = (String) HostBridge.call("Player", getUniqueId(), "getCookieReply(NamespacedKey)", String.class, key);
                    if ("pending".equals(reply)) {
                        if (System.nanoTime() > deadline) result.completeExceptionally(new java.util.concurrent.TimeoutException("The client did not answer the cookie request"));
                        else COOKIE_POLLER.schedule(this, 50, java.util.concurrent.TimeUnit.MILLISECONDS);
                    } else {
                        result.complete(reply == null ? null : java.util.Base64.getDecoder().decode(reply));
                    }
                } catch (RuntimeException failure) {
                    result.completeExceptionally(failure);
                }
            }
        };
        COOKIE_POLLER.schedule(poll, 50, java.util.concurrent.TimeUnit.MILLISECONDS);
        return result;
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
    private final FerriteCombatTracker combat = new FerriteCombatTracker(this);
    @Override public io.papermc.paper.world.damagesource.CombatTracker getCombatTracker() { return combat; }
    /** Called by the host's damage event so the tracker knows what hit the player. */
    void recordDamage(org.bukkit.damage.DamageSource source, double damage) { combat.record(source, (float) damage); }
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
        playEffect(location, effect, FerriteEffectData.levelEventData(effect, data));
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
    /** Shows the item-break effect to every client that sees this player, and to the player. */
    @Override public void broadcastSlotBreak(org.bukkit.inventory.EquipmentSlot slot) {
        Objects.requireNonNull(slot, "slot");
        var viewers = new LinkedHashSet<org.bukkit.entity.Player>(getTrackedBy());
        viewers.add(this);
        broadcastSlotBreak(slot, viewers);
    }
    /** Breaks the block under this player's own rules; false when the player may not, or the block is air. */
    @Override public boolean breakBlock(org.bukkit.block.Block block) {
        Objects.requireNonNull(block, "block");
        if (!Objects.equals(block.getWorld(), getWorld())) throw new IllegalArgumentException("Cannot break blocks across worlds");
        return HostBridge.callBoolean("Player", getUniqueId(), "breakBlock(int,int,int)", block.getX(), block.getY(), block.getZ());
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
    /** Bans the address this player connected from (`Player#banIp`); the server list forwards it to `/ban-ip`. */
    private org.bukkit.BanEntry<java.net.InetAddress> applyBanIp(String reason, java.util.Date expires, String source, boolean kick) {
        var address = getAddress().getAddress();
        var list = (org.bukkit.ban.IpBanList) getServer().getBanList(org.bukkit.BanList.Type.IP);
        var entry = list.addBan(address, reason, expires, source);
        if (kick) kickPlayer(reason);
        return entry;
    }
    @Override public org.bukkit.BanEntry<java.net.InetAddress> banIp(String reason, java.util.Date expires, String source, boolean kick) { return applyBanIp(reason, expires, source, kick); }
    @Override public org.bukkit.BanEntry<java.net.InetAddress> banIp(String reason, java.time.Instant expires, String source, boolean kick) { return applyBanIp(reason, expires == null ? null : java.util.Date.from(expires), source, kick); }
    @Override public org.bukkit.BanEntry<java.net.InetAddress> banIp(String reason, java.time.Duration duration, String source, boolean kick) { return applyBanIp(reason, duration == null ? null : java.util.Date.from(java.time.Instant.now().plus(duration)), source, kick); }
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
