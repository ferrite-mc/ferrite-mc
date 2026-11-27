package dev.ferrite.api.item;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Objects;
import java.util.Random;
import java.util.function.UnaryOperator;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.event.HoverEvent;
import net.md_5.bungee.api.chat.BaseComponent;
import net.md_5.bungee.api.chat.hover.content.Content;
import net.md_5.bungee.api.chat.hover.content.Text;
import org.bukkit.Color;
import org.bukkit.Material;
import org.bukkit.Registry;
import org.bukkit.World;
import org.bukkit.enchantments.Enchantment;
import org.bukkit.entity.Entity;
import org.bukkit.entity.EntityType;
import org.bukkit.inventory.ItemFactory;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.ItemMeta;

/** Clean-room {@link ItemFactory} implementation backed by {@link FerriteItemMeta} and {@link ItemStack}. */
public final class FerriteItemFactory implements ItemFactory {

    private static final Color DEFAULT_LEATHER_COLOR = Color.fromRGB(0xA06540);

    @Override
    public ItemMeta getItemMeta(Material material) {
        if (material == null) {
            throw new IllegalArgumentException("Material cannot be null");
        }
        return material == Material.AIR ? null : new FerriteItemMeta();
    }

    @Override
    public boolean isApplicable(ItemMeta meta, ItemStack stack) throws IllegalArgumentException {
        return stack != null && isApplicable(meta, stack.getType());
    }

    @Override
    public boolean isApplicable(ItemMeta meta, Material material) throws IllegalArgumentException {
        if (material == null) {
            throw new IllegalArgumentException("Material cannot be null");
        }
        return meta == null || (material != Material.AIR && meta instanceof FerriteItemMeta);
    }

    @Override
    public boolean equals(ItemMeta meta1, ItemMeta meta2) throws IllegalArgumentException {
        if (meta1 == meta2) {
            return true;
        }
        if (meta1 == null) {
            return meta2 instanceof FerriteItemMeta ferrite && ferrite.isEmpty();
        }
        if (meta2 == null) {
            return meta1 instanceof FerriteItemMeta ferrite && ferrite.isEmpty();
        }
        return Objects.equals(meta1, meta2);
    }

    @Override
    public ItemMeta asMetaFor(ItemMeta meta, ItemStack stack) throws IllegalArgumentException {
        Objects.requireNonNull(stack, "stack");
        return asMetaFor(meta, stack.getType());
    }

    @Override
    public ItemMeta asMetaFor(ItemMeta meta, Material material) throws IllegalArgumentException {
        if (material == null) {
            throw new IllegalArgumentException("Material cannot be null");
        }
        if (material == Material.AIR) {
            return null;
        }
        if (meta == null) {
            return new FerriteItemMeta();
        }
        return meta.clone();
    }

    @Override
    public Color getDefaultLeatherColor() {
        return DEFAULT_LEATHER_COLOR;
    }

    @Override
    public ItemStack createItemStack(String input) throws IllegalArgumentException {
        if (input == null || input.isBlank()) {
            throw new IllegalArgumentException("Item input cannot be null or blank");
        }
        String trimmed = input.trim();
        int bracket = trimmed.indexOf('[');
        int brace = trimmed.indexOf('{');
        int cut = -1;
        if (bracket >= 0 && brace >= 0) {
            cut = Math.min(bracket, brace);
        } else if (bracket >= 0) {
            cut = bracket;
        } else if (brace >= 0) {
            cut = brace;
        }
        String matPart = cut >= 0 ? trimmed.substring(0, cut).trim() : trimmed;
        Material material = Material.matchMaterial(matPart);
        if (material == null) {
            throw new IllegalArgumentException("Unknown item material in '" + input + "'");
        }
        return new ItemStack(material, 1);
    }

    @Override
    public Material getSpawnEgg(EntityType type) {
        if (type == null || type == EntityType.UNKNOWN) {
            return null;
        }
        return Material.matchMaterial(type.getKey().getKey().toUpperCase(Locale.ROOT) + "_SPAWN_EGG");
    }

    @Override
    public ItemStack enchantItem(Entity entity, ItemStack item, int level, boolean allowTreasures) {
        return enchantWithLevels(item, level, allowTreasures, new Random());
    }

    @Override
    public ItemStack enchantItem(World world, ItemStack item, int level, boolean allowTreasures) {
        return enchantWithLevels(item, level, allowTreasures, new Random());
    }

    @Override
    public ItemStack enchantItem(ItemStack item, int level, boolean allowTreasures) {
        return enchantWithLevels(item, level, allowTreasures, new Random());
    }

    @Override
    public HoverEvent<HoverEvent.ShowItem> asHoverEvent(
            ItemStack item, UnaryOperator<HoverEvent.ShowItem> op) {
        Objects.requireNonNull(item, "item");
        HoverEvent.ShowItem show =
                HoverEvent.ShowItem.showItem(item.getType().getKey(), Math.max(1, item.getAmount()));
        return HoverEvent.showItem(op == null ? show : Objects.requireNonNull(op.apply(show), "op result"));
    }

