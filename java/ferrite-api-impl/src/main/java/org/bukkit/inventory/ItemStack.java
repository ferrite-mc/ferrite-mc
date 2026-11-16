package org.bukkit.inventory;

import dev.ferrite.api.item.FerriteCustomModelDataComponent;
import dev.ferrite.api.item.FerriteDataComponents;
import dev.ferrite.api.item.FerriteEquippableComponent;
import dev.ferrite.api.item.FerriteFoodComponent;
import dev.ferrite.api.item.FerriteItemMeta;
import dev.ferrite.api.item.FerriteJukeboxPlayableComponent;
import dev.ferrite.api.item.FerritePersistentDataContainerView;
import dev.ferrite.api.item.FerriteToolComponent;
import dev.ferrite.api.item.FerriteUseCooldownComponent;
import io.papermc.paper.datacomponent.DataComponentBuilder;
import io.papermc.paper.datacomponent.DataComponentHolder;
import io.papermc.paper.datacomponent.DataComponentType;
import io.papermc.paper.datacomponent.item.CustomModelData;
import io.papermc.paper.datacomponent.item.Enchantable;
import io.papermc.paper.datacomponent.item.Equippable;
import io.papermc.paper.datacomponent.item.FoodProperties;
import io.papermc.paper.datacomponent.item.ItemAttributeModifiers;
import io.papermc.paper.datacomponent.item.ItemEnchantments;
import io.papermc.paper.datacomponent.item.ItemLore;
import io.papermc.paper.datacomponent.item.JukeboxPlayable;
import io.papermc.paper.datacomponent.item.Tool;
import io.papermc.paper.datacomponent.item.TooltipDisplay;
import io.papermc.paper.datacomponent.item.UseCooldown;
import io.papermc.paper.persistence.PersistentDataContainerView;
import io.papermc.paper.persistence.PersistentDataViewHolder;
import io.papermc.paper.registry.TypedKey;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import java.util.function.Consumer;
import java.util.function.Predicate;
import net.kyori.adventure.key.Key;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.event.HoverEvent;
import net.kyori.adventure.text.event.HoverEventSource;
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer;
import org.bukkit.JukeboxSong;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.Registry;
import org.bukkit.Sound;
import org.bukkit.Translatable;
import org.bukkit.attribute.Attribute;
import org.bukkit.attribute.AttributeModifier;
import org.bukkit.block.BlockType;
import org.bukkit.configuration.serialization.ConfigurationSerializable;
import org.bukkit.enchantments.Enchantment;
import org.bukkit.entity.EntityType;
import org.bukkit.inventory.meta.Damageable;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.inventory.meta.components.ToolComponent;
import org.bukkit.persistence.PersistentDataContainer;

/**
 * Ferrite's implementation of the Bukkit item type.
 *
 * <p>This class deliberately shadows Paper's API {@code ItemStack}. Paper's own class
 * delegates every accessor to a {@code craftDelegate} field that only a CraftBukkit
 * server installs, so it cannot be constructed or read without one. Ferrite has no
 * CraftBukkit, so the facade provides the class with the API's binary name and implements
 * the supported slice: material, amount, display name, lore, damage/durability, custom
 * model data, enchantments and persistent data.
 *
 * <p>Because the class shadows the API jar, the {@code ferrite-api-impl} jar must come
 * before {@code paper-api} on the host classpath. That ordering requirement is part of the
 * host integration contract documented in {@code docs/handoff/plugin-b-lane-2-api.md}.
 */
