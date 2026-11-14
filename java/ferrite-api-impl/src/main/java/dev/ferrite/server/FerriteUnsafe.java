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
        java.util.Objects.requireNonNull(stack, "stack");
        java.util.Objects.requireNonNull(arguments, "arguments");
        return ItemComponents.apply(stack, arguments);
    }

    @Override public void checkSupported(PluginDescriptionFile description) throws InvalidPluginException {
        String api = description.getAPIVersion();
        if (api == null || api.equals("26.3") || LEGACY_API_VERSIONS.contains(api)) return;
        throw new InvalidPluginException("Ferrite runs Minecraft 26.3 (api-version 26.3); "
                + description.getName() + " requests api-version " + api);
    }

    @Override public byte[] processClass(PluginDescriptionFile description, String path, byte[] data) { return data; }

    @Override public Advancement loadAdvancement(Key key, String data, boolean replace) {
        throw advancementRegistration("loadAdvancement");
    }

    @Override public List<Advancement> loadAdvancements(Map<Key, String> data, boolean replace) {
        throw advancementRegistration("loadAdvancements");
    }

    @Override public boolean removeAdvancement(NamespacedKey key) { throw advancementRegistration("removeAdvancement"); }

    /**
     * Advancements are generated from the 26.3 data pack into Rust and published read-only to
     * plugins; the host has no route that would register a new advancement or its progress.
     */
    private static UnsupportedOperationException advancementRegistration(String member) {
        return new UnsupportedOperationException("Ferrite: UnsafeValues#" + member
 + " is not implemented: the host loads advancements only from the vanilla data pack and has no"
            + " runtime advancement-registration route, so a Java-side entry would never reach player"
            + " progress. Ship the advancement as a data pack, run Server#reloadData, then read it with"
            + " Server#getAdvancement and award progress through Player#getAdvancementProgress");
    }

    @Override public PotionType.InternalPotionData getInternalPotionData(NamespacedKey key) {
        return FerritePotionTypeData.get(key);
    }

    @Override public String get(Class<?> type, String key) {
        java.util.Objects.requireNonNull(type, "type");
        throw new UnsupportedOperationException("Ferrite: UnsafeValues#get(" + type.getName() + ", String)"
                + " is not implemented: it exposes CraftBukkit's internal class/key table, which the"
                + " server does not have. Use the typed API instead: Registry for keyed values,"
                + " Server#getAdvancement for advancements, and Material/Bukkit.createBlockData for blocks");
    }

    @SuppressWarnings("unchecked")
    @Override public <B extends org.bukkit.Keyed> B get(RegistryKey<B> registry, NamespacedKey key) {
        return (B) io.papermc.paper.registry.RegistryAccess.registryAccess().getRegistry(registry).get(key);
    }

    @Override public boolean isSupportedApiVersion(String apiVersion) {
        return apiVersion == null || apiVersion.equals("26.3") || LEGACY_API_VERSIONS.contains(apiVersion);
    }

    @Override public JsonObject serializeItemAsJson(ItemStack item) {
        throw new UnsupportedOperationException("Ferrite: UnsafeValues#serializeItemAsJson is not"
                + " implemented: the item component JSON is written by the Rust item code and there is no"
                + " host route for it. Use ItemStack#serialize (map form) with"
                + " UnsafeValues#deserializeStack, which round-trips through the Bukkit item registry");
    }

    @Override public ItemStack deserializeItemFromJson(JsonObject data) {
        throw new UnsupportedOperationException("Ferrite: UnsafeValues#deserializeItemFromJson is not"
                + " implemented: the host has no item-component JSON route. Use"
                + " UnsafeValues#deserializeStack with the map from ItemStack#serialize");
    }

    @Override public byte[] serializeEntity(Entity entity, EntitySerializationFlag... flags) {
        throw new UnsupportedOperationException("Ferrite: UnsafeValues#serializeEntity is not"
                + " implemented: entity NBT is captured by the Rust snapshot boundary, and the host has no"
                + " route that would return it here. Use Server#getEntityFactory()#createEntitySnapshot or"
                + " the entity's PersistentDataContainer for plugin-owned data");
    }

    @Override public Entity deserializeEntity(byte[] data, org.bukkit.World world, boolean uniqueId, boolean intoWorld) {
        throw new UnsupportedOperationException("Ferrite: UnsafeValues#deserializeEntity is not"
                + " implemented: the host creates entities from its own snapshot format, not arbitrary NBT."
                + " Use EntitySnapshot#createEntity or World#spawnEntity with an EntityType");
    }

    @Override public int nextEntityId(org.bukkit.World world) {
        throw new UnsupportedOperationException("Ferrite: UnsafeValues#nextEntityId is not implemented:"
                + " protocol entity ids are allocated by the owning region when the entity spawns, so no"
                + " public pre-spawn id exists. Spawn the entity and use Entity#getEntityId");
    }

    @Override public String getMainLevelName() { return "world"; }

    @Override public int getProtocolVersion() { return PROTOCOL_VERSION; }

    @Override public ItemStack deserializeStack(Map<String, Object> args) { return ItemStack.deserialize(args); }

    @Override public ItemStack deserializeItemHover(HoverEvent.ShowItem item) {
        java.util.Objects.requireNonNull(item, "item");
        if (item.nbt() != null) throw new UnsupportedOperationException(
                "Ferrite: UnsafeValues#deserializeItemHover cannot carry ShowItem component NBT; use the"
                + " ItemStack the hover event was created from, or ItemStack#deserializeBytes");
        var material = Material.matchMaterial(item.item().asString());
        if (material == null) throw new IllegalArgumentException("Unknown item key: " + item.item().asString());
        return new ItemStack(material, item.count());
    }
}