    @Override
    public Component displayName(ItemStack itemStack) {
        Objects.requireNonNull(itemStack, "itemStack");
        ItemMeta meta = itemStack.getItemMeta();
        if (meta != null && meta.hasCustomName()) {
            Component custom = meta.customName();
            if (custom != null) {
                return custom;
            }
        }
        if (meta != null && meta.hasItemName()) {
            Component itemName = meta.itemName();
            if (itemName != null) {
                return itemName;
            }
        }
        String key = itemStack.getType().isBlock() && !itemStack.getType().isItem()
                ? "block.minecraft." + itemStack.getType().getKey().getKey()
                : "item.minecraft." + itemStack.getType().getKey().getKey();
        return Component.translatable(key);
    }

    @Override
    @SuppressWarnings("deprecation")
    public String getI18NDisplayName(ItemStack item) {
        if (item == null) {
            return null;
        }
        if (item.hasItemMeta()) {
            ItemMeta meta = item.getItemMeta();
            if (meta != null && meta.hasDisplayName()) {
                return meta.getDisplayName();
            }
        }
        return item.getType().name();
    }

    @Override
    public ItemStack ensureServerConversions(ItemStack item) {
        return item == null ? null : item.clone();
    }

    @Override
    @SuppressWarnings("deprecation")
    public Content hoverContentOf(ItemStack itemStack) {
        Objects.requireNonNull(itemStack, "itemStack");
        return new net.md_5.bungee.api.chat.hover.content.Item(
                itemStack.getType().getKey().asString(), Math.max(1, itemStack.getAmount()), null);
    }

    @Override
    @SuppressWarnings("deprecation")
    public Content hoverContentOf(Entity entity) {
        return hoverContentOf(entity, (BaseComponent) null);
    }

    @Override
    @SuppressWarnings("deprecation")
    public Content hoverContentOf(Entity entity, String customName) {
        Objects.requireNonNull(entity, "entity");
        return new net.md_5.bungee.api.chat.hover.content.Entity(
                entity.getType().getKey().asString(),
                entity.getUniqueId().toString(),
                customName == null ? null : new net.md_5.bungee.api.chat.TextComponent(customName));
    }

    @Override
    @SuppressWarnings("deprecation")
    public Content hoverContentOf(Entity entity, BaseComponent customName) {
        Objects.requireNonNull(entity, "entity");
        return new net.md_5.bungee.api.chat.hover.content.Entity(
                entity.getType().getKey().asString(), entity.getUniqueId().toString(), customName);
    }

    @Override
    @SuppressWarnings("deprecation")
    public Content hoverContentOf(Entity entity, BaseComponent[] customName) {
        Objects.requireNonNull(entity, "entity");
        BaseComponent combined = customName == null || customName.length == 0
                ? null
                : new net.md_5.bungee.api.chat.TextComponent(customName);
        return hoverContentOf(entity, combined);
    }

    @Override
    public ItemStack enchantWithLevels(
            ItemStack itemStack, int levels, boolean allowTreasure, Random random) {
        Objects.requireNonNull(itemStack, "itemStack");
        Objects.requireNonNull(random, "random");
        if (levels <= 0) {
            throw new IllegalArgumentException("levels must be > 0, was " + levels);
        }
        ItemStack copy = itemStack.clone();
        if (copy.isEmpty()) {
            return copy;
        }
        Registry<Enchantment> registry = Registry.ENCHANTMENT;
        if (registry == null) {
            return copy;
        }
        List<Enchantment> candidates = new ArrayList<>();
        for (Enchantment ench : registry) {
            if ((!ench.isTreasure() || allowTreasure) && ench.canEnchantItem(copy)) {
                candidates.add(ench);
            }
        }
        if (!candidates.isEmpty()) {
            Enchantment chosen = candidates.get(random.nextInt(candidates.size()));
            int power = Math.max(
                    chosen.getStartLevel(),
                    Math.min(chosen.getMaxLevel(), 1 + levels / 10));
            copy.addUnsafeEnchantment(chosen, power);
        }
        return copy;
    }

    @Override
    public ItemStack enchantWithLevels(
            ItemStack itemStack,
            int levels,
            io.papermc.paper.registry.set.RegistryKeySet<Enchantment> keySet,
            Random random) {
        Objects.requireNonNull(itemStack, "itemStack");
        Objects.requireNonNull(keySet, "keySet");
        Objects.requireNonNull(random, "random");
        if (levels <= 0) {
            throw new IllegalArgumentException("levels must be > 0, was " + levels);
        }
        ItemStack copy = itemStack.clone();
        Registry<Enchantment> registry = Registry.ENCHANTMENT;
        if (copy.isEmpty() || registry == null) {
            return copy;
        }
        List<Enchantment> candidates = new ArrayList<>();
        for (var typedKey : keySet.values()) {
            Enchantment ench = registry.get(typedKey.key());
            if (ench != null && ench.canEnchantItem(copy)) {
                candidates.add(ench);
            }
        }
        if (!candidates.isEmpty()) {
            Enchantment chosen = candidates.get(random.nextInt(candidates.size()));
            int power = Math.max(
                    chosen.getStartLevel(),
                    Math.min(chosen.getMaxLevel(), 1 + levels / 10));
            copy.addUnsafeEnchantment(chosen, power);
        }
        return copy;
    }
}
