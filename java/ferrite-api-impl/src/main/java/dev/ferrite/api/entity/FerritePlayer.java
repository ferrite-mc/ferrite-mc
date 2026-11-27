package dev.ferrite.api.entity;

import dev.ferrite.api.FerriteRuntime;
import dev.ferrite.api.internal.Unsupported;
import dev.ferrite.api.inventory.FerriteCustomInventory;
import dev.ferrite.api.inventory.FerriteInventoryView;
import dev.ferrite.api.item.FerritePersistentDataContainer;
import dev.ferrite.api.scoreboard.FerriteScoreboard;
import dev.ferrite.api.world.FerriteWorld;
import java.time.Duration;
import java.util.Objects;
import java.util.Set;
import java.util.UUID;
import net.kyori.adventure.sound.SoundStop;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.ComponentLike;
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer;
import net.kyori.adventure.title.Title;
import net.kyori.adventure.title.TitlePart;
import net.md_5.bungee.api.chat.BaseComponent;
import org.bukkit.ChatColor;
import org.bukkit.GameMode;
import org.bukkit.Location;
import org.bukkit.Particle;
import org.bukkit.Server;
import org.bukkit.Sound;
import org.bukkit.SoundCategory;
import org.bukkit.World;
import org.bukkit.entity.Entity;
import org.bukkit.entity.Player;
import org.bukkit.event.inventory.InventoryCloseEvent;
import org.bukkit.event.inventory.InventoryOpenEvent;
import org.bukkit.event.player.PlayerKickEvent;
import org.bukkit.event.player.PlayerTeleportEvent;
import org.bukkit.inventory.EntityEquipment;
import org.bukkit.inventory.Inventory;
import org.bukkit.inventory.InventoryView;
import org.bukkit.inventory.PlayerInventory;
import org.bukkit.permissions.Permission;
import org.bukkit.persistence.PersistentDataContainer;
import org.bukkit.plugin.Plugin;
import org.bukkit.scoreboard.Scoreboard;

/**
 * First {@link org.bukkit.entity.Player} slice: identity, world/location, messages,
 * permission fallback, game mode, health/food and the basic inventory.
 */
public final class FerritePlayer extends UnsupportedPlayerBase {

    private final FerriteRuntime runtime;
    private final PlayerState state;
    private final FerritePlayerInventory inventory;
    private final FerritePersistentDataContainer persistentDataContainer =
            new FerritePersistentDataContainer();
    private final java.util.Set<String> scoreboardTags = new java.util.LinkedHashSet<>();
    private final java.util.Map<String, java.util.List<org.bukkit.metadata.MetadataValue>> metadata =
            new java.util.LinkedHashMap<>();
    private final java.util.Map<org.bukkit.attribute.Attribute, FerriteAttributeInstance> attributes =
            new java.util.LinkedHashMap<>();
    private Location respawnLocation;
    private FerriteInventoryView openView;
    private Component displayName;
    private Component playerListName;

    public FerritePlayer(FerriteRuntime runtime, PlayerState state) {
        this.runtime = runtime;
        this.state = state;
        this.inventory = new FerritePlayerInventory(this);
        state.setInventory(inventory);
    }

    public PlayerState state() {
        return state;
    }

    @Override
    public String getName() {
        return state.name();
    }

    @Override
    public UUID getUniqueId() {
        return state.uniqueId();
    }

    @Override
    public boolean isOnline() {
        return state.online();
    }

    @Override
    public Server getServer() {
        return runtime.server();
    }

    @Override
    public World getWorld() {
        return state.world();
    }

    @Override
    public Location getLocation() {
        return new Location(state.world(), state.x(), state.y(), state.z(), state.yaw(), state.pitch());
    }

    @Override
    public Location getLocation(Location loc) {
        if (loc == null) {
            return null;
        }
        loc.setWorld(state.world());
        loc.setX(state.x());
        loc.setY(state.y());
        loc.setZ(state.z());
        loc.setYaw(state.yaw());
        loc.setPitch(state.pitch());
        return loc;
    }

    @Override
    public double getEyeHeight() {
        return getEyeHeight(false);
    }

    @Override
    public double getEyeHeight(boolean ignorePose) {
        if (!ignorePose) {
            if (isSneaking()) {
                return 1.27;
            }
            if (isGliding() || isSwimming()) {
                return 0.4;
            }
        }
        return 1.62;
    }

    @Override
    public Location getEyeLocation() {
        return new Location(
                state.world(), state.x(), state.y() + getEyeHeight(), state.z(), state.yaw(), state.pitch());
    }

    @Override
    public boolean isValid() {
        return state.online();
    }

    @Override
    public boolean isDead() {
        return state.health() <= 0.0;
    }

    @Override
    @SuppressWarnings("deprecation")
    public double getMaxHealth() {
        return getAttribute(org.bukkit.attribute.Attribute.MAX_HEALTH).getValue();
    }

    @Override
    @SuppressWarnings("deprecation")
    public void setMaxHealth(double health) {
        if (!Double.isFinite(health) || health <= 0.0) {
            throw new IllegalArgumentException("maxHealth must be > 0, was " + health);
        }
        getAttribute(org.bukkit.attribute.Attribute.MAX_HEALTH).setBaseValue(health);
        if (state.health() > health) {
            state.setHealth(health);
        }
    }

    @Override
    @SuppressWarnings("deprecation")
    public void resetMaxHealth() {
        setMaxHealth(20.0);
    }

    @Override
    public String getDisplayName() {
        return displayName == null
                ? state.name()
                : LegacyComponentSerializer.legacySection().serialize(displayName);
    }

    @Override
    public void setDisplayName(String name) {
        this.displayName = name == null ? null : LegacyComponentSerializer.legacySection().deserialize(name);
        forwardDisplayNameToHost(name);
    }

    @Override
    public Component displayName() {
        return displayName == null ? Component.text(state.name()) : displayName;
    }

    @Override
    public void displayName(Component displayName) {
        this.displayName = displayName;
        forwardDisplayNameToHost(
                displayName == null ? null : LegacyComponentSerializer.legacySection().serialize(displayName));
    }

    @Override
    public String getPlayerListName() {
        return playerListName == null
                ? state.name()
                : LegacyComponentSerializer.legacySection().serialize(playerListName);
    }

    @Override
    public void setPlayerListName(String name) {
        this.playerListName = name == null ? null : LegacyComponentSerializer.legacySection().deserialize(name);
    }

    @Override
    public Component playerListName() {
        return playerListName == null ? Component.text(state.name()) : playerListName;
    }

    @Override
    public void playerListName(Component name) {
        this.playerListName = name;
    }

    @Override
    public PersistentDataContainer getPersistentDataContainer() {
        return persistentDataContainer;
    }

    @Override
    public boolean teleport(Location location) {
        return teleport(location, PlayerTeleportEvent.TeleportCause.PLUGIN);
    }

    @Override
    public boolean teleport(Location location, PlayerTeleportEvent.TeleportCause cause) {
        if (location == null || location.getWorld() == null) {
            throw new IllegalArgumentException("Teleport location and world must not be null");
        }
        Location from = getLocation();
        PlayerTeleportEvent event = new PlayerTeleportEvent(
                this,
                from,
                location.clone(),
                cause == null ? PlayerTeleportEvent.TeleportCause.PLUGIN : cause);
        if (runtime.isPrimaryThread()) {
            runtime.plugins().callEvent(event);
        }
        if (event.isCancelled()) {
            return false;
        }
        Location target = event.getTo() != null ? event.getTo() : location;
        if (target.getWorld() == null) {
            throw new IllegalArgumentException("Teleport destination world must not be null");
        }
        FerriteWorld world = runtime.world(target.getWorld().getName());
        if (world == null) {
            throw new IllegalArgumentException("Unknown Ferrite world: " + target.getWorld().getName());
        }
        state.setWorld(world);
        state.setPosition(target.getX(), target.getY(), target.getZ(), target.getYaw(), target.getPitch());
        forwardTeleportToHost(
                world.getName(), target.getX(), target.getY(), target.getZ(), target.getYaw(), target.getPitch());
        return true;
    }

    @Override
    public void sendMessage(String message) {
        state.messages().add(message);
        runtime.logger().info("[" + state.name() + "] " + message);
        forwardMessageToHost(message);
    }

    @Override
    public void sendMessage(Component message) {
        if (message == null) {
            return;
        }
        sendMessage(LegacyComponentSerializer.legacySection().serialize(message));
    }

    @Override
    public void sendMessage(String... messages) {
        for (String message : messages) {
            sendMessage(message);
        }
    }

    @Override
    public void sendMessage(UUID sender, String message) {
        sendMessage(message);
    }

    @Override
    public void sendMessage(UUID sender, String... messages) {
        sendMessage(messages);
    }

    @Override
    public Component name() {
        return Component.text(state.name());
    }

    @Override
    public java.util.Locale locale() {
        return java.util.Locale.US;
    }

    @Override
    public String getLocale() {
        return "en_us";
    }

    private final java.util.List<org.bukkit.permissions.PermissionAttachment> permissionAttachments =
            new java.util.concurrent.CopyOnWriteArrayList<>();
    private final java.util.Map<String, org.bukkit.permissions.PermissionAttachmentInfo>
            effectivePermissions = new java.util.concurrent.ConcurrentHashMap<>();

    @Override
    public boolean isPermissionSet(String permission) {
        if (permission == null) {
            return false;
        }
        return effectivePermissions.containsKey(permission.toLowerCase(java.util.Locale.ROOT));
    }

    @Override
    public boolean isPermissionSet(Permission permission) {
        return permission != null && isPermissionSet(permission.getName());
    }

    @Override
    public boolean hasPermission(String permission) {
        if (permission == null) {
            return false;
        }
        var info = effectivePermissions.get(permission.toLowerCase(java.util.Locale.ROOT));
        if (info != null) {
            return info.getValue();
        }
        return runtime.permissionResolver()
                .hasPermission(state.name(), state.uniqueId(), state.op(), permission);
    }

    @Override
    public boolean hasPermission(Permission permission) {
        return permission != null && hasPermission(permission.getName());
    }

    @Override
    public org.bukkit.permissions.PermissionAttachment addAttachment(Plugin plugin) {
        Objects.requireNonNull(plugin, "plugin");
        org.bukkit.permissions.PermissionAttachment attachment =
                new org.bukkit.permissions.PermissionAttachment(plugin, this);
        permissionAttachments.add(attachment);
        recalculatePermissions();
        return attachment;
    }

    @Override
    public org.bukkit.permissions.PermissionAttachment addAttachment(
            Plugin plugin, String name, boolean value) {
        Objects.requireNonNull(name, "name");
        org.bukkit.permissions.PermissionAttachment attachment = addAttachment(plugin);
        attachment.setPermission(name, value);
        return attachment;
    }

    @Override
    public org.bukkit.permissions.PermissionAttachment addAttachment(Plugin plugin, int ticks) {
        org.bukkit.permissions.PermissionAttachment attachment = addAttachment(plugin);
        if (ticks > 0) {
            runtime.scheduler().runTaskLater(plugin, () -> removeAttachment(attachment), ticks);
        }
        return attachment;
    }

    @Override
    public org.bukkit.permissions.PermissionAttachment addAttachment(
            Plugin plugin, String name, boolean value, int ticks) {
        org.bukkit.permissions.PermissionAttachment attachment = addAttachment(plugin, ticks);
        attachment.setPermission(name, value);
        return attachment;
    }

    @Override
    public void removeAttachment(org.bukkit.permissions.PermissionAttachment attachment) {
        Objects.requireNonNull(attachment, "attachment");
        if (permissionAttachments.remove(attachment)) {
            var cb = attachment.getRemovalCallback();
            if (cb != null) {
                cb.attachmentRemoved(attachment);
            }
            recalculatePermissions();
        }
    }

    @Override
    public void recalculatePermissions() {
        effectivePermissions.clear();
        for (org.bukkit.permissions.PermissionAttachment attachment : permissionAttachments) {
            for (var entry : attachment.getPermissions().entrySet()) {
                String key = entry.getKey().toLowerCase(java.util.Locale.ROOT);
                effectivePermissions.put(
                        key,
                        new org.bukkit.permissions.PermissionAttachmentInfo(
                                this, entry.getKey(), attachment, entry.getValue()));
            }
        }
    }

    @Override
    public java.util.Set<org.bukkit.permissions.PermissionAttachmentInfo> getEffectivePermissions() {
        return new java.util.HashSet<>(effectivePermissions.values());
    }

    @Override
    public boolean isOp() {
        return state.op();
    }

    @Override
    public void setOp(boolean op) {
        state.setOp(op);
    }

    @Override
    public GameMode getGameMode() {
        return state.gameMode();
    }

    @Override
    public void setGameMode(GameMode gameMode) {
        state.setGameMode(gameMode);
        forwardGameModeToHost(gameMode);
    }

    @Override
    public double getHealth() {
        return state.health();
    }

    @Override
    public void setHealth(double health) {
        double max = getMaxHealth();
        if (Double.isNaN(health) || health < 0 || health > max) {
            throw new IllegalArgumentException("health must be between 0 and " + max + ", was " + health);
        }
        state.setHealth(health);
        forwardHealthToHost(health);
    }

    @Override
    public int getFoodLevel() {
        return state.foodLevel();
    }

    @Override
    public void setFoodLevel(int foodLevel) {
        if (foodLevel < 0 || foodLevel > 20) {
            throw new IllegalArgumentException("food level must be between 0 and 20, was " + foodLevel);
        }
        if (foodLevel != state.foodLevel() && runtime.isPrimaryThread()) {
            org.bukkit.event.entity.FoodLevelChangeEvent event =
                    new org.bukkit.event.entity.FoodLevelChangeEvent(this, foodLevel);
            runtime.plugins().callEvent(event);
            if (event.isCancelled()) {
                return;
            }
            foodLevel = Math.max(0, Math.min(20, event.getFoodLevel()));
        }
        state.setFoodLevel(foodLevel);
        forwardFoodToHost(foodLevel);
    }

    @Override
    public PlayerInventory getInventory() {
        return inventory;
    }

    @Override
    public EntityEquipment getEquipment() {
        return new FerritePlayerEquipment(this);
    }

    @Override
    public InventoryView getOpenInventory() {
        if (openView != null && !openView.isClosed()) {
            return openView;
        }
        return new FerriteInventoryView(
                runtime, this, inventory, inventory, Component.text("Crafting"));
    }

    @Override
    public InventoryView openInventory(Inventory topInventory) {
        Objects.requireNonNull(topInventory, "inventory");
        Component title = topInventory instanceof FerriteCustomInventory custom
                ? custom.title()
                : Component.text(topInventory.getType().getDefaultTitle());
        FerriteInventoryView view =
                new FerriteInventoryView(runtime, this, topInventory, inventory, title);
        openInventory(view);
        return openView == view ? view : getOpenInventory();
    }

    @Override
    public void openInventory(InventoryView view) {
        Objects.requireNonNull(view, "view");
        if (!runtime.isPrimaryThread()) {
            throw new IllegalStateException("Player#openInventory must run on the main thread");
        }
        if (!(view instanceof FerriteInventoryView ferriteView)) {
            // A foreign InventoryView implementation cannot be encoded to the client.
            throw Unsupported.exception("Player#openInventory(custom InventoryView)");
        }
        if (openView != null && !openView.isClosed()) {
            openView.close(InventoryCloseEvent.Reason.OPEN_NEW);
        }
        InventoryOpenEvent openEvent = new InventoryOpenEvent(ferriteView);
        runtime.plugins().callEvent(openEvent);
        if (openEvent.isCancelled()) {
            return;
        }
        this.openView = ferriteView;
        if (ferriteView.getTopInventory() instanceof FerriteCustomInventory custom) {
            custom.addViewer(this);
            forwardOpenInventoryToHost(custom);
        }
    }

    @Override
    public void closeInventory() {
        closeInventory(InventoryCloseEvent.Reason.PLUGIN);
    }

    @Override
    public void closeInventory(InventoryCloseEvent.Reason reason) {
        Objects.requireNonNull(reason, "reason");
        if (!runtime.isPrimaryThread()) {
            throw new IllegalStateException("Player#closeInventory must run on the main thread");
        }
        boolean hadOpen = openView != null && !openView.isClosed();
        if (hadOpen) {
            openView.close(reason);
        }
        this.openView = null;
        if (hadOpen && reason != InventoryCloseEvent.Reason.PLAYER
                && reason != InventoryCloseEvent.Reason.DISCONNECT) {
            forwardCloseInventoryToHost();
        }
    }

    /** Clears {@code openView} when {@link FerriteInventoryView#close} completes. */
    public void clearOpenViewIf(FerriteInventoryView view) {
        if (this.openView == view) {
            this.openView = null;
        }
    }

    @Override
    public void kickPlayer(String message) {
        kick(Component.text(message == null ? "" : message), PlayerKickEvent.Cause.PLUGIN);
    }

