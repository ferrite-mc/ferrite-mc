package dev.ferrite.api.item;

import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import org.bukkit.Color;
import org.bukkit.inventory.meta.components.CustomModelDataComponent;

/**
 * Facade implementation of {@link CustomModelDataComponent} storing floats, flags, strings
 * and colors with defensive copies on reads and writes.
 */
public final class FerriteCustomModelDataComponent implements CustomModelDataComponent {

    private List<Float> floats = List.of();
    private List<Boolean> flags = List.of();
    private List<String> strings = List.of();
    private List<Color> colors = List.of();

    public FerriteCustomModelDataComponent() {
    }

    public FerriteCustomModelDataComponent(CustomModelDataComponent source) {
        Objects.requireNonNull(source, "source");
        setFloats(source.getFloats());
        setFlags(source.getFlags());
        setStrings(source.getStrings());
        setColors(source.getColors());
    }

    @Override
    public List<Float> getFloats() {
        return Collections.unmodifiableList(new ArrayList<>(floats));
    }

    @Override
    public void setFloats(List<Float> floats) {
        Objects.requireNonNull(floats, "floats");
        List<Float> copy = new ArrayList<>(floats.size());
        for (Float value : floats) {
            copy.add(Objects.requireNonNull(value, "float entry"));
        }
        this.floats = List.copyOf(copy);
    }

    @Override
    public List<Boolean> getFlags() {
        return Collections.unmodifiableList(new ArrayList<>(flags));
    }

    @Override
    public void setFlags(List<Boolean> flags) {
        Objects.requireNonNull(flags, "flags");
        List<Boolean> copy = new ArrayList<>(flags.size());
        for (Boolean value : flags) {
            copy.add(Objects.requireNonNull(value, "flag entry"));
        }
        this.flags = List.copyOf(copy);
    }

    @Override
    public List<String> getStrings() {
        return Collections.unmodifiableList(new ArrayList<>(strings));
    }

    @Override
    public void setStrings(List<String> strings) {
        Objects.requireNonNull(strings, "strings");
        List<String> copy = new ArrayList<>(strings.size());
        for (String value : strings) {
            copy.add(Objects.requireNonNull(value, "string entry"));
        }
        this.strings = List.copyOf(copy);
    }

    @Override
    public List<Color> getColors() {
        return Collections.unmodifiableList(new ArrayList<>(colors));
    }

    @Override
    public void setColors(List<Color> colors) {
        Objects.requireNonNull(colors, "colors");
        List<Color> copy = new ArrayList<>(colors.size());
        for (Color value : colors) {
            copy.add(Objects.requireNonNull(value, "color entry"));
        }
        this.colors = List.copyOf(copy);
    }

    public boolean isEmpty() {
        return floats.isEmpty() && flags.isEmpty() && strings.isEmpty() && colors.isEmpty();
    }

    public FerriteCustomModelDataComponent copy() {
        return new FerriteCustomModelDataComponent(this);
    }

    @Override
    public Map<String, Object> serialize() {
        Map<String, Object> result = new LinkedHashMap<>();
        if (!floats.isEmpty()) {
            result.put("floats", new ArrayList<>(floats));
        }
        if (!flags.isEmpty()) {
            result.put("flags", new ArrayList<>(flags));
        }
        if (!strings.isEmpty()) {
            result.put("strings", new ArrayList<>(strings));
        }
        if (!colors.isEmpty()) {
            result.put("colors", colors.stream().map(Color::asARGB).toList());
        }
        return result;
    }

    public static FerriteCustomModelDataComponent deserialize(Map<?, ?> args) {
        FerriteCustomModelDataComponent component = new FerriteCustomModelDataComponent();
        if (args == null) {
            return component;
        }
        if (args.get("floats") instanceof List<?> list) {
            component.setFloats(list.stream().map(v -> ((Number) v).floatValue()).toList());
        }
        if (args.get("flags") instanceof List<?> list) {
            component.setFlags(list.stream().map(v -> (Boolean) v).toList());
        }
        if (args.get("strings") instanceof List<?> list) {
            component.setStrings(list.stream().map(String::valueOf).toList());
        }
        if (args.get("colors") instanceof List<?> list) {
            List<Color> decoded = new ArrayList<>(list.size());
            for (Object entry : list) {
                if (entry instanceof Number number) {
                    decoded.add(Color.fromARGB(number.intValue()));
                } else if (entry instanceof Color color) {
                    decoded.add(color);
                } else {
                    throw new IllegalArgumentException("Unsupported color entry: " + entry);
                }
            }
            component.setColors(decoded);
        }
        return component;
    }

    @Override
    public boolean equals(Object object) {
        if (this == object) {
            return true;
        }
        if (!(object instanceof FerriteCustomModelDataComponent other)) {
            return false;
        }
        return Objects.equals(floats, other.floats)
                && Objects.equals(flags, other.flags)
                && Objects.equals(strings, other.strings)
                && Objects.equals(colors, other.colors);
    }

    @Override
    public int hashCode() {
        return Objects.hash(floats, flags, strings, colors);
    }
}
