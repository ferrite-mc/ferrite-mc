package dev.ferrite.api.registry;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.io.IOException;
import java.io.Reader;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import java.util.stream.Stream;

/**
 * Reads the generated Minecraft 26.3 data tree: {@code reports/registries.json} plus the
 * datapack directories under {@code data/minecraft}.
 *
 * <p>The tree is produced by {@code tools/fetch_vanilla.py} into the gitignored
 * {@code vanilla/26.3/generated}. A plugin host may point at it with the
 * {@value #ROOT_PROPERTY} system property or the {@value #ROOT_ENVIRONMENT} environment
 * variable; otherwise the loader walks up from the working directory looking for the
 * repository layout.
 *
 * <p>{@link #get()} is lenient: when no tree is present every collection is empty, which
 * keeps the facade usable without generated data. {@link #load(Path)} is strict and is
 * what tests use so a missing fixture fails loudly instead of quietly returning nothing.
 */
public final class VanillaData {

    public static final String ROOT_PROPERTY = "ferrite.vanilla.generated";
    public static final String ROOT_ENVIRONMENT = "FERRITE_VANILLA_GENERATED";
    public static final String VERSION = "26.3";

    private static final String REPORTS = "reports/registries.json";
    private static final String BLOCKS_REPORT = "reports/blocks.json";
    private static final String DATA = "data/minecraft";

    private static volatile VanillaData instance;

    private final Path root;
    private final Map<String, Map<String, JsonObject>> registries;
    private final Map<String, JsonObject> enchantments;
    private final Map<String, JsonObject> damageTypes;
    private final Map<String, JsonObject> biomes;
    private final Map<String, JsonObject> jukeboxSongs;
    private final Set<String> blocks;
    private final Map<String, List<String>> enchantmentTags;
    private final Map<String, List<String>> itemTags;
    private final Map<String, List<String>> blockTags;
    private final Map<String, List<String>> entityTypeTags;
    private final Map<String, Set<String>> enchantmentTagValues = new ConcurrentHashMap<>();
    private final Map<String, Set<String>> itemTagValues = new ConcurrentHashMap<>();
    private final Map<String, Set<String>> blockTagValues = new ConcurrentHashMap<>();
    private final Map<String, Set<String>> entityTypeTagValues = new ConcurrentHashMap<>();

    private VanillaData(
            Path root,
            Map<String, Map<String, JsonObject>> registries,
            Map<String, JsonObject> enchantments,
            Map<String, JsonObject> damageTypes,
            Map<String, JsonObject> biomes,
            Map<String, JsonObject> jukeboxSongs,
            Set<String> blocks,
            Map<String, List<String>> enchantmentTags,
            Map<String, List<String>> itemTags,
            Map<String, List<String>> blockTags,
            Map<String, List<String>> entityTypeTags) {
        this.root = root;
        this.registries = registries;
        this.enchantments = enchantments;
        this.damageTypes = damageTypes;
        this.biomes = biomes;
        this.jukeboxSongs = jukeboxSongs;
        this.blocks = blocks;
        this.enchantmentTags = enchantmentTags;
        this.itemTags = itemTags;
        this.blockTags = blockTags;
        this.entityTypeTags = entityTypeTags;
    }

    /** Returns the shared, lazily loaded tree; empty collections when it is absent. */
    public static VanillaData get() {
        VanillaData local = instance;
        if (local == null) {
            synchronized (VanillaData.class) {
                local = instance;
                if (local == null) {
                    local = resolveRoot().map(VanillaData::load).orElseGet(VanillaData::empty);
                    instance = local;
                }
            }
        }
        return local;
    }

    /** Loads the tree at {@code root}, failing when the expected reports are missing. */
    public static VanillaData load(Path root) {
        Path reportsFile = root.resolve(REPORTS);
        if (!Files.isRegularFile(reportsFile)) {
            throw new IllegalStateException(
                    "No generated vanilla data at " + root.toAbsolutePath()
                            + " (missing " + REPORTS + "); run `python tools/fetch_vanilla.py`");
        }
        Path data = root.resolve(DATA);
        return new VanillaData(
                root,
                readRegistries(reportsFile),
                readDirectory(data.resolve("enchantment")),
                readDirectory(data.resolve("damage_type")),
                readDirectory(data.resolve("worldgen/biome")),
                readDirectory(data.resolve("jukebox_song")),
                readBlockKeys(root.resolve(BLOCKS_REPORT)),
                readTags(data.resolve("tags/enchantment")),
                readTags(data.resolve("tags/item")),
                readTags(data.resolve("tags/block")),
                readTags(data.resolve("tags/entity_type")));
    }

    private static VanillaData empty() {
        return new VanillaData(
                null,
                Map.of(),
                Map.of(),
                Map.of(),
                Map.of(),
                Map.of(),
                Set.of(),
                Map.of(),
                Map.of(),
                Map.of(),
                Map.of());
    }