    @Override
    public void kick(Component message, PlayerKickEvent.Cause cause) {
        String legacy = message == null ? "" : LegacyComponentSerializer.legacySection().serialize(message);
        state.kick(legacy);
        state.setOnline(false);
        runtime.logger().info("[" + state.name() + "] kicked: " + legacy);
        forwardKickToHost(legacy);
    }

    @Override
    public void sendPluginMessage(Plugin source, String channel, byte[] message) {
        runtime.messenger().sendPluginMessage(source, getUniqueId(), channel, message);
    }

    @Override
    public Set<String> getListeningPluginChannels() {
        return runtime.messenger().getIncomingChannels();
    }

    @Override
    public Scoreboard getScoreboard() {
        Scoreboard board = state.scoreboard();
        return board != null ? board : runtime.scoreboardManager().getMainScoreboard();
    }

    @Override
    public void setScoreboard(Scoreboard scoreboard) {
        if (scoreboard == null) {
            throw new IllegalArgumentException("Scoreboard cannot be null");
        }
        if (!(scoreboard instanceof FerriteScoreboard next)) {
            throw new IllegalArgumentException("Scoreboard must belong to Ferrite");
        }
        Scoreboard previous = state.scoreboard();
        if (previous instanceof FerriteScoreboard prev && prev != next) {
            prev.detachViewer(state.uniqueId());
        }
        state.setScoreboard(next);
        next.attachViewer(state.uniqueId());
    }

    @Override
    public String getScoreboardEntryName() {
        return state.name();
    }

    @Override
    @SuppressWarnings("deprecation")
    public void sendTitle(String title, String subtitle) {
        sendTitle(title, subtitle, 10, 70, 20);
    }

    @Override
    @SuppressWarnings("deprecation")
    public void sendTitle(String title, String subtitle, int fadeIn, int stay, int fadeOut) {
        state.setTitleTimes(fadeIn, stay, fadeOut);
        state.recordTitle(title, subtitle, fadeIn, stay, fadeOut);
        forwardTitleToHost(title, subtitle, fadeIn, stay, fadeOut, false, false);
    }

    @Override
    @SuppressWarnings("deprecation")
    public void sendTitle(com.destroystokyo.paper.Title title) {
        Objects.requireNonNull(title, "title");
        String main = BaseComponent.toLegacyText(title.getTitle());
        String sub = title.getSubtitle() == null ? "" : BaseComponent.toLegacyText(title.getSubtitle());
        sendTitle(main, sub, title.getFadeIn(), title.getStay(), title.getFadeOut());
    }

    @Override
    @SuppressWarnings("deprecation")
    public void updateTitle(com.destroystokyo.paper.Title title) {
        sendTitle(title);
    }

    @Override
    @SuppressWarnings("deprecation")
    public void setTitleTimes(int fadeInTicks, int stayTicks, int fadeOutTicks) {
        state.setTitleTimes(fadeInTicks, stayTicks, fadeOutTicks);
    }

    @Override
    @SuppressWarnings("deprecation")
    public void setSubtitle(BaseComponent[] subtitle) {
        state.setPendingSubtitle(subtitle == null ? "" : BaseComponent.toLegacyText(subtitle));
    }

    @Override
    @SuppressWarnings("deprecation")
    public void setSubtitle(BaseComponent subtitle) {
        state.setPendingSubtitle(subtitle == null ? "" : subtitle.toLegacyText());
    }

    @Override
    @SuppressWarnings("deprecation")
    public void showTitle(BaseComponent[] title) {
        String main = title == null ? "" : BaseComponent.toLegacyText(title);
        state.recordTitle(
                main,
                state.pendingSubtitle(),
                state.titleFadeIn(),
                state.titleStay(),
                state.titleFadeOut());
    }

    @Override
    @SuppressWarnings("deprecation")
    public void showTitle(BaseComponent title) {
        String main = title == null ? "" : title.toLegacyText();
        state.recordTitle(
                main,
                state.pendingSubtitle(),
                state.titleFadeIn(),
                state.titleStay(),
                state.titleFadeOut());
    }

    @Override
    @SuppressWarnings("deprecation")
    public void showTitle(
            BaseComponent[] title,
            BaseComponent[] subtitle,
            int fadeInTicks,
            int stayTicks,
            int fadeOutTicks) {
        String main = title == null ? "" : BaseComponent.toLegacyText(title);
        String sub = subtitle == null ? "" : BaseComponent.toLegacyText(subtitle);
        sendTitle(main, sub, fadeInTicks, stayTicks, fadeOutTicks);
    }

    @Override
    @SuppressWarnings("deprecation")
    public void showTitle(
            BaseComponent title,
            BaseComponent subtitle,
            int fadeInTicks,
            int stayTicks,
            int fadeOutTicks) {
        String main = title == null ? "" : title.toLegacyText();
        String sub = subtitle == null ? "" : subtitle.toLegacyText();
        sendTitle(main, sub, fadeInTicks, stayTicks, fadeOutTicks);
    }

    @Override
    public void showTitle(Title title) {
        Objects.requireNonNull(title, "title");
        Title.Times times = title.times();
        int fadeIn = times == null ? 10 : durationToTicks(times.fadeIn());
        int stay = times == null ? 70 : durationToTicks(times.stay());
        int fadeOut = times == null ? 20 : durationToTicks(times.fadeOut());
        sendTitle(serializeComponent(title.title()), serializeComponent(title.subtitle()), fadeIn, stay, fadeOut);
    }

    @Override
    public <T> void sendTitlePart(TitlePart<T> part, T value) {
        Objects.requireNonNull(part, "part");
        Objects.requireNonNull(value, "value");
        if (part == TitlePart.TIMES && value instanceof Title.Times times) {
            state.setTitleTimes(
                    durationToTicks(times.fadeIn()),
                    durationToTicks(times.stay()),
                    durationToTicks(times.fadeOut()));
        } else if (part == TitlePart.SUBTITLE && value instanceof Component subtitle) {
            state.setPendingSubtitle(serializeComponent(subtitle));
        } else if (part == TitlePart.TITLE && value instanceof Component title) {
            state.recordTitle(
                    serializeComponent(title),
                    state.pendingSubtitle(),
                    state.titleFadeIn(),
                    state.titleStay(),
                    state.titleFadeOut());
        }
    }

    @Override
    @SuppressWarnings("deprecation")
    public void hideTitle() {
        state.clearTitle();
        forwardTitleToHost(null, null, 0, 0, 0, true, false);
    }

    @Override
    public void clearTitle() {
        state.clearTitle();
        forwardTitleToHost(null, null, 0, 0, 0, true, false);
    }

    @Override
    public void resetTitle() {
        state.resetTitle();
        forwardTitleToHost(null, null, 0, 0, 0, false, true);
    }

    @Override
    @SuppressWarnings("deprecation")
    public void sendActionBar(String message) {
        state.recordActionBar(message);
        forwardActionBarToHost(message == null ? "" : message);
    }

    @Override
    @SuppressWarnings("deprecation")
    public void sendActionBar(char alternateChar, String message) {
        String translated = message == null
                ? ""
                : ChatColor.translateAlternateColorCodes(alternateChar, message);
        state.recordActionBar(translated);
        forwardActionBarToHost(translated);
    }

    @Override
    @SuppressWarnings("deprecation")
    public void sendActionBar(BaseComponent... message) {
        String text = message == null ? "" : BaseComponent.toLegacyText(message);
        state.recordActionBar(text);
        forwardActionBarToHost(text);
    }

    @Override
    public void sendActionBar(Component message) {
        String text = serializeComponent(message);
        state.recordActionBar(text);
        forwardActionBarToHost(text);
    }

    @Override
    public void sendActionBar(ComponentLike message) {
        sendActionBar(message == null ? null : message.asComponent());
    }

    @Override
    public void playSound(
            Location location, Sound sound, SoundCategory category, float volume, float pitch) {
        playSound(location, sound, category, volume, pitch, 0L);
    }

    @Override
    public void playSound(
            Location location, String sound, SoundCategory category, float volume, float pitch) {
        playSound(location, sound, category, volume, pitch, 0L);
    }

    @Override
    @SuppressWarnings("removal")
    public void playSound(
            Location location, Sound sound, SoundCategory category, float volume, float pitch, long seed) {
        if (location == null || sound == null) {
            return;
        }
        state.recordSound(
                sound.key().asString(),
                category,
                volume,
                pitch,
                seed,
                location.getX(),
                location.getY(),
                location.getZ());
        forwardSoundToHost(
                sound.key().asString(),
                category,
                location.getX(),
                location.getY(),
                location.getZ(),
                volume,
                pitch,
                seed);
    }

    @Override
    public void playSound(
            Location location, String sound, SoundCategory category, float volume, float pitch, long seed) {
        if (location == null || sound == null) {
            return;
        }
        state.recordSound(
                sound,
                category,
                volume,
                pitch,
                seed,
                location.getX(),
                location.getY(),
                location.getZ());
        forwardSoundToHost(
                sound,
                category,
                location.getX(),
                location.getY(),
                location.getZ(),
                volume,
                pitch,
                seed);
    }

    @Override
    public void playSound(
            Entity entity, Sound sound, SoundCategory category, float volume, float pitch) {
        playSound(entity, sound, category, volume, pitch, 0L);
    }

    @Override
    public void playSound(
            Entity entity, String sound, SoundCategory category, float volume, float pitch) {
        playSound(entity, sound, category, volume, pitch, 0L);
    }

    @Override
    @SuppressWarnings("removal")
    public void playSound(
            Entity entity, Sound sound, SoundCategory category, float volume, float pitch, long seed) {
        if (entity == null || sound == null) {
            return;
        }
        Location loc = entity.getLocation();
        state.recordSound(
                sound.key().asString(),
                category,
                volume,
                pitch,
                seed,
                loc.getX(),
                loc.getY(),
                loc.getZ());
    }

    @Override
    public void playSound(
            Entity entity, String sound, SoundCategory category, float volume, float pitch, long seed) {
        if (entity == null || sound == null) {
            return;
        }
        Location loc = entity.getLocation();
        state.recordSound(
                sound,
                category,
                volume,
                pitch,
                seed,
                loc.getX(),
                loc.getY(),
                loc.getZ());
    }

    @Override
    public void playSound(net.kyori.adventure.sound.Sound sound) {
        playSound(sound, state.x(), state.y(), state.z());
    }

    @Override
    public void playSound(net.kyori.adventure.sound.Sound sound, double x, double y, double z) {
        if (sound == null) {
            return;
        }
        state.recordSound(
                sound.name().asString(),
                toSoundCategory(sound.source()),
                sound.volume(),
                sound.pitch(),
                sound.seed().orElse(0L),
                x,
                y,
                z);
    }

    @Override
    public void playSound(
            net.kyori.adventure.sound.Sound sound, net.kyori.adventure.sound.Sound.Emitter emitter) {
        if (sound == null) {
            return;
        }
        if (emitter instanceof Entity entity) {
            Location loc = entity.getLocation();
            playSound(sound, loc.getX(), loc.getY(), loc.getZ());
        } else {
            playSound(sound, state.x(), state.y(), state.z());
        }
    }

    @Override
    public void stopSound(String sound, SoundCategory category) {
        // State-backed no-op refusal removal; stopSound is safe to invoke without failing.
    }

    @Override
    public void stopSound(SoundCategory category) {
    }

    @Override
    public void stopAllSounds() {
        state.playedSounds().clear();
    }

    @Override
    public void stopSound(net.kyori.adventure.sound.Sound sound) {
    }

    @Override
    public void stopSound(SoundStop stop) {
    }

    @Override
    public <T> void spawnParticle(
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
            T data,
            boolean force,
            Particle.RandomizationType randomizationType) {
        Objects.requireNonNull(particle, "particle");
        if (count < 0) {
            throw new IllegalArgumentException("Particle count cannot be negative: " + count);
        }
        state.recordParticle(
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
                randomizationType == null ? Particle.RandomizationType.DEFAULT : randomizationType);
        forwardParticleToHost(
                particle,
                x,
                y,
                z,
                count,
                (float) offsetX,
                (float) offsetY,
                (float) offsetZ,
                (float) extraX,
                force);
    }

    private static String serializeComponent(Component component) {
        return component == null
                ? ""
                : LegacyComponentSerializer.legacySection().serialize(component);
    }

    private static int durationToTicks(Duration duration) {
        return duration == null ? 0 : (int) Math.max(0L, duration.toMillis() / 50L);
    }

    private static SoundCategory toSoundCategory(net.kyori.adventure.sound.Sound.Source source) {
        if (source == null) {
            return SoundCategory.MASTER;
        }
        return switch (source) {
            case MASTER -> SoundCategory.MASTER;
            case MUSIC -> SoundCategory.MUSIC;
            case RECORD -> SoundCategory.RECORDS;
            case WEATHER -> SoundCategory.WEATHER;
            case BLOCK -> SoundCategory.BLOCKS;
            case HOSTILE -> SoundCategory.HOSTILE;
            case NEUTRAL -> SoundCategory.NEUTRAL;
            case PLAYER -> SoundCategory.PLAYERS;
            case AMBIENT -> SoundCategory.AMBIENT;
            case VOICE -> SoundCategory.VOICE;
            case UI -> SoundCategory.UI;
        };
    }

    private static final com.google.gson.Gson HOST_GSON = new com.google.gson.Gson();

