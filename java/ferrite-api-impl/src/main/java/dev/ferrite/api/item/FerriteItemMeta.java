package dev.ferrite.api.item;

import com.google.common.collect.LinkedHashMultimap;
import com.google.common.collect.Multimap;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Collections;
import java.util.EnumSet;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer;
import org.bukkit.NamespacedKey;
import org.bukkit.Registry;
import org.bukkit.attribute.Attribute;
import org.bukkit.attribute.AttributeModifier;
import org.bukkit.enchantments.Enchantment;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.ItemFlag;
import org.bukkit.inventory.meta.Damageable;
import org.bukkit.inventory.meta.components.CustomModelDataComponent;
import org.bukkit.inventory.meta.components.EquippableComponent;
import org.bukkit.inventory.meta.components.FoodComponent;
import org.bukkit.inventory.meta.components.JukeboxPlayableComponent;
import org.bukkit.inventory.meta.components.ToolComponent;
import org.bukkit.inventory.meta.components.UseCooldownComponent;
import org.bukkit.persistence.PersistentDataContainer;

/**
 * Item metadata slice: display/custom name, lore, damage/max-damage, unbreakable, item
 * flags, custom model data, item model, tooltip style, enchantable, max stack size, food,
 * attribute modifiers, enchantments and persistent data.
 *
 * <p>Tool, equippable, use cooldown, jukebox playable, hide tooltip and other components
 * outside the slice inherit explicit refusals from {@link UnsupportedItemMetaBase}.
 */
public final class FerriteItemMeta extends UnsupportedItemMetaBase implements Damageable {

    private final FerritePersistentDataContainer persistentData = new FerritePersistentDataContainer();
    private final Map<Enchantment, Integer> enchantments = new LinkedHashMap<>();
    private final EnumSet<ItemFlag> itemFlags = EnumSet.noneOf(ItemFlag.class);
    private final Multimap<Attribute, AttributeModifier> attributeModifiers =
            LinkedHashMultimap.create();
    private String displayName;
    private List<String> lore;
    private Integer damage;
    private Integer maxDamage;
    private boolean unbreakable;
    private boolean hideTooltip;
    private FerriteCustomModelDataComponent customModelData;
    private NamespacedKey itemModel;
    private NamespacedKey tooltipStyle;
    private Integer enchantable;
    private Integer maxStackSize;
    private FerriteFoodComponent food;
    private FerriteUseCooldownComponent useCooldown;
    private FerriteJukeboxPlayableComponent jukeboxPlayable;
    private FerriteToolComponent tool;
    private FerriteEquippableComponent equippable;

    public FerriteItemMeta() {
    }

    @Override
    public boolean hasCustomName() {
        return displayName != null;
    }

    @Override
    public Component customName() {
        return displayName == null
                ? null
                : LegacyComponentSerializer.legacySection().deserialize(displayName);
    }

    @Override
    public void customName(Component customName) {
        this.displayName = customName == null
                ? null
                : LegacyComponentSerializer.legacySection().serialize(customName);
    }

    @Override
    public boolean hasDisplayName() {
        return hasCustomName();
    }

    @Override
    public String getDisplayName() {
        return displayName;
    }

    @Override
    public void setDisplayName(String name) {
        this.displayName = name;
    }

    @Override
    public boolean hasLore() {
        return lore != null && !lore.isEmpty();
    }

    @Override
    public List<String> getLore() {
        return lore == null ? null : new ArrayList<>(lore);
    }

    @Override
    public void setLore(List<String> lore) {
        if (lore == null) {
            this.lore = null;
            return;
        }
        List<String> copy = new ArrayList<>(lore.size());
        for (String line : lore) {
            copy.add(Objects.requireNonNull(line, "lore line"));
        }
        this.lore = copy.isEmpty() ? null : copy;
    }

    @Override
    public List<Component> lore() {
        if (lore == null) {
            return null;
        }
        return lore.stream()
                .map(line -> (Component) LegacyComponentSerializer.legacySection().deserialize(line))
                .toList();
    }

    @Override
    public void lore(List<? extends Component> lore) {
        if (lore == null) {
            this.lore = null;
            return;
        }
        List<String> converted = new ArrayList<>(lore.size());
        for (Component line : lore) {
            Objects.requireNonNull(line, "lore line");
            converted.add(LegacyComponentSerializer.legacySection().serialize(line));
        }
        this.lore = converted.isEmpty() ? null : converted;
    }