    /** Whether a generated tree was found. */
    public boolean available() {
        return this.root != null;
    }

    /** The tree root, or {@code null} when no data was found. */
    public Path root() {
        return this.root;
    }

    public Map<String, JsonObject> enchantments() {
        return this.enchantments;
    }

    public Map<String, JsonObject> damageTypes() {
        return this.damageTypes;
    }

    public Map<String, JsonObject> biomes() {
        return this.biomes;
    }

    public Map<String, JsonObject> jukeboxSongs() {
        return this.jukeboxSongs;
    }

    /** Every block id from {@code reports/blocks.json}; empty when the report is absent. */
    public Set<String> blocks() {
        return this.blocks;
    }

    /** Whether {@code id} (namespaced or bare) is a block in the 26.3 block report. */
    public boolean isBlock(String id) {
        return this.blocks.contains(namespace(id));
    }

    /** Entries of a {@code reports/registries.json} registry, keyed by namespaced id. */
    public Map<String, JsonObject> registry(String registry) {
        return this.registries.getOrDefault(registry, Map.of());
    }

    /** Whether an enchantment (namespaced id) belongs to a datapack tag such as {@code treasure}. */
    public boolean enchantmentTagContains(String tag, String key) {
        return tagMembers(this.enchantmentTags, this.enchantmentTagValues, tag).contains(namespace(key));
    }

    /** Resolves an item selector ({@code minecraft:foo} or {@code #minecraft:tag}) to item ids. */
    public Set<String> resolveItemSelector(String selector) {
        if (selector == null || selector.isBlank()) {
            return Set.of();
        }
        if (selector.startsWith("#")) {
            return tagMembers(this.itemTags, this.itemTagValues, selector.substring(1));
        }
        return Set.of(namespace(selector));
    }

    /** Resolves every string selector in an array to item ids. */
    public Set<String> resolveItemSelectors(JsonArray selectors) {
        Set<String> keys = new LinkedHashSet<>();
        for (JsonElement element : selectors) {
            if (element.isJsonPrimitive()) {
                keys.addAll(resolveItemSelector(element.getAsString()));
            }
        }
        return Set.copyOf(keys);
    }

    /** Returns all known tag names for a Bukkit tag registry name ({@code blocks}, {@code items}, {@code entity_types}). */
    public Set<String> tagNames(String registry) {
        return switch (registry) {
            case "blocks" -> blockTags.keySet();
            case "items" -> itemTags.keySet();
            case "entity_types" -> entityTypeTags.keySet();
            case "enchantments" -> enchantmentTags.keySet();
            default -> Set.of();
        };
    }

    /** Expands a named tag in a Bukkit tag registry ({@code blocks}, {@code items}, {@code entity_types}). */
    public Set<String> tagValues(String registry, String tag) {
        String name = namespace(tag);
        return switch (registry) {
            case "blocks" -> blockTags.containsKey(name)
                    ? tagMembers(blockTags, blockTagValues, name)
                    : Set.of();
            case "items" -> itemTags.containsKey(name)
                    ? tagMembers(itemTags, itemTagValues, name)
                    : Set.of();
            case "entity_types" -> entityTypeTags.containsKey(name)
                    ? tagMembers(entityTypeTags, entityTypeTagValues, name)
                    : Set.of();
            case "enchantments" -> enchantmentTags.containsKey(name)
                    ? tagMembers(enchantmentTags, enchantmentTagValues, name)
                    : Set.of();
            default -> Set.of();
        };
    }

    /** Resolves enchantment selectors, which may themselves refer to tag files. */
    public Set<String> resolveEnchantmentSelectors(JsonArray selectors) {
        Set<String> keys = new LinkedHashSet<>();
        for (JsonElement element : selectors) {
            if (!element.isJsonPrimitive()) {
                continue;
            }
            String selector = element.getAsString();
            if (selector.startsWith("#")) {
                keys.addAll(tagMembers(this.enchantmentTags, this.enchantmentTagValues, selector.substring(1)));
            } else {
                keys.add(namespace(selector));
            }
        }
        return Set.copyOf(keys);
    }

    private Set<String> tagMembers(
            Map<String, List<String>> tags, Map<String, Set<String>> cache, String tag) {
        String name = namespace(tag);
        Set<String> cached = cache.get(name);
        if (cached != null) {
            return cached;
        }
        Set<String> expanded = Set.copyOf(expand(tags, name, new LinkedHashSet<>()));
        cache.put(name, expanded);
        return expanded;
    }

    private static Set<String> expand(
            Map<String, List<String>> tags, String tag, Set<String> visiting) {
        List<String> values = tags.get(tag);
        if (values == null || !visiting.add(tag)) {
            return Set.of();
        }
        Set<String> result = new LinkedHashSet<>();
        for (String value : values) {
            if (value.startsWith("#")) {
                result.addAll(expand(tags, namespace(value.substring(1)), visiting));
            } else {
                result.add(namespace(value));
            }
        }
        visiting.remove(tag);
        return result;
    }

