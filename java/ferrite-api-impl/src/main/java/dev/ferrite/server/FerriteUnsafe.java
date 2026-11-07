package dev.ferrite.server;

import com.google.gson.JsonObject;
import io.papermc.paper.entity.EntitySerializationFlag;
import io.papermc.paper.registry.RegistryKey;
import java.util.List;
import java.util.Map;
import net.kyori.adventure.key.Key;
import net.kyori.adventure.text.event.HoverEvent;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.UnsafeValues;
import org.bukkit.advancement.Advancement;
import org.bukkit.block.data.BlockData;
import org.bukkit.entity.Entity;
import org.bukkit.inventory.ItemStack;
import org.bukkit.material.MaterialData;
import org.bukkit.plugin.InvalidPluginException;
import org.bukkit.plugin.PluginDescriptionFile;
import org.bukkit.potion.PotionType;

/**
 * The Bukkit {@code UnsafeValues} bridge.
 *
 * <p>Only the version and registry lookups that plugins legitimately need are supplied. NBT
 * rewriting, advancement JSON and entity serialization are owned by the Rust world code and throw
 * a targeted exception instead of silently doing nothing.
 */
public final class FerriteUnsafe implements UnsafeValues {
    /** Mirrors {@code ferrite_world::DATA_VERSION} (26.3 chunk NBT). */
    public static final int DATA_VERSION = 5023;
    /** Mirrors {@code ferrite_protocol::PROTOCOL_VERSION}. */
    public static final int PROTOCOL_VERSION = 777;

    private static final java.util.Set<String> LEGACY_API_VERSIONS = java.util.Set.of(
            "1.13", "1.14", "1.15", "1.16", "1.17", "1.18", "1.19", "1.20", "1.21");

    private static UnsupportedOperationException unimplemented(String member) {
        return new UnsupportedOperationException(
                "Ferrite: UnsafeValues#" + member + " is not implemented; it needs the Rust world/registry code");
    }

    @Override public Material toLegacy(Material material) { return java.util.Objects.requireNonNull(material, "material"); }

    @Override public Material fromLegacy(Material material) { return java.util.Objects.requireNonNull(material, "material"); }

    @Override public Material fromLegacy(MaterialData data) {
        java.util.Objects.requireNonNull(data, "data");
        return java.util.Objects.requireNonNull(data.getItemType(), "material data has no item type");
    }

    @Override public Material fromLegacy(MaterialData data, boolean itemPriority) { return fromLegacy(data); }

    @Override public BlockData fromLegacy(Material material, byte data) {
        java.util.Objects.requireNonNull(material, "material");
        if (data != 0) throw new UnsupportedOperationException(
                "Ferrite: legacy block data byte " + data + " for " + material.getKey() + " is not mapped");
        return FerriteBlockData.create(material);
    }

    @Override public Material getMaterial(String name, int version) {
        var material = Material.matchMaterial(name);
        if (material == null) throw new IllegalArgumentException("Unknown material: " + name);
        return material;
    }

    @Override public int getDataVersion() { return DATA_VERSION; }

    @Override public ItemStack modifyItemStack(ItemStack stack, String arguments) {
        return (ItemStack) HostBridge.call("UnsafeValues", "unsafe", "modifyItemStack(ItemStack,String)", ItemStack.class, stack, arguments);
    }

    @Override public void checkSupported(PluginDescriptionFile description) throws InvalidPluginException {
        String api = description.getAPIVersion();
        if (api == null || api.equals("26.3") || LEGACY_API_VERSIONS.contains(api)) return;
        throw new InvalidPluginException("Ferrite runs Minecraft 26.3 (api-version 26.3); "
                + description.getName() + " requests api-version " + api);
    }

    @Override public byte[] processClass(PluginDescriptionFile description, String path, byte[] data) { return data; }

    @Override public Advancement loadAdvancement(Key key, String data, boolean replace) {
        return (Advancement) HostBridge.call("UnsafeValues", "unsafe", "loadAdvancement(Key,String,boolean)", Advancement.class, key, data, replace);
    }

    @Override public List<Advancement> loadAdvancements(Map<Key, String> data, boolean replace) {
        return (List<Advancement>) HostBridge.call("UnsafeValues", "unsafe", "loadAdvancements(Map,boolean)", List.class, data, replace);
    }

    @Override public boolean removeAdvancement(NamespacedKey key) { return HostBridge.callBoolean("UnsafeValues", "unsafe", "removeAdvancement(NamespacedKey)", key); }

    @Override public PotionType.InternalPotionData getInternalPotionData(NamespacedKey key) {
        return FerritePotionTypeData.get(key);
    }

    @Override public String get(Class<?> type, String key) { return (String) HostBridge.call("UnsafeValues", "unsafe", "get(Class,String)", String.class, type, key); }

    @SuppressWarnings("unchecked")
    @Override public <B extends org.bukkit.Keyed> B get(RegistryKey<B> registry, NamespacedKey key) {
        return (B) io.papermc.paper.registry.RegistryAccess.registryAccess().getRegistry(registry).get(key);
    }

    @Override public boolean isSupportedApiVersion(String apiVersion) {
        return apiVersion == null || apiVersion.equals("26.3") || LEGACY_API_VERSIONS.contains(apiVersion);
    }

    @Override public JsonObject serializeItemAsJson(ItemStack item) { return (JsonObject) HostBridge.call("UnsafeValues", "unsafe", "serializeItemAsJson(ItemStack)", JsonObject.class, item); }

    @Override public ItemStack deserializeItemFromJson(JsonObject data) { return (ItemStack) HostBridge.call("UnsafeValues", "unsafe", "deserializeItemFromJson(JsonObject)", ItemStack.class, data); }

    @Override public byte[] serializeEntity(Entity entity, EntitySerializationFlag... flags) {
        return (byte[]) HostBridge.call("UnsafeValues", "unsafe", "serializeEntity(Entity,)", byte[].class, entity, flags);
    }

    @Override public Entity deserializeEntity(byte[] data, org.bukkit.World world, boolean uniqueId, boolean intoWorld) {
        return (Entity) HostBridge.call("UnsafeValues", "unsafe", "deserializeEntity(byte[],World,boolean,boolean)", Entity.class, data, world, uniqueId, intoWorld);
    }

    @Override public int nextEntityId(org.bukkit.World world) { return HostBridge.callInt("UnsafeValues", "unsafe", "nextEntityId(World)", world); }

    @Override public String getMainLevelName() { return "world"; }

    @Override public int getProtocolVersion() { return PROTOCOL_VERSION; }

    @Override public ItemStack deserializeStack(Map<String, Object> args) { return (ItemStack) HostBridge.call("UnsafeValues", "unsafe", "deserializeStack(Map)", ItemStack.class, args); }

    @Override public ItemStack deserializeItemHover(HoverEvent.ShowItem item) { return (ItemStack) HostBridge.call("UnsafeValues", "unsafe", "deserializeItemHover(ShowItem)", ItemStack.class, item); }
}