    @Override
    public boolean hasDamage() {
        return damage != null && damage > 0;
    }

    @Override
    public int getDamage() {
        return damage == null ? 0 : damage;
    }

    @Override
    public void setDamage(int damage) {
        if (damage < 0) {
            throw new IllegalArgumentException("damage must be >= 0, was " + damage);
        }
        this.damage = damage;
    }

    @Override
    public boolean hasDamageValue() {
        return damage != null;
    }

    @Override
    public void resetDamage() {
        this.damage = null;
    }

    @Override
    public boolean hasMaxDamage() {
        return maxDamage != null;
    }

    @Override
    public int getMaxDamage() {
        if (maxDamage == null) {
            throw new IllegalStateException("maxDamage is not set; check hasMaxDamage() first");
        }
        return maxDamage;
    }

    @Override
    public void setMaxDamage(Integer maxDamage) {
        if (maxDamage != null && maxDamage <= 0) {
            throw new IllegalArgumentException("maxDamage must be > 0, was " + maxDamage);
        }
        this.maxDamage = maxDamage;
    }

    @Override
    public boolean hasCustomModelData() {
        return customModelData != null && !customModelData.getFloats().isEmpty();
    }

    @Override
    public int getCustomModelData() {
        if (!hasCustomModelData()) {
            throw new IllegalStateException(
                    "CustomModelData is not set; check hasCustomModelData() first");
        }
        return customModelData.getFloats().get(0).intValue();
    }

    @Override
    public void setCustomModelData(Integer data) {
        if (data == null) {
            if (customModelData != null) {
                customModelData.setFloats(List.of());
                if (customModelData.isEmpty()) {
                    customModelData = null;
                }
            }
            return;
        }
        if (customModelData == null) {
            customModelData = new FerriteCustomModelDataComponent();
        }
        customModelData.setFloats(List.of(data.floatValue()));
    }

    @Override
    public boolean hasCustomModelDataComponent() {
        return customModelData != null && !customModelData.isEmpty();
    }

    @Override
    public CustomModelDataComponent getCustomModelDataComponent() {
        return customModelData == null
                ? new FerriteCustomModelDataComponent()
                : customModelData.copy();
    }

    @Override
    public void setCustomModelDataComponent(CustomModelDataComponent component) {
        if (component == null) {
            this.customModelData = null;
            return;
        }
        FerriteCustomModelDataComponent copy = new FerriteCustomModelDataComponent(component);
        this.customModelData = copy.isEmpty() ? null : copy;
    }

    @Override
    public boolean hasEnchants() {
        return !enchantments.isEmpty();
    }

    @Override
    public boolean hasEnchant(Enchantment ench) {
        Objects.requireNonNull(ench, "enchantment");
        return enchantments.containsKey(ench);
    }

    @Override
    public int getEnchantLevel(Enchantment ench) {
        Objects.requireNonNull(ench, "enchantment");
        return enchantments.getOrDefault(ench, 0);
    }

    @Override
    public Map<Enchantment, Integer> getEnchants() {
        return Collections.unmodifiableMap(new LinkedHashMap<>(enchantments));
    }

    @Override
    public boolean addEnchant(Enchantment ench, int level, boolean ignoreLevelRestriction) {
        Objects.requireNonNull(ench, "enchantment");
        if (!ignoreLevelRestriction
                && (level < ench.getStartLevel() || level > ench.getMaxLevel())) {
            return false;
        }
        enchantments.put(ench, level);
        return true;
    }

    @Override
    public boolean removeEnchant(Enchantment ench) {
        Objects.requireNonNull(ench, "enchantment");
        return enchantments.remove(ench) != null;
    }

    @Override
    public void removeEnchantments() {
        enchantments.clear();
    }

    @Override
    public boolean hasConflictingEnchant(Enchantment ench) {
        Objects.requireNonNull(ench, "enchantment");
        for (Enchantment existing : enchantments.keySet()) {
            if (!existing.equals(ench) && existing.conflictsWith(ench)) {
                return true;
            }
        }
        return false;
    }

