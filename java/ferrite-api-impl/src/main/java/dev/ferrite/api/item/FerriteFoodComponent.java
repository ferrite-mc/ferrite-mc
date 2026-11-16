package dev.ferrite.api.item;

import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Objects;
import org.bukkit.inventory.meta.components.FoodComponent;

/**
 * Facade implementation of {@link FoodComponent} storing nutrition, saturation and
 * canAlwaysEat with defensive copying and serialization.
 */
public final class FerriteFoodComponent implements FoodComponent {

    private int nutrition;
    private float saturation;
    private boolean canAlwaysEat;

    public FerriteFoodComponent() {
    }

    public FerriteFoodComponent(int nutrition, float saturation, boolean canAlwaysEat) {
        setNutrition(nutrition);
        setSaturation(saturation);
        this.canAlwaysEat = canAlwaysEat;
    }

    public FerriteFoodComponent(FoodComponent source) {
        Objects.requireNonNull(source, "source");
        setNutrition(source.getNutrition());
        setSaturation(source.getSaturation());
        setCanAlwaysEat(source.canAlwaysEat());
    }

    @Override
    public int getNutrition() {
        return nutrition;
    }

    @Override
    public void setNutrition(int nutrition) {
        if (nutrition < 0) {
            throw new IllegalArgumentException("nutrition must be >= 0, was " + nutrition);
        }
        this.nutrition = nutrition;
    }

    @Override
    public float getSaturation() {
        return saturation;
    }

    @Override
    public void setSaturation(float saturation) {
        if (Float.isNaN(saturation) || Float.isInfinite(saturation)) {
            throw new IllegalArgumentException("saturation must be finite, was " + saturation);
        }
        this.saturation = saturation;
    }

    @Override
    public boolean canAlwaysEat() {
        return canAlwaysEat;
    }

    @Override
    public void setCanAlwaysEat(boolean canAlwaysEat) {
        this.canAlwaysEat = canAlwaysEat;
    }

    public FerriteFoodComponent copy() {
        return new FerriteFoodComponent(this);
    }

    @Override
    public Map<String, Object> serialize() {
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("nutrition", nutrition);
        result.put("saturation", saturation);
        result.put("can-always-eat", canAlwaysEat);
        return result;
    }

    public static FerriteFoodComponent deserialize(Map<?, ?> args) {
        Objects.requireNonNull(args, "args");
        FerriteFoodComponent component = new FerriteFoodComponent();
        Object nutrition = args.get("nutrition");
        if (nutrition instanceof Number number) {
            component.setNutrition(number.intValue());
        }
        Object saturation = args.get("saturation");
        if (saturation instanceof Number number) {
            component.setSaturation(number.floatValue());
        }
        Object always = args.get("can-always-eat");
        if (always instanceof Boolean flag) {
            component.setCanAlwaysEat(flag);
        }
        return component;
    }

    @Override
    public boolean equals(Object object) {
        if (this == object) {
            return true;
        }
        if (!(object instanceof FerriteFoodComponent other)) {
            return false;
        }
        return nutrition == other.nutrition
                && Float.compare(saturation, other.saturation) == 0
                && canAlwaysEat == other.canAlwaysEat;
    }

    @Override
    public int hashCode() {
        return Objects.hash(nutrition, saturation, canAlwaysEat);
    }

    @Override
    public String toString() {
        return "FerriteFoodComponent{nutrition="
                + nutrition
                + ", saturation="
                + saturation
                + ", canAlwaysEat="
                + canAlwaysEat
                + "}";
    }
}
