package dev.ferrite.api.registry;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import io.papermc.paper.enchantments.EnchantmentRarity;
import io.papermc.paper.registry.RegistryKey;
import io.papermc.paper.registry.set.RegistryKeySet;
import java.util.LinkedHashSet;
import java.util.Map;
import java.util.Set;
import net.kyori.adventure.text.Component;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.enchantments.Enchantment;
import org.bukkit.enchantments.EnchantmentTarget;
import org.bukkit.entity.EntityCategory;
import org.bukkit.entity.EntityType;
import org.bukkit.inventory.EquipmentSlotGroup;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.ItemType;

/**
 * An enchantment decoded from {@code data/minecraft/enchantment/<name>.json}.
 *
 * <p>Costs, weight, slots and supported items come straight from the datapack. Rarity and
 * the treasure/curse/tradeable/discoverable flags follow vanilla's mapping: rarity from the
 * weight, the flags from the {@code enchantment} item tags. Effects are not evaluated — the
 * facade has no combat engine — so {@code getDamageIncrease} reports zero.
 */
public final class FerriteEnchantment extends Enchantment {

    private final VanillaData data;
    private final NamespacedKey key;
    private final JsonObject json;
    private final Set<String> supportedItems;
    private final Set<String> primaryItems;
    private final Set<String> exclusiveWith;
    private final Set<EquipmentSlotGroup> slotGroups;
    private final Component description;
    private final String translationKey;

    public FerriteEnchantment(VanillaData data, NamespacedKey key) {
        this.data = data;
        this.key = key;
        JsonObject source = data.enchantments().get(key.getKey());
        this.json = source == null ? new JsonObject() : source;
        this.supportedItems = data.resolveItemSelector(VanillaData.string(this.json, "supported_items", ""));
        String primary = VanillaData.string(this.json, "primary_items", "");
        this.primaryItems = primary.isEmpty() ? this.supportedItems : data.resolveItemSelector(primary);
        JsonArray exclusive = this.json.has("exclusive_with")
                        && this.json.get("exclusive_with").isJsonArray()
                ? this.json.getAsJsonArray("exclusive_with")
                : new JsonArray();
        this.exclusiveWith = data.resolveEnchantmentSelectors(exclusive);
        this.slotGroups = readSlots(this.json);
        this.description = readDescription(this.json);
        this.translationKey = readTranslationKey(this.json);
    }

    @Override
    public NamespacedKey getKey() {
        return this.key;
    }

    @Override
    public String getName() {
        return this.key.getKey();
    }

    @Override
    public int getMaxLevel() {
        return VanillaData.integer(this.json, "max_level", 1);
    }

    @Override
    public int getStartLevel() {
        return 1;
    }

    @Override
    public EnchantmentTarget getItemTarget() {
        return EnchantmentTarget.BREAKABLE;
    }

    @Override
    public boolean isTreasure() {
        return this.data.enchantmentTagContains("minecraft:treasure", this.key.toString());
    }

    @Override
    public boolean isCursed() {
        return this.data.enchantmentTagContains("minecraft:curse", this.key.toString());
    }

    @Override
    public boolean conflictsWith(Enchantment other) {
        if (other == null || other.getKey() == null) {
            return false;
        }
        return this.exclusiveWith.contains(other.getKey().toString());
    }

    @Override
    public boolean canEnchantItem(ItemStack item) {
        if (item == null || item.getType() == null) {
            return false;
        }
        try {
            Material type = item.getType();
            return this.supportedItems.contains(type.getKey().toString());
        } catch (IllegalStateException legacy) {
            return false;
        }
    }

    @Override
    public Component displayName(int level) {
        return this.description;
    }

    @Override
    public boolean isTradeable() {
        return this.data.enchantmentTagContains("minecraft:tradeable", this.key.toString());
    }

    @Override
    public boolean isDiscoverable() {
        return this.data.enchantmentTagContains("minecraft:in_enchanting_table", this.key.toString());
    }

    @Override
    public int getMinModifiedCost(int level) {
        return cost("min_cost", level);
    }

    @Override
    public int getMaxModifiedCost(int level) {
        return cost("max_cost", level);
    }

    @Override
    public int getAnvilCost() {
        return VanillaData.integer(this.json, "anvil_cost", 0);
    }

