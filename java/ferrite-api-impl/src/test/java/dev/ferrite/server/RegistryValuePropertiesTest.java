package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import com.google.gson.JsonParser;
import io.papermc.paper.registry.RegistryAccess;
import io.papermc.paper.registry.RegistryKey;
import java.nio.file.Files;
import java.nio.file.Path;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.TranslatableComponent;
import net.kyori.adventure.text.serializer.gson.GsonComponentSerializer;
import org.bukkit.NamespacedKey;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class RegistryValuePropertiesTest {
    private static final Path DATA = Path.of(System.getProperty("test.vanilla.data"));
    @TempDir Path temporary;
    private HostBridge.Transport transport;

    @BeforeEach void configure() {
        Bootstrap.configureDataDir(DATA.toString());
        transport = HostBridge.transport;
        HostBridge.transport = (owner, subject, method, args) -> {
            throw new AssertionError("Registry property reached host: " + owner + "#" + method);
        };
    }
    @AfterEach void restore() {
        HostBridge.transport = transport;
        Bootstrap.configureDataDir(DATA.toString());
    }

    @Test void everySongMatchesCanonicalFieldsAndTypedSound() throws Exception {
        var registry = RegistryAccess.registryAccess().getRegistry(RegistryKey.JUKEBOX_SONG);
        for (var song : registry) {
            var json = json("jukebox_song", song.getKey());
            assertEquals(json.get("length_in_seconds").getAsFloat(), song.getLengthInSeconds());
            assertEquals(json.get("comparator_output").getAsInt(), song.getComparatorOutput());
            assertSame(org.bukkit.Registry.SOUND_EVENT.get(NamespacedKey.fromString(json.get("sound_event").getAsString())), song.getSound());
            assertEquals(component(json.get("description")), song.getDescription());
            assertEquals(((TranslatableComponent) song.getDescription()).key(), song.getTranslationKey());
        }
    }

    @Test void everyPaintingMatchesDimensionsAssetAndOptionalComponents() throws Exception {
        var registry = RegistryAccess.registryAccess().getRegistry(RegistryKey.PAINTING_VARIANT);
        for (var art : registry) {
            var json = json("painting_variant", art.getKey());
            assertEquals(json.get("width").getAsInt(), art.getBlockWidth());
            assertEquals(json.get("height").getAsInt(), art.getBlockHeight());
            assertEquals(json.get("asset_id").getAsString(), art.assetId().asString());
            assertEquals(component(json.get("title")), art.title());
            assertEquals(component(json.get("author")), art.author());
        }
        assertNull(org.bukkit.Art.EARTH.author());
        assertNotNull(org.bukkit.Art.KEBAB.author());
    }

    @Test void trimDescriptionsPreserveStyleAndTranslation() throws Exception {
        for (var material : org.bukkit.Registry.TRIM_MATERIAL) {
            assertEquals(component(json("trim_material", material.getKey()).get("description")), material.description());
            assertEquals(((TranslatableComponent) material.description()).key(), material.getTranslationKey());
        }
        for (var pattern : org.bukkit.Registry.TRIM_PATTERN) {
            assertEquals(component(json("trim_pattern", pattern.getKey()).get("description")), pattern.description());
            assertEquals(((TranslatableComponent) pattern.description()).key(), pattern.getTranslationKey());
        }
    }

    @Test void allInstrumentsMatchCanonicalDataWithoutAHost() throws Exception {
        HostBridge.transport = null;
        for (var instrument : org.bukkit.Registry.INSTRUMENT) {
            var json = json("instrument", instrument.getKey());
            assertEquals(json.get("use_duration").getAsFloat(), instrument.getDuration());
            assertEquals(json.get("range").getAsFloat(), instrument.getRange());
            assertEquals(component(json.get("description")), instrument.description());
            assertSame(org.bukkit.Registry.SOUND_EVENT.get(NamespacedKey.fromString(json.get("sound_event").getAsString())), instrument.getSound());
            assertEquals(instrument, new FerriteMusicInstrument(instrument.getKey()));
            assertEquals(instrument.hashCode(), new FerriteMusicInstrument(instrument.getKey()).hashCode());
        }
        assertEquals(185f, org.bukkit.JukeboxSong.CAT.getLengthInSeconds());
        assertEquals(1, org.bukkit.Art.KEBAB.getBlockWidth());
    }

    @Test void trimValuesPreserveTypedReferencesAndKeyEqualityAcrossInstances() throws Exception {
        var material = org.bukkit.inventory.meta.trim.TrimMaterial.IRON;
        var pattern = org.bukkit.inventory.meta.trim.TrimPattern.SENTRY;
        var otherMaterial = isolated("trim_material", org.bukkit.inventory.meta.trim.TrimMaterial.class).get(material.getKey());
        var otherPattern = isolated("trim_pattern", org.bukkit.inventory.meta.trim.TrimPattern.class).get(pattern.getKey());
        assertEquals(material, otherMaterial);
        assertEquals(otherMaterial, material);
        assertEquals(material.hashCode(), otherMaterial.hashCode());
        var trim = new org.bukkit.inventory.meta.trim.ArmorTrim(material, pattern);
        assertSame(material, trim.getMaterial());
        assertSame(pattern, trim.getPattern());
        assertEquals(pattern, otherPattern);
        // The installed ArmorTrim.equals uses reference comparisons, so registry interning matters.
        assertEquals(trim, new org.bukkit.inventory.meta.trim.ArmorTrim(
                org.bukkit.Registry.TRIM_MATERIAL.get(material.getKey()), org.bukkit.Registry.TRIM_PATTERN.get(pattern.getKey())));
        assertNotEquals(trim, new org.bukkit.inventory.meta.trim.ArmorTrim(otherMaterial, otherPattern));
        assertNotEquals(material, pattern);
        assertNotEquals(material, null);
    }

    @Test void installedVariantTypesAreCachedAndHaveNoInventedPropertyMethods() {
        var access = RegistryAccess.registryAccess();
        for (var type : java.util.List.of(org.bukkit.entity.Wolf.Variant.class, org.bukkit.entity.Pig.Variant.class,
                org.bukkit.entity.Cow.Variant.class, org.bukkit.entity.Chicken.Variant.class)) {
            var registry = access.getRegistry(type);
            for (var value : registry) {
                assertTrue(type.isInstance(value));
                assertSame(value, registry.get(value.getKey()));
                assertEquals(value.getKey(), value.key());
            }
            assertEquals(0, java.util.Arrays.stream(type.getDeclaredMethods()).filter(method -> !java.lang.reflect.Modifier.isStatic(method.getModifiers())).count());
        }
        assertNotEquals(org.bukkit.entity.Pig.Variant.COLD, org.bukkit.entity.Cow.Variant.COLD);
    }

    @Test void legacyVariantAndPaintingIdsFollowTheHostsSortedGeneratedKeys() throws Exception {
        assertOrder("cat_variant", org.bukkit.entity.Cat.Type.class);
        assertOrder("frog_variant", org.bukkit.entity.Frog.Variant.class);
        assertOrder("painting_variant", org.bukkit.Art.class);
    }

    private static <T extends org.bukkit.Keyed & org.bukkit.util.OldEnum<T>> void assertOrder(String kind, Class<T> type) throws Exception {
        var registry = RegistryAccess.registryAccess().getRegistry(type);
        java.util.List<String> keys;
        try (var files = Files.list(DATA.resolve("data/minecraft/" + kind))) {
            keys = files.map(path -> "minecraft:" + path.getFileName().toString().replace(".json", "")).sorted().toList();
        }
        for (int index = 0; index < keys.size(); index++) {
            var value = registry.get(NamespacedKey.fromString(keys.get(index)));
            assertEquals(index, value.ordinal());
            assertEquals(0, value.compareTo(value));
            assertEquals(Integer.signum(index), Integer.signum(value.compareTo(registry.get(NamespacedKey.fromString(keys.getFirst())))));
            if (value instanceof org.bukkit.Art art) assertEquals(index, art.getId());
        }
    }

    @Test void concurrentReadsReturnCachedImmutableValues() throws Exception {
        var registry = isolated("jukebox_song", org.bukkit.JukeboxSong.class);
        var song = registry.get(NamespacedKey.minecraft("cat"));
        java.util.stream.IntStream.range(0, 512).parallel().forEach(index -> {
            assertSame(song, registry.get(song.getKey()));
            assertEquals(185f, song.getLengthInSeconds());
            assertEquals(2, song.getComparatorOutput());
            assertEquals(song.getDescription(), song.getDescription().append(Component.text("changed")).children(java.util.List.of()));
            assertEquals(song.getKey().hashCode(), song.hashCode());
        });
        assertNotEquals(song, registry.get(NamespacedKey.minecraft("blocks")));
        assertNotEquals(song, org.bukkit.Sound.MUSIC_DISC_CAT);
    }

    @Test void instrumentRejectsInvalidNumbersAndUnknownSoundWithoutHostFallback() throws Exception {
        Bootstrap.configureDataDir(temporary.toString());
        write("instrument", "bad", "{\"use_duration\":-1,\"range\":0,\"sound_event\":\"minecraft:missing\"}");
        var instrument = new FerriteMusicInstrument(NamespacedKey.minecraft("bad"));
        assertThrows(IllegalStateException.class, instrument::getDuration);
        assertThrows(IllegalStateException.class, instrument::getRange);
        assertThrows(IllegalStateException.class, instrument::getSound);
    }

    @Test void numericBoundariesFollowInstalledRegistryEntryAnnotations() throws Exception {
        Bootstrap.configureDataDir(temporary.toString());
        write("instrument", "zero", "{\"use_duration\":0,\"range\":1}");
        assertEquals(0f, new FerriteMusicInstrument(NamespacedKey.minecraft("zero")).getDuration());
        write("jukebox_song", "cat", "{\"length_in_seconds\":0}");
        var song = isolated("jukebox_song", org.bukkit.JukeboxSong.class).get(NamespacedKey.minecraft("cat"));
        assertThrows(IllegalStateException.class, song::getLengthInSeconds);
    }

    @Test void negativeDurationCannotUnderflowIntoAnAcceptedZero() throws Exception {
        Bootstrap.configureDataDir(temporary.toString());
        write("instrument", "negative", "{\"use_duration\":-1e-100}");
        var instrument = new FerriteMusicInstrument(NamespacedKey.minecraft("negative"));
        assertThrows(IllegalStateException.class, instrument::getDuration);
    }

    // A fresh registry avoids contaminating the process-wide cache with temporary data snapshots.
    @SuppressWarnings("unchecked")
    private static <T extends org.bukkit.Keyed> org.bukkit.Registry<T> isolated(String kind, Class<T> type) throws Exception {
        var factory = FerriteRegistryAccess.class.getDeclaredMethod("identityRegistry", String.class, Class.class);
        factory.setAccessible(true);
        return (org.bukkit.Registry<T>) factory.invoke(null, kind, type);
    }

    @Test void missingMalformedAndInvalidSongDataFailExplicitlyAndNeverReachHost() throws Exception {
        Bootstrap.configureDataDir(temporary.toString());
        var songs = isolated("jukebox_song", org.bukkit.JukeboxSong.class);
        var song = songs.get(NamespacedKey.minecraft("cat"));
        assertThrows(UnsupportedOperationException.class, song::getDescription);
        write("jukebox_song", "cat", "{broken");
        assertThrows(IllegalStateException.class, song::getDescription);
        write("jukebox_song", "cat", "{}");
        assertThrows(IllegalStateException.class, song::getSound);
        assertThrows(IllegalStateException.class, song::getLengthInSeconds);
        assertThrows(IllegalStateException.class, song::getDescription);
        assertNull(songs.get(NamespacedKey.minecraft("unknown")));

        for (String contents : java.util.List.of(
                "{\"comparator_output\":16}", "{\"comparator_output\":-1}",
                "{\"comparator_output\":2.5}", "{\"comparator_output\":\"2\"}")) {
            write("jukebox_song", "cat", contents);
            var fresh = isolated("jukebox_song", org.bukkit.JukeboxSong.class).get(song.getKey());
            assertThrows(IllegalStateException.class, fresh::getComparatorOutput, contents);
        }
        for (String number : java.util.List.of("-1", "1e100", "\"185\"")) {
            write("jukebox_song", "cat", "{\"length_in_seconds\":" + number + "}");
            var fresh = isolated("jukebox_song", org.bukkit.JukeboxSong.class).get(song.getKey());
            assertThrows(IllegalStateException.class, fresh::getLengthInSeconds, number);
        }
    }

    @Test void paintingOptionalMeansAbsentNotMalformedAndDimensionsAreBounded() throws Exception {
        Bootstrap.configureDataDir(temporary.toString());
        for (String contents : java.util.List.of("{\"width\":0}", "{\"width\":17}", "{\"width\":1.5}")) {
            write("painting_variant", "kebab", contents);
            var art = isolated("painting_variant", org.bukkit.Art.class).get(NamespacedKey.minecraft("kebab"));
            assertThrows(IllegalStateException.class, art::getBlockWidth);
        }
        write("painting_variant", "kebab", "{\"asset_id\":\"INVALID KEY\",\"title\":null}");
        var art = isolated("painting_variant", org.bukkit.Art.class).get(NamespacedKey.minecraft("kebab"));
        assertNull(art.author());
        assertThrows(IllegalStateException.class, art::title);
        assertThrows(IllegalStateException.class, art::assetId);
    }

    @Test void successfulReadsAreSnapshotsAndDoNotPromiseReload() throws Exception {
        Bootstrap.configureDataDir(temporary.toString());
        write("jukebox_song", "cat", "{\"length_in_seconds\":0.5,\"comparator_output\":15,\"description\":{\"text\":\"Custom\"}}");
        var song = isolated("jukebox_song", org.bukkit.JukeboxSong.class).get(NamespacedKey.minecraft("cat"));
        assertEquals(0.5f, song.getLengthInSeconds());
        assertEquals(15, song.getComparatorOutput());
        assertEquals(Component.text("Custom"), song.getDescription());
        // The legacy translation accessor cannot manufacture a translation for a literal description.
        assertThrows(UnsupportedOperationException.class, song::getTranslationKey);
        write("jukebox_song", "cat", "{\"length_in_seconds\":123}");
        assertEquals(0.5f, song.getLengthInSeconds());
        Bootstrap.configureDataDir(DATA.toString());
        assertEquals(0.5f, song.getLengthInSeconds());
    }

    private void write(String kind, String key, String contents) throws Exception {
        var path = temporary.resolve("data/minecraft/" + kind + "/" + key + ".json");
        Files.createDirectories(path.getParent());
        Files.writeString(path, contents);
    }
    private static com.google.gson.JsonObject json(String kind, NamespacedKey key) throws Exception {
        return JsonParser.parseString(Files.readString(DATA.resolve("data/" + key.getNamespace() + "/" + kind + "/" + key.getKey() + ".json"))).getAsJsonObject();
    }
    private static Component component(com.google.gson.JsonElement json) {
        return json == null ? null : GsonComponentSerializer.gson().deserialize(json.toString());
    }
}