public class ItemStack
        implements Cloneable,
                ConfigurationSerializable,
                Translatable,
                HoverEventSource<HoverEvent.ShowItem>,
                PersistentDataViewHolder,
                DataComponentHolder {

    private static final String KEY_CUSTOM_NAME = "minecraft:custom_name";
    private static final String KEY_LORE = "minecraft:lore";
    private static final String KEY_DAMAGE = "minecraft:damage";
    private static final String KEY_MAX_DAMAGE = "minecraft:max_damage";
    private static final String KEY_UNBREAKABLE = "minecraft:unbreakable";
    private static final String KEY_CUSTOM_MODEL_DATA = "minecraft:custom_model_data";
    private static final String KEY_ENCHANTMENTS = "minecraft:enchantments";
    private static final String KEY_MAX_STACK_SIZE = "minecraft:max_stack_size";
    private static final String KEY_ITEM_MODEL = "minecraft:item_model";
    private static final String KEY_TOOLTIP_STYLE = "minecraft:tooltip_style";
    private static final String KEY_ENCHANTABLE = "minecraft:enchantable";
    private static final String KEY_FOOD = "minecraft:food";
    private static final String KEY_ATTRIBUTE_MODIFIERS = "minecraft:attribute_modifiers";
    private static final String KEY_TOOLTIP_DISPLAY = "minecraft:tooltip_display";
    private static final String KEY_USE_COOLDOWN = "minecraft:use_cooldown";
    private static final String KEY_JUKEBOX_PLAYABLE = "minecraft:jukebox_playable";
    private static final String KEY_TOOL = "minecraft:tool";
    private static final String KEY_EQUIPPABLE = "minecraft:equippable";

    private Material type;
    private int amount;
    private FerriteItemMeta meta;
    private final Map<String, DataComponentType> knownComponentTypes = new LinkedHashMap<>();

    private static RuntimeException unsupported(String method) {
        return dev.ferrite.api.internal.Unsupported.exception("ItemStack#" + method);
    }

    private RuntimeException unsupportedComponent(DataComponentType componentType, String method) {
        String key = componentType == null ? "null" : componentType.key().asString();
        return dev.ferrite.api.internal.Unsupported.exception(
                "ItemStack#"
                        + method
                        + " for item "
                        + this.type.name()
                        + " and component '"
                        + key
                        + "': component is not supported by the Ferrite item slice");
    }

    protected ItemStack() {
        this(Material.AIR, 0);
    }

    public ItemStack(Material type) {
        this(type, 1);
    }

    public ItemStack(Material type, int amount) {
        if (type == null) {
            throw new IllegalArgumentException("Material cannot be null");
        }
        if (amount < 0) {
            throw new IllegalArgumentException("amount must be >= 0, was " + amount);
        }
        this.type = type;
        this.amount = amount;
    }

    public ItemStack(Material type, int amount, short durability) {
        this(type, amount);
        if (durability < 0) {
            throw new IllegalArgumentException("durability must be >= 0, was " + durability);
        }
        if (durability > 0 && type != Material.AIR) {
            FerriteItemMeta initialMeta = new FerriteItemMeta();
            initialMeta.setDamage(durability);
            this.meta = initialMeta;
        }
    }

    public ItemStack(Material type, int amount, short durability, Byte data) {
        this(type, amount, durability);
        if (data != null) {
            throw dev.ferrite.api.internal.Unsupported.exception(
                    "ItemStack(Material, int, short, Byte): legacy material data is not stored");
        }
    }

    public ItemStack(ItemStack stack) {
        Objects.requireNonNull(stack, "ItemStack cannot be null");
        this.type = stack.type;
        this.amount = stack.amount;
        this.meta = stack.meta == null || stack.meta.isEmpty()
                ? null
                : (FerriteItemMeta) stack.meta.clone();
        this.knownComponentTypes.putAll(stack.knownComponentTypes);
    }

    public static ItemStack of(Material type) {
        return new ItemStack(type);
    }

    public static ItemStack of(Material type, int amount) {
        return new ItemStack(type, amount);
    }

    public static ItemStack empty() {
        return new ItemStack(Material.AIR, 0);
    }

    public Material getType() {
        return this.type;
    }

    public void setType(Material type) {
        this.type = Objects.requireNonNull(type, "Material cannot be null");
        if (this.type == Material.AIR) {
            this.meta = null;
            this.knownComponentTypes.clear();
        }
    }

    public ItemStack withType(Material type) {
        ItemStack copy = clone();
        copy.setType(type);
        return copy;
    }

    public int getAmount() {
        return this.amount;
    }

    public void setAmount(int amount) {
        if (amount < 0) {
            throw new IllegalArgumentException("amount must be >= 0, was " + amount);
        }
        this.amount = amount;
    }

    public boolean isEmpty() {
        return this.type == Material.AIR || this.amount <= 0;
    }

    public int getMaxStackSize() {
        if (this.meta != null && this.meta.hasMaxStackSize()) {
            return this.meta.getMaxStackSize();
        }
        // Material#getMaxStackSize asks the Paper item registry, which needs a
        // RegistryAccess only CraftBukkit installs. The facade keeps a name-based table
        // for the first slice; a host registry adapter can replace this later.
        String name = this.type.name();
        if (name.equals("AIR")) {
            return 64;
        }
        if (name.endsWith("_SWORD")
                || name.endsWith("_PICKAXE")
                || name.endsWith("_AXE")
                || name.endsWith("_SHOVEL")
                || name.endsWith("_HOE")
                || name.endsWith("_HELMET")
                || name.endsWith("_CHESTPLATE")
                || name.endsWith("_LEGGINGS")
                || name.endsWith("_BOOTS")
                || name.equals("SHIELD")
                || name.equals("ELYTRA")) {
            return 1;
        }
        return switch (this.type) {
            case ENDER_PEARL, EGG, SNOWBALL, OAK_SIGN, OAK_HANGING_SIGN, ARMOR_STAND, WHITE_BANNER,
                    WATER_BUCKET, LAVA_BUCKET, MILK_BUCKET, POWDER_SNOW_BUCKET, BUCKET -> 16;
            default -> 64;
        };
    }

    public ItemMeta getItemMeta() {
        if (this.type == Material.AIR) {
            return null;
        }
        if (this.meta == null) {
            return new FerriteItemMeta();
        }
        return this.meta.clone();
    }

    public boolean setItemMeta(ItemMeta meta) {
        if (this.type == Material.AIR) {
            return false;
        }
        if (meta == null) {
            this.meta = null;
            this.knownComponentTypes.clear();
            return true;
        }
        if (!(meta instanceof FerriteItemMeta ferrite)) {
            throw new IllegalArgumentException(
                    "Unsupported ItemMeta implementation: " + meta.getClass().getName());
        }
        this.meta = ferrite.isEmpty() ? null : (FerriteItemMeta) ferrite.clone();
        if (this.meta == null) {
            this.knownComponentTypes.clear();
        }
        return true;
    }

    public boolean hasItemMeta() {
        return this.type != Material.AIR && this.meta != null && !this.meta.isEmpty();
    }

    public boolean editMeta(Consumer<? super ItemMeta> consumer) {
        Objects.requireNonNull(consumer, "consumer");
        ItemMeta current = getItemMeta();
        if (current == null) {
            return false;
        }
        consumer.accept(current);
        return setItemMeta(current);
    }

    public <M extends ItemMeta> boolean editMeta(Class<M> clazz, Consumer<? super M> consumer) {
        Objects.requireNonNull(clazz, "clazz");
        Objects.requireNonNull(consumer, "consumer");
        ItemMeta current = getItemMeta();
        if (current == null || !clazz.isInstance(current)) {
            return false;
        }
        consumer.accept(clazz.cast(current));
        return setItemMeta(current);
    }

    public List<String> getLore() {
        if (this.meta == null || !this.meta.hasLore()) {
            return null;
        }
        return this.meta.getLore();
    }

    public void setLore(List<String> lore) {
        editMeta(current -> current.setLore(lore));
    }

    public List<Component> lore() {
        if (this.meta == null || !this.meta.hasLore()) {
            return null;
        }
        return this.meta.lore();
    }

    public void lore(List<? extends Component> lore) {
        editMeta(current -> current.lore(lore));
    }

    public Component displayName() {
        FerriteItemMeta current = this.meta;
        if (current != null && current.hasDisplayName()) {
            return LegacyComponentSerializer.legacySection().deserialize(current.getDisplayName());
        }
        return Component.translatable(getTranslationKey());
    }

    public String getTranslationKey() {
        return this.type.getTranslationKey();
    }

    public short getDurability() {
        return (short) (this.meta == null ? 0 : this.meta.getDamage());
    }

    public void setDurability(short durability) {
        if (durability < 0) {
            throw new IllegalArgumentException("durability must be >= 0, was " + durability);
        }
        if (this.type == Material.AIR) {
            return;
        }
        if (durability == 0) {
            if (this.meta != null) {
                editMeta(m -> ((Damageable) m).resetDamage());
            }
            return;
        }
        editMeta(m -> ((Damageable) m).setDamage(durability));
    }

    public boolean containsEnchantment(Enchantment ench) {
        Objects.requireNonNull(ench, "enchantment");
        return this.meta != null && this.meta.hasEnchant(ench);
    }

    public int getEnchantmentLevel(Enchantment ench) {
        Objects.requireNonNull(ench, "enchantment");
        return this.meta == null ? 0 : this.meta.getEnchantLevel(ench);
    }

    public Map<Enchantment, Integer> getEnchantments() {
        return this.meta == null ? Collections.emptyMap() : this.meta.getEnchants();
    }

    public void addEnchantments(Map<Enchantment, Integer> enchantments) {
        Objects.requireNonNull(enchantments, "enchantments");
        for (Map.Entry<Enchantment, Integer> entry : enchantments.entrySet()) {
            addEnchantment(entry.getKey(), entry.getValue());
        }
    }

    public void addEnchantment(Enchantment ench, int level) {
        Objects.requireNonNull(ench, "enchantment");
        if (level < ench.getStartLevel() || level > ench.getMaxLevel()) {
            throw new IllegalArgumentException(
                    "Enchantment level "
                            + level
                            + " is outside ["
                            + ench.getStartLevel()
                            + ", "
                            + ench.getMaxLevel()
                            + "]");
        }
        if (!ench.canEnchantItem(this)) {
            throw new IllegalArgumentException(
                    "Enchantment " + ench.getKey() + " cannot be applied to " + this.type);
        }
        if (this.meta != null && this.meta.hasConflictingEnchant(ench)) {
            throw new IllegalArgumentException(
                    "Enchantment " + ench.getKey() + " conflicts with an existing enchantment");
        }
        addUnsafeEnchantment(ench, level);
    }

    public void addUnsafeEnchantments(Map<Enchantment, Integer> enchantments) {
        Objects.requireNonNull(enchantments, "enchantments");
        for (Map.Entry<Enchantment, Integer> entry : enchantments.entrySet()) {
            addUnsafeEnchantment(entry.getKey(), entry.getValue());
        }
    }

    public void addUnsafeEnchantment(Enchantment ench, int level) {
        Objects.requireNonNull(ench, "enchantment");
        editMeta(current -> current.addEnchant(ench, level, true));
    }

    public int removeEnchantment(Enchantment ench) {
        Objects.requireNonNull(ench, "enchantment");
        if (this.meta == null) {
            return 0;
        }
        int previous = this.meta.getEnchantLevel(ench);
        if (previous != 0 || this.meta.hasEnchant(ench)) {
            editMeta(current -> current.removeEnchant(ench));
        }
        return previous;
    }

    public void removeEnchantments() {
        if (this.meta != null && this.meta.hasEnchants()) {
            editMeta(ItemMeta::removeEnchantments);
        }
    }

    public PersistentDataContainerView getPersistentDataContainer() {
        return new FerritePersistentDataContainerView(
                this.meta == null ? null : this.meta.persistentData());
    }

    public boolean editPersistentDataContainer(Consumer<PersistentDataContainer> consumer) {
        Objects.requireNonNull(consumer, "consumer");
        ItemMeta current = getItemMeta();
        if (current == null) {
            return false;
        }
        consumer.accept(current.getPersistentDataContainer());
        return setItemMeta(current);
    }

    @Override
    @SuppressWarnings("unchecked")
    public <T> T getData(DataComponentType.Valued<T> type) {
        String key = requireSupportedComponent(type, "getData");
        if (this.meta == null) {
            return null;
        }
        return switch (key) {
            case KEY_CUSTOM_NAME -> (T) this.meta.customName();
            case KEY_LORE -> this.meta.hasLore()
                    ? (T) FerriteDataComponents.lore(this.meta.lore())
                    : null;
            case KEY_DAMAGE -> this.meta.hasDamageValue()
                    ? (T) Integer.valueOf(this.meta.getDamage())
                    : null;
            case KEY_MAX_DAMAGE -> this.meta.hasMaxDamage()
                    ? (T) Integer.valueOf(this.meta.getMaxDamage())
                    : null;
            case KEY_CUSTOM_MODEL_DATA -> this.meta.hasCustomModelDataComponent()
                    ? (T) FerriteDataComponents.customModelData(
                            (FerriteCustomModelDataComponent)
                                    this.meta.getCustomModelDataComponent())
                    : null;
            case KEY_ENCHANTMENTS -> this.meta.hasEnchants()
                    ? (T) FerriteDataComponents.enchantments(this.meta.getEnchants())
                    : null;
            case KEY_MAX_STACK_SIZE -> this.meta.hasMaxStackSize()
                    ? (T) Integer.valueOf(this.meta.getMaxStackSize())
                    : null;
            case KEY_ITEM_MODEL -> (T) this.meta.getItemModel();
            case KEY_TOOLTIP_STYLE -> (T) this.meta.getTooltipStyle();
            case KEY_ENCHANTABLE -> this.meta.hasEnchantable()
                    ? (T) FerriteDataComponents.enchantable(this.meta.getEnchantable())
                    : null;
            case KEY_FOOD -> this.meta.hasFood()
                    ? (T) FerriteDataComponents.foodProperties(
                            (FerriteFoodComponent) this.meta.getFood())
                    : null;
            case KEY_ATTRIBUTE_MODIFIERS -> this.meta.hasAttributeModifiers()
                    ? (T) FerriteDataComponents.attributeModifiers(
                            this.meta.attributeModifiersMap())
                    : null;
            case KEY_TOOLTIP_DISPLAY -> this.meta.isHideTooltip()
                    ? (T) FerriteDataComponents.tooltipDisplay(true)
                    : null;
            case KEY_USE_COOLDOWN -> this.meta.hasUseCooldown()
                    ? (T) FerriteDataComponents.useCooldown(
                            (FerriteUseCooldownComponent) this.meta.getUseCooldown())
                    : null;
            case KEY_JUKEBOX_PLAYABLE -> {
                if (!this.meta.hasJukeboxPlayable()) {
                    yield null;
                }
                JukeboxSong song = this.meta.getJukeboxPlayable().getSong();
                yield song == null ? null : (T) FerriteDataComponents.jukeboxPlayable(song);
            }
            case KEY_TOOL -> this.meta.hasTool()
                    ? (T) FerriteDataComponents.tool((FerriteToolComponent) this.meta.getTool())
                    : null;
            case KEY_EQUIPPABLE -> this.meta.hasEquippable()
                    ? (T) FerriteDataComponents.equippable(
                            (FerriteEquippableComponent) this.meta.getEquippable())
                    : null;
            default -> throw unsupportedComponent(type, "getData");
        };
    }

    @Override
    public <T> T getDataOrDefault(DataComponentType.Valued<? extends T> type, T fallback) {
        @SuppressWarnings("unchecked")
        T value = getData((DataComponentType.Valued<T>) type);
        return value != null ? value : fallback;
    }

    @Override
    public boolean hasData(DataComponentType type) {
        String key = requireSupportedComponent(type, "hasData");
        if (this.meta == null) {
            return false;
        }
        return switch (key) {
            case KEY_CUSTOM_NAME -> this.meta.hasCustomName();
            case KEY_LORE -> this.meta.hasLore();
            case KEY_DAMAGE -> this.meta.hasDamageValue();
            case KEY_MAX_DAMAGE -> this.meta.hasMaxDamage();
            case KEY_UNBREAKABLE -> this.meta.isUnbreakable();
            case KEY_CUSTOM_MODEL_DATA -> this.meta.hasCustomModelDataComponent();
            case KEY_ENCHANTMENTS -> this.meta.hasEnchants();
            case KEY_MAX_STACK_SIZE -> this.meta.hasMaxStackSize();
            case KEY_ITEM_MODEL -> this.meta.hasItemModel();
            case KEY_TOOLTIP_STYLE -> this.meta.hasTooltipStyle();
            case KEY_ENCHANTABLE -> this.meta.hasEnchantable();
            case KEY_FOOD -> this.meta.hasFood();
            case KEY_ATTRIBUTE_MODIFIERS -> this.meta.hasAttributeModifiers();
            case KEY_TOOLTIP_DISPLAY -> this.meta.isHideTooltip();
            case KEY_USE_COOLDOWN -> this.meta.hasUseCooldown();
            case KEY_JUKEBOX_PLAYABLE -> this.meta.hasJukeboxPlayable();
            case KEY_TOOL -> this.meta.hasTool();
            case KEY_EQUIPPABLE -> this.meta.hasEquippable();
            default -> throw unsupportedComponent(type, "hasData");
        };
    }

    public Set<DataComponentType> getDataTypes() {
        if (this.meta == null || this.meta.isEmpty()) {
            return Set.of();
        }
        Set<DataComponentType> types = new LinkedHashSet<>();
        if (this.meta.hasCustomName()) {
            types.add(resolveComponentType(KEY_CUSTOM_NAME));
        }
        if (this.meta.hasLore()) {
            types.add(resolveComponentType(KEY_LORE));
        }
        if (this.meta.hasDamageValue()) {
            types.add(resolveComponentType(KEY_DAMAGE));
        }
        if (this.meta.hasMaxDamage()) {
            types.add(resolveComponentType(KEY_MAX_DAMAGE));
        }
        if (this.meta.isUnbreakable()) {
            types.add(resolveComponentType(KEY_UNBREAKABLE));
        }
        if (this.meta.hasCustomModelDataComponent()) {
            types.add(resolveComponentType(KEY_CUSTOM_MODEL_DATA));
        }
        if (this.meta.hasEnchants()) {
            types.add(resolveComponentType(KEY_ENCHANTMENTS));
        }
        if (this.meta.hasMaxStackSize()) {
            types.add(resolveComponentType(KEY_MAX_STACK_SIZE));
        }
        if (this.meta.hasItemModel()) {
            types.add(resolveComponentType(KEY_ITEM_MODEL));
        }
        if (this.meta.hasTooltipStyle()) {
            types.add(resolveComponentType(KEY_TOOLTIP_STYLE));
        }
        if (this.meta.hasEnchantable()) {
            types.add(resolveComponentType(KEY_ENCHANTABLE));
        }
        if (this.meta.hasFood()) {
            types.add(resolveComponentType(KEY_FOOD));
        }
        if (this.meta.hasAttributeModifiers()) {
            types.add(resolveComponentType(KEY_ATTRIBUTE_MODIFIERS));
        }
        if (this.meta.isHideTooltip()) {
            types.add(resolveComponentType(KEY_TOOLTIP_DISPLAY));
        }
        if (this.meta.hasUseCooldown()) {
            types.add(resolveComponentType(KEY_USE_COOLDOWN));
        }
        if (this.meta.hasJukeboxPlayable()) {
            types.add(resolveComponentType(KEY_JUKEBOX_PLAYABLE));
        }
        if (this.meta.hasTool()) {
            types.add(resolveComponentType(KEY_TOOL));
        }
        if (this.meta.hasEquippable()) {
            types.add(resolveComponentType(KEY_EQUIPPABLE));
        }
        return Collections.unmodifiableSet(types);
    }

    @Override
    public <T> void setData(DataComponentType.Valued<T> type, DataComponentBuilder<T> valueBuilder) {
        Objects.requireNonNull(valueBuilder, "valueBuilder");
        setData(type, valueBuilder.build());
    }

    @Override
    public <T> void setData(DataComponentType.Valued<T> type, T value) {
        String key = requireSupportedComponent(type, "setData");
        if (value == null) {
            unsetData(type);
            return;
        }
        if (this.type == Material.AIR) {
            return;
        }
        this.knownComponentTypes.put(key, type);
        switch (key) {
            case KEY_CUSTOM_NAME -> editMeta(m -> m.customName((Component) value));
            case KEY_LORE -> editMeta(m -> m.lore(((ItemLore) value).lines()));
            case KEY_DAMAGE -> editMeta(m -> ((Damageable) m).setDamage((Integer) value));
            case KEY_MAX_DAMAGE -> editMeta(m -> ((Damageable) m).setMaxDamage((Integer) value));
            case KEY_CUSTOM_MODEL_DATA -> {
                CustomModelData cmd = (CustomModelData) value;
                FerriteCustomModelDataComponent comp = new FerriteCustomModelDataComponent();
                comp.setFloats(cmd.floats());
                comp.setFlags(cmd.flags());
                comp.setStrings(cmd.strings());
                comp.setColors(cmd.colors());
                editMeta(m -> m.setCustomModelDataComponent(comp));
            }
            case KEY_ENCHANTMENTS -> {
                ItemEnchantments ench = (ItemEnchantments) value;
                editMeta(m -> {
                    m.removeEnchantments();
                    for (Map.Entry<Enchantment, Integer> entry : ench.enchantments().entrySet()) {
                        m.addEnchant(entry.getKey(), entry.getValue(), true);
                    }
                });
            }
            case KEY_MAX_STACK_SIZE -> editMeta(m -> m.setMaxStackSize((Integer) value));
            case KEY_ITEM_MODEL -> {
                Key modelKey = (Key) value;
                editMeta(m -> m.setItemModel(new NamespacedKey(modelKey.namespace(), modelKey.value())));
            }
            case KEY_TOOLTIP_STYLE -> {
                Key styleKey = (Key) value;
                editMeta(m -> m.setTooltipStyle(new NamespacedKey(styleKey.namespace(), styleKey.value())));
            }
            case KEY_ENCHANTABLE -> editMeta(m -> m.setEnchantable(((Enchantable) value).value()));
            case KEY_FOOD -> {
                FoodProperties props = (FoodProperties) value;
                editMeta(m -> m.setFood(new FerriteFoodComponent(
                        props.nutrition(), props.saturation(), props.canAlwaysEat())));
            }
            case KEY_ATTRIBUTE_MODIFIERS -> {
                ItemAttributeModifiers mods = (ItemAttributeModifiers) value;
                editMeta(m -> {
                    m.setAttributeModifiers(null);
                    for (ItemAttributeModifiers.Entry entry : mods.modifiers()) {
                        m.addAttributeModifier(entry.attribute(), entry.modifier());
                    }
                });
            }
            case KEY_TOOLTIP_DISPLAY -> {
                TooltipDisplay display = (TooltipDisplay) value;
                if (!display.hiddenComponents().isEmpty()) {
                    throw unsupportedComponent(type, "setData(hiddenComponents)");
                }
                editMeta(m -> m.setHideTooltip(display.hideTooltip()));
            }
            case KEY_USE_COOLDOWN -> {
                UseCooldown cooldown = (UseCooldown) value;
                Key group = cooldown.cooldownGroup();
                NamespacedKey groupKey = group == null
                        ? null
                        : new NamespacedKey(group.namespace(), group.value());
                editMeta(m -> m.setUseCooldown(
                        new FerriteUseCooldownComponent(cooldown.seconds(), groupKey)));
            }
            case KEY_JUKEBOX_PLAYABLE -> {
                JukeboxPlayable playable = (JukeboxPlayable) value;
                editMeta(m -> m.setJukeboxPlayable(
                        new FerriteJukeboxPlayableComponent(
                                playable.jukeboxSong().getKey(), true)));
            }
            case KEY_TOOL -> {
                Tool toolData = (Tool) value;
                FerriteToolComponent comp = new FerriteToolComponent();
                comp.setDefaultMiningSpeed(toolData.defaultMiningSpeed());
                comp.setDamagePerBlock(toolData.damagePerBlock());
                List<ToolComponent.ToolRule> rules = new ArrayList<>();
                for (Tool.Rule rule : toolData.rules()) {
                    Set<Material> mats = new LinkedHashSet<>();
                    for (TypedKey<BlockType> tk : rule.blocks().values()) {
                        Material mat = Material.matchMaterial(tk.key().asString());
                        if (mat != null) {
                            mats.add(mat);
                        }
                    }
                    Boolean correct = rule.correctForDrops().toBoolean();
                    rules.add(new FerriteToolComponent.FerriteToolRule(
                            mats, rule.speed(), correct));
                }
                comp.setRules(rules);
                editMeta(m -> m.setTool(comp));
            }
            case KEY_EQUIPPABLE -> {
                Equippable eq = (Equippable) value;
                FerriteEquippableComponent comp = new FerriteEquippableComponent();
                comp.setSlot(eq.slot());
                if (eq.equipSound() != null) {
                    comp.setEquipSound(Registry.SOUND_EVENT.get(
                            new NamespacedKey(eq.equipSound().namespace(), eq.equipSound().value())));
                }
                if (eq.assetId() != null) {
                    comp.setModel(new NamespacedKey(eq.assetId().namespace(), eq.assetId().value()));
                }
                if (eq.cameraOverlay() != null) {
                    comp.setCameraOverlay(new NamespacedKey(
                            eq.cameraOverlay().namespace(), eq.cameraOverlay().value()));
                }
                if (eq.allowedEntities() != null) {
                    comp.setAllowedEntities(eq.allowedEntities().resolve(Registry.ENTITY_TYPE));
                }
                comp.setDispensable(eq.dispensable());
                comp.setSwappable(eq.swappable());
                comp.setDamageOnHurt(eq.damageOnHurt());
                comp.setEquipOnInteract(eq.equipOnInteract());
                comp.setCanBeSheared(eq.canBeSheared());
                if (eq.shearSound() != null) {
                    comp.setShearingSound(Registry.SOUND_EVENT.get(
                            new NamespacedKey(eq.shearSound().namespace(), eq.shearSound().value())));
                }
                editMeta(m -> m.setEquippable(comp));
            }
            default -> throw unsupportedComponent(type, "setData");
        }
    }

    @Override
    public void setData(DataComponentType.NonValued type) {
        Objects.requireNonNull(type, "type");
        if (KEY_UNBREAKABLE.equals(type.key().asString())) {
            if (this.type == Material.AIR) {
                return;
            }
            this.knownComponentTypes.put(KEY_UNBREAKABLE, type);
            editMeta(m -> m.setUnbreakable(true));
            return;
        }
        throw unsupportedComponent(type, "setData");
    }

    public void unsetData(DataComponentType type) {
        String key = requireSupportedComponent(type, "unsetData");
        this.knownComponentTypes.remove(key);
        if (this.meta == null) {
            return;
        }
        switch (key) {
            case KEY_CUSTOM_NAME -> editMeta(m -> m.customName(null));
            case KEY_LORE -> editMeta(m -> m.lore(null));
            case KEY_DAMAGE -> editMeta(m -> ((Damageable) m).resetDamage());
            case KEY_MAX_DAMAGE -> editMeta(m -> ((Damageable) m).setMaxDamage(null));
            case KEY_UNBREAKABLE -> editMeta(m -> m.setUnbreakable(false));
            case KEY_CUSTOM_MODEL_DATA -> editMeta(m -> m.setCustomModelDataComponent(null));
            case KEY_ENCHANTMENTS -> editMeta(ItemMeta::removeEnchantments);
            case KEY_MAX_STACK_SIZE -> editMeta(m -> m.setMaxStackSize(null));
            case KEY_ITEM_MODEL -> editMeta(m -> m.setItemModel(null));
            case KEY_TOOLTIP_STYLE -> editMeta(m -> m.setTooltipStyle(null));
            case KEY_ENCHANTABLE -> editMeta(m -> m.setEnchantable(null));
            case KEY_FOOD -> editMeta(m -> m.setFood(null));
            case KEY_ATTRIBUTE_MODIFIERS -> editMeta(m -> m.setAttributeModifiers(null));
            case KEY_TOOLTIP_DISPLAY -> editMeta(m -> m.setHideTooltip(false));
            case KEY_USE_COOLDOWN -> editMeta(m -> m.setUseCooldown(null));
            case KEY_JUKEBOX_PLAYABLE -> editMeta(m -> m.setJukeboxPlayable(null));
            case KEY_TOOL -> editMeta(m -> m.setTool(null));
            case KEY_EQUIPPABLE -> editMeta(m -> m.setEquippable(null));
            default -> throw unsupportedComponent(type, "unsetData");
        }
    }

    public void resetData(DataComponentType type) {
        unsetData(type);
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    public void copyDataFrom(ItemStack source, Predicate<DataComponentType> filter) {
        Objects.requireNonNull(source, "source");
        Objects.requireNonNull(filter, "filter");
        for (DataComponentType componentType : source.getDataTypes()) {
            if (!filter.test(componentType)) {
                continue;
            }
            if (componentType instanceof DataComponentType.Valued valued) {
                Object value = source.getData(valued);
                setData(valued, value);
            } else if (componentType instanceof DataComponentType.NonValued nonValued
                    && KEY_UNBREAKABLE.equals(componentType.key().asString())) {
                setData(nonValued);
            } else {
                throw unsupportedComponent(componentType, "copyDataFrom");
            }
        }
    }

    public boolean isDataOverridden(DataComponentType type) {
        return hasData(type);
    }

    public boolean matchesWithoutData(ItemStack item, Set<DataComponentType> exclude) {
        return matchesWithoutData(item, exclude, false);
    }

    public boolean matchesWithoutData(
            ItemStack item, Set<DataComponentType> exclude, boolean ignoreCount) {
        Objects.requireNonNull(exclude, "exclude");
        for (DataComponentType componentType : exclude) {
            requireSupportedComponent(componentType, "matchesWithoutData");
        }
        if (item == null) {
            return false;
        }
        ItemStack copySelf = clone();
        ItemStack copyOther = item.clone();
        for (DataComponentType componentType : exclude) {
            copySelf.unsetData(componentType);
            copyOther.unsetData(componentType);
        }
        return ignoreCount ? copySelf.isSimilar(copyOther) : copySelf.equals(copyOther);
    }

    private String requireSupportedComponent(DataComponentType type, String method) {
        Objects.requireNonNull(type, "type");
        String key = type.key().asString();
        switch (key) {
            case KEY_CUSTOM_NAME,
                    KEY_LORE,
                    KEY_DAMAGE,
                    KEY_MAX_DAMAGE,
                    KEY_UNBREAKABLE,
                    KEY_CUSTOM_MODEL_DATA,
                    KEY_ENCHANTMENTS,
                    KEY_MAX_STACK_SIZE,
                    KEY_ITEM_MODEL,
                    KEY_TOOLTIP_STYLE,
                    KEY_ENCHANTABLE,
                    KEY_FOOD,
                    KEY_ATTRIBUTE_MODIFIERS,
                    KEY_TOOLTIP_DISPLAY,
                    KEY_USE_COOLDOWN,
                    KEY_JUKEBOX_PLAYABLE,
                    KEY_TOOL,
                    KEY_EQUIPPABLE -> {
                return key;
            }
            default -> throw unsupportedComponent(type, method);
        }
    }

    private DataComponentType resolveComponentType(String key) {
        DataComponentType known = this.knownComponentTypes.get(key);
        if (known != null) {
            return known;
        }
        NamespacedKey namespacedKey = NamespacedKey.fromString(key);
        if (namespacedKey != null) {
            Registry<DataComponentType> registry = Registry.DATA_COMPONENT_TYPE;
            DataComponentType resolved = registry == null ? null : registry.get(namespacedKey);
            if (resolved != null) {
                return resolved;
            }
        }
        throw dev.ferrite.api.internal.Unsupported.exception(
                "ItemStack#getDataTypes for item "
                        + this.type.name()
                        + " and component '"
                        + key
                        + "': DataComponentType registry is not populated");
    }

    public String toString() {
        return "ItemStack{" + this.type + " x" + this.amount + "}";
    }

    public boolean equals(Object object) {
        if (this == object) {
            return true;
        }
        if (!(object instanceof ItemStack other)) {
            return false;
        }
        return this.type == other.type
                && this.amount == other.amount
                && Objects.equals(normalizedMeta(this.meta), normalizedMeta(other.meta));
    }

    public int hashCode() {
        return Objects.hash(this.type, this.amount, normalizedMeta(this.meta));
    }

    public boolean isSimilar(ItemStack stack) {
        return stack != null
                && this.type == stack.type
                && Objects.equals(normalizedMeta(this.meta), normalizedMeta(stack.meta));
    }

    private static FerriteItemMeta normalizedMeta(FerriteItemMeta meta) {
        return meta == null || meta.isEmpty() ? null : meta;
    }

    public ItemStack clone() {
        return new ItemStack(this);
    }

    public Map<String, Object> serialize() {
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("type", this.type.name());
        result.put("amount", this.amount);
        if (this.meta != null && !this.meta.isEmpty()) {
            result.put("meta", this.meta.serialize());
        }
        return result;
    }

    @SuppressWarnings("unchecked")
    public static ItemStack deserialize(Map<String, Object> args) {
        Material type = Material.matchMaterial(String.valueOf(args.get("type")));
        if (type == null) {
            throw new IllegalArgumentException("Unknown material: " + args.get("type"));
        }
        ItemStack stack = new ItemStack(type);
        Object amount = args.get("amount");
        if (amount instanceof Number number) {
            stack.setAmount(number.intValue());
        }
        Object meta = args.get("meta");
        if (meta instanceof Map<?, ?> map) {
            stack.setItemMeta(FerriteItemMeta.deserialize((Map<String, Object>) map));
        }
        return stack;
    }

    public ItemStack asOne() {
        return asQuantity(1);
    }

    public ItemStack asQuantity(int quantity) {
        ItemStack copy = clone();
        copy.setAmount(quantity);
        return copy;
    }

    public ItemStack add() {
        return add(1);
    }

    public ItemStack add(int quantity) {
        return asQuantity(this.amount + quantity);
    }

    public ItemStack subtract() {
        return subtract(1);
    }

    public ItemStack subtract(int quantity) {
        return asQuantity(this.amount - quantity);
    }

    public org.bukkit.material.MaterialData getData() {
        throw unsupported("getData");
    }

    public void setData(org.bukkit.material.MaterialData p0) {
        throw unsupported("setData");
    }

    public org.bukkit.inventory.ItemStack enchantWithLevels(int p0, boolean p1, java.util.Random p2) {
        throw unsupported("enchantWithLevels");
    }

    public org.bukkit.inventory.ItemStack enchantWithLevels(
            int p0,
            io.papermc.paper.registry.set.RegistryKeySet<org.bukkit.enchantments.Enchantment> p1,
            java.util.Random p2) {
        throw unsupported("enchantWithLevels");
    }

    public net.kyori.adventure.text.event.HoverEvent<net.kyori.adventure.text.event.HoverEvent.ShowItem> asHoverEvent(
            java.util.function.UnaryOperator<net.kyori.adventure.text.event.HoverEvent.ShowItem> p0) {
        throw unsupported("asHoverEvent");
    }

    public net.kyori.adventure.text.Component effectiveName() {
        throw unsupported("effectiveName");
    }

    public org.bukkit.inventory.ItemStack ensureServerConversions() {
        throw unsupported("ensureServerConversions");
    }

    public byte[] serializeAsBytes() {
        throw unsupported("serializeAsBytes");
    }

    public java.lang.String getI18NDisplayName() {
        throw unsupported("getI18NDisplayName");
    }

    public int getMaxItemUseDuration() {
        throw unsupported("getMaxItemUseDuration");
    }

    public int getMaxItemUseDuration(org.bukkit.entity.LivingEntity p0) {
        throw unsupported("getMaxItemUseDuration");
    }

    public void addItemFlags(ItemFlag... itemFlags) {
        Objects.requireNonNull(itemFlags, "itemFlags");
        for (ItemFlag flag : itemFlags) {
            Objects.requireNonNull(flag, "itemFlag");
        }
        editMeta(current -> current.addItemFlags(itemFlags));
    }

    public void removeItemFlags(ItemFlag... itemFlags) {
        Objects.requireNonNull(itemFlags, "itemFlags");
        for (ItemFlag flag : itemFlags) {
            Objects.requireNonNull(flag, "itemFlag");
        }
        if (this.meta != null && !this.meta.getItemFlags().isEmpty()) {
            editMeta(current -> current.removeItemFlags(itemFlags));
        }
    }

    public Set<ItemFlag> getItemFlags() {
        return this.meta == null ? Set.of() : this.meta.getItemFlags();
    }

    public boolean hasItemFlag(ItemFlag flag) {
        Objects.requireNonNull(flag, "flag");
        return this.meta != null && this.meta.hasItemFlag(flag);
    }

    public java.lang.String translationKey() {
        throw unsupported("translationKey");
    }

    public io.papermc.paper.inventory.ItemRarity getRarity() {
        throw unsupported("getRarity");
    }

    public boolean isRepairableBy(org.bukkit.inventory.ItemStack p0) {
        throw unsupported("isRepairableBy");
    }

    public boolean canRepair(org.bukkit.inventory.ItemStack p0) {
        throw unsupported("canRepair");
    }

    public org.bukkit.inventory.ItemStack damage(int p0, org.bukkit.entity.LivingEntity p1) {
        throw unsupported("damage");
    }

    public java.util.List<net.kyori.adventure.text.Component> computeTooltipLines(
            io.papermc.paper.inventory.tooltip.TooltipContext p0, org.bukkit.entity.Player p1) {
        throw unsupported("computeTooltipLines");
    }
}
