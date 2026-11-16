package dev.ferrite.api.item;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.persistence.PersistentDataContainer;

/**
 * Item metadata slice: display name, lore and persistent data.
 *
 * <p>Enchantments, attributes, custom model data, flags and components are outside the
 * slice; they inherit explicit refusals from {@link UnsupportedItemMetaBase}.
 */
public final class FerriteItemMeta extends UnsupportedItemMetaBase {

    private final FerritePersistentDataContainer persistentData = new FerritePersistentDataContainer();
    private String displayName;
    private List<String> lore;

    public FerriteItemMeta() {
    }

    @Override
    public boolean hasDisplayName() {
        return displayName != null;
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
        this.lore = lore == null ? null : new ArrayList<>(lore);
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
        this.lore = lore.stream()
                .map(line -> LegacyComponentSerializer.legacySection().serialize(line))
                .toList();
    }

    @Override
    public PersistentDataContainer getPersistentDataContainer() {
        return persistentData;
    }

    @Override
    public ItemMeta clone() {
        FerriteItemMeta copy = new FerriteItemMeta();
        copy.displayName = displayName;
        copy.lore = lore == null ? null : new ArrayList<>(lore);
        copy.persistentData.copyFrom(persistentData);
        return copy;
    }

    /** Facade helper: no display name, lore or persistent data. */
    public boolean isEmpty() {
        return displayName == null && (lore == null || lore.isEmpty()) && persistentData.isEmpty();
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
        if (!persistentData.isEmpty()) {
            result.put("persistent-data", persistentData.serialize());
        }
        return result;
    }

    @SuppressWarnings("unchecked")
    public static FerriteItemMeta deserialize(Map<String, Object> args) {
        FerriteItemMeta meta = new FerriteItemMeta();
        Object name = args.get("display-name");
        if (name != null) {
            meta.setDisplayName(String.valueOf(name));
        }
        Object lore = args.get("lore");
        if (lore instanceof List<?> list) {
            meta.setLore(list.stream().map(String::valueOf).toList());
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
                && Objects.equals(persistentData.serialize(), other.persistentData.serialize());
    }

    @Override
    public int hashCode() {
        return Objects.hash(displayName, lore);
    }

    @Override
    public String toString() {
        return "FerriteItemMeta{displayName=" + displayName + ", lore=" + lore + "}";
    }
}
