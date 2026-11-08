package dev.ferrite.server;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import io.papermc.paper.advancement.AdvancementDisplay;
import io.papermc.paper.advancement.AdvancementDisplay.Frame;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Collections;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.serializer.gson.GsonComponentSerializer;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.advancement.Advancement;
import org.bukkit.inventory.ItemStack;
import org.bukkit.advancement.AdvancementRequirement;
import org.bukkit.advancement.AdvancementRequirements;

/**
 * Vanilla 26.3 advancements read from the generated data pack.
 *
 * <p>The whole {@code data/minecraft/advancement} tree is loaded lazily, keyed by
 * {@code minecraft:<relative path without .json>}. Criteria and their requirement groups come
 * from the JSON; the display data is left to the host, so it is not parsed.
 */
final class FerriteAdvancements {
    private FerriteAdvancements() {}

    private static volatile Map<NamespacedKey, Advancement> advancements;

    static Advancement get(NamespacedKey key) {
        Objects.requireNonNull(key, "key");
        return entries().get(key);
    }

    static Iterator<Advancement> iterator() { return entries().values().iterator(); }

    static Collection<Advancement> all() { return entries().values(); }

    private static Map<NamespacedKey, Advancement> entries() {
        var loaded = advancements;
        if (loaded == null) synchronized (FerriteAdvancements.class) {
            loaded = advancements;
            if (loaded == null) advancements = loaded = load();
        }
        return loaded;
    }

    private static Map<NamespacedKey, Advancement> load() {
        var dir = Bootstrap.dataFile("data/minecraft/advancement");
        if (dir == null || !dir.isDirectory()) return Map.of();
        var root = dir.toPath();
        var loaded = new LinkedHashMap<NamespacedKey, Advancement>();
        try (var files = Files.walk(root)) {
            for (var path : files.filter(Files::isRegularFile)
                    .filter(file -> file.getFileName().toString().endsWith(".json"))
                    .sorted().toList()) {
                String relative = root.relativize(path).toString().replace('\\', '/');
                var key = new NamespacedKey("minecraft", relative.substring(0, relative.length() - ".json".length()));
                var json = JsonParser.parseString(Files.readString(path)).getAsJsonObject();
                loaded.put(key, new VanillaAdvancement(key, json));
            }
        } catch (IOException error) {
            throw new UncheckedIOException(error);
        }
        return Collections.unmodifiableMap(loaded);
    }

    private static final class VanillaAdvancement implements Advancement {
        private final NamespacedKey key;
        private final Set<String> criteria;
        private final AdvancementRequirements requirements;
        private final NamespacedKey parentKey;
        private final Display display;

        VanillaAdvancement(NamespacedKey key, JsonObject json) {
            this.key = key;
            var criteriaJson = json.getAsJsonObject("criteria");
            var names = new LinkedHashSet<String>();
            if (criteriaJson != null) names.addAll(criteriaJson.keySet());
            this.criteria = Collections.unmodifiableSet(names);
            this.parentKey = json.has("parent") ? NamespacedKey.fromString(json.get("parent").getAsString()) : null;
            this.display = json.has("display") && json.get("display").isJsonObject()
                    ? new Display(json.getAsJsonObject("display")) : null;

            var groups = new ArrayList<AdvancementRequirement>();
            var requirementsJson = json.getAsJsonArray("requirements");
            if (requirementsJson != null) {
                for (var group : requirementsJson) {
                    var required = new LinkedHashSet<String>();
                    for (var criterion : group.getAsJsonArray()) required.add(criterion.getAsString());
                    groups.add(new Requirement(required));
                }
            } else {
                // Vanilla treats every criterion as its own requirement group when the key is absent.
                for (var criterion : names) groups.add(new Requirement(Set.of(criterion)));
            }
            this.requirements = () -> List.copyOf(groups);
        }

        @Override public NamespacedKey getKey() { return key; }
        @Override public Collection<String> getCriteria() { return criteria; }
        @Override public AdvancementRequirements getRequirements() { return requirements; }

        @Override public AdvancementDisplay getDisplay() { return display; }

        @Override public Component displayName() {
            return display == null ? Component.text(key.toString()) : display.title();
        }

        @Override public Advancement getParent() { return parentKey == null ? null : FerriteAdvancements.get(parentKey); }

        @Override public Collection<Advancement> getChildren() {
            var children = new ArrayList<Advancement>();
            for (var candidate : FerriteAdvancements.all())
                if (candidate instanceof VanillaAdvancement vanilla && key.equals(vanilla.parentKey)) children.add(candidate);
            return List.copyOf(children);
        }

        @Override public Advancement getRoot() {
            Advancement current = this;
            var visited = new LinkedHashSet<NamespacedKey>();
            while (current.getParent() != null && visited.add(current.getKey())) current = current.getParent();
            return current;
        }

        @Override public String toString() { return "Advancement[" + key + "]"; }
    }

    /** The display fields in the vanilla advancement JSON that the Paper interface exposes. */
    private static final class Display implements AdvancementDisplay {
        private final JsonObject json;

        Display(JsonObject json) { this.json = json; }

        private String string(String field) {
            var value = json.get(field);
            return value == null || value.isJsonNull() ? null : value.getAsString();
        }

        private boolean bool(String field, boolean fallback) {
            var value = json.get(field);
            return value == null || value.isJsonNull() ? fallback : value.getAsBoolean();
        }

        private Component text(String field) {
            var value = json.get(field);
            if (value == null || value.isJsonNull()) return Component.empty();
            try {
                return GsonComponentSerializer.gson().deserialize(value.toString());
            } catch (RuntimeException malformed) {
                return Component.text(value.toString());
            }
        }

        @Override public Frame frame() {
            var frame = string("frame");
            if (frame == null) return Frame.TASK;
            return switch (frame.toLowerCase(Locale.ROOT)) {
                case "challenge" -> Frame.CHALLENGE;
                case "goal" -> Frame.GOAL;
                default -> Frame.TASK;
            };
        }

        @Override public Component title() { return text("title"); }
        @Override public Component description() { return text("description"); }

        @Override public ItemStack icon() {
            var value = json.get("icon");
            if (value == null || !value.isJsonObject()) return new ItemStack(Material.AIR);
            var icon = value.getAsJsonObject();
            var id = icon.get("id");
            if (id == null || id.isJsonNull()) return new ItemStack(Material.AIR);
            var material = Material.matchMaterial(id.getAsString());
            if (material == null) return new ItemStack(Material.AIR);
            int count = icon.has("count") ? icon.get("count").getAsInt() : 1;
            return new FerriteItemStack(material, count);
        }

        @Override public boolean doesShowToast() { return bool("show_toast", true); }
        @Override public boolean doesAnnounceToChat() { return bool("announce_to_chat", true); }
        @Override public boolean isHidden() { return bool("hidden", false); }

        @Override public NamespacedKey backgroundPath() {
            var background = string("background");
            if (background == null) return null;
            try {
                return NamespacedKey.fromString(background.contains(":") ? background : "minecraft:" + background);
            } catch (IllegalArgumentException malformed) {
                return null;
            }
        }

        @Override public Component displayName() { return title(); }
    }

    /** One alternative set from the advancement's {@code requirements} array. */
    private record Requirement(Set<String> criteria) implements AdvancementRequirement {
        @Override public List<String> getRequiredCriteria() { return List.copyOf(criteria); }
        /** Vanilla lists every alternative in one group unless the data marks the entry strict. */
        @Override public boolean isStrict() { return false; }
    }
}