    @Override
    public boolean isUnbreakable() {
        return unbreakable;
    }

    @Override
    public void setUnbreakable(boolean unbreakable) {
        this.unbreakable = unbreakable;
    }

    @Override
    public void addItemFlags(ItemFlag... itemFlags) {
        Objects.requireNonNull(itemFlags, "itemFlags");
        for (ItemFlag flag : itemFlags) {
            this.itemFlags.add(Objects.requireNonNull(flag, "itemFlag"));
        }
    }

    @Override
    public void removeItemFlags(ItemFlag... itemFlags) {
        Objects.requireNonNull(itemFlags, "itemFlags");
        for (ItemFlag flag : itemFlags) {
            this.itemFlags.remove(Objects.requireNonNull(flag, "itemFlag"));
        }
    }

    @Override
    public Set<ItemFlag> getItemFlags() {
        return itemFlags.isEmpty() ? Set.of() : Collections.unmodifiableSet(EnumSet.copyOf(itemFlags));
    }

    @Override
    public boolean hasItemFlag(ItemFlag flag) {
        Objects.requireNonNull(flag, "flag");
        return itemFlags.contains(flag);
    }

    @Override
    public boolean hasItemModel() {
        return itemModel != null;
    }

    @Override
    public NamespacedKey getItemModel() {
        return itemModel;
    }

    @Override
    public void setItemModel(NamespacedKey itemModel) {
        this.itemModel = itemModel;
    }

    @Override
    public boolean hasTooltipStyle() {
        return tooltipStyle != null;
    }

    @Override
    public NamespacedKey getTooltipStyle() {
        return tooltipStyle;
    }

    @Override
    public void setTooltipStyle(NamespacedKey tooltipStyle) {
        this.tooltipStyle = tooltipStyle;
    }

    @Override
    public boolean hasEnchantable() {
        return enchantable != null;
    }

    @Override
    public int getEnchantable() {
        if (enchantable == null) {
            throw new IllegalStateException("enchantable is not set; check hasEnchantable() first");
        }
        return enchantable;
    }

    @Override
    public void setEnchantable(Integer enchantable) {
        if (enchantable != null && enchantable <= 0) {
            throw new IllegalArgumentException("enchantable must be > 0, was " + enchantable);
        }
        this.enchantable = enchantable;
    }

    @Override
    public boolean hasMaxStackSize() {
        return maxStackSize != null;
    }

    @Override
    public int getMaxStackSize() {
        if (maxStackSize == null) {
            throw new IllegalStateException(
                    "maxStackSize is not set; check hasMaxStackSize() first");
        }
        return maxStackSize;
    }

    @Override
    public void setMaxStackSize(Integer maxStackSize) {
        if (maxStackSize != null && (maxStackSize < 1 || maxStackSize > 99)) {
            throw new IllegalArgumentException(
                    "maxStackSize must be between 1 and 99, was " + maxStackSize);
        }
        this.maxStackSize = maxStackSize;
    }

    @Override
    public boolean hasFood() {
        return food != null;
    }

    @Override
    public FoodComponent getFood() {
        return food == null ? new FerriteFoodComponent() : food.copy();
    }

    @Override
    public void setFood(FoodComponent food) {
        this.food = food == null ? null : new FerriteFoodComponent(food);
    }

    @Override
    public boolean isHideTooltip() {
        return hideTooltip;
    }

    @Override
    public void setHideTooltip(boolean hideTooltip) {
        this.hideTooltip = hideTooltip;
    }

    @Override
    public boolean hasUseCooldown() {
        return useCooldown != null;
    }

    @Override
    public UseCooldownComponent getUseCooldown() {
        return useCooldown == null ? new FerriteUseCooldownComponent() : useCooldown.copy();
    }

    @Override
    public void setUseCooldown(UseCooldownComponent cooldown) {
        this.useCooldown = cooldown == null ? null : new FerriteUseCooldownComponent(cooldown);
    }

    @Override
    public boolean hasJukeboxPlayable() {
        return jukeboxPlayable != null;
    }

    @Override
    public JukeboxPlayableComponent getJukeboxPlayable() {
        return jukeboxPlayable == null
                ? new FerriteJukeboxPlayableComponent()
                : jukeboxPlayable.copy();
    }

