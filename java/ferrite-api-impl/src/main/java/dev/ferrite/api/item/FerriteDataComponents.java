package dev.ferrite.api.item;

import com.google.common.collect.Multimap;
import dev.ferrite.api.registry.FerriteKeySet;
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
import io.papermc.paper.datacomponent.item.attribute.AttributeModifierDisplay;
import io.papermc.paper.registry.RegistryKey;
import io.papermc.paper.registry.set.RegistryKeySet;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import net.kyori.adventure.key.Key;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.util.TriState;
import org.bukkit.Color;
import org.bukkit.JukeboxSong;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.attribute.Attribute;
import org.bukkit.attribute.AttributeModifier;
import org.bukkit.block.BlockType;
import org.bukkit.enchantments.Enchantment;
import org.bukkit.entity.EntityType;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.meta.components.ToolComponent;

/**
 * Clean-room immutable value implementations for the Paper {@code DataComponentType} value
 * interfaces supported by the Ferrite item slice.
 *
 * <p>Paper's own static builder factories delegate through {@code ItemComponentTypesBridge},
 * which is not registered in the facade's {@code META-INF/services}. Providing these direct
 * implementations avoids a service-loader failure while matching the Paper 26.3 interfaces.
 */
public final class FerriteDataComponents {

    private FerriteDataComponents() {
    }

    public static ItemLore lore(List<Component> lines) {
        return new FerriteItemLore(lines);
    }

    public static CustomModelData customModelData(FerriteCustomModelDataComponent component) {
        return new FerritePaperCustomModelData(
                component.getFloats(),
                component.getFlags(),
                component.getStrings(),
                component.getColors());
    }

    public static ItemEnchantments enchantments(Map<Enchantment, Integer> enchants) {
        return new FerriteItemEnchantments(enchants);
    }

    public static FoodProperties foodProperties(FerriteFoodComponent food) {
        return new FerriteFoodProperties(
                food.getNutrition(), food.getSaturation(), food.canAlwaysEat());
    }

    public static Enchantable enchantable(int value) {
        return new FerriteEnchantable(value);
    }

    public static ItemAttributeModifiers attributeModifiers(
            Multimap<Attribute, AttributeModifier> modifiers) {
        List<ItemAttributeModifiers.Entry> entries = new ArrayList<>(modifiers.size());
        for (Map.Entry<Attribute, AttributeModifier> entry : modifiers.entries()) {
            entries.add(new FerriteAttributeModifierEntry(entry.getKey(), entry.getValue()));
        }
        return new FerriteItemAttributeModifiers(entries);
    }

    public static TooltipDisplay tooltipDisplay(boolean hideTooltip) {
        return new FerriteTooltipDisplay(hideTooltip, Set.of());
    }

    public static UseCooldown useCooldown(FerriteUseCooldownComponent component) {
        return new FerriteUseCooldown(component.getCooldownSeconds(), component.getCooldownGroup());
    }

    public static JukeboxPlayable jukeboxPlayable(JukeboxSong song) {
        return new FerriteJukeboxPlayable(song);
    }

    public static Tool tool(FerriteToolComponent component) {
        List<Tool.Rule> rules = new ArrayList<>();
        for (ToolComponent.ToolRule rule : component.getRules()) {
            List<String> blockKeys = rule.getBlocks().stream()
                    .map(mat -> mat.getKey().asString())
                    .toList();
            TriState correct = rule.isCorrectForDrops() == null
                    ? TriState.NOT_SET
                    : TriState.byBoolean(rule.isCorrectForDrops());
            rules.add(new FerriteToolRuleData(
                    new FerriteKeySet<>(RegistryKey.BLOCK, blockKeys),
                    rule.getSpeed(),
                    correct));
        }
        return new FerriteTool(
                component.getDefaultMiningSpeed(),
                component.getDamagePerBlock(),
                rules,
                true);
    }

    public static Equippable equippable(FerriteEquippableComponent component) {
        Collection<EntityType> allowed = component.getAllowedEntities();
        RegistryKeySet<EntityType> allowedSet = null;
        if (allowed != null) {
            List<String> keys = new ArrayList<>(allowed.size());
            for (EntityType entityType : allowed) {
                if (entityType != EntityType.UNKNOWN) {
                    keys.add(entityType.getKey().asString());
                }
            }
            allowedSet = new FerriteKeySet<>(RegistryKey.ENTITY_TYPE, keys);
        }
        return new FerriteEquippable(
                component.getSlot(),
                component.getEquipSound() == null ? null : component.getEquipSound().getKey(),
                component.getModel(),
                component.getCameraOverlay(),
                allowedSet,
                component.isDispensable(),
                component.isSwappable(),
                component.isDamageOnHurt(),
                component.isEquipOnInteract(),
                component.canBeSheared(),
                component.getShearingSound() == null ? null : component.getShearingSound().getKey());
    }

    public record FerriteItemLore(List<Component> lines) implements ItemLore {

        public FerriteItemLore {
            Objects.requireNonNull(lines, "lines");
            lines = List.copyOf(lines);
        }

        @Override
        public List<Component> styledLines() {
            return lines;
        }
    }