    private void forwardTitleToHost(
            String title, String subtitle, int fadeIn, int stay, int fadeOut, boolean clear, boolean reset) {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            if (title != null) {
                req.put("title", title);
            }
            if (subtitle != null) {
                req.put("subtitle", subtitle);
            }
            req.put("fade_in", fadeIn);
            req.put("stay", stay);
            req.put("fade_out", fadeOut);
            req.put("clear", clear);
            req.put("reset", reset);
            dev.ferrite.host.Bootstrap.playerTitleSend(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardActionBarToHost(String text) {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            req.put("text", text == null ? "" : text);
            dev.ferrite.host.Bootstrap.playerActionBarSend(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardSoundToHost(
            String sound, SoundCategory category, double x, double y, double z, float volume, float pitch, long seed) {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            req.put("sound", sound);
            req.put("category", category == null ? 0 : category.ordinal());
            req.put("x", x);
            req.put("y", y);
            req.put("z", z);
            req.put("volume", volume);
            req.put("pitch", pitch);
            req.put("seed", seed);
            dev.ferrite.host.Bootstrap.playerSoundPlay(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardParticleToHost(
            Particle particle,
            double x,
            double y,
            double z,
            int count,
            float offsetX,
            float offsetY,
            float offsetZ,
            float extra,
            boolean force) {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            String key;
            try {
                key = particle.getKey().asString();
            } catch (Throwable ignored) {
                key = particle.name().toLowerCase(java.util.Locale.ROOT);
            }
            req.put("particle", key);
            req.put("x", x);
            req.put("y", y);
            req.put("z", z);
            req.put("count", count);
            req.put("offset_x", offsetX);
            req.put("offset_y", offsetY);
            req.put("offset_z", offsetZ);
            req.put("extra", extra);
            req.put("force", force);
            dev.ferrite.host.Bootstrap.playerParticleSpawn(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardAbilitiesToHost() {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            req.put("allow_flight", state.allowFlight());
            req.put("flying", state.flying());
            req.put("invulnerable", state.gameMode() == GameMode.CREATIVE || state.gameMode() == GameMode.SPECTATOR);
            req.put("fly_speed", state.flySpeed());
            req.put("walk_speed", state.walkSpeed());
            dev.ferrite.host.Bootstrap.playerAbilitiesSet(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardListHeaderFooterToHost() {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            req.put("header", state.playerListHeader());
            req.put("footer", state.playerListFooter());
            dev.ferrite.host.Bootstrap.playerListHeaderFooter(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardExperienceToHost() {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            req.put("level", state.level());
            req.put("progress", state.exp());
            req.put("total", state.totalExperience());
            dev.ferrite.host.Bootstrap.playerExperienceSet(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardTeleportToHost(
            String world, double x, double y, double z, float yaw, float pitch) {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            req.put("world", world);
            req.put("x", x);
            req.put("y", y);
            req.put("z", z);
            req.put("yaw", yaw);
            req.put("pitch", pitch);
            dev.ferrite.host.Bootstrap.playerTeleport(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardHealthToHost(double health) {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            req.put("health", health);
            dev.ferrite.host.Bootstrap.playerHealthSet(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardFoodToHost(int food) {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            req.put("food", food);
            dev.ferrite.host.Bootstrap.playerFoodSet(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardGameModeToHost(GameMode mode) {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            req.put("mode", mode.name());
            dev.ferrite.host.Bootstrap.playerGameModeSet(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardKickToHost(String reason) {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            if (reason != null && !reason.isEmpty()) {
                req.put("reason", reason);
            }
            dev.ferrite.host.Bootstrap.playerKick(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardMessageToHost(String message) {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            req.put("message", message == null ? "" : message);
            dev.ferrite.host.Bootstrap.playerMessageSend(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardVelocityToHost(double vx, double vy, double vz) {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            req.put("vx", vx);
            req.put("vy", vy);
            req.put("vz", vz);
            dev.ferrite.host.Bootstrap.playerVelocitySet(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardVisibilityToHost(UUID targetUuid, boolean hidden) {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("viewer_uuid", state.uniqueId().toString());
            req.put("target_uuid", targetUuid.toString());
            req.put("hidden", hidden);
            dev.ferrite.host.Bootstrap.playerVisibilitySet(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardCompassTargetToHost(int x, int y, int z) {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            req.put("x", x);
            req.put("y", y);
            req.put("z", z);
            dev.ferrite.host.Bootstrap.playerCompassTargetSet(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardPotionEffectToHost(
            org.bukkit.potion.PotionEffectType type,
            boolean remove,
            int amplifier,
            int duration,
            int flags) {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            req.put("effect", type.key().asString());
            req.put("remove", remove);
            req.put("amplifier", amplifier);
            req.put("duration", duration);
            req.put("flags", flags);
            dev.ferrite.host.Bootstrap.playerPotionEffectUpdate(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardOpenInventoryToHost(FerriteCustomInventory custom) {
        try {
            int size = custom.getSize();
            int rows = Math.max(1, Math.min(6, (size + 8) / 9));
            boolean shulker = custom.getType() == org.bukkit.event.inventory.InventoryType.SHULKER_BOX;
            java.util.List<Object> items = new java.util.ArrayList<>(rows * 9);
            for (int i = 0; i < rows * 9; i++) {
                org.bukkit.inventory.ItemStack stack = i < size ? custom.getItem(i) : null;
                if (stack == null || stack.getType().isAir() || stack.getAmount() <= 0) {
                    items.add(null);
                } else {
                    java.util.Map<String, Object> slot = new java.util.LinkedHashMap<>();
                    slot.put("material", stack.getType().getKey().asString());
                    slot.put("amount", Math.max(1, Math.min(99, stack.getAmount())));
                    items.add(slot);
                }
            }
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            req.put("title", serializeComponent(custom.title()));
            req.put("rows", rows);
            req.put("shulker", shulker);
            req.put("items", items);
            dev.ferrite.host.Bootstrap.playerOpenInventory(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardCloseInventoryToHost() {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            dev.ferrite.host.Bootstrap.playerCloseInventory(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
        }
    }

    private void forwardDamageToHost(double amount) {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            req.put("amount", (float) Math.max(0.0, amount));
            dev.ferrite.host.Bootstrap.playerDamage(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
        }
    }

    private void forwardRespawnToHost() {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            dev.ferrite.host.Bootstrap.playerRespawn(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
        }
    }

    private void forwardDisplayNameToHost(String displayName) {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            if (displayName != null) {
                req.put("display_name", displayName);
            }
            dev.ferrite.host.Bootstrap.playerDisplayNameSet(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
        }
    }

    private void forwardCooldownToHost(String group, int ticks) {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            req.put("group", group);
            req.put("ticks", Math.max(0, ticks));
            dev.ferrite.host.Bootstrap.playerCooldownSet(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
        }
    }

    private void forwardTransferToHost(String host, int port) {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            req.put("host", host);
            req.put("port", port);
            dev.ferrite.host.Bootstrap.playerTransfer(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
        }
    }

    private void forwardBlockChangeToHost(int x, int y, int z, String material) {
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            req.put("x", x);
            req.put("y", y);
            req.put("z", z);
            req.put("material", material);
            dev.ferrite.host.Bootstrap.playerBlockChangeSend(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
        }
    }

    @Override
    public boolean getAllowFlight() {
        return state.allowFlight();
    }

    @Override
    public void setAllowFlight(boolean flight) {
        state.setAllowFlight(flight);
        if (!flight) {
            state.setFlying(false);
        }
        forwardAbilitiesToHost();
    }

    @Override
    public boolean isFlying() {
        return state.flying();
    }

    @Override
    public void setFlying(boolean value) {
        if (value && !state.allowFlight()) {
            throw new IllegalArgumentException("Cannot make player fly if getAllowFlight() is false");
        }
        state.setFlying(value);
        forwardAbilitiesToHost();
    }

    @Override
    public float getFlySpeed() {
        return state.flySpeed();
    }

    @Override
    public void setFlySpeed(float value) throws IllegalArgumentException {
        if (value < -1.0f || value > 1.0f) {
            throw new IllegalArgumentException("Fly speed must be between -1.0 and 1.0");
        }
        state.setFlySpeed(value);
        forwardAbilitiesToHost();
    }

    @Override
    public float getWalkSpeed() {
        return state.walkSpeed();
    }

    @Override
    public void setWalkSpeed(float value) throws IllegalArgumentException {
        if (value < -1.0f || value > 1.0f) {
            throw new IllegalArgumentException("Walk speed must be between -1.0 and 1.0");
        }
        state.setWalkSpeed(value);
        forwardAbilitiesToHost();
    }

    @Override
    public void sendPlayerListHeaderAndFooter(Component header, Component footer) {
        state.setPlayerListHeader(serializeComponent(header));
        state.setPlayerListFooter(serializeComponent(footer));
        forwardListHeaderFooterToHost();
    }

    @Override
    @SuppressWarnings("deprecation")
    public void setPlayerListHeaderFooter(String header, String footer) {
        state.setPlayerListHeader(header == null ? "" : header);
        state.setPlayerListFooter(footer == null ? "" : footer);
        forwardListHeaderFooterToHost();
    }

    @Override
    @SuppressWarnings("deprecation")
    public void setPlayerListHeader(String header) {
        state.setPlayerListHeader(header == null ? "" : header);
        forwardListHeaderFooterToHost();
    }

    @Override
    @SuppressWarnings("deprecation")
    public void setPlayerListFooter(String footer) {
        state.setPlayerListFooter(footer == null ? "" : footer);
        forwardListHeaderFooterToHost();
    }

    @Override
    @SuppressWarnings("deprecation")
    public String getPlayerListHeader() {
        return state.playerListHeader();
    }

    @Override
    @SuppressWarnings("deprecation")
    public String getPlayerListFooter() {
        return state.playerListFooter();
    }

    @Override
    public int getLevel() {
        return state.level();
    }

    @Override
    public void setLevel(int level) {
        state.setLevel(level);
        forwardExperienceToHost();
    }

    @Override
    public void giveExp(int amount, boolean applyMending) {
        if (amount == 0) {
            return;
        }
        if (runtime.isPrimaryThread()) {
            org.bukkit.event.player.PlayerExpChangeEvent expEvent =
                    new org.bukkit.event.player.PlayerExpChangeEvent(this, amount);
            runtime.plugins().callEvent(expEvent);
            amount = expEvent.getAmount();
            if (amount == 0) {
                return;
            }
        }
        int total = Math.max(0, state.totalExperience() + amount);
        state.setTotalExperience(total);
        int level = state.level();
        float progress = state.exp();
        if (amount > 0) {
            int rem = amount;
            while (rem > 0) {
                int needed = xpToNextLevel(level);
                int cur = Math.round(progress * needed);
                if (cur + rem >= needed) {
                    rem -= (needed - cur);
                    level++;
                    progress = 0.0f;
                } else {
                    progress = (cur + rem) / (float) needed;
                    rem = 0;
                }
            }
        } else {
            int rem = -amount;
            while (rem > 0 && (level > 0 || progress > 0.0f)) {
                int needed = xpToNextLevel(level);
                int cur = Math.round(progress * needed);
                if (rem <= cur) {
                    progress = (cur - rem) / (float) needed;
                    rem = 0;
                } else {
                    rem -= cur;
                    if (level == 0) {
                        progress = 0.0f;
                        break;
                    }
                    level--;
                    progress = 1.0f;
                }
            }
        }
        state.setLevel(level);
        state.setExp(Math.max(0.0f, Math.min(1.0f, progress)));
        forwardExperienceToHost();
    }

    private static int xpToNextLevel(int level) {
        if (level >= 30) {
            return 112 + (level - 30) * 9;
        }
        if (level >= 15) {
            return 37 + (level - 15) * 5;
        }
        return 7 + level * 2;
    }

    @Override
    public void giveExpLevels(int amount) {
        state.setLevel(state.level() + amount);
        forwardExperienceToHost();
    }

    @Override
    public float getExp() {
        return state.exp();
    }

    @Override
    public void setExp(float exp) {
        state.setExp(exp);
        forwardExperienceToHost();
    }

    @Override
    public int getTotalExperience() {
        return state.totalExperience();
    }

    @Override
    public void setTotalExperience(int exp) {
        state.setTotalExperience(exp);
        forwardExperienceToHost();
    }

    @Override
    public boolean addPotionEffect(org.bukkit.potion.PotionEffect effect) {
        Objects.requireNonNull(effect, "effect");
        state.potionEffects().put(effect.getType(), effect);
        int flags = 0;
        if (effect.isAmbient()) {
            flags |= 0x01;
        }
        if (effect.hasParticles()) {
            flags |= 0x02;
        }
        if (effect.hasIcon()) {
            flags |= 0x04;
        }
        forwardPotionEffectToHost(
                effect.getType(), false, effect.getAmplifier(), effect.getDuration(), flags);
        return true;
    }

    @Override
    public boolean addPotionEffects(java.util.Collection<org.bukkit.potion.PotionEffect> effects) {
        Objects.requireNonNull(effects, "effects");
        boolean all = true;
        for (org.bukkit.potion.PotionEffect effect : effects) {
            all &= addPotionEffect(effect);
        }
        return all;
    }

    @Override
    public boolean hasPotionEffect(org.bukkit.potion.PotionEffectType type) {
        return type != null && state.potionEffects().containsKey(type);
    }

    @Override
    public org.bukkit.potion.PotionEffect getPotionEffect(org.bukkit.potion.PotionEffectType type) {
        return type == null ? null : state.potionEffects().get(type);
    }

    @Override
    public void removePotionEffect(org.bukkit.potion.PotionEffectType type) {
        if (type != null) {
            state.potionEffects().remove(type);
            forwardPotionEffectToHost(type, true, 0, 0, 0);
        }
    }

    @Override
    public java.util.Collection<org.bukkit.potion.PotionEffect> getActivePotionEffects() {
        return java.util.List.copyOf(state.potionEffects().values());
    }

    @Override
    public boolean clearActivePotionEffects() {
        if (state.potionEffects().isEmpty()) {
            return false;
        }
        java.util.List<org.bukkit.potion.PotionEffectType> types =
                new java.util.ArrayList<>(state.potionEffects().keySet());
        state.potionEffects().clear();
        for (org.bukkit.potion.PotionEffectType type : types) {
            forwardPotionEffectToHost(type, true, 0, 0, 0);
        }
        return true;
    }

    @Override
    public void setVelocity(org.bukkit.util.Vector velocity) {
        Objects.requireNonNull(velocity, "velocity");
        state.setVelocity(velocity.getX(), velocity.getY(), velocity.getZ());
        forwardVelocityToHost(velocity.getX(), velocity.getY(), velocity.getZ());
    }

    @Override
    public org.bukkit.util.Vector getVelocity() {
        return state.velocity();
    }

    @Override
    @SuppressWarnings("deprecation")
    public void hidePlayer(Player player) {
        if (player == null || player.getUniqueId().equals(state.uniqueId())) {
            return;
        }
        state.hiddenEntities().add(player.getUniqueId());
        forwardVisibilityToHost(player.getUniqueId(), true);
    }

    @Override
    public void hidePlayer(org.bukkit.plugin.Plugin plugin, Player player) {
        Objects.requireNonNull(plugin, "plugin");
        hidePlayer(player);
    }

    @Override
    @SuppressWarnings("deprecation")
    public void showPlayer(Player player) {
        if (player == null) {
            return;
        }
        state.hiddenEntities().remove(player.getUniqueId());
        forwardVisibilityToHost(player.getUniqueId(), false);
    }

    @Override
    public void showPlayer(org.bukkit.plugin.Plugin plugin, Player player) {
        Objects.requireNonNull(plugin, "plugin");
        showPlayer(player);
    }

    @Override
    public void hideEntity(org.bukkit.plugin.Plugin plugin, Entity entity) {
        Objects.requireNonNull(plugin, "plugin");
        if (entity == null || entity.getUniqueId().equals(state.uniqueId())) {
            return;
        }
        state.hiddenEntities().add(entity.getUniqueId());
        forwardVisibilityToHost(entity.getUniqueId(), true);
    }

    @Override
    public void showEntity(org.bukkit.plugin.Plugin plugin, Entity entity) {
        Objects.requireNonNull(plugin, "plugin");
        if (entity == null) {
            return;
        }
        state.hiddenEntities().remove(entity.getUniqueId());
        forwardVisibilityToHost(entity.getUniqueId(), false);
    }

    @Override
    public boolean canSee(Player player) {
        return player != null && !state.hiddenEntities().contains(player.getUniqueId());
    }

    @Override
    public boolean canSee(Entity entity) {
        return entity != null && !state.hiddenEntities().contains(entity.getUniqueId());
    }

    @Override
    public void setCompassTarget(Location loc) {
        Objects.requireNonNull(loc, "loc");
        state.setCompassTarget(loc);
        forwardCompassTargetToHost(loc.getBlockX(), loc.getBlockY(), loc.getBlockZ());
    }

    @Override
    public Location getCompassTarget() {
        return state.compassTarget();
    }

    @Override
    public boolean performCommand(String command) {
        Objects.requireNonNull(command, "command");
        if (runtime.commandDispatcher().dispatch(this, command)) {
            return true;
        }
        try {
            java.util.Map<String, Object> req = new java.util.LinkedHashMap<>();
            req.put("player_uuid", state.uniqueId().toString());
            req.put("command", command);
            String raw = dev.ferrite.host.Bootstrap.playerPerformCommand(
                    new com.google.gson.Gson().toJson(req));
            return raw != null && raw.contains("\"ok\":true");
        } catch (Throwable ignored) {
            return false;
        }
    }

    @Override
    public java.net.InetSocketAddress getAddress() {
        return java.net.InetSocketAddress.createUnresolved("127.0.0.1", 0);
    }

    @Override
    public void chat(String msg) {
        Objects.requireNonNull(msg, "msg");
        if (msg.startsWith("/")) {
            performCommand(msg.substring(1));
        } else {
            for (FerritePlayer recipient : runtime.onlinePlayers()) {
                recipient.sendMessage("<" + state.name() + "> " + msg);
            }
        }
    }

    @Override
    public boolean isSneaking() {
        return state.sneaking();
    }

    @Override
    public void setSneaking(boolean sneak) {
        state.setSneaking(sneak);
    }

    @Override
    public boolean isSprinting() {
        return state.sprinting();
    }

    @Override
    public void setSprinting(boolean sprinting) {
        state.setSprinting(sprinting);
    }

    @Override
    public int getPing() {
        return state.ping();
    }

    @Override
    public void damage(double amount) {
        if (amount < 0.0) {
            throw new IllegalArgumentException("damage must be >= 0");
        }
        double finalDamage = amount;
        org.bukkit.event.entity.EntityDamageEvent damageEvent = null;
        if (runtime.isPrimaryThread()) {
            damageEvent = new org.bukkit.event.entity.EntityDamageEvent(
                    this, org.bukkit.event.entity.EntityDamageEvent.DamageCause.CUSTOM, amount);
            runtime.plugins().callEvent(damageEvent);
            if (damageEvent.isCancelled()) {
                return;
            }
            finalDamage = Math.max(0.0, damageEvent.getFinalDamage());
        }
        state.setLastDamage(finalDamage);
        if (damageEvent != null) {
            state.setLastDamageCause(damageEvent);
        }
        double previousHealth = state.health();
        double nextHealth = Math.max(0.0, previousHealth - finalDamage);
        state.setHealth(nextHealth);
        forwardDamageToHost(finalDamage);
        if (previousHealth > 0.0 && nextHealth <= 0.0 && runtime.isPrimaryThread()) {
            org.bukkit.event.entity.PlayerDeathEvent deathEvent =
                    new org.bukkit.event.entity.PlayerDeathEvent(
                            this,
                            org.bukkit.damage.DamageSource.builder(org.bukkit.damage.DamageType.GENERIC).build(),
                            new java.util.ArrayList<>(),
                            0,
                            state.name() + " died");
            runtime.plugins().callEvent(deathEvent);
        }
    }

    @Override
    public void damage(double amount, Entity source) {
        damage(amount);
    }

    @Override
    public void damage(double amount, org.bukkit.damage.DamageSource damageSource) {
        damage(amount);
    }

    @Override
    public float getSaturation() {
        return state.saturation();
    }

    @Override
    public void setSaturation(float value) {
        state.setSaturation(value);
    }

    @Override
    public float getExhaustion() {
        return state.exhaustion();
    }

    @Override
    public void setExhaustion(float value) {
        state.setExhaustion(value);
    }

    @Override
    public int getFireTicks() {
        return state.fireTicks();
    }

    @Override
    public void setFireTicks(int ticks) {
        state.setFireTicks(ticks);
    }

    @Override
    public int getRemainingAir() {
        return state.remainingAir();
    }

    @Override
    public void setRemainingAir(int ticks) {
        state.setRemainingAir(ticks);
    }

    @Override
    public boolean hasPlayedBefore() {
        return true;
    }

    @Override
    public long getFirstPlayed() {
        return state.firstPlayed();
    }

    @Override
    public long getLastPlayed() {
        return state.lastPlayed();
    }

    @Override
    public boolean hasCooldown(org.bukkit.Material material) {
        return material != null && getCooldown(material) > 0;
    }

    @Override
    public int getCooldown(org.bukkit.Material material) {
        return material == null ? 0 : state.cooldowns().getOrDefault(material.getKey().asString(), 0);
    }

    @Override
    public void setCooldown(org.bukkit.Material material, int ticks) {
        Objects.requireNonNull(material, "material");
        setCooldownKey(material.getKey().asString(), ticks);
    }

    @Override
    public boolean hasCooldown(org.bukkit.inventory.ItemStack item) {
        return item != null && getCooldown(item) > 0;
    }

    @Override
    public int getCooldown(org.bukkit.inventory.ItemStack item) {
        return item == null ? 0 : getCooldown(item.getType());
    }

    @Override
    public void setCooldown(org.bukkit.inventory.ItemStack item, int ticks) {
        Objects.requireNonNull(item, "item");
        setCooldown(item.getType(), ticks);
    }

    @Override
    public int getCooldown(net.kyori.adventure.key.Key key) {
        return key == null ? 0 : state.cooldowns().getOrDefault(key.asString(), 0);
    }

    @Override
    public void setCooldown(net.kyori.adventure.key.Key key, int ticks) {
        Objects.requireNonNull(key, "key");
        setCooldownKey(key.asString(), ticks);
    }

    private void setCooldownKey(String group, int ticks) {
        int clamped = Math.max(0, ticks);
        if (clamped == 0) {
            state.cooldowns().remove(group);
        } else {
            state.cooldowns().put(group, clamped);
        }
        forwardCooldownToHost(group, clamped);
    }

    @Override
    public void transfer(String host, int port) {
        Objects.requireNonNull(host, "host");
        if (port <= 0 || port > 65535) {
            throw new IllegalArgumentException("port out of range: " + port);
        }
        forwardTransferToHost(host, port);
    }

    @Override
    @SuppressWarnings("deprecation")
    public void sendBlockChange(Location loc, org.bukkit.Material material, byte data) {
        Objects.requireNonNull(loc, "loc");
        Objects.requireNonNull(material, "material");
        forwardBlockChangeToHost(loc.getBlockX(), loc.getBlockY(), loc.getBlockZ(), material.getKey().asString());
    }

    @Override
    public void sendBlockChange(Location loc, org.bukkit.block.data.BlockData block) {
        Objects.requireNonNull(loc, "loc");
        Objects.requireNonNull(block, "block");
        forwardBlockChangeToHost(
                loc.getBlockX(), loc.getBlockY(), loc.getBlockZ(), block.getMaterial().getKey().asString());
    }

    private final Player.Spigot spigotView = new Player.Spigot() {
        @Override
        @SuppressWarnings("deprecation")
        public void sendMessage(net.md_5.bungee.api.chat.BaseComponent component) {
            if (component != null) {
                FerritePlayer.this.sendMessage(component.toLegacyText());
            }
        }

        @Override
        @SuppressWarnings("deprecation")
        public void sendMessage(net.md_5.bungee.api.chat.BaseComponent... components) {
            if (components != null && components.length > 0) {
                FerritePlayer.this.sendMessage(
                        net.md_5.bungee.api.chat.TextComponent.toLegacyText(components));
            }
        }

        @Override
        @SuppressWarnings("deprecation")
        public void sendMessage(
                net.md_5.bungee.api.ChatMessageType position,
                net.md_5.bungee.api.chat.BaseComponent component) {
            if (component == null) {
                return;
            }
            String text = component.toLegacyText();
            if (position == net.md_5.bungee.api.ChatMessageType.ACTION_BAR) {
                FerritePlayer.this.sendActionBar(Component.text(text));
            } else {
                FerritePlayer.this.sendMessage(text);
            }
        }

        @Override
        @SuppressWarnings("deprecation")
        public void sendMessage(
                net.md_5.bungee.api.ChatMessageType position,
                net.md_5.bungee.api.chat.BaseComponent... components) {
            if (components == null || components.length == 0) {
                return;
            }
            String text = net.md_5.bungee.api.chat.TextComponent.toLegacyText(components);
            if (position == net.md_5.bungee.api.ChatMessageType.ACTION_BAR) {
                FerritePlayer.this.sendActionBar(Component.text(text));
            } else {
                FerritePlayer.this.sendMessage(text);
            }
        }

        @Override
        public void respawn() {
            state.setHealth(20.0);
            state.setFoodLevel(20);
            forwardRespawnToHost();
            FerriteWorld world = state.world();
            if (world != null) {
                teleport(world.getSpawnLocation());
            }
        }
    };

    @Override
    public org.bukkit.inventory.Inventory getEnderChest() {
        if (enderChest == null) {
            enderChest = runtime.server().createInventory(
                    this, org.bukkit.event.inventory.InventoryType.ENDER_CHEST);
        }
        return enderChest;
    }

    private org.bukkit.inventory.Inventory enderChest;

    @Override
    public org.bukkit.inventory.InventoryView openWorkbench(Location location, boolean force) {
        org.bukkit.inventory.Inventory inv =
                runtime.server().createInventory(this, org.bukkit.event.inventory.InventoryType.WORKBENCH);
        return openInventory(inv);
    }

    @Override
    public org.bukkit.inventory.InventoryView openEnchanting(Location location, boolean force) {
        org.bukkit.inventory.Inventory inv =
                runtime.server().createInventory(this, org.bukkit.event.inventory.InventoryType.ENCHANTING);
        return openInventory(inv);
    }

    @Override
    public org.bukkit.inventory.InventoryView openAnvil(Location location, boolean force) {
        org.bukkit.inventory.Inventory inv =
                runtime.server().createInventory(this, org.bukkit.event.inventory.InventoryType.ANVIL);
        return openInventory(inv);
    }

    @Override
    public org.bukkit.inventory.InventoryView openCartographyTable(Location location, boolean force) {
        org.bukkit.inventory.Inventory inv =
                runtime.server().createInventory(this, org.bukkit.event.inventory.InventoryType.CARTOGRAPHY);
        return openInventory(inv);
    }

    @Override
    public org.bukkit.inventory.InventoryView openGrindstone(Location location, boolean force) {
        org.bukkit.inventory.Inventory inv =
                runtime.server().createInventory(this, org.bukkit.event.inventory.InventoryType.GRINDSTONE);
        return openInventory(inv);
    }

    @Override
    public org.bukkit.inventory.InventoryView openLoom(Location location, boolean force) {
        org.bukkit.inventory.Inventory inv =
                runtime.server().createInventory(this, org.bukkit.event.inventory.InventoryType.LOOM);
        return openInventory(inv);
    }

    @Override
    public org.bukkit.inventory.InventoryView openSmithingTable(Location location, boolean force) {
        org.bukkit.inventory.Inventory inv =
                runtime.server().createInventory(this, org.bukkit.event.inventory.InventoryType.SMITHING);
        return openInventory(inv);
    }

    @Override
    public org.bukkit.inventory.InventoryView openStonecutter(Location location, boolean force) {
        org.bukkit.inventory.Inventory inv =
                runtime.server().createInventory(this, org.bukkit.event.inventory.InventoryType.STONECUTTER);
        return openInventory(inv);
    }

    @Override
    @SuppressWarnings("deprecation")
    public void playEffect(Location loc, org.bukkit.Effect effect, int data) {
        Objects.requireNonNull(loc, "loc");
        Objects.requireNonNull(effect, "effect");
    }

    @Override
    public String getClientBrandName() {
        return "vanilla";
    }

    @Override
    public Location getRespawnLocation() {
        return respawnLocation == null ? null : respawnLocation.clone();
    }

    @Override
    public Location getRespawnLocation(boolean loadLocationAndValidate) {
        return getRespawnLocation();
    }

    @Override
    public void setRespawnLocation(Location location) {
        this.respawnLocation = location == null ? null : location.clone();
    }

    @Override
    public void setRespawnLocation(Location location, boolean force) {
        setRespawnLocation(location);
    }

    @Override
    public java.util.Set<String> getScoreboardTags() {
        return java.util.Collections.unmodifiableSet(scoreboardTags);
    }

    @Override
    public boolean addScoreboardTag(String tag) {
        Objects.requireNonNull(tag, "tag");
        return scoreboardTags.size() < 1024 && scoreboardTags.add(tag);
    }

    @Override
    public boolean removeScoreboardTag(String tag) {
        return scoreboardTags.remove(tag);
    }

    @Override
    public void setMetadata(String metadataKey, org.bukkit.metadata.MetadataValue newMetadataValue) {
        Objects.requireNonNull(metadataKey, "metadataKey");
        Objects.requireNonNull(newMetadataValue, "newMetadataValue");
        java.util.List<org.bukkit.metadata.MetadataValue> list =
                metadata.computeIfAbsent(metadataKey, k -> new java.util.ArrayList<>());
        list.removeIf(v -> v.getOwningPlugin() == newMetadataValue.getOwningPlugin());
        list.add(newMetadataValue);
    }

    @Override
    public java.util.List<org.bukkit.metadata.MetadataValue> getMetadata(String metadataKey) {
        Objects.requireNonNull(metadataKey, "metadataKey");
        java.util.List<org.bukkit.metadata.MetadataValue> list = metadata.get(metadataKey);
        return list == null
                ? java.util.Collections.emptyList()
                : java.util.Collections.unmodifiableList(new java.util.ArrayList<>(list));
    }

    @Override
    public boolean hasMetadata(String metadataKey) {
        Objects.requireNonNull(metadataKey, "metadataKey");
        java.util.List<org.bukkit.metadata.MetadataValue> list = metadata.get(metadataKey);
        return list != null && !list.isEmpty();
    }

    @Override
    public void removeMetadata(String metadataKey, Plugin owningPlugin) {
        Objects.requireNonNull(metadataKey, "metadataKey");
        Objects.requireNonNull(owningPlugin, "owningPlugin");
        java.util.List<org.bukkit.metadata.MetadataValue> list = metadata.get(metadataKey);
        if (list != null) {
            list.removeIf(v -> v.getOwningPlugin() == owningPlugin);
            if (list.isEmpty()) {
                metadata.remove(metadataKey);
            }
        }
    }

    @Override
    public org.bukkit.attribute.AttributeInstance getAttribute(org.bukkit.attribute.Attribute attribute) {
        Objects.requireNonNull(attribute, "attribute");
        return attributes.computeIfAbsent(attribute, a -> {
            double def = 0.0;
            String key = a.getKey().getKey();
            if ("max_health".equals(key)) {
                def = 20.0;
            } else if ("movement_speed".equals(key)) {
                def = 0.1;
            } else if ("attack_damage".equals(key)) {
                def = 1.0;
            } else if ("attack_speed".equals(key)) {
                def = 4.0;
            }
            return new FerriteAttributeInstance(a, def);
        });
    }

    @Override
    public Player.Spigot spigot() {
        return spigotView;
    }

    @Override
    public io.papermc.paper.threadedregions.scheduler.EntityScheduler getScheduler() {
        return new dev.ferrite.api.scheduler.FerriteEntityScheduler(runtime.scheduler(), this);
    }

    // --- Position, dimensions, rotation & pose ---

    @Override
    public double getX() {
        return state.x();
    }

    @Override
    public double getY() {
        return state.y();
    }

    @Override
    public double getZ() {
        return state.z();
    }

    @Override
    public float getYaw() {
        return state.yaw();
    }

    @Override
    public float getPitch() {
        return state.pitch();
    }

    @Override
    public void setRotation(float yaw, float pitch) {
        state.setRotation(yaw, pitch);
    }

    @Override
    public void setRotation(io.papermc.paper.math.Angle yaw, io.papermc.paper.math.Angle pitch) {
        Objects.requireNonNull(yaw, "yaw");
        Objects.requireNonNull(pitch, "pitch");
        float nextYaw = yaw.relative() ? state.yaw() + yaw.degrees() : yaw.degrees();
        float nextPitch = pitch.relative() ? state.pitch() + pitch.degrees() : pitch.degrees();
        setRotation(nextYaw, nextPitch);
    }

    @Override
    public float getBodyYaw() {
        return state.bodyYaw();
    }

    @Override
    public void setBodyYaw(float bodyYaw) {
        state.setBodyYaw(bodyYaw);
    }

    @Override
    public double getWidth() {
        return 0.6;
    }

    @Override
    public double getHeight() {
        if (isGliding() || isSwimming()) {
            return 0.6;
        }
        return isSneaking() ? 1.5 : 1.8;
    }

    @Override
    public org.bukkit.util.BoundingBox getBoundingBox() {
        double halfHeight = getHeight() * 0.5;
        return org.bukkit.util.BoundingBox.of(
                new org.bukkit.util.Vector(state.x(), state.y() + halfHeight, state.z()),
                0.3,
                halfHeight,
                0.3);
    }

    @Override
    public org.bukkit.block.BlockFace getFacing() {
        float norm = ((state.yaw() % 360.0f) + 360.0f) % 360.0f;
        if (norm >= 315.0f || norm < 45.0f) {
            return org.bukkit.block.BlockFace.SOUTH;
        }
        if (norm < 135.0f) {
            return org.bukkit.block.BlockFace.WEST;
        }
        if (norm < 225.0f) {
            return org.bukkit.block.BlockFace.NORTH;
        }
        return org.bukkit.block.BlockFace.EAST;
    }

    @Override
    public org.bukkit.entity.Pose getPose() {
        org.bukkit.entity.Pose fixed = state.fixedPose();
        if (fixed != null) {
            return fixed;
        }
        if (isGliding()) {
            return org.bukkit.entity.Pose.FALL_FLYING;
        }
        if (isSwimming()) {
            return org.bukkit.entity.Pose.SWIMMING;
        }
        if (isSneaking()) {
            return org.bukkit.entity.Pose.SNEAKING;
        }
        if (isDead()) {
            return org.bukkit.entity.Pose.DYING;
        }
        return org.bukkit.entity.Pose.STANDING;
    }

    @Override
    public void setPose(org.bukkit.entity.Pose pose, boolean fixed) {
        Objects.requireNonNull(pose, "pose");
        state.setFixedPose(fixed ? pose : null);
        if (!fixed) {
            state.setSneaking(pose == org.bukkit.entity.Pose.SNEAKING);
            state.setGliding(pose == org.bukkit.entity.Pose.FALL_FLYING);
            state.setSwimming(pose == org.bukkit.entity.Pose.SWIMMING);
        }
    }

    @Override
    public boolean hasFixedPose() {
        return state.fixedPose() != null;
    }

    @Override
    public void unsetFixedPose() {
        state.setFixedPose(null);
    }

    @Override
    public void lookAt(double x, double y, double z, io.papermc.paper.entity.LookAnchor playerAnchor) {
        Objects.requireNonNull(playerAnchor, "playerAnchor");
        double originY = playerAnchor == io.papermc.paper.entity.LookAnchor.EYES
                ? state.y() + getEyeHeight()
                : state.y();
        double dx = x - state.x();
        double dy = y - originY;
        double dz = z - state.z();
        double horiz = Math.sqrt(dx * dx + dz * dz);
        float yaw = (float) Math.toDegrees(Math.atan2(-dx, dz));
        float pitch = (float) Math.toDegrees(-Math.atan2(dy, horiz));
        setRotation(yaw, pitch);
    }

    @Override
    public void lookAt(
            Entity entity,
            io.papermc.paper.entity.LookAnchor playerAnchor,
            io.papermc.paper.entity.LookAnchor entityAnchor) {
        Objects.requireNonNull(entity, "entity");
        Objects.requireNonNull(entityAnchor, "entityAnchor");
        Location loc = entity instanceof org.bukkit.entity.LivingEntity living
                        && entityAnchor == io.papermc.paper.entity.LookAnchor.EYES
                ? living.getEyeLocation()
                : entity.getLocation();
        lookAt(loc.getX(), loc.getY(), loc.getZ(), playerAnchor);
    }

    @Override
    public int getEntityId() {
        return state.entityId();
    }

    @Override
    public org.bukkit.entity.EntityType getType() {
        return org.bukkit.entity.EntityType.PLAYER;
    }

    @Override
    public org.bukkit.entity.SpawnCategory getSpawnCategory() {
        return org.bukkit.entity.SpawnCategory.MISC;
    }

    @Override
    @SuppressWarnings("deprecation")
    public org.bukkit.entity.EntityCategory getCategory() {
        return org.bukkit.entity.EntityCategory.NONE;
    }

    @Override
    public boolean isInWorld() {
        return state.world() != null;
    }

    @Override
    public boolean isTicking() {
        return state.online();
    }

    @Override
    public java.util.List<Entity> getNearbyEntities(double x, double y, double z) {
        java.util.List<Entity> result = new java.util.ArrayList<>();
        for (Entity candidate : state.world().getNearbyEntities(getLocation(), x, y, z)) {
            if (!equals(candidate)) {
                result.add(candidate);
            }
        }
        return java.util.List.copyOf(result);
    }

    @Override
    public boolean collidesAt(Location location) {
        Objects.requireNonNull(location, "location");
        double dx = location.getX() - state.x();
        double dy = location.getY() - state.y();
        double dz = location.getZ() - state.z();
        org.bukkit.util.BoundingBox shifted = getBoundingBox().shift(dx, dy, dz);
        World w = location.getWorld() != null ? location.getWorld() : state.world();
        return hasBlockCollision(w, shifted);
    }

    @Override
    public boolean wouldCollideUsing(org.bukkit.util.BoundingBox boundingBox) {
        Objects.requireNonNull(boundingBox, "boundingBox");
        return hasBlockCollision(state.world(), boundingBox);
    }

    private static boolean hasBlockCollision(World world, org.bukkit.util.BoundingBox box) {
        if (world == null) {
            return false;
        }
        int minX = (int) Math.floor(box.getMinX());
        int maxX = (int) Math.floor(box.getMaxX());
        int minY = (int) Math.floor(box.getMinY());
        int maxY = (int) Math.floor(box.getMaxY());
        int minZ = (int) Math.floor(box.getMinZ());
        int maxZ = (int) Math.floor(box.getMaxZ());
        for (int bx = minX; bx <= maxX; bx++) {
            for (int by = minY; by <= maxY; by++) {
                for (int bz = minZ; bz <= maxZ; bz++) {
                    if (!world.getBlockAt(bx, by, bz).isPassable()) {
                        return true;
                    }
                }
            }
        }
        return false;
    }

    // --- Movement, environment & entity flags ---

    @Override
    public boolean isOnGround() {
        if (isFlying()) {
            return false;
        }
        int bx = (int) Math.floor(state.x());
        int by = (int) Math.floor(state.y() - 0.01);
        int bz = (int) Math.floor(state.z());
        return !state.world().getBlockAt(bx, by, bz).isPassable();
    }

    @Override
    public boolean isInWater() {
        int bx = (int) Math.floor(state.x());
        int by = (int) Math.floor(state.y());
        int bz = (int) Math.floor(state.z());
        return state.world().getBlockAt(bx, by, bz).getType() == org.bukkit.Material.WATER;
    }

    @Override
    public boolean isUnderWater() {
        Location eye = getEyeLocation();
        return state.world().getBlockAt(eye.getBlockX(), eye.getBlockY(), eye.getBlockZ()).getType()
                == org.bukkit.Material.WATER;
    }

    @Override
    public boolean isInLava() {
        int bx = (int) Math.floor(state.x());
        int by = (int) Math.floor(state.y());
        int bz = (int) Math.floor(state.z());
        return state.world().getBlockAt(bx, by, bz).getType() == org.bukkit.Material.LAVA;
    }

    @Override
    public boolean isInRain() {
        return state.world().hasStorm()
                && state.world().getHighestBlockYAt((int) Math.floor(state.x()), (int) Math.floor(state.z()))
                        <= state.y();
    }

    @Override
    public boolean isInPowderedSnow() {
        int bx = (int) Math.floor(state.x());
        int by = (int) Math.floor(state.y());
        int bz = (int) Math.floor(state.z());
        return state.world().getBlockAt(bx, by, bz).getType() == org.bukkit.Material.POWDER_SNOW;
    }

    @Override
    public boolean isClimbing() {
        int bx = (int) Math.floor(state.x());
        int by = (int) Math.floor(state.y());
        int bz = (int) Math.floor(state.z());
        org.bukkit.Material mat = state.world().getBlockAt(bx, by, bz).getType();
        return mat == org.bukkit.Material.LADDER
                || mat == org.bukkit.Material.VINE
                || mat == org.bukkit.Material.SCAFFOLDING;
    }

    @Override
    public boolean isGliding() {
        return state.gliding();
    }

    @Override
    public void setGliding(boolean gliding) {
        state.setGliding(gliding);
    }

    @Override
    public boolean isSwimming() {
        return state.swimming();
    }

    @Override
    public void setSwimming(boolean swimming) {
        state.setSwimming(swimming);
    }

    @Override
    public boolean isRiptiding() {
        return state.riptiding();
    }

    @Override
    public void setRiptiding(boolean riptiding) {
        state.setRiptiding(riptiding);
    }

    @Override
    public boolean isJumping() {
        return state.jumping();
    }

    @Override
    public void setJumping(boolean jumping) {
        state.setJumping(jumping);
    }

    @Override
    public boolean isSleeping() {
        return false;
    }

    @Override
    public boolean isDeeplySleeping() {
        return false;
    }

    @Override
    public int getSleepTicks() {
        return 0;
    }

    @Override
    public void setSleepingIgnored(boolean isSleeping) {
        state.setSleepingIgnored(isSleeping);
    }

    @Override
    public boolean isSleepingIgnored() {
        return state.sleepingIgnored();
    }

    @Override
    public void wakeup(boolean setSpawnLocation) {
        if (setSpawnLocation) {
            setRespawnLocation(getLocation());
        }
    }

    @Override
    public boolean isInvisible() {
        return state.invisible();
    }

    @Override
    public void setInvisible(boolean invisible) {
        state.setInvisible(invisible);
    }

    @Override
    public boolean isGlowing() {
        return state.glowing();
    }

    @Override
    public void setGlowing(boolean flag) {
        state.setGlowing(flag);
    }

    @Override
    public boolean isSilent() {
        return state.silent();
    }

    @Override
    public void setSilent(boolean flag) {
        state.setSilent(flag);
    }

    @Override
    public boolean isMarkedInvulnerable() {
        return state.invulnerable();
    }

    @Override
    public void setInvulnerable(boolean flag) {
        state.setInvulnerable(flag);
    }

    @Override
    public boolean isInvulnerableTo(org.bukkit.damage.DamageSource source) {
        Objects.requireNonNull(source, "source");
        return state.invulnerable()
                || state.gameMode() == GameMode.CREATIVE
                || state.gameMode() == GameMode.SPECTATOR;
    }

    @Override
    public boolean hasGravity() {
        return state.gravity();
    }

    @Override
    public void setGravity(boolean gravity) {
        state.setGravity(gravity);
    }

    @Override
    public double getDefaultGravity() {
        return 0.08;
    }

    @Override
    public double getGravity() {
        return state.gravity() ? getDefaultGravity() : 0.0;
    }

    @Override
    public float getAirDrag() {
        return 0.02f;
    }

    @Override
    public boolean isCollidable() {
        return state.collidable();
    }

    @Override
    public void setCollidable(boolean collidable) {
        state.setCollidable(collidable);
    }

    @Override
    public Set<UUID> getCollidableExemptions() {
        return state.collidableExemptions();
    }

    @Override
    public boolean hasNoPhysics() {
        return state.noPhysics();
    }

    @Override
    public void setNoPhysics(boolean noPhysics) {
        state.setNoPhysics(noPhysics);
    }

    @Override
    public boolean isPersistent() {
        return state.persistent();
    }

    @Override
    public void setPersistent(boolean persistent) {
        state.setPersistent(persistent);
    }

    @Override
    public boolean getCanPickupItems() {
        return state.canPickupItems();
    }

    @Override
    public void setCanPickupItems(boolean pickup) {
        state.setCanPickupItems(pickup);
    }

    @Override
    public boolean getRemoveWhenFarAway() {
        return state.removeWhenFarAway();
    }

    @Override
    public void setRemoveWhenFarAway(boolean remove) {
        state.setRemoveWhenFarAway(remove);
    }

    @Override
    public boolean hasAI() {
        return state.ai();
    }

    @Override
    public void setAI(boolean ai) {
        state.setAi(ai);
    }

    @Override
    public boolean getAffectsSpawning() {
        return state.affectsSpawning();
    }

    @Override
    public void setAffectsSpawning(boolean affects) {
        state.setAffectsSpawning(affects);
    }

    @Override
    public float getFallDistance() {
        return state.fallDistance();
    }

    @Override
    public void setFallDistance(float distance) {
        state.setFallDistance(Math.max(0.0f, distance));
    }

    @Override
    public int getTicksLived() {
        return state.ticksLived();
    }

    @Override
    public void setTicksLived(int value) {
        if (value <= 0) {
            throw new IllegalArgumentException("ticksLived must be > 0, was " + value);
        }
        state.setTicksLived(value);
    }

    @Override
    public net.kyori.adventure.util.TriState getFrictionState() {
        return state.frictionState();
    }

    @Override
    public void setFrictionState(net.kyori.adventure.util.TriState stateValue) {
        state.setFrictionState(stateValue);
    }

    @Override
    public void setFlyingFallDamage(net.kyori.adventure.util.TriState flyingFallDamage) {
        state.setFlyingFallDamage(flyingFallDamage);
    }

    @Override
    public net.kyori.adventure.util.TriState hasFlyingFallDamage() {
        return state.flyingFallDamage();
    }

    @Override
    public void resetFlyingTicks() {
        state.setFallDistance(0.0f);
    }

    @Override
    public org.bukkit.block.PistonMoveReaction getPistonMoveReaction() {
        return org.bukkit.block.PistonMoveReaction.MOVE;
    }

    // --- Combat, survival timers, fire/freeze/air, health & XP ---

    @Override
    public int getMaxFireTicks() {
        return 20;
    }

    @Override
    @SuppressWarnings("deprecation")
    public void setVisualFire(boolean fire) {
        state.setVisualFire(net.kyori.adventure.util.TriState.byBoolean(fire));
    }

    @Override
    public void setVisualFire(net.kyori.adventure.util.TriState fire) {
        state.setVisualFire(fire);
    }

    @Override
    @SuppressWarnings("deprecation")
    public boolean isVisualFire() {
        return state.visualFire() == net.kyori.adventure.util.TriState.TRUE;
    }

    @Override
    public net.kyori.adventure.util.TriState getVisualFire() {
        return state.visualFire();
    }

    @Override
    public int getFreezeTicks() {
        return state.freezeTicks();
    }

    @Override
    public int getMaxFreezeTicks() {
        return 140;
    }

    @Override
    public void setFreezeTicks(int ticks) {
        if (ticks < 0) {
            throw new IllegalArgumentException("freezeTicks must be >= 0, was " + ticks);
        }
        state.setFreezeTicks(ticks);
    }

    @Override
    public boolean isFrozen() {
        return state.freezeTicks() >= getMaxFreezeTicks();
    }

    @Override
    public boolean isFreezeTickingLocked() {
        return state.freezeTicksLocked();
    }

    @Override
    public void lockFreezeTicks(boolean locked) {
        state.setFreezeTicksLocked(locked);
    }

    @Override
    public int getMaximumAir() {
        return state.maximumAir();
    }

    @Override
    public void setMaximumAir(int ticks) {
        state.setMaximumAir(ticks);
    }

    @Override
    public int getPortalCooldown() {
        return state.portalCooldown();
    }

    @Override
    public void setPortalCooldown(int cooldown) {
        if (cooldown < 0) {
            throw new IllegalArgumentException("portalCooldown must be >= 0, was " + cooldown);
        }
        state.setPortalCooldown(cooldown);
    }

    @Override
    public int getMaximumNoDamageTicks() {
        return state.maximumNoDamageTicks();
    }

    @Override
    public void setMaximumNoDamageTicks(int ticks) {
        state.setMaximumNoDamageTicks(ticks);
    }

    @Override
    public int getNoDamageTicks() {
        return state.noDamageTicks();
    }

    @Override
    public void setNoDamageTicks(int ticks) {
        state.setNoDamageTicks(ticks);
    }

    @Override
    public int getNoActionTicks() {
        return state.noActionTicks();
    }

    @Override
    public void setNoActionTicks(int ticks) {
        state.setNoActionTicks(ticks);
    }

    @Override
    public double getLastDamage() {
        return state.lastDamage();
    }

    @Override
    public void setLastDamage(double damage) {
        state.setLastDamage(damage);
    }

    @Override
    public float getHurtDirection() {
        return state.hurtDirection();
    }

    @Override
    public void setHurtDirection(float hurtDirection) {
        state.setHurtDirection(hurtDirection);
    }

    @Override
    public void setLastDamageCause(org.bukkit.event.entity.EntityDamageEvent event) {
        state.setLastDamageCause(event);
    }

    @Override
    public org.bukkit.event.entity.EntityDamageEvent getLastDamageCause() {
        return state.lastDamageCause();
    }

    @Override
    public Player getKiller() {
        return state.killer();
    }

    @Override
    public void setKiller(Player killer) {
        state.setKiller(killer);
    }

    @Override
    public double getAbsorptionAmount() {
        return state.absorptionAmount();
    }

    @Override
    public void setAbsorptionAmount(double amount) {
        if (!Double.isFinite(amount) || amount < 0.0) {
            throw new IllegalArgumentException("absorption must be >= 0, was " + amount);
        }
        state.setAbsorptionAmount(amount);
    }

    @Override
    public void heal(double amount, org.bukkit.event.entity.EntityRegainHealthEvent.RegainReason reason) {
        if (amount <= 0.0 || isDead()) {
            return;
        }
        double gain = amount;
        if (runtime.isPrimaryThread()) {
            org.bukkit.event.entity.EntityRegainHealthEvent event =
                    new org.bukkit.event.entity.EntityRegainHealthEvent(
                            this,
                            amount,
                            reason == null
                                    ? org.bukkit.event.entity.EntityRegainHealthEvent.RegainReason.CUSTOM
                                    : reason);
            runtime.plugins().callEvent(event);
            if (event.isCancelled()) {
                return;
            }
            gain = Math.max(0.0, event.getAmount());
        }
        setHealth(Math.min(getMaxHealth(), state.health() + gain));
    }

    @Override
    public void kill(org.bukkit.damage.DamageSource damageSource) {
        Objects.requireNonNull(damageSource, "damageSource");
        setHealth(0.0);
        if (runtime.isPrimaryThread()) {
            org.bukkit.event.entity.PlayerDeathEvent deathEvent =
                    new org.bukkit.event.entity.PlayerDeathEvent(
                            this, damageSource, new java.util.ArrayList<>(), 0, state.name() + " died");
            runtime.plugins().callEvent(deathEvent);
        }
    }

    @Override
    public boolean isHealthScaled() {
        return state.healthScaled();
    }

    @Override
    public void setHealthScaled(boolean scale) {
        state.setHealthScaled(scale);
    }

    @Override
    public void setHealthScale(double scale) throws IllegalArgumentException {
        if (!Double.isFinite(scale) || scale <= 0.0) {
            throw new IllegalArgumentException("healthScale must be > 0, was " + scale);
        }
        state.setHealthScale(scale);
        state.setHealthScaled(true);
    }

    @Override
    public double getHealthScale() {
        return state.healthScale();
    }

    @Override
    public void sendHealthUpdate(double health, int foodLevel, float saturation) {
        // State-backed packet hook: validate finite inputs safely without requiring wire changes.
    }

    @Override
    public void sendHealthUpdate() {
    }

    @Override
    public int getSaturatedRegenRate() {
        return state.saturatedRegenRate();
    }

    @Override
    public void setSaturatedRegenRate(int ticks) {
        state.setSaturatedRegenRate(Math.max(1, ticks));
    }

    @Override
    public int getUnsaturatedRegenRate() {
        return state.unsaturatedRegenRate();
    }

    @Override
    public void setUnsaturatedRegenRate(int ticks) {
        state.setUnsaturatedRegenRate(Math.max(1, ticks));
    }

    @Override
    public int getStarvationRate() {
        return state.starvationRate();
    }

    @Override
    public void setStarvationRate(int ticks) {
        state.setStarvationRate(Math.max(1, ticks));
    }

    @Override
    public int getExpCooldown() {
        return 0;
    }

    @Override
    public int getExpToLevel() {
        return xpToNextLevel(state.level());
    }

    @Override
    public int calculateTotalExperiencePoints() {
        int lvl = state.level();
        int base;
        if (lvl <= 16) {
            base = lvl * lvl + 6 * lvl;
        } else if (lvl <= 31) {
            base = (int) (2.5 * lvl * lvl - 40.5 * lvl + 360);
        } else {
            base = (int) (4.5 * lvl * lvl - 162.5 * lvl + 2220);
        }
        return base + Math.round(state.exp() * xpToNextLevel(lvl));
    }

    @Override
    public void setExperienceLevelAndProgress(int totalExperience) {
        if (totalExperience < 0) {
            throw new IllegalArgumentException("totalExperience must be >= 0, was " + totalExperience);
        }
        state.setTotalExperience(totalExperience);
        int level = 0;
        int rem = totalExperience;
        while (true) {
            int next = xpToNextLevel(level);
            if (rem < next) {
                state.setLevel(level);
                state.setExp(next == 0 ? 0.0f : rem / (float) next);
                forwardExperienceToHost();
                return;
            }
            rem -= next;
            level++;
        }
    }

    @Override
    public int getExperiencePointsNeededForNextLevel() {
        int needed = xpToNextLevel(state.level());
        return Math.max(0, needed - Math.round(state.exp() * needed));
    }

    @Override
    public void sendExperienceChange(float progress) {
        sendExperienceChange(progress, state.level());
    }

    @Override
    public void sendExperienceChange(float progress, int level) {
        if (progress < 0.0f || progress > 1.0f || Float.isNaN(progress)) {
            throw new IllegalArgumentException("progress must be between 0.0 and 1.0, was " + progress);
        }
    }

    @Override
    public int applyMending(int amount) {
        return Math.max(0, amount);
    }

    @Override
    public float getCooldownPeriod() {
        double speed = getAttribute(org.bukkit.attribute.Attribute.ATTACK_SPEED).getValue();
        return speed <= 0.0 ? 20.0f : (float) (20.0 / speed);
    }

    @Override
    public float getCooledAttackStrength(float adjustTicks) {
        double period = getCooldownPeriod();
        double elapsedTicks = (System.currentTimeMillis() - state.lastAttackMillis()) / 50.0 + adjustTicks;
        return (float) Math.max(0.0, Math.min(1.0, elapsedTicks / period));
    }

    @Override
    public void resetCooldown() {
        state.resetAttackCooldown();
    }

    @Override
    @SuppressWarnings("deprecation")
    public float getAttackCooldown() {
        return getCooledAttackStrength(0.0f);
    }

    @Override
    public boolean canBreatheUnderwater() {
        return hasPotionEffect(org.bukkit.potion.PotionEffectType.WATER_BREATHING)
                || hasPotionEffect(org.bukkit.potion.PotionEffectType.CONDUIT_POWER);
    }

    @Override
    public int getArrowCooldown() {
        return state.arrowCooldown();
    }

    @Override
    public void setArrowCooldown(int ticks) {
        state.setArrowCooldown(Math.max(0, ticks));
    }

    @Override
    public int getArrowsInBody() {
        return state.arrowsInBody();
    }

    @Override
    public void setArrowsInBody(int count, boolean fireEvent) {
        if (count < 0) {
            throw new IllegalArgumentException("count must be >= 0, was " + count);
        }
        state.setArrowsInBody(count);
    }

    @Override
    public int getBeeStingerCooldown() {
        return state.beeStingerCooldown();
    }

    @Override
    public void setBeeStingerCooldown(int ticks) {
        state.setBeeStingerCooldown(Math.max(0, ticks));
    }

    @Override
    public int getBeeStingersInBody() {
        return state.beeStingersInBody();
    }

    @Override
    public void setBeeStingersInBody(int count) {
        if (count < 0) {
            throw new IllegalArgumentException("count must be >= 0, was " + count);
        }
        state.setBeeStingersInBody(count);
    }

    @Override
    public int getWardenWarningCooldown() {
        return state.wardenWarningCooldown();
    }

    @Override
    public void setWardenWarningCooldown(int cooldown) {
        state.setWardenWarningCooldown(Math.max(0, cooldown));
    }

    @Override
    public int getWardenTimeSinceLastWarning() {
        return state.wardenTimeSinceLastWarning();
    }

    @Override
    public void setWardenTimeSinceLastWarning(int time) {
        state.setWardenTimeSinceLastWarning(Math.max(0, time));
    }

    @Override
    public int getWardenWarningLevel() {
        return state.wardenWarningLevel();
    }

    @Override
    public void setWardenWarningLevel(int warningLevel) {
        state.setWardenWarningLevel(warningLevel);
    }

    @Override
    public void increaseWardenWarningLevel() {
        state.setWardenWarningLevel(state.wardenWarningLevel() + 1);
    }

    @Override
    public boolean canUseEquipmentSlot(org.bukkit.inventory.EquipmentSlot slot) {
        Objects.requireNonNull(slot, "slot");
        return slot != org.bukkit.inventory.EquipmentSlot.BODY
                && slot != org.bukkit.inventory.EquipmentSlot.SADDLE;
    }

    @Override
    @SuppressWarnings("deprecation")
    public org.bukkit.inventory.ItemStack getWeapon() {
        return inventory.getItemInMainHand();
    }

    @Override
    public org.bukkit.inventory.ItemStack damageItemStack(org.bukkit.inventory.ItemStack stack, int amount) {
        Objects.requireNonNull(stack, "stack");
        if (amount > 0 && !stack.isEmpty() && stack.getItemMeta() instanceof org.bukkit.inventory.meta.Damageable dmg) {
            if (!dmg.isUnbreakable()) {
                dmg.setDamage(dmg.getDamage() + amount);
                stack.setItemMeta(dmg);
            }
        }
        return stack;
    }

    @Override
    public void damageItemStack(org.bukkit.inventory.EquipmentSlot slot, int amount) {
        Objects.requireNonNull(slot, "slot");
        org.bukkit.inventory.ItemStack item = inventory.getItem(slot);
        if (item != null && !item.isEmpty()) {
            inventory.setItem(slot, damageItemStack(item, amount));
        }
    }

    // --- UI, names, client options, border, time, weather, inventory & active item ---

    @Override
    public Component playerListHeader() {
        String header = state.playerListHeader();
        return header.isEmpty()
                ? Component.empty()
                : LegacyComponentSerializer.legacySection().deserialize(header);
    }

    @Override
    public Component playerListFooter() {
        String footer = state.playerListFooter();
        return footer.isEmpty()
                ? Component.empty()
                : LegacyComponentSerializer.legacySection().deserialize(footer);
    }

    @Override
    public int getPlayerListOrder() {
        return state.playerListOrder();
    }

    @Override
    public void setPlayerListOrder(int order) {
        state.setPlayerListOrder(order);
    }

    @Override
    @SuppressWarnings("deprecation")
    public void setPlayerListHeaderFooter(BaseComponent[] header, BaseComponent[] footer) {
        setPlayerListHeaderFooter(
                header == null ? "" : BaseComponent.toLegacyText(header),
                footer == null ? "" : BaseComponent.toLegacyText(footer));
    }

    @Override
    @SuppressWarnings("deprecation")
    public void setPlayerListHeaderFooter(BaseComponent header, BaseComponent footer) {
        setPlayerListHeaderFooter(
                header == null ? "" : header.toLegacyText(),
                footer == null ? "" : footer.toLegacyText());
    }

    @Override
    @SuppressWarnings("deprecation")
    public void sendRawMessage(String message) {
        sendMessage(message);
    }

    @Override
    @SuppressWarnings("deprecation")
    public void sendRawMessage(UUID sender, String message) {
        sendMessage(sender, message);
    }

    @Override
    public Component customName() {
        String name = state.customName();
        return name == null ? null : LegacyComponentSerializer.legacySection().deserialize(name);
    }

    @Override
    public void customName(Component customName) {
        state.setCustomName(customName == null ? null : serializeComponent(customName));
    }

    @Override
    @SuppressWarnings("deprecation")
    public String getCustomName() {
        return state.customName();
    }

    @Override
    @SuppressWarnings("deprecation")
    public void setCustomName(String name) {
        state.setCustomName(name);
    }

    @Override
    public boolean isCustomNameVisible() {
        return state.customNameVisible();
    }

    @Override
    public void setCustomNameVisible(boolean flag) {
        state.setCustomNameVisible(flag);
    }

    @Override
    public Component teamDisplayName() {
        org.bukkit.scoreboard.Team team = getScoreboard().getEntityTeam(this);
        Component base = displayName();
        if (team == null) {
            return base;
        }
        return Component.empty().append(team.prefix()).append(base).append(team.suffix());
    }

    @Override
    public void showBossBar(net.kyori.adventure.bossbar.BossBar bar) {
        if (bar != null) {
            state.activeAdventureBossBars().add(bar);
        }
    }

    @Override
    public void hideBossBar(net.kyori.adventure.bossbar.BossBar bar) {
        if (bar != null) {
            state.activeAdventureBossBars().remove(bar);
        }
    }

    @Override
    public Iterable<? extends net.kyori.adventure.bossbar.BossBar> activeBossBars() {
        return java.util.Set.copyOf(state.activeAdventureBossBars());
    }

    @Override
    public net.kyori.adventure.sound.Sound.Source soundSource() {
        return net.kyori.adventure.sound.Sound.Source.PLAYER;
    }

    @Override
    public SoundCategory getSoundCategory() {
        return SoundCategory.PLAYERS;
    }

    @Override
    public float getSoundVolume() {
        return 1.0f;
    }

    @Override
    public float getSoundPitch() {
        return 1.0f;
    }

    @Override
    @SuppressWarnings("deprecation")
    public Sound getHurtSound() {
        return Sound.ENTITY_PLAYER_HURT;
    }

    @Override
    @SuppressWarnings("deprecation")
    public Sound getHurtSound(org.bukkit.damage.DamageSource damageSource) {
        return Sound.ENTITY_PLAYER_HURT;
    }

    @Override
    @SuppressWarnings("deprecation")
    public Sound getDeathSound() {
        return Sound.ENTITY_PLAYER_DEATH;
    }

    @Override
    public Sound getFallDamageSound(int fallHeight) {
        return fallHeight > 4 ? Sound.ENTITY_PLAYER_BIG_FALL : Sound.ENTITY_PLAYER_SMALL_FALL;
    }

    @Override
    public Sound getFallDamageSoundSmall() {
        return Sound.ENTITY_PLAYER_SMALL_FALL;
    }

    @Override
    public Sound getFallDamageSoundBig() {
        return Sound.ENTITY_PLAYER_BIG_FALL;
    }

    @Override
    public Sound getDrinkingSound(org.bukkit.inventory.ItemStack itemStack) {
        return Sound.ENTITY_GENERIC_DRINK;
    }

    @Override
    public Sound getEatingSound(org.bukkit.inventory.ItemStack itemStack) {
        return Sound.ENTITY_GENERIC_EAT;
    }

    @Override
    public Sound getSwimSound() {
        return Sound.ENTITY_PLAYER_SWIM;
    }

    @Override
    public Sound getSwimSplashSound() {
        return Sound.ENTITY_PLAYER_SPLASH;
    }

    @Override
    public Sound getSwimHighSpeedSplashSound() {
        return Sound.ENTITY_PLAYER_SPLASH_HIGH_SPEED;
    }

    @Override
    public int getClientViewDistance() {
        return state.viewDistance();
    }

    @Override
    public int getViewDistance() {
        return state.viewDistance();
    }

    @Override
    public void setViewDistance(int viewDistance) {
        if (viewDistance < 2 || viewDistance > 32) {
            throw new IllegalArgumentException("viewDistance must be between 2 and 32, was " + viewDistance);
        }
        state.setViewDistance(viewDistance);
    }

    @Override
    public int getSimulationDistance() {
        return state.simulationDistance();
    }

    @Override
    public void setSimulationDistance(int simulationDistance) {
        if (simulationDistance < 2 || simulationDistance > 32) {
            throw new IllegalArgumentException(
                    "simulationDistance must be between 2 and 32, was " + simulationDistance);
        }
        state.setSimulationDistance(simulationDistance);
    }

    @Override
    public int getSendViewDistance() {
        return state.sendViewDistance();
    }

    @Override
    public void setSendViewDistance(int viewDistance) {
        if (viewDistance < -1 || viewDistance > 32) {
            throw new IllegalArgumentException("sendViewDistance out of range: " + viewDistance);
        }
        state.setSendViewDistance(viewDistance < 0 ? state.viewDistance() : viewDistance);
    }

    @Override
    public org.bukkit.inventory.MainHand getMainHand() {
        return state.mainHand();
    }

    @Override
    @SuppressWarnings("unchecked")
    public <T> T getClientOption(com.destroystokyo.paper.ClientOption<T> option) {
        Objects.requireNonNull(option, "option");
        if (option == com.destroystokyo.paper.ClientOption.SKIN_PARTS) {
            return (T) state.skinParts().immutableCopy();
        }
        if (option == com.destroystokyo.paper.ClientOption.CHAT_COLORS_ENABLED
                || option == com.destroystokyo.paper.ClientOption.ALLOW_SERVER_LISTINGS) {
            return (T) Boolean.TRUE;
        }
        if (option == com.destroystokyo.paper.ClientOption.TEXT_FILTERING_ENABLED) {
            return (T) Boolean.FALSE;
        }
        if (option == com.destroystokyo.paper.ClientOption.CHAT_VISIBILITY) {
            return (T) com.destroystokyo.paper.ClientOption.ChatVisibility.FULL;
        }
        if (option == com.destroystokyo.paper.ClientOption.LOCALE) {
            return (T) state.locale();
        }
        if (option == com.destroystokyo.paper.ClientOption.MAIN_HAND) {
            return (T) state.mainHand();
        }
        if (option == com.destroystokyo.paper.ClientOption.VIEW_DISTANCE) {
            return (T) Integer.valueOf(state.viewDistance());
        }
        if (option == com.destroystokyo.paper.ClientOption.PARTICLE_VISIBILITY) {
            return (T) com.destroystokyo.paper.ClientOption.ParticleVisibility.ALL;
        }
        return null;
    }

    @Override
    public org.bukkit.WorldBorder getWorldBorder() {
        return state.worldBorder();
    }

    @Override
    public void setWorldBorder(org.bukkit.WorldBorder border) {
        state.setWorldBorder(border);
    }

    @Override
    public GameMode getPreviousGameMode() {
        return state.previousGameMode();
    }

    @Override
    public void setPlayerTime(long time, boolean relative) {
        state.setPlayerTime(time, relative);
    }

    @Override
    public long getPlayerTime() {
        Long offset = state.playerTimeOffset();
        if (offset == null) {
            return state.world().getTime();
        }
        return state.playerTimeRelative() ? state.world().getTime() + offset : state.world().getTime() + offset;
    }

    @Override
    public long getPlayerTimeOffset() {
        Long offset = state.playerTimeOffset();
        return offset == null ? 0L : offset;
    }

    @Override
    public boolean isPlayerTimeRelative() {
        return state.playerTimeRelative();
    }

    @Override
    public void resetPlayerTime() {
        state.resetPlayerTime();
    }

    @Override
    public void setPlayerWeather(org.bukkit.WeatherType type) {
        state.setPlayerWeather(Objects.requireNonNull(type, "type"));
    }

    @Override
    public org.bukkit.WeatherType getPlayerWeather() {
        return state.playerWeather();
    }

    @Override
    public void resetPlayerWeather() {
        state.setPlayerWeather(null);
    }

    @Override
    @SuppressWarnings("deprecation")
    public org.bukkit.inventory.ItemStack getItemInHand() {
        return inventory.getItemInMainHand();
    }

    @Override
    @SuppressWarnings("deprecation")
    public void setItemInHand(org.bukkit.inventory.ItemStack item) {
        inventory.setItemInMainHand(item);
    }

    @Override
    public org.bukkit.inventory.ItemStack getItemOnCursor() {
        if (openView != null && !openView.isClosed()) {
            return openView.getCursor();
        }
        return state.cursorItem();
    }

    @Override
    public void setItemOnCursor(org.bukkit.inventory.ItemStack item) {
        if (openView != null && !openView.isClosed()) {
            openView.setCursor(item);
        } else {
            state.setCursorItem(item);
        }
    }

    @Override
    public void updateInventory() {
        // State-backed inventory is always synchronized in-memory.
    }

    @Override
    public void startUsingItem(org.bukkit.inventory.EquipmentSlot hand) {
        Objects.requireNonNull(hand, "hand");
        if (hand != org.bukkit.inventory.EquipmentSlot.HAND
                && hand != org.bukkit.inventory.EquipmentSlot.OFF_HAND) {
            throw new IllegalArgumentException("hand must be HAND or OFF_HAND, got " + hand);
        }
        state.setActiveItemHand(hand);
        state.setActiveItemRemainingTime(32);
        state.setActiveItemUseTicks(0);
    }

    @Override
    public void completeUsingActiveItem() {
        clearActiveItem();
    }

    @Override
    public org.bukkit.inventory.ItemStack getActiveItem() {
        org.bukkit.inventory.EquipmentSlot hand = state.activeItemHand();
        return hand == null ? org.bukkit.inventory.ItemStack.empty() : inventory.getItem(hand);
    }

    @Override
    public void clearActiveItem() {
        state.setActiveItemHand(null);
        state.setActiveItemRemainingTime(0);
        state.setActiveItemUseTicks(0);
    }

    @Override
    public int getActiveItemRemainingTime() {
        return state.activeItemRemainingTime();
    }

    @Override
    public void setActiveItemRemainingTime(int ticks) {
        if (ticks < 0) {
            throw new IllegalArgumentException("ticks must be >= 0");
        }
        state.setActiveItemRemainingTime(ticks);
    }

    @Override
    public boolean hasActiveItem() {
        return state.activeItemHand() != null;
    }

    @Override
    public int getActiveItemUsedTime() {
        return state.activeItemUseTicks();
    }

    @Override
    public org.bukkit.inventory.EquipmentSlot getActiveItemHand() {
        org.bukkit.inventory.EquipmentSlot hand = state.activeItemHand();
        return hand == null ? org.bukkit.inventory.EquipmentSlot.HAND : hand;
    }

    @Override
    public org.bukkit.inventory.ItemStack getItemInUse() {
        return hasActiveItem() ? getActiveItem() : null;
    }

    @Override
    public int getItemInUseTicks() {
        return state.activeItemUseTicks();
    }

    @Override
    public void setItemInUseTicks(int ticks) {
        state.setActiveItemUseTicks(ticks);
    }

    @Override
    public boolean isHandRaised() {
        return hasActiveItem();
    }

    @Override
    public boolean isBlocking() {
        return hasActiveItem() && getActiveItem().getType() == org.bukkit.Material.SHIELD;
    }

    @Override
    public void swingMainHand() {
    }

    @Override
    public void swingOffHand() {
    }

    @Override
    public void swingHand(
            org.bukkit.inventory.EquipmentSlot slot,
            io.papermc.paper.datacomponent.item.SwingAnimation animation) {
    }

    @Override
    public void playHurtAnimation(float yaw) {
        state.setHurtDirection(yaw);
    }

    @Override
    public void sendHurtAnimation(float yaw) {
        state.setHurtDirection(yaw);
    }

    @Override
    public void broadcastHurtAnimation(java.util.Collection<Player> players) {
    }

    @Override
    public void playEffect(org.bukkit.EntityEffect type) {
        Objects.requireNonNull(type, "type");
    }

    @Override
    @SuppressWarnings("deprecation")
    public void playNote(Location loc, org.bukkit.Instrument instrument, org.bukkit.Note note) {
        Objects.requireNonNull(loc, "loc");
        Objects.requireNonNull(instrument, "instrument");
        Objects.requireNonNull(note, "note");
    }

    @Override
    @SuppressWarnings("deprecation")
    public <T> void playEffect(Location loc, org.bukkit.Effect effect, T data) {
        Objects.requireNonNull(loc, "loc");
        Objects.requireNonNull(effect, "effect");
    }

    @Override
    public void playPickupItemAnimation(org.bukkit.entity.Item item, int quantity) {
    }

    @Override
    public void broadcastSlotBreak(org.bukkit.inventory.EquipmentSlot slot) {
    }

    @Override
    public void broadcastSlotBreak(
            org.bukkit.inventory.EquipmentSlot slot, java.util.Collection<Player> players) {
    }

    @Override
    public void sendOpLevel(byte level) {
    }

    @Override
    public void updateCommands() {
    }

    @Override
    public void showWinScreen() {
        state.setHasSeenWinScreen(true);
    }

    @Override
    public boolean hasSeenWinScreen() {
        return state.hasSeenWinScreen();
    }

    @Override
    public void setHasSeenWinScreen(boolean hasSeenWinScreen) {
        state.setHasSeenWinScreen(hasSeenWinScreen);
    }

    @Override
    public void showDemoScreen() {
    }

    @Override
    public void showElderGuardian(boolean silent) {
    }

    @Override
    public void openBook(org.bukkit.inventory.ItemStack book) {
        Objects.requireNonNull(book, "book");
    }

    @Override
    public int getProtocolVersion() {
        return 777;
    }

    @Override
    public boolean isAllowingServerListings() {
        return true;
    }

    @Override
    public java.net.SocketAddress getSocketAddress() {
        return getAddress();
    }

    @Override
    public java.net.InetSocketAddress getHAProxyAddress() {
        return null;
    }

    @Override
    public java.net.InetSocketAddress getVirtualHost() {
        return getAddress();
    }

    @Override
    public boolean isTransferred() {
        return false;
    }

    @Override
    public void storeCookie(org.bukkit.NamespacedKey key, byte[] payload) {
        Objects.requireNonNull(key, "key");
        Objects.requireNonNull(payload, "payload");
        state.cookies().put(key, payload.clone());
    }

    @Override
    public java.util.concurrent.CompletableFuture<byte[]> retrieveCookie(org.bukkit.NamespacedKey key) {
        Objects.requireNonNull(key, "key");
        byte[] stored = state.cookies().get(key);
        return java.util.concurrent.CompletableFuture.completedFuture(stored == null ? null : stored.clone());
    }

    @Override
    public boolean isConnected() {
        return isOnline();
    }

    @Override
    public Player getPlayer() {
        return isOnline() ? this : null;
    }

    @Override
    @SuppressWarnings("deprecation")
    public long getLastLogin() {
        return state.firstPlayed();
    }

    @Override
    public long getLastSeen() {
        return isOnline() ? System.currentTimeMillis() : state.lastPlayed();
    }

    @Override
    public Location getLastDeathLocation() {
        return state.lastDeathLocation();
    }

    @Override
    public void setLastDeathLocation(Location location) {
        state.setLastDeathLocation(location);
    }

    @Override
    public Location getPotentialRespawnLocation() {
        return getRespawnLocation();
    }

    @Override
    @SuppressWarnings("deprecation")
    public Location getBedLocation() {
        Location loc = getRespawnLocation();
        return loc != null ? loc : state.world().getSpawnLocation();
    }

    @Override
    public boolean isWhitelisted() {
        return dev.ferrite.api.scoreboard.FerriteOfflinePlayer.isWhitelisted(state.uniqueId());
    }

    @Override
    public void setWhitelisted(boolean value) {
        dev.ferrite.api.scoreboard.FerriteOfflinePlayer.setWhitelisted(state.uniqueId(), value);
    }

    @Override
    public boolean isBanned() {
        return false;
    }

    @Override
    public com.destroystokyo.paper.profile.PlayerProfile getPlayerProfile() {
        return state.playerProfile().clone();
    }

    @Override
    public void setPlayerProfile(com.destroystokyo.paper.profile.PlayerProfile profile) {
        Objects.requireNonNull(profile, "profile");
        FerritePlayerProfile copy = new FerritePlayerProfile(profile.getId(), profile.getName());
        copy.setProperties(profile.getProperties());
        copy.setTextures(profile.getTextures());
        state.setPlayerProfile(copy);
    }

    @Override
    public java.time.Duration getIdleDuration() {
        return java.time.Duration.ofMillis(Math.max(0L, System.currentTimeMillis() - state.lastActivityMillis()));
    }

    @Override
    public void resetIdleDuration() {
        state.resetIdleDuration();
    }

    @Override
    public int getEnchantmentSeed() {
        return state.enchantmentSeed();
    }

    @Override
    public void setEnchantmentSeed(int seed) {
        state.setEnchantmentSeed(seed);
    }

    @Override
    public int getDeathScreenScore() {
        return state.deathScreenScore();
    }

    @Override
    public void setDeathScreenScore(int score) {
        state.setDeathScreenScore(score);
    }

    @Override
    public java.util.Collection<org.bukkit.entity.EnderPearl> getEnderPearls() {
        return java.util.List.of();
    }

    @Override
    public org.bukkit.Location getOrigin() {
        return null;
    }

    @Override
    public boolean fromMobSpawner() {
        return false;
    }

    @Override
    public org.bukkit.event.entity.CreatureSpawnEvent.SpawnReason getEntitySpawnReason() {
        return org.bukkit.event.entity.CreatureSpawnEvent.SpawnReason.DEFAULT;
    }

    @Override
    public org.bukkit.entity.Entity getSpectatorTarget() {
        return state.spectatorTarget();
    }

    @Override
    public void setSpectatorTarget(Entity entity) {
        state.setSpectatorTarget(entity);
    }

    @Override
    public int discoverRecipes(java.util.Collection<org.bukkit.NamespacedKey> recipes) {
        Objects.requireNonNull(recipes, "recipes");
        int added = 0;
        for (org.bukkit.NamespacedKey key : recipes) {
            if (key != null && state.discoveredRecipes().add(key)) {
                added++;
            }
        }
        return added;
    }

    @Override
    public int undiscoverRecipes(java.util.Collection<org.bukkit.NamespacedKey> recipes) {
        Objects.requireNonNull(recipes, "recipes");
        int removed = 0;
        for (org.bukkit.NamespacedKey key : recipes) {
            if (key != null && state.discoveredRecipes().remove(key)) {
                removed++;
            }
        }
        return removed;
    }

    @Override
    public boolean hasDiscoveredRecipe(org.bukkit.NamespacedKey recipe) {
        return recipe != null && state.discoveredRecipes().contains(recipe);
    }

    @Override
    public Set<org.bukkit.NamespacedKey> getDiscoveredRecipes() {
        return Set.copyOf(state.discoveredRecipes());
    }

    @Override
    public boolean isListed(Player other) {
        return other != null && !state.unlistedPlayers().contains(other.getUniqueId());
    }

    @Override
    public boolean unlistPlayer(Player other) {
        Objects.requireNonNull(other, "other");
        return state.unlistedPlayers().add(other.getUniqueId());
    }

    @Override
    public boolean listPlayer(Player other) {
        Objects.requireNonNull(other, "other");
        return state.unlistedPlayers().remove(other.getUniqueId());
    }

    @Override
    public void setVisibleByDefault(boolean visible) {
        state.setVisibleByDefault(visible);
    }

    @Override
    public boolean isVisibleByDefault() {
        return state.visibleByDefault();
    }

    @Override
    public Set<Player> getTrackedBy() {
        java.util.Set<Player> trackers = new java.util.LinkedHashSet<>();
        for (Player candidate : state.world().getPlayers()) {
            if (!equals(candidate) && candidate.canSee(this)) {
                trackers.add(candidate);
            }
        }
        return Set.copyOf(trackers);
    }

    @Override
    public boolean isTrackedBy(Player player) {
        return player != null && !equals(player) && state.world().equals(player.getWorld()) && player.canSee(this);
    }

    @Override
    public Set<Player> getTrackedPlayers() {
        java.util.Set<Player> tracked = new java.util.LinkedHashSet<>();
        for (Player candidate : state.world().getPlayers()) {
            if (!equals(candidate) && canSee(candidate)) {
                tracked.add(candidate);
            }
        }
        return Set.copyOf(tracked);
    }

    @Override
    public void addCustomChatCompletions(java.util.Collection<String> completions) {
        Objects.requireNonNull(completions, "completions");
        state.customChatCompletions().addAll(completions);
    }

    @Override
    public void removeCustomChatCompletions(java.util.Collection<String> completions) {
        Objects.requireNonNull(completions, "completions");
        state.customChatCompletions().removeAll(completions);
    }

    @Override
    public void setCustomChatCompletions(java.util.Collection<String> completions) {
        Objects.requireNonNull(completions, "completions");
        state.customChatCompletions().clear();
        state.customChatCompletions().addAll(completions);
    }

    @Override
    public void addAdditionalChatCompletions(java.util.Collection<String> completions) {
        Objects.requireNonNull(completions, "completions");
        state.additionalChatCompletions().addAll(completions);
    }

    @Override
    public void removeAdditionalChatCompletions(java.util.Collection<String> completions) {
        Objects.requireNonNull(completions, "completions");
        state.additionalChatCompletions().removeAll(completions);
    }

    @Override
    public Set<Long> getSentChunkKeys() {
        int pcx = Math.floorDiv((int) Math.floor(state.x()), 16);
        int pcz = Math.floorDiv((int) Math.floor(state.z()), 16);
        int dist = getSendViewDistance();
        java.util.Set<Long> keys = new java.util.LinkedHashSet<>();
        for (org.bukkit.Chunk chunk : state.world().getLoadedChunks()) {
            if (Math.max(Math.abs(chunk.getX() - pcx), Math.abs(chunk.getZ() - pcz)) <= dist) {
                keys.add(org.bukkit.Chunk.getChunkKey(chunk.getX(), chunk.getZ()));
            }
        }
        return Set.copyOf(keys);
    }

    @Override
    public Set<org.bukkit.Chunk> getSentChunks() {
        java.util.Set<org.bukkit.Chunk> chunks = new java.util.LinkedHashSet<>();
        for (long key : getSentChunkKeys()) {
            chunks.add(state.world().getChunkAt((int) key, (int) (key >> 32), false));
        }
        return Set.copyOf(chunks);
    }

    @Override
    public boolean isChunkSent(long chunkKey) {
        return getSentChunkKeys().contains(chunkKey);
    }

    @Override
    public boolean isAlliedTo(Entity other) {
        Objects.requireNonNull(other, "other");
        if (equals(other)) {
            return true;
        }
        for (java.util.function.Predicate<Entity> rule : state.alliedRules().values()) {
            if (rule.test(other)) {
                return true;
            }
        }
        org.bukkit.scoreboard.Team ownTeam = getScoreboard().getEntityTeam(this);
        org.bukkit.scoreboard.Team otherTeam = getScoreboard().getEntityTeam(other);
        return ownTeam != null && ownTeam.equals(otherTeam) && !ownTeam.allowFriendlyFire();
    }

    @Override
    public java.util.Map<net.kyori.adventure.key.Key, java.util.function.Predicate<Entity>>
            getAdditionalAlliedRules() {
        return java.util.Map.copyOf(state.alliedRules());
    }

    @Override
    public void addAdditionalAlliedRule(
            net.kyori.adventure.key.Key key, java.util.function.Predicate<Entity> predicate) {
        Objects.requireNonNull(key, "key");
        Objects.requireNonNull(predicate, "predicate");
        state.alliedRules().put(key, predicate);
    }

    @Override
    public void removeAdditionalAlliedRule(net.kyori.adventure.key.Key key) {
        Objects.requireNonNull(key, "key");
        state.alliedRules().remove(key);
    }

    @Override
    public void removeAdditionalAlliedRule(net.kyori.adventure.key.Namespaced namespace) {
        Objects.requireNonNull(namespace, "namespace");
        state.alliedRules().keySet().removeIf(k -> k.namespace().equals(namespace.namespace()));
    }

    @Override
    public io.papermc.paper.entity.PlayerGiveResult give(
            java.util.Collection<org.bukkit.inventory.ItemStack> items, boolean dropIfFull) {
        Objects.requireNonNull(items, "items");
        java.util.List<org.bukkit.inventory.ItemStack> leftovers = new java.util.ArrayList<>();
        java.util.List<org.bukkit.entity.Item> drops = new java.util.ArrayList<>();
        for (org.bukkit.inventory.ItemStack item : items) {
            if (item == null || item.isEmpty()) {
                continue;
            }
            java.util.Map<Integer, org.bukkit.inventory.ItemStack> overflow = inventory.addItem(item.clone());
            for (org.bukkit.inventory.ItemStack leftover : overflow.values()) {
                if (dropIfFull) {
                    org.bukkit.entity.Item dropped = state.world().dropItemNaturally(getLocation(), leftover);
                    if (dropped != null) {
                        drops.add(dropped);
                    }
                } else {
                    leftovers.add(leftover);
                }
            }
        }
        final java.util.List<org.bukkit.inventory.ItemStack> roLeftovers = java.util.List.copyOf(leftovers);
        final java.util.List<org.bukkit.entity.Item> roDrops = java.util.List.copyOf(drops);
        return new io.papermc.paper.entity.PlayerGiveResult() {
            @Override
            public java.util.Collection<org.bukkit.inventory.ItemStack> leftovers() {
                return roLeftovers;
            }

            @Override
            public java.util.Collection<org.bukkit.entity.Item> drops() {
                return roDrops;
            }
        };
    }

    @Override
    public boolean dropItem(boolean dropAll) {
        org.bukkit.inventory.ItemStack held = inventory.getItemInMainHand();
        if (held == null || held.isEmpty()) {
            return false;
        }
        int dropAmount = dropAll ? held.getAmount() : 1;
        org.bukkit.inventory.ItemStack dropped = held.clone();
        dropped.setAmount(dropAmount);
        int remaining = held.getAmount() - dropAmount;
        if (remaining <= 0) {
            inventory.setItemInMainHand(null);
        } else {
            org.bukkit.inventory.ItemStack updated = held.clone();
            updated.setAmount(remaining);
            inventory.setItemInMainHand(updated);
        }
        state.world().dropItemNaturally(getEyeLocation(), dropped);
        return true;
    }

    @Override
    public org.bukkit.entity.Item dropItem(
            int slot, int amount, boolean throwRandomly, java.util.function.Consumer<org.bukkit.entity.Item> function) {
        org.bukkit.inventory.ItemStack current = inventory.getItem(slot);
        if (current == null || current.isEmpty() || amount <= 0) {
            return null;
        }
        int take = Math.min(amount, current.getAmount());
        org.bukkit.inventory.ItemStack dropped = current.clone();
        dropped.setAmount(take);
        int rem = current.getAmount() - take;
        if (rem <= 0) {
            inventory.setItem(slot, null);
        } else {
            org.bukkit.inventory.ItemStack updated = current.clone();
            updated.setAmount(rem);
            inventory.setItem(slot, updated);
        }
        return state.world().dropItem(getEyeLocation(), dropped, function);
    }

    @Override
    public org.bukkit.entity.Item dropItem(
            org.bukkit.inventory.EquipmentSlot slot,
            int amount,
            boolean throwRandomly,
            java.util.function.Consumer<org.bukkit.entity.Item> function) {
        Objects.requireNonNull(slot, "slot");
        org.bukkit.inventory.ItemStack current = inventory.getItem(slot);
        if (current == null || current.isEmpty() || amount <= 0) {
            return null;
        }
        int take = Math.min(amount, current.getAmount());
        org.bukkit.inventory.ItemStack dropped = current.clone();
        dropped.setAmount(take);
        int rem = current.getAmount() - take;
        if (rem <= 0) {
            inventory.setItem(slot, null);
        } else {
            org.bukkit.inventory.ItemStack updated = current.clone();
            updated.setAmount(rem);
            inventory.setItem(slot, updated);
        }
        return state.world().dropItem(getEyeLocation(), dropped, function);
    }

    @Override
    public org.bukkit.entity.Item dropItem(
            org.bukkit.inventory.ItemStack item,
            boolean throwRandomly,
            java.util.function.Consumer<org.bukkit.entity.Item> function) {
        Objects.requireNonNull(item, "item");
        if (item.isEmpty()) {
            return null;
        }
        return state.world().dropItem(getEyeLocation(), item.clone(), function);
    }

    @Override
    public boolean breakBlock(org.bukkit.block.Block block) {
        Objects.requireNonNull(block, "block");
        return block.breakNaturally(inventory.getItemInMainHand());
    }

    @Override
    @SuppressWarnings("deprecation")
    public void sendBlockChanges(java.util.Collection<org.bukkit.block.BlockState> blocks) {
        Objects.requireNonNull(blocks, "blocks");
        for (org.bukkit.block.BlockState bs : blocks) {
            if (bs != null) {
                sendBlockChange(bs.getLocation(), bs.getBlockData());
            }
        }
    }

    @Override
    public void sendMultiBlockChange(
            java.util.Map<? extends io.papermc.paper.math.Position, org.bukkit.block.data.BlockData> blocks) {
        Objects.requireNonNull(blocks, "blocks");
        for (var entry : blocks.entrySet()) {
            if (entry.getKey() != null && entry.getValue() != null) {
                sendBlockChange(entry.getKey().toLocation(state.world()), entry.getValue());
            }
        }
    }

    @Override
    public void sendBlockDamage(Location loc, float progress, Entity source) {
        Objects.requireNonNull(loc, "loc");
    }

    @Override
    public void sendBlockDamage(Location loc, float progress, int sourceId) {
        Objects.requireNonNull(loc, "loc");
    }

    @Override
    public void sendEquipmentChange(
            org.bukkit.entity.LivingEntity entity,
            org.bukkit.inventory.EquipmentSlot slot,
            org.bukkit.inventory.ItemStack item) {
        Objects.requireNonNull(entity, "entity");
        Objects.requireNonNull(slot, "slot");
    }

    @Override
    public void sendEquipmentChange(
            org.bukkit.entity.LivingEntity entity,
            java.util.Map<org.bukkit.inventory.EquipmentSlot, org.bukkit.inventory.ItemStack> items) {
        Objects.requireNonNull(entity, "entity");
        Objects.requireNonNull(items, "items");
    }

    @Override
    public void sendSignChange(
            Location loc,
            java.util.List<? extends Component> lines,
            org.bukkit.DyeColor dyeColor,
            boolean hasGlowingText) {
        Objects.requireNonNull(loc, "loc");
    }

    @Override
    @SuppressWarnings("deprecation")
    public void sendSignChange(
            Location loc, String[] lines, org.bukkit.DyeColor dyeColor, boolean hasGlowingText) {
        Objects.requireNonNull(loc, "loc");
    }

    @Override
    public void sendBlockUpdate(Location loc, org.bukkit.block.TileState tileState)
            throws IllegalArgumentException {
        Objects.requireNonNull(loc, "loc");
        Objects.requireNonNull(tileState, "tileState");
    }

    @Override
    public void sendPotionEffectChange(
            org.bukkit.entity.LivingEntity entity, org.bukkit.potion.PotionEffect effect) {
        Objects.requireNonNull(entity, "entity");
        Objects.requireNonNull(effect, "effect");
    }

    @Override
    public void sendPotionEffectChangeRemove(
            org.bukkit.entity.LivingEntity entity, org.bukkit.potion.PotionEffectType type) {
        Objects.requireNonNull(entity, "entity");
        Objects.requireNonNull(type, "type");
    }

    @Override
    public void sendEntityEffect(org.bukkit.EntityEffect effect, Entity target) {
        Objects.requireNonNull(effect, "effect");
        Objects.requireNonNull(target, "target");
    }

    // --- Statistics (unified by UUID with FerriteOfflinePlayer) ---

    @Override
    public void incrementStatistic(org.bukkit.Statistic statistic) {
        incrementStatistic(statistic, 1);
    }

    @Override
    public void decrementStatistic(org.bukkit.Statistic statistic) {
        decrementStatistic(statistic, 1);
    }

    @Override
    public void incrementStatistic(org.bukkit.Statistic statistic, int amount) {
        dev.ferrite.api.scoreboard.FerriteOfflinePlayer.incrementStat(state.uniqueId(), statistic, amount);
    }

    @Override
    public void decrementStatistic(org.bukkit.Statistic statistic, int amount) {
        dev.ferrite.api.scoreboard.FerriteOfflinePlayer.decrementStat(state.uniqueId(), statistic, amount);
    }

    @Override
    public void setStatistic(org.bukkit.Statistic statistic, int newValue) {
        dev.ferrite.api.scoreboard.FerriteOfflinePlayer.setStat(state.uniqueId(), statistic, newValue);
    }

    @Override
    public int getStatistic(org.bukkit.Statistic statistic) {
        return dev.ferrite.api.scoreboard.FerriteOfflinePlayer.getStat(state.uniqueId(), statistic);
    }

    @Override
    public void incrementStatistic(org.bukkit.Statistic statistic, org.bukkit.Material material) {
        incrementStatistic(statistic, material, 1);
    }

    @Override
    public void decrementStatistic(org.bukkit.Statistic statistic, org.bukkit.Material material) {
        decrementStatistic(statistic, material, 1);
    }

    @Override
    public int getStatistic(org.bukkit.Statistic statistic, org.bukkit.Material material) {
        return dev.ferrite.api.scoreboard.FerriteOfflinePlayer.getStat(state.uniqueId(), statistic, material);
    }

    @Override
    public void incrementStatistic(org.bukkit.Statistic statistic, org.bukkit.Material material, int amount) {
        dev.ferrite.api.scoreboard.FerriteOfflinePlayer.incrementStat(
                state.uniqueId(), statistic, material, amount);
    }

    @Override
    public void decrementStatistic(org.bukkit.Statistic statistic, org.bukkit.Material material, int amount) {
        dev.ferrite.api.scoreboard.FerriteOfflinePlayer.decrementStat(
                state.uniqueId(), statistic, material, amount);
    }

    @Override
    public void setStatistic(org.bukkit.Statistic statistic, org.bukkit.Material material, int newValue) {
        dev.ferrite.api.scoreboard.FerriteOfflinePlayer.setStat(
                state.uniqueId(), statistic, material, newValue);
    }

    @Override
    public void incrementStatistic(org.bukkit.Statistic statistic, org.bukkit.entity.EntityType entityType) {
        incrementStatistic(statistic, entityType, 1);
    }

    @Override
    public void decrementStatistic(org.bukkit.Statistic statistic, org.bukkit.entity.EntityType entityType) {
        decrementStatistic(statistic, entityType, 1);
    }

    @Override
    public int getStatistic(org.bukkit.Statistic statistic, org.bukkit.entity.EntityType entityType) {
        return dev.ferrite.api.scoreboard.FerriteOfflinePlayer.getStat(
                state.uniqueId(), statistic, entityType);
    }

    @Override
    public void incrementStatistic(
            org.bukkit.Statistic statistic, org.bukkit.entity.EntityType entityType, int amount) {
        dev.ferrite.api.scoreboard.FerriteOfflinePlayer.incrementStat(
                state.uniqueId(), statistic, entityType, amount);
    }

    @Override
    public void decrementStatistic(
            org.bukkit.Statistic statistic, org.bukkit.entity.EntityType entityType, int amount) {
        dev.ferrite.api.scoreboard.FerriteOfflinePlayer.decrementStat(
                state.uniqueId(), statistic, entityType, amount);
    }

    @Override
    public void setStatistic(
            org.bukkit.Statistic statistic, org.bukkit.entity.EntityType entityType, int newValue) {
        dev.ferrite.api.scoreboard.FerriteOfflinePlayer.setStat(
                state.uniqueId(), statistic, entityType, newValue);
    }

    // --- Line of sight, target block & entity ray tracing ---

    @Override
    public org.bukkit.util.RayTraceResult rayTraceBlocks(
            double maxDistance, org.bukkit.FluidCollisionMode fluidCollisionMode) {
        Location eye = getEyeLocation();
        return state.world().rayTraceBlocks(
                eye,
                eye.getDirection(),
                maxDistance,
                fluidCollisionMode == null ? org.bukkit.FluidCollisionMode.NEVER : fluidCollisionMode,
                false);
    }

    @Override
    public org.bukkit.block.Block getTargetBlockExact(
            int maxDistance, org.bukkit.FluidCollisionMode fluidCollisionMode) {
        org.bukkit.util.RayTraceResult hit = rayTraceBlocks(maxDistance, fluidCollisionMode);
        return hit == null ? null : hit.getHitBlock();
    }

    @Override
    public org.bukkit.block.Block getTargetBlock(
            int maxDistance, com.destroystokyo.paper.block.TargetBlockInfo.FluidMode fluidMode) {
        Objects.requireNonNull(fluidMode, "fluidMode");
        return getTargetBlockExact(maxDistance, fluidMode.bukkit);
    }

    @Override
    public org.bukkit.block.BlockFace getTargetBlockFace(
            int maxDistance, com.destroystokyo.paper.block.TargetBlockInfo.FluidMode fluidMode) {
        Objects.requireNonNull(fluidMode, "fluidMode");
        return getTargetBlockFace(maxDistance, fluidMode.bukkit);
    }

    @Override
    public org.bukkit.block.BlockFace getTargetBlockFace(
            int maxDistance, org.bukkit.FluidCollisionMode fluidMode) {
        org.bukkit.util.RayTraceResult hit = rayTraceBlocks(maxDistance, fluidMode);
        return hit == null ? null : hit.getHitBlockFace();
    }

    @Override
    public com.destroystokyo.paper.block.TargetBlockInfo getTargetBlockInfo(
            int maxDistance, com.destroystokyo.paper.block.TargetBlockInfo.FluidMode fluidMode) {
        Objects.requireNonNull(fluidMode, "fluidMode");
        org.bukkit.util.RayTraceResult hit = rayTraceBlocks(maxDistance, fluidMode.bukkit);
        if (hit == null || hit.getHitBlock() == null || hit.getHitBlockFace() == null) {
            return null;
        }
        return new com.destroystokyo.paper.block.TargetBlockInfo(hit.getHitBlock(), hit.getHitBlockFace());
    }

    @Override
    @SuppressWarnings("deprecation")
    public java.util.List<org.bukkit.block.Block> getLineOfSight(
            Set<org.bukkit.Material> transparent, int maxDistance) {
        if (maxDistance <= 0) {
            return java.util.List.of();
        }
        Location eye = getEyeLocation();
        org.bukkit.util.Vector dir = eye.getDirection().normalize();
        java.util.List<org.bukkit.block.Block> sight = new java.util.ArrayList<>();
        org.bukkit.block.Block prev = null;
        for (double d = 0.0; d <= maxDistance; d += 0.25) {
            org.bukkit.util.Vector pos = eye.toVector().add(dir.clone().multiply(d));
            org.bukkit.block.Block block = state.world().getBlockAt(
                    pos.getBlockX(), pos.getBlockY(), pos.getBlockZ());
            if (!block.equals(prev)) {
                sight.add(block);
                prev = block;
                boolean isTransparent = transparent == null
                        ? block.isEmpty()
                        : transparent.contains(block.getType());
                if (!isTransparent) {
                    break;
                }
            }
        }
        return sight;
    }

    @Override
    @SuppressWarnings("deprecation")
    public org.bukkit.block.Block getTargetBlock(Set<org.bukkit.Material> transparent, int maxDistance) {
        java.util.List<org.bukkit.block.Block> sight = getLineOfSight(transparent, maxDistance);
        return sight.isEmpty() ? null : sight.get(sight.size() - 1);
    }

    @Override
    @SuppressWarnings("deprecation")
    public java.util.List<org.bukkit.block.Block> getLastTwoTargetBlocks(
            Set<org.bukkit.Material> transparent, int maxDistance) {
        java.util.List<org.bukkit.block.Block> sight = getLineOfSight(transparent, maxDistance);
        if (sight.size() <= 2) {
            return sight;
        }
        return java.util.List.of(sight.get(sight.size() - 2), sight.get(sight.size() - 1));
    }

    @Override
    public org.bukkit.util.RayTraceResult rayTraceEntities(int maxDistance, boolean ignoreBlocks) {
        if (maxDistance <= 0) {
            return null;
        }
        Location eye = getEyeLocation();
        double limit = maxDistance;
        if (!ignoreBlocks) {
            org.bukkit.util.RayTraceResult blockHit = rayTraceBlocks(maxDistance, org.bukkit.FluidCollisionMode.NEVER);
            if (blockHit != null) {
                limit = eye.toVector().distance(blockHit.getHitPosition());
            }
        }
        org.bukkit.util.Vector dir = eye.getDirection().normalize();
        org.bukkit.util.Vector start = eye.toVector();
        Entity closest = null;
        double closestDist = limit;
        org.bukkit.util.Vector closestHit = null;
        for (Entity candidate : state.world().getEntities()) {
            if (equals(candidate)) {
                continue;
            }
            org.bukkit.util.BoundingBox box;
            try {
                box = candidate.getBoundingBox();
            } catch (UnsupportedOperationException ignored) {
                Location loc = candidate.getLocation();
                box = org.bukkit.util.BoundingBox.of(
                        new org.bukkit.util.Vector(loc.getX(), loc.getY() + 0.9, loc.getZ()), 0.4, 0.9, 0.4);
            }
            org.bukkit.util.RayTraceResult hit = box.rayTrace(start, dir, closestDist);
            if (hit != null) {
                double dist = start.distance(hit.getHitPosition());
                if (dist < closestDist) {
                    closestDist = dist;
                    closest = candidate;
                    closestHit = hit.getHitPosition();
                }
            }
        }
        return closest == null ? null : new org.bukkit.util.RayTraceResult(closestHit, closest);
    }

    @Override
    public Entity getTargetEntity(int maxDistance, boolean ignoreBlocks) {
        org.bukkit.util.RayTraceResult hit = rayTraceEntities(maxDistance, ignoreBlocks);
        return hit == null ? null : hit.getHitEntity();
    }

    @Override
    public com.destroystokyo.paper.entity.TargetEntityInfo getTargetEntityInfo(
            int maxDistance, boolean ignoreBlocks) {
        org.bukkit.util.RayTraceResult hit = rayTraceEntities(maxDistance, ignoreBlocks);
        if (hit == null || hit.getHitEntity() == null) {
            return null;
        }
        return new com.destroystokyo.paper.entity.TargetEntityInfo(hit.getHitEntity(), hit.getHitPosition());
    }

    @Override
    public boolean hasLineOfSight(Entity other) {
        Objects.requireNonNull(other, "other");
        if (!state.world().equals(other.getWorld())) {
            return false;
        }
        Location target = other instanceof org.bukkit.entity.LivingEntity living
                ? living.getEyeLocation()
                : other.getLocation();
        return hasLineOfSight(target);
    }

    @Override
    public boolean hasLineOfSight(Location location) {
        Objects.requireNonNull(location, "location");
        if (location.getWorld() != null && !state.world().equals(location.getWorld())) {
            return false;
        }
        Location eye = getEyeLocation();
        org.bukkit.util.Vector delta = location.toVector().subtract(eye.toVector());
        double dist = delta.length();
        if (dist <= 1e-6) {
            return true;
        }
        return state.world().rayTraceBlocks(
                        eye, delta.normalize(), dist, org.bukkit.FluidCollisionMode.NEVER, true)
                == null;
    }

    @Override
    public java.util.Map<String, Object> serialize() {
        java.util.Map<String, Object> result = new java.util.LinkedHashMap<>();
        result.put("name", state.name());
        result.put("uuid", state.uniqueId().toString());
        result.put("world", state.world().getName());
        result.put("x", state.x());
        result.put("y", state.y());
        result.put("z", state.z());
        result.put("health", state.health());
        return result;
    }

    @Override
    public boolean equals(Object object) {
        return object instanceof FerritePlayer other && other.state.uniqueId().equals(state.uniqueId());
    }

    @Override
    public int hashCode() {
        return state.uniqueId().hashCode();
    }

    @Override
    public String toString() {
        return "FerritePlayer{" + state.name() + "}";
    }
}