    @Override
    public void setJukeboxPlayable(JukeboxPlayableComponent jukeboxPlayable) {
        this.jukeboxPlayable = jukeboxPlayable == null
                ? null
                : new FerriteJukeboxPlayableComponent(jukeboxPlayable);
    }

    @Override
    public boolean hasTool() {
        return tool != null;
    }

    @Override
    public ToolComponent getTool() {
        return tool == null ? new FerriteToolComponent() : tool.copy();
    }

    @Override
    public void setTool(ToolComponent tool) {
        this.tool = tool == null ? null : new FerriteToolComponent(tool);
    }

    @Override
    public boolean hasEquippable() {
        return equippable != null;
    }

    @Override
    public EquippableComponent getEquippable() {
        return equippable == null ? new FerriteEquippableComponent() : equippable.copy();
    }

    @Override
    public void setEquippable(EquippableComponent equippable) {
        this.equippable = equippable == null ? null : new FerriteEquippableComponent(equippable);
    }

    @Override
    public boolean hasAttributeModifiers() {
        return !attributeModifiers.isEmpty();
    }

    @Override
    public Multimap<Attribute, AttributeModifier> getAttributeModifiers() {
        return attributeModifiers.isEmpty() ? null : LinkedHashMultimap.create(attributeModifiers);
    }

    @Override
    public Multimap<Attribute, AttributeModifier> getAttributeModifiers(EquipmentSlot slot) {
        Objects.requireNonNull(slot, "slot");
        Multimap<Attribute, AttributeModifier> result = LinkedHashMultimap.create();
        for (Map.Entry<Attribute, AttributeModifier> entry : attributeModifiers.entries()) {
            if (entry.getValue().getSlotGroup().test(slot)) {
                result.put(entry.getKey(), entry.getValue());
            }
        }
        return result;
    }

    @Override
    public Collection<AttributeModifier> getAttributeModifiers(Attribute attribute) {
        Objects.requireNonNull(attribute, "attribute");
        return attributeModifiers.containsKey(attribute)
                ? List.copyOf(attributeModifiers.get(attribute))
                : null;
    }

    @Override
    public boolean addAttributeModifier(Attribute attribute, AttributeModifier modifier) {
        Objects.requireNonNull(attribute, "attribute");
        Objects.requireNonNull(modifier, "modifier");
        return attributeModifiers.put(attribute, modifier);
    }

    @Override
    public void setAttributeModifiers(
            Multimap<Attribute, AttributeModifier> attributeModifiers) {
        this.attributeModifiers.clear();
        if (attributeModifiers != null) {
            for (Map.Entry<Attribute, AttributeModifier> entry : attributeModifiers.entries()) {
                this.attributeModifiers.put(
                        Objects.requireNonNull(entry.getKey(), "attribute"),
                        Objects.requireNonNull(entry.getValue(), "modifier"));
            }
        }
    }

    @Override
    public boolean removeAttributeModifier(Attribute attribute) {
        Objects.requireNonNull(attribute, "attribute");
        return !attributeModifiers.removeAll(attribute).isEmpty();
    }

    @Override
    public boolean removeAttributeModifier(EquipmentSlot slot) {
        Objects.requireNonNull(slot, "slot");
        boolean removed = false;
        Iterator<Map.Entry<Attribute, AttributeModifier>> iterator =
                attributeModifiers.entries().iterator();
        while (iterator.hasNext()) {
            AttributeModifier modifier = iterator.next().getValue();
            if (slot.getGroup().equals(modifier.getSlotGroup()) || modifier.getSlot() == slot) {
                iterator.remove();
                removed = true;
            }
        }
        return removed;
    }

    @Override
    public boolean removeAttributeModifier(Attribute attribute, AttributeModifier modifier) {
        Objects.requireNonNull(attribute, "attribute");
        Objects.requireNonNull(modifier, "modifier");
        return attributeModifiers.remove(attribute, modifier);
    }

    public Multimap<Attribute, AttributeModifier> attributeModifiersMap() {
        return attributeModifiers;
    }

    @Override
    public PersistentDataContainer getPersistentDataContainer() {
        return persistentData;
    }

    public FerritePersistentDataContainer persistentData() {
        return persistentData;
    }

