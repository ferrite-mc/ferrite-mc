package dev.ferrite.api.item;

import io.papermc.paper.datacomponent.item.CustomModelData;
import io.papermc.paper.datacomponent.item.ItemEnchantments;
import io.papermc.paper.datacomponent.item.ItemLore;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import net.kyori.adventure.text.Component;
import org.bukkit.Color;
import org.bukkit.enchantments.Enchantment;

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
}
