package org.bukkit.inventory;

import dev.ferrite.api.item.FerriteItemMeta;
import dev.ferrite.api.item.FerritePersistentDataContainer;
import io.papermc.paper.datacomponent.DataComponentHolder;
import io.papermc.paper.persistence.PersistentDataContainerView;
import io.papermc.paper.persistence.PersistentDataViewHolder;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.function.Consumer;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.event.HoverEvent;
import net.kyori.adventure.text.event.HoverEventSource;
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer;
import org.bukkit.Material;
import org.bukkit.Translatable;
import org.bukkit.configuration.serialization.ConfigurationSerializable;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.persistence.PersistentDataContainer;

/**
 * Ferrite's implementation of the Bukkit item type.
 *
 * <p>This class deliberately shadows Paper's API {@code ItemStack}. Paper's own class
 * delegates every accessor to a {@code craftDelegate} field that only a CraftBukkit
 * server installs, so it cannot be constructed or read without one. Ferrite has no
 * CraftBukkit, so the facade provides the class with the API's binary name and implements
 * the supported slice: material, amount, display name, lore and persistent data.
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

    private Material type;
    private int amount;
    private ItemMeta meta;

    private static RuntimeException unsupported(String method) {
        return dev.ferrite.api.internal.Unsupported.exception("ItemStack#" + method);
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
        if (durability != 0) {
            throw dev.ferrite.api.internal.Unsupported.exception(
                    "ItemStack(Material, int, short): legacy durability is not stored");
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
        this.meta = stack.meta == null ? null : (ItemMeta) stack.meta.clone();
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

    public int getMaxStackSize() {
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
        if (this.meta == null) {
            this.meta = new FerriteItemMeta();
        }
        return this.meta;
    }

    public boolean setItemMeta(ItemMeta meta) {
        this.meta = meta;
        return true;
    }

    public boolean hasItemMeta() {
        return this.meta instanceof FerriteItemMeta ferrite && !ferrite.isEmpty();
    }

    public boolean editMeta(Consumer<? super ItemMeta> consumer) {
        Objects.requireNonNull(consumer, "consumer");
        ItemMeta current = getItemMeta();
        consumer.accept(current);
        return setItemMeta(current);
    }

    public <M extends ItemMeta> boolean editMeta(Class<M> clazz, Consumer<? super M> consumer) {
        Objects.requireNonNull(clazz, "clazz");
        Objects.requireNonNull(consumer, "consumer");
        ItemMeta current = getItemMeta();
        if (!clazz.isInstance(current)) {
            return false;
        }
        consumer.accept(clazz.cast(current));
        return setItemMeta(current);
    }

    public List<String> getLore() {
        if (!hasItemMeta()) {
            return null;
        }
        ItemMeta current = getItemMeta();
        return current.hasLore() ? current.getLore() : null;
    }

    public void setLore(List<String> lore) {
        getItemMeta().setLore(lore);
    }

    public List<Component> lore() {
        if (!hasItemMeta()) {
            return null;
        }
        return getItemMeta().lore();
    }

    public void lore(List<? extends Component> lore) {
        getItemMeta().lore(lore);
    }

    public Component displayName() {
        ItemMeta current = this.meta;
        if (current != null && current.hasDisplayName()) {
            return LegacyComponentSerializer.legacySection().deserialize(current.getDisplayName());
        }
        return Component.translatable(getTranslationKey());
    }

    public String getTranslationKey() {
        return this.type.getTranslationKey();
    }

    public PersistentDataContainerView getPersistentDataContainer() {
        ItemMeta current = this.meta;
        if (current == null) {
            return new FerritePersistentDataContainer();
        }
        return current.getPersistentDataContainer();
    }

    public boolean editPersistentDataContainer(Consumer<PersistentDataContainer> consumer) {
        Objects.requireNonNull(consumer, "consumer");
        consumer.accept(getItemMeta().getPersistentDataContainer());
        return true;
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
                && Objects.equals(this.meta, other.meta);
    }

    public int hashCode() {
        return Objects.hash(this.type, this.amount, this.meta);
    }

    public boolean isSimilar(ItemStack stack) {
        return stack != null && this.type == stack.type && Objects.equals(this.meta, stack.meta);
    }

    public ItemStack clone() {
        return new ItemStack(this);
    }

    public Map<String, Object> serialize() {
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("type", this.type.name());
        result.put("amount", this.amount);
        if (this.meta instanceof FerriteItemMeta ferrite && !ferrite.isEmpty()) {
            result.put("meta", ferrite.serialize());
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

    public void setDurability(short p0) {
        throw unsupported("setDurability");
    }

    public short getDurability() {
        throw unsupported("getDurability");
    }

    public boolean containsEnchantment(org.bukkit.enchantments.Enchantment p0) {
        throw unsupported("containsEnchantment");
    }

    public int getEnchantmentLevel(org.bukkit.enchantments.Enchantment p0) {
        throw unsupported("getEnchantmentLevel");
    }

    public java.util.Map<org.bukkit.enchantments.Enchantment, java.lang.Integer> getEnchantments() {
        throw unsupported("getEnchantments");
    }

    public void addEnchantments(java.util.Map<org.bukkit.enchantments.Enchantment, java.lang.Integer> p0) {
        throw unsupported("addEnchantments");
    }

    public void addEnchantment(org.bukkit.enchantments.Enchantment p0, int p1) {
        throw unsupported("addEnchantment");
    }

    public void addUnsafeEnchantments(java.util.Map<org.bukkit.enchantments.Enchantment, java.lang.Integer> p0) {
        throw unsupported("addUnsafeEnchantments");
    }

    public void addUnsafeEnchantment(org.bukkit.enchantments.Enchantment p0, int p1) {
        throw unsupported("addUnsafeEnchantment");
    }

    public int removeEnchantment(org.bukkit.enchantments.Enchantment p0) {
        throw unsupported("removeEnchantment");
    }

    public void removeEnchantments() {
        throw unsupported("removeEnchantments");
    }

    public org.bukkit.inventory.ItemStack enchantWithLevels(int p0, boolean p1, java.util.Random p2) {
        throw unsupported("enchantWithLevels");
    }

    public org.bukkit.inventory.ItemStack enchantWithLevels(int p0, io.papermc.paper.registry.set.RegistryKeySet<org.bukkit.enchantments.Enchantment> p1, java.util.Random p2) {
        throw unsupported("enchantWithLevels");
    }

    public net.kyori.adventure.text.event.HoverEvent<net.kyori.adventure.text.event.HoverEvent.ShowItem> asHoverEvent(java.util.function.UnaryOperator<net.kyori.adventure.text.event.HoverEvent.ShowItem> p0) {
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

    public void addItemFlags(org.bukkit.inventory.ItemFlag... p0) {
        throw unsupported("addItemFlags");
    }

    public void removeItemFlags(org.bukkit.inventory.ItemFlag... p0) {
        throw unsupported("removeItemFlags");
    }

    public java.util.Set<org.bukkit.inventory.ItemFlag> getItemFlags() {
        throw unsupported("getItemFlags");
    }

    public boolean hasItemFlag(org.bukkit.inventory.ItemFlag p0) {
        throw unsupported("hasItemFlag");
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

    public boolean isEmpty() {
        throw unsupported("isEmpty");
    }

    public java.util.List<net.kyori.adventure.text.Component> computeTooltipLines(io.papermc.paper.inventory.tooltip.TooltipContext p0, org.bukkit.entity.Player p1) {
        throw unsupported("computeTooltipLines");
    }

    public <T> T getData(io.papermc.paper.datacomponent.DataComponentType.Valued<T> p0) {
        throw unsupported("getData");
    }

    public <T> T getDataOrDefault(io.papermc.paper.datacomponent.DataComponentType.Valued<? extends T> p0, T p1) {
        throw unsupported("getDataOrDefault");
    }

    public boolean hasData(io.papermc.paper.datacomponent.DataComponentType p0) {
        throw unsupported("hasData");
    }

    public java.util.Set<io.papermc.paper.datacomponent.DataComponentType> getDataTypes() {
        throw unsupported("getDataTypes");
    }

    public <T> void setData(io.papermc.paper.datacomponent.DataComponentType.Valued<T> p0, io.papermc.paper.datacomponent.DataComponentBuilder<T> p1) {
        throw unsupported("setData");
    }

    public <T> void setData(io.papermc.paper.datacomponent.DataComponentType.Valued<T> p0, T p1) {
        throw unsupported("setData");
    }

    public void setData(io.papermc.paper.datacomponent.DataComponentType.NonValued p0) {
        throw unsupported("setData");
    }

    public void unsetData(io.papermc.paper.datacomponent.DataComponentType p0) {
        throw unsupported("unsetData");
    }

    public void resetData(io.papermc.paper.datacomponent.DataComponentType p0) {
        throw unsupported("resetData");
    }

    public void copyDataFrom(org.bukkit.inventory.ItemStack p0, java.util.function.Predicate<io.papermc.paper.datacomponent.DataComponentType> p1) {
        throw unsupported("copyDataFrom");
    }

    public boolean isDataOverridden(io.papermc.paper.datacomponent.DataComponentType p0) {
        throw unsupported("isDataOverridden");
    }

    public boolean matchesWithoutData(org.bukkit.inventory.ItemStack p0, java.util.Set<io.papermc.paper.datacomponent.DataComponentType> p1) {
        throw unsupported("matchesWithoutData");
    }

    public boolean matchesWithoutData(org.bukkit.inventory.ItemStack p0, java.util.Set<io.papermc.paper.datacomponent.DataComponentType> p1, boolean p2) {
        throw unsupported("matchesWithoutData");
    }
}