    @Override
    public Damageable clone() {
        FerriteItemMeta copy = new FerriteItemMeta();
        copy.displayName = displayName;
        copy.lore = lore == null ? null : new ArrayList<>(lore);
        copy.damage = damage;
        copy.maxDamage = maxDamage;
        copy.unbreakable = unbreakable;
        copy.hideTooltip = hideTooltip;
        copy.itemFlags.addAll(itemFlags);
        copy.customModelData = customModelData == null ? null : customModelData.copy();
        copy.itemModel = itemModel;
        copy.tooltipStyle = tooltipStyle;
        copy.enchantable = enchantable;
        copy.maxStackSize = maxStackSize;
        copy.food = food == null ? null : food.copy();
        copy.useCooldown = useCooldown == null ? null : useCooldown.copy();
        copy.jukeboxPlayable = jukeboxPlayable == null ? null : jukeboxPlayable.copy();
        copy.tool = tool == null ? null : tool.copy();
        copy.equippable = equippable == null ? null : equippable.copy();
        copy.attributeModifiers.putAll(attributeModifiers);
        copy.enchantments.putAll(enchantments);
        copy.persistentData.copyFrom(persistentData);
        return copy;
    }

    /** Facade helper: no supported metadata or PDC entries are set. */
    public boolean isEmpty() {
        return displayName == null
                && (lore == null || lore.isEmpty())
                && damage == null
                && maxDamage == null
                && !unbreakable
                && !hideTooltip
                && itemFlags.isEmpty()
                && (customModelData == null || customModelData.isEmpty())
                && itemModel == null
                && tooltipStyle == null
                && enchantable == null
                && maxStackSize == null
                && food == null
                && useCooldown == null
                && jukeboxPlayable == null
                && tool == null
                && equippable == null
                && attributeModifiers.isEmpty()
                && enchantments.isEmpty()
                && persistentData.isEmpty();
    }

    @Override
    public Map<String, Object> serialize() {
        Map<String, Object> result = new LinkedHashMap<>();
        if (displayName != null) {
            result.put("display-name", displayName);
        }
        if (lore != null) {
            result.put("lore", new ArrayList<>(lore));
        }
        if (damage != null) {
            result.put("damage", damage);
        }
        if (maxDamage != null) {
            result.put("max-damage", maxDamage);
        }
        if (unbreakable) {
            result.put("unbreakable", true);
        }
        if (hideTooltip) {
            result.put("hide-tooltip", true);
        }
        if (!itemFlags.isEmpty()) {
            result.put("item-flags", itemFlags.stream().map(ItemFlag::name).toList());
        }
        if (customModelData != null && !customModelData.isEmpty()) {
            result.put("custom-model-data", customModelData.serialize());
        }
        if (itemModel != null) {
            result.put("item-model", itemModel.toString());
        }
        if (tooltipStyle != null) {
            result.put("tooltip-style", tooltipStyle.toString());
        }
        if (enchantable != null) {
            result.put("enchantable", enchantable);
        }
        if (maxStackSize != null) {
            result.put("max-stack-size", maxStackSize);
        }
        if (food != null) {
            result.put("food", food.serialize());
        }
        if (useCooldown != null) {
            result.put("use-cooldown", useCooldown.serialize());
        }
        if (jukeboxPlayable != null) {
            result.put("jukebox-playable", jukeboxPlayable.serialize());
        }
        if (tool != null) {
            result.put("tool", tool.serialize());
        }
        if (equippable != null) {
            result.put("equippable", equippable.serialize());
        }
        if (!attributeModifiers.isEmpty()) {
            List<Map<String, Object>> modList = new ArrayList<>(attributeModifiers.size());
            for (Map.Entry<Attribute, AttributeModifier> entry : attributeModifiers.entries()) {
                Map<String, Object> modEntry = new LinkedHashMap<>();
                modEntry.put("attribute", entry.getKey().getKey().toString());
                modEntry.put("modifier", entry.getValue().serialize());
                modList.add(modEntry);
            }
            result.put("attribute-modifiers", modList);
        }
        if (!enchantments.isEmpty()) {
            Map<String, Integer> enchMap = new LinkedHashMap<>();
            for (Map.Entry<Enchantment, Integer> entry : enchantments.entrySet()) {
                enchMap.put(entry.getKey().getKey().toString(), entry.getValue());
            }
            result.put("enchants", enchMap);
        }
        if (!persistentData.isEmpty()) {
            result.put("persistent-data", persistentData.serialize());
        }
        return result;
    }

