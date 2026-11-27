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
        return 1.62;
    }

    @Override
    public double getEyeHeight(boolean ignorePose) {
        return 1.62;
    }

    @Override
    public Location getEyeLocation() {
        return new Location(
                state.world(), state.x(), state.y() + 1.62, state.z(), state.yaw(), state.pitch());
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
    public double getMaxHealth() {
        return 20.0;
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
        FerriteWorld world = runtime.world(location.getWorld().getName());
        if (world == null) {
            throw new IllegalArgumentException("Unknown Ferrite world: " + location.getWorld().getName());
        }
        state.setWorld(world);
        state.setPosition(location.getX(), location.getY(), location.getZ(), location.getYaw(), location.getPitch());
        forwardTeleportToHost(
                world.getName(), location.getX(), location.getY(), location.getZ(), location.getYaw(), location.getPitch());
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
        if (health < 0 || health > 20) {
            throw new IllegalArgumentException("health must be between 0 and 20, was " + health);
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
        state.setHealth(Math.max(0.0, state.health() - amount));
        forwardDamageToHost(amount);
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