    @Override
    public EnchantmentRarity getRarity() {
        int weight = getWeight();
        if (weight >= 10) {
            return EnchantmentRarity.COMMON;
        }
        if (weight >= 5) {
            return EnchantmentRarity.UNCOMMON;
        }
        if (weight >= 2) {
            return EnchantmentRarity.RARE;
        }
        return EnchantmentRarity.VERY_RARE;
    }

    @Override
    public float getDamageIncrease(int level, EntityCategory category) {
        return 0.0F;
    }

    @Override
    public float getDamageIncrease(int level, EntityType entityType) {
        return 0.0F;
    }

    @Override
    public Set<EquipmentSlotGroup> getActiveSlotGroups() {
        return this.slotGroups;
    }

    @Override
    public Component description() {
        return this.description;
    }

    @Override
    public RegistryKeySet<ItemType> getSupportedItems() {
        return new FerriteKeySet<>(RegistryKey.ITEM, this.supportedItems);
    }

    @Override
    public RegistryKeySet<ItemType> getPrimaryItems() {
        return new FerriteKeySet<>(RegistryKey.ITEM, this.primaryItems);
    }

    @Override
    public int getWeight() {
        return VanillaData.integer(this.json, "weight", 1);
    }

    @Override
    public RegistryKeySet<Enchantment> getExclusiveWith() {
        return new FerriteKeySet<>(RegistryKey.ENCHANTMENT, this.exclusiveWith);
    }

    @Override
    public String translationKey() {
        return this.translationKey;
    }

    @Override
    public String getTranslationKey() {
        return this.translationKey;
    }

    @Override
    public boolean equals(Object object) {
        return object instanceof FerriteEnchantment enchantment && this.key.equals(enchantment.key);
    }

    @Override
    public int hashCode() {
        return this.key.hashCode();
    }

    @Override
    public String toString() {
        return "FerriteEnchantment[" + this.key + "]";
    }

    private int cost(String member, int level) {
        if (!this.json.has(member) || !this.json.get(member).isJsonObject()) {
            return 0;
        }
        JsonObject cost = this.json.getAsJsonObject(member);
        int base = VanillaData.integer(cost, "base", 0);
        int perLevel = VanillaData.integer(cost, "per_level_above_first", 0);
        return base + perLevel * Math.max(0, level - 1);
    }

    private static Set<EquipmentSlotGroup> readSlots(JsonObject json) {
        if (!json.has("slots") || !json.get("slots").isJsonArray()) {
            return Set.of();
        }
        Set<EquipmentSlotGroup> groups = new LinkedHashSet<>();
        for (JsonElement element : json.getAsJsonArray("slots")) {
            EquipmentSlotGroup group = slotGroup(element.getAsString());
            if (group != null) {
                groups.add(group);
            }
        }
        return Set.copyOf(groups);
    }

    /**
     * Maps a datapack slot name to the API group.
     *
     * <p>Deliberately a switch rather than a static map: enchantment instances are
     * constructed while {@code Enchantment} and this class are still being initialized,
     * so this class's static fields may not be assigned yet.
     */
    private static EquipmentSlotGroup slotGroup(String name) {
        return switch (name) {
            case "any" -> EquipmentSlotGroup.ANY;
            case "mainhand" -> EquipmentSlotGroup.MAINHAND;
            case "hand" -> EquipmentSlotGroup.HAND;
            case "offhand" -> EquipmentSlotGroup.OFFHAND;
            case "feet" -> EquipmentSlotGroup.FEET;
            case "legs" -> EquipmentSlotGroup.LEGS;
            case "chest" -> EquipmentSlotGroup.CHEST;
            case "head" -> EquipmentSlotGroup.HEAD;
            case "armor" -> EquipmentSlotGroup.ARMOR;
            case "body" -> EquipmentSlotGroup.BODY;
            case "saddle" -> EquipmentSlotGroup.SADDLE;
            default -> null;
        };
    }

    private Component readDescription(JsonObject json) {
        JsonElement element = json.get("description");
        if (element != null && element.isJsonObject()) {
            JsonObject object = element.getAsJsonObject();
            if (object.has("translate")) {
                return Component.translatable(object.get("translate").getAsString());
            }
            if (object.has("text")) {
                return Component.text(object.get("text").getAsString());
            }
        }
        return Component.text(getName());
    }

    private String readTranslationKey(JsonObject json) {
        JsonElement element = json.get("description");
        if (element != null && element.isJsonObject()) {
            JsonObject object = element.getAsJsonObject();
            if (object.has("translate")) {
                return object.get("translate").getAsString();
            }
        }
        return "enchantment.minecraft." + this.key.getKey();
    }
}