    public static FerriteItemMeta deserialize(Map<String, Object> args) {
        FerriteItemMeta meta = new FerriteItemMeta();
        if (args == null) {
            return meta;
        }
        Object name = args.get("display-name");
        if (name != null) {
            meta.setDisplayName(String.valueOf(name));
        }
        Object lore = args.get("lore");
        if (lore instanceof List<?> list) {
            meta.setLore(list.stream().map(String::valueOf).toList());
        }
        Object damage = args.get("damage");
        if (damage instanceof Number number) {
            meta.setDamage(number.intValue());
        }
        Object maxDamage = args.get("max-damage");
        if (maxDamage instanceof Number number) {
            meta.setMaxDamage(number.intValue());
        }
        Object unbreakable = args.get("unbreakable");
        if (unbreakable instanceof Boolean flag && flag) {
            meta.setUnbreakable(true);
        }
        Object hideTooltip = args.get("hide-tooltip");
        if (hideTooltip instanceof Boolean flag && flag) {
            meta.setHideTooltip(true);
        }
        Object flags = args.get("item-flags");
        if (flags instanceof Collection<?> list) {
            for (Object entry : list) {
                if (entry instanceof ItemFlag itemFlag) {
                    meta.addItemFlags(itemFlag);
                } else {
                    meta.addItemFlags(ItemFlag.valueOf(String.valueOf(entry)));
                }
            }
        }
        Object cmd = args.get("custom-model-data");
        if (cmd instanceof Map<?, ?> map) {
            meta.setCustomModelDataComponent(FerriteCustomModelDataComponent.deserialize(map));
        } else if (cmd instanceof Number number) {
            meta.setCustomModelData(number.intValue());
        }
        Object itemModel = args.get("item-model");
        if (itemModel != null) {
            NamespacedKey key = NamespacedKey.fromString(String.valueOf(itemModel));
            if (key == null) {
                throw new IllegalArgumentException("Invalid item-model key: " + itemModel);
            }
            meta.setItemModel(key);
        }
        Object tooltipStyle = args.get("tooltip-style");
        if (tooltipStyle != null) {
            NamespacedKey key = NamespacedKey.fromString(String.valueOf(tooltipStyle));
            if (key == null) {
                throw new IllegalArgumentException("Invalid tooltip-style key: " + tooltipStyle);
            }
            meta.setTooltipStyle(key);
        }
        Object enchantable = args.get("enchantable");
        if (enchantable instanceof Number number) {
            meta.setEnchantable(number.intValue());
        }
        Object maxStackSize = args.get("max-stack-size");
        if (maxStackSize instanceof Number number) {
            meta.setMaxStackSize(number.intValue());
        }
        Object food = args.get("food");
        if (food instanceof Map<?, ?> map) {
            meta.setFood(FerriteFoodComponent.deserialize(map));
        }
        Object useCooldown = args.get("use-cooldown");
        if (useCooldown instanceof Map<?, ?> map) {
            meta.setUseCooldown(FerriteUseCooldownComponent.deserialize(map));
        }
        Object jukeboxPlayable = args.get("jukebox-playable");
        if (jukeboxPlayable instanceof Map<?, ?> map) {
            meta.setJukeboxPlayable(FerriteJukeboxPlayableComponent.deserialize(map));
        }
        Object tool = args.get("tool");
        if (tool instanceof Map<?, ?> map) {
            meta.setTool(FerriteToolComponent.deserialize(map));
        }
        Object equippable = args.get("equippable");
        if (equippable instanceof Map<?, ?> map) {
            meta.setEquippable(FerriteEquippableComponent.deserialize(map));
        }
        Object attrMods = args.get("attribute-modifiers");
        if (attrMods instanceof List<?> list && !list.isEmpty()) {
            Registry<Attribute> registry = Registry.ATTRIBUTE;
            for (Object rawEntry : list) {
                if (!(rawEntry instanceof Map<?, ?> entryMap)) {
                    throw new IllegalArgumentException("Expected map in attribute-modifiers");
                }
                NamespacedKey key =
                        NamespacedKey.fromString(String.valueOf(entryMap.get("attribute")));
                if (key == null) {
                    throw new IllegalArgumentException(
                            "Invalid attribute key: " + entryMap.get("attribute"));
                }
                Attribute attribute = registry == null ? null : registry.get(key);
                if (attribute == null) {
                    throw new UnsupportedOperationException(
                            "ItemMeta#deserialize: cannot resolve attribute '"
                                    + key
                                    + "' because the attribute registry is empty");
                }
                @SuppressWarnings("unchecked")
                Map<String, Object> modMap = (Map<String, Object>) entryMap.get("modifier");
                meta.addAttributeModifier(attribute, AttributeModifier.deserialize(modMap));
            }
        }
        Object enchants = args.get("enchants");
        if (enchants instanceof Map<?, ?> map && !map.isEmpty()) {
            Registry<Enchantment> registry = Registry.ENCHANTMENT;
            for (Map.Entry<?, ?> entry : map.entrySet()) {
                NamespacedKey key = NamespacedKey.fromString(String.valueOf(entry.getKey()));
                if (key == null) {
                    throw new IllegalArgumentException(
                            "Invalid enchantment key: " + entry.getKey());
                }
                Enchantment enchantment = registry == null ? null : registry.get(key);
                if (enchantment == null) {
                    throw new UnsupportedOperationException(
                            "ItemMeta#deserialize: cannot resolve enchantment '"
                                    + key
                                    + "' from the vanilla registry");
                }
                int level = ((Number) entry.getValue()).intValue();
                meta.addEnchant(enchantment, level, true);
            }
        }
        Object pdc = args.get("persistent-data");
        if (pdc instanceof Map<?, ?> map) {
            meta.persistentData.copyFrom(FerritePersistentDataContainer.deserialize(map));
        }
        return meta;
    }

