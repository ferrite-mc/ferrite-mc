// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.world.item;

import dev.ferrite.nms.NmsUnsupported;
import io.papermc.paper.datacomponent.DataComponentType;
import io.papermc.paper.datacomponent.DataComponentTypes;
import java.util.Objects;
import java.util.Set;
import net.kyori.adventure.text.Component;
import org.bukkit.Material;

/**
 * Clean-room item stack. It is a value object, not world state: when built from a Paper stack it
 * keeps a defensive copy of the whole stack, so display name, lore, persistent data and the
 * specialised metadata all survive the NMS round trip without being reinterpreted.
 */
public final class ItemStack {
    public static final ItemStack EMPTY = new ItemStack(Material.AIR, 0, null);

    private final Material material;
    private final org.bukkit.inventory.ItemStack source;
    private int count;

    private ItemStack(Material material, int count, org.bukkit.inventory.ItemStack source) {
        this.material = material;
        this.count = count;
        this.source = source;
    }

    /** A material/count stack without Paper metadata; useful for fixtures and NMS-side values. */
    public static ItemStack of(Material material, int count) {
        Objects.requireNonNull(material, "material");
        if (count < 0) {
            throw new IllegalArgumentException("count must not be negative");
        }
        if (count == 0 || material.isAir()) {
            return EMPTY;
        }
        return new ItemStack(material, count, null);
    }

    /** A lossless copy of a Paper stack, carrying every metadata field the facade provided. */
    public static ItemStack of(org.bukkit.inventory.ItemStack stack) {
        if (stack == null) {
            return EMPTY;
        }
        Material type = Objects.requireNonNull(stack.getType(), "stack type");
        int amount = stack.getAmount();
        if (type.isAir() || amount <= 0) {
            return EMPTY;
        }
        return new ItemStack(type, amount, stack.clone());
    }

    public boolean isEmpty() {
        return count <= 0 || material.isAir();
    }

    public int getCount() {
        return isEmpty() ? 0 : count;
    }

    public void setCount(int count) {
        if (isEmpty()) {
            if (count > 0) {
                throw new IllegalStateException("Cannot give a non-zero count to an empty Ferrite NMS item stack");
            }
            this.count = 0;
            return;
        }
        this.count = Math.max(count, 0);
        if (source != null) {
            source.setAmount(this.count);
        }
    }

    public void shrink(int amount) {
        if (amount < 0) {
            throw new IllegalArgumentException("shrink amount must not be negative");
        }
        setCount(getCount() - amount);
    }

    public void grow(int amount) {
        if (amount < 0) {
            throw new IllegalArgumentException("grow amount must not be negative");
        }
        setCount(getCount() + amount);
    }

    public Material getBukkitMaterial() {
        return material;
    }

    public Item getItem() {
        return new Item(material);
    }

    /**
     * The effective max stack size, preferring the Paper {@code max_stack_size} component over
     * the material default. A material-only stack has no component data and uses the default.
     */
    public int getMaxStackSize() {
        if (source != null) {
            Integer component = source.getData(DataComponentTypes.MAX_STACK_SIZE);
            if (component != null) {
                return component;
            }
        }
        return material.getMaxStackSize();
    }

    /**
     * Reads one 26.3 data component from the defensive Paper copy. An absent component is null,
     * matching Paper. A stack built from only a material has no component data and refuses.
     */
    public <T> T getComponent(DataComponentType.Valued<T> type) {
        Objects.requireNonNull(type, "type");
        return requireComponentSource("getComponent").getData(type);
    }

    /** Sets one data component on the defensive Paper copy; other components are preserved. */
    public <T> void setComponent(DataComponentType.Valued<T> type, T value) {
        Objects.requireNonNull(type, "type");
        requireComponentSource("setComponent").setData(type, value);
    }

    /** Removes one component from the defensive Paper copy. */
    public void unsetComponent(DataComponentType type) {
        Objects.requireNonNull(type, "type");
        requireComponentSource("unsetComponent").unsetData(type);
    }

    /** Every component explicitly present on the defensive Paper copy. */
    public Set<DataComponentType> componentTypes() {
        return Set.copyOf(requireComponentSource("componentTypes").getDataTypes());
    }

    /** The 26.3 {@code damage} component, or 0 when the item has none. */
    public int getDamageValue() {
        Integer damage = getComponent(DataComponentTypes.DAMAGE);
        return damage == null ? 0 : damage;
    }

    public void setDamageValue(int damage) {
        if (damage < 0) {
            throw new IllegalArgumentException("damage must not be negative");
        }
        setComponent(DataComponentTypes.DAMAGE, damage);
    }

    /** The 26.3 {@code max_damage} component, or 0 when the item is not damageable. */
    public int getMaxDamage() {
        Integer maxDamage = getComponent(DataComponentTypes.MAX_DAMAGE);
        return maxDamage == null ? 0 : maxDamage;
    }

    public boolean isDamageableItem() {
        return getMaxDamage() > 0;
    }

    public boolean isUnbreakable() {
        return componentTypes().contains(DataComponentTypes.UNBREAKABLE);
    }

    /** The custom name component, or the item's default display name when it has none. */
    public Component getHoverName() {
        Component customName = getComponent(DataComponentTypes.CUSTOM_NAME);
        return customName != null
                ? customName
                : requireComponentSource("getHoverName").displayName();
    }

    /** Sets or clears the custom name component without disturbing the other components. */
    public void setHoverName(Component name) {
        if (name == null) {
            unsetComponent(DataComponentTypes.CUSTOM_NAME);
        } else {
            setComponent(DataComponentTypes.CUSTOM_NAME, name);
        }
    }

    /** The 26.3 translation key, e.g. {@code item.minecraft.stone}. */
    public String getDescriptionId() {
        return (material.isBlock() ? "block." : "item.") + material.getKey();
    }

    public ItemStack copy() {
        if (isEmpty()) {
            return EMPTY;
        }
        return source == null ? of(material, count) : of(source.clone());
    }

    /**
     * A defensive Paper copy. The metadata-less form materialises through the Bukkit facade; when
     * only Lane 2's item factory can do that, the existing explicit facade error is preserved.
     */
    public org.bukkit.inventory.ItemStack toBukkit() {
        if (isEmpty()) {
            return null;
        }
        if (source != null) {
            org.bukkit.inventory.ItemStack copy = source.clone();
            copy.setAmount(count);
            return copy;
        }
        return new org.bukkit.inventory.ItemStack(material, count);
    }

    @Override
    public boolean equals(Object other) {
        if (this == other) {
            return true;
        }
        if (!(other instanceof ItemStack stack)) {
            return false;
        }
        return count == stack.count
                && material == stack.material
                && Objects.equals(source, stack.source);
    }

    private org.bukkit.inventory.ItemStack requireComponentSource(String accessor) {
        if (source == null || isEmpty()) {
            throw NmsUnsupported.operation(
                    "net.minecraft.world.item.ItemStack",
                    accessor,
                    "component access requires a stack created from a Paper ItemStack; "
                            + "ItemStack.of(Material, int) has no component data. Use "
                            + "CraftItemStack.asNMSCopy(...) so all components are carried");
        }
        return source;
    }

    @Override
    public int hashCode() {
        return Objects.hash(material, count, source);
    }

    @Override
    public String toString() {
        return getCount() + " " + material.getKey();
    }
}