    public record FerritePaperCustomModelData(
            List<Float> floats,
            List<Boolean> flags,
            List<String> strings,
            List<Color> colors) implements CustomModelData {

        public FerritePaperCustomModelData {
            floats = List.copyOf(Objects.requireNonNull(floats, "floats"));
            flags = List.copyOf(Objects.requireNonNull(flags, "flags"));
            strings = List.copyOf(Objects.requireNonNull(strings, "strings"));
            colors = List.copyOf(Objects.requireNonNull(colors, "colors"));
        }
    }

    public static final class FerriteItemEnchantments implements ItemEnchantments {

        private final Map<Enchantment, Integer> enchantments;

        public FerriteItemEnchantments(Map<Enchantment, Integer> enchantments) {
            Objects.requireNonNull(enchantments, "enchantments");
            this.enchantments = Collections.unmodifiableMap(new LinkedHashMap<>(enchantments));
        }

        @Override
        public Map<Enchantment, Integer> enchantments() {
            return enchantments;
        }

        @Override
        public boolean equals(Object object) {
            return object instanceof ItemEnchantments other
                    && Objects.equals(enchantments, other.enchantments());
        }

        @Override
        public int hashCode() {
            return enchantments.hashCode();
        }
    }

    public record FerriteFoodProperties(int nutrition, float saturation, boolean canAlwaysEat)
            implements FoodProperties {

        public FerriteFoodProperties {
            if (nutrition < 0) {
                throw new IllegalArgumentException("nutrition must be >= 0, was " + nutrition);
            }
            if (Float.isNaN(saturation) || Float.isInfinite(saturation)) {
                throw new IllegalArgumentException("saturation must be finite, was " + saturation);
            }
        }

        @Override
        public FoodProperties.Builder toBuilder() {
            return new Builder(nutrition, saturation, canAlwaysEat);
        }

        private static final class Builder implements FoodProperties.Builder {
            private int nutrition;
            private float saturation;
            private boolean canAlwaysEat;

            private Builder(int nutrition, float saturation, boolean canAlwaysEat) {
                this.nutrition = nutrition;
                this.saturation = saturation;
                this.canAlwaysEat = canAlwaysEat;
            }

            @Override
            public FoodProperties.Builder nutrition(int nutrition) {
                this.nutrition = nutrition;
                return this;
            }

            @Override
            public FoodProperties.Builder saturation(float saturation) {
                this.saturation = saturation;
                return this;
            }

            @Override
            public FoodProperties.Builder canAlwaysEat(boolean canAlwaysEat) {
                this.canAlwaysEat = canAlwaysEat;
                return this;
            }

            @Override
            public FoodProperties build() {
                return new FerriteFoodProperties(nutrition, saturation, canAlwaysEat);
            }
        }
    }

    public record FerriteEnchantable(int value) implements Enchantable {

        public FerriteEnchantable {
            if (value <= 0) {
                throw new IllegalArgumentException("enchantable must be > 0, was " + value);
            }
        }
    }

    public record FerriteAttributeModifierEntry(
            Attribute attribute, AttributeModifier modifier)
            implements ItemAttributeModifiers.Entry {

        public FerriteAttributeModifierEntry {
            Objects.requireNonNull(attribute, "attribute");
            Objects.requireNonNull(modifier, "modifier");
        }

        @Override
        public AttributeModifierDisplay display() {
            return AttributeModifierDisplay.reset();
        }
    }

    public record FerriteItemAttributeModifiers(List<ItemAttributeModifiers.Entry> modifiers)
            implements ItemAttributeModifiers {

        public FerriteItemAttributeModifiers {
            modifiers = List.copyOf(Objects.requireNonNull(modifiers, "modifiers"));
        }
    }

    public record FerriteTooltipDisplay(
            boolean hideTooltip, Set<DataComponentType> hiddenComponents)
            implements TooltipDisplay {

        public FerriteTooltipDisplay {
            hiddenComponents = Set.copyOf(Objects.requireNonNull(hiddenComponents, "hiddenComponents"));
        }
    }

    public record FerriteUseCooldown(float seconds, Key cooldownGroup) implements UseCooldown {
    }

    public record FerriteJukeboxPlayable(JukeboxSong jukeboxSong) implements JukeboxPlayable {
    }

    public record FerriteToolRuleData(
            RegistryKeySet<BlockType> blocks, Float speed, TriState correctForDrops)
            implements Tool.Rule {
    }

    public record FerriteTool(
            float defaultMiningSpeed,
            int damagePerBlock,
            List<Tool.Rule> rules,
            boolean canDestroyBlocksInCreative)
            implements Tool {

        public FerriteTool {
            rules = List.copyOf(Objects.requireNonNull(rules, "rules"));
        }
    }

    public record FerriteEquippable(
            EquipmentSlot slot,
            Key equipSound,
            Key assetId,
            Key cameraOverlay,
            RegistryKeySet<EntityType> allowedEntities,
            boolean dispensable,
            boolean swappable,
            boolean damageOnHurt,
            boolean equipOnInteract,
            boolean canBeSheared,
            Key shearSound)
            implements Equippable {

        @Override
        public Equippable.Builder toBuilder() {
            throw dev.ferrite.api.internal.Unsupported.exception("Equippable#toBuilder");
        }
    }
}