    @Override
    public boolean equals(Object object) {
        if (this == object) {
            return true;
        }
        if (!(object instanceof FerriteItemMeta other)) {
            return false;
        }
        return Objects.equals(displayName, other.displayName)
                && Objects.equals(lore, other.lore)
                && Objects.equals(damage, other.damage)
                && Objects.equals(maxDamage, other.maxDamage)
                && unbreakable == other.unbreakable
                && hideTooltip == other.hideTooltip
                && Objects.equals(itemFlags, other.itemFlags)
                && Objects.equals(customModelData, other.customModelData)
                && Objects.equals(itemModel, other.itemModel)
                && Objects.equals(tooltipStyle, other.tooltipStyle)
                && Objects.equals(enchantable, other.enchantable)
                && Objects.equals(maxStackSize, other.maxStackSize)
                && Objects.equals(food, other.food)
                && Objects.equals(useCooldown, other.useCooldown)
                && Objects.equals(jukeboxPlayable, other.jukeboxPlayable)
                && Objects.equals(tool, other.tool)
                && Objects.equals(equippable, other.equippable)
                && Objects.equals(attributeModifiers, other.attributeModifiers)
                && Objects.equals(enchantments, other.enchantments)
                && Objects.equals(persistentData, other.persistentData);
    }

    @Override
    public int hashCode() {
        return Objects.hash(
                displayName,
                lore,
                damage,
                maxDamage,
                unbreakable,
                hideTooltip,
                itemFlags,
                customModelData,
                itemModel,
                tooltipStyle,
                enchantable,
                maxStackSize,
                food,
                useCooldown,
                jukeboxPlayable,
                tool,
                equippable,
                attributeModifiers,
                enchantments,
                persistentData);
    }

    @Override
    public String toString() {
        return "FerriteItemMeta{"
                + "displayName=" + displayName
                + ", lore=" + lore
                + ", damage=" + damage
                + ", maxDamage=" + maxDamage
                + ", unbreakable=" + unbreakable
                + ", itemFlags=" + itemFlags
                + ", customModelData=" + customModelData
                + ", itemModel=" + itemModel
                + ", tooltipStyle=" + tooltipStyle
                + ", enchantable=" + enchantable
                + ", maxStackSize=" + maxStackSize
                + ", food=" + food
                + ", attributeModifiers=" + attributeModifiers
                + ", enchantments=" + enchantments
                + ", persistentDataSize=" + persistentData.getSize()
                + "}";
    }
}