    private static Map<String, Map<String, JsonObject>> readRegistries(Path file) {
        JsonObject reports = readJson(file);
        Map<String, Map<String, JsonObject>> registries = new LinkedHashMap<>();
        for (Map.Entry<String, JsonElement> registry : reports.entrySet()) {
            if (!registry.getValue().isJsonObject()) {
                continue;
            }
            JsonObject body = registry.getValue().getAsJsonObject();
            JsonObject entries = body.has("entries") && body.get("entries").isJsonObject()
                    ? body.getAsJsonObject("entries")
                    : new JsonObject();
            Map<String, JsonObject> values = new LinkedHashMap<>();
            for (Map.Entry<String, JsonElement> entry : entries.entrySet()) {
                if (entry.getValue().isJsonObject()) {
                    values.put(entry.getKey(), entry.getValue().getAsJsonObject());
                }
            }
            registries.put(registry.getKey(), values);
        }
        return registries;
    }

    private static Set<String> readBlockKeys(Path file) {
        if (!Files.isRegularFile(file)) {
            return Set.of();
        }
        return Set.copyOf(readJson(file).keySet());
    }

    private static Map<String, JsonObject> readDirectory(Path directory) {
        if (!Files.isDirectory(directory)) {
            return Map.of();
        }
        Map<String, JsonObject> entries = new LinkedHashMap<>();
        try (Stream<Path> paths = Files.walk(directory)) {
            paths.filter(Files::isRegularFile)
                    .filter(path -> path.toString().endsWith(".json"))
                    .sorted()
                    .forEach(path -> {
                        String relative = directory.relativize(path).toString().replace('\\', '/');
                        String name = relative.substring(0, relative.length() - ".json".length());
                        entries.put(name, readJson(path));
                    });
        } catch (IOException exception) {
            throw new UncheckedIOException(exception);
        }
        return entries;
    }

    private static Map<String, List<String>> readTags(Path directory) {
        if (!Files.isDirectory(directory)) {
            return Map.of();
        }
        Map<String, List<String>> tags = new LinkedHashMap<>();
        try (Stream<Path> paths = Files.walk(directory)) {
            paths.filter(Files::isRegularFile)
                    .filter(path -> path.toString().endsWith(".json"))
                    .sorted()
                    .forEach(path -> {
                        String relative = directory.relativize(path).toString().replace('\\', '/');
                        String name = "minecraft:"
                                + relative.substring(0, relative.length() - ".json".length());
                        JsonObject json = readJson(path);
                        List<String> values = new ArrayList<>();
                        if (json.has("values") && json.get("values").isJsonArray()) {
                            for (JsonElement element : json.getAsJsonArray("values")) {
                                if (element.isJsonPrimitive()) {
                                    values.add(element.getAsString());
                                } else if (element.isJsonObject()
                                        && element.getAsJsonObject().has("id")) {
                                    values.add(element.getAsJsonObject().get("id").getAsString());
                                }
                            }
                        }
                        tags.put(name, List.copyOf(values));
                    });
        } catch (IOException exception) {
            throw new UncheckedIOException(exception);
        }
        return tags;
    }

    private static JsonObject readJson(Path file) {
        try (Reader reader = Files.newBufferedReader(file, StandardCharsets.UTF_8)) {
            return JsonParser.parseReader(reader).getAsJsonObject();
        } catch (IOException exception) {
            throw new UncheckedIOException("Failed to read " + file, exception);
        }
    }

    private static Optional<Path> resolveRoot() {
        String property = System.getProperty(ROOT_PROPERTY);
        if (property != null && !property.isBlank()) {
            return Optional.of(Path.of(property).toAbsolutePath().normalize());
        }
        String environment = System.getenv(ROOT_ENVIRONMENT);
        if (environment != null && !environment.isBlank()) {
            return Optional.of(Path.of(environment).toAbsolutePath().normalize());
        }
        Path directory = Path.of("").toAbsolutePath();
        for (int depth = 0; depth < 6 && directory != null; depth++) {
            Path candidate = directory.resolve("vanilla").resolve(VERSION).resolve("generated");
            if (Files.isRegularFile(candidate.resolve(REPORTS))) {
                return Optional.of(candidate);
            }
            directory = directory.getParent();
        }
        return Optional.empty();
    }

    static String namespace(String key) {
        return key.indexOf(':') >= 0 ? key : "minecraft:" + key;
    }

    static int integer(JsonObject object, String member, int fallback) {
        JsonElement element = object.get(member);
        return element != null && element.isJsonPrimitive() ? element.getAsInt() : fallback;
    }

    static String string(JsonObject object, String member, String fallback) {
        JsonElement element = object.get(member);
        return element != null && element.isJsonPrimitive() ? element.getAsString() : fallback;
    }
}
