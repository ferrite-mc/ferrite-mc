package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.gson.JsonParser;
import io.papermc.paper.registry.RegistryAccess;
import io.papermc.paper.registry.RegistryKey;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;
import java.util.stream.Collectors;
import net.kyori.adventure.text.TranslatableComponent;
import org.bukkit.NamespacedKey;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

/** Registries are backed by the locally generated vanilla data; counts are recomputed here without the generator. */
class FerriteRegistryBackingTest {
    private static final Path DATA = Path.of(System.getProperty("test.vanilla.data"));

    @BeforeAll static void configure() { Bootstrap.configureDataDir(System.getProperty("test.vanilla.data")); }

    private static int reportCount(String registry) throws IOException {
        var json = JsonParser.parseString(Files.readString(DATA.resolve("reports/registries.json"), StandardCharsets.UTF_8)).getAsJsonObject();
        return json.getAsJsonObject("minecraft:" + registry).getAsJsonObject("entries").size();
    }

    private static int dataFileCount(String directory) throws IOException {
        try (var files = Files.walk(DATA.resolve("data/minecraft/" + directory))) {
            return (int) files.filter(path -> path.toString().endsWith(".json")).count();
        }
    }

    private static final Map<String, RegistryKey<?>> BUILTIN = Map.of(
            "villager_profession", RegistryKey.VILLAGER_PROFESSION,
            "point_of_interest_type", RegistryKey.POINT_OF_INTEREST_TYPE,
            "villager_type", RegistryKey.VILLAGER_TYPE,
            "data_component_type", RegistryKey.DATA_COMPONENT_TYPE,
            "particle_type", RegistryKey.PARTICLE_TYPE,
            "potion", RegistryKey.POTION,
            "menu", RegistryKey.MENU,
            "entity_type", RegistryKey.ENTITY_TYPE,
            "block", RegistryKey.BLOCK,
            "item", RegistryKey.ITEM);

    private static final Map<String, RegistryKey<?>> DATA_DRIVEN = Map.ofEntries(
            Map.entry("trim_material", RegistryKey.TRIM_MATERIAL),
            Map.entry("trim_pattern", RegistryKey.TRIM_PATTERN),
            Map.entry("wolf_variant", RegistryKey.WOLF_VARIANT),
            Map.entry("wolf_sound_variant", RegistryKey.WOLF_SOUND_VARIANT),
            Map.entry("jukebox_song", RegistryKey.JUKEBOX_SONG),
            Map.entry("banner_pattern", RegistryKey.BANNER_PATTERN),
            Map.entry("decorated_pot_pattern", RegistryKey.DECORATED_POT_PATTERN),
            Map.entry("painting_variant", RegistryKey.PAINTING_VARIANT),
            Map.entry("instrument", RegistryKey.INSTRUMENT),
            Map.entry("cat_variant", RegistryKey.CAT_VARIANT),
            Map.entry("cat_sound_variant", RegistryKey.CAT_SOUND_VARIANT),
            Map.entry("frog_variant", RegistryKey.FROG_VARIANT),
            Map.entry("chicken_variant", RegistryKey.CHICKEN_VARIANT),
            Map.entry("chicken_sound_variant", RegistryKey.CHICKEN_SOUND_VARIANT),
            Map.entry("cow_variant", RegistryKey.COW_VARIANT),
            Map.entry("cow_sound_variant", RegistryKey.COW_SOUND_VARIANT),
            Map.entry("pig_variant", RegistryKey.PIG_VARIANT),
            Map.entry("pig_sound_variant", RegistryKey.PIG_SOUND_VARIANT),
            Map.entry("zombie_nautilus_variant", RegistryKey.ZOMBIE_NAUTILUS_VARIANT),
            Map.entry("sulfur_cube_archetype", RegistryKey.SULFUR_CUBE_ARCHETYPE),
            Map.entry("dialog", RegistryKey.DIALOG));

    @SuppressWarnings({"unchecked", "rawtypes"})
    private static org.bukkit.Registry<?> registry(RegistryKey<?> key) {
        return RegistryAccess.registryAccess().getRegistry((RegistryKey) key);
    }

    @SuppressWarnings("unchecked")
    private static void assertConsistent(String name, org.bukkit.Registry<?> wildcard, int expected) {
        var registry = (org.bukkit.Registry<org.bukkit.Keyed>) wildcard;
        assertEquals(expected, registry.size(), name + " size");
        var keys = registry.keyStream().toList();
        assertEquals(expected, keys.size(), name + " keyStream");
        assertEquals(expected, (int) registry.stream().count(), name + " stream");
        int iterated = 0;
        for (var value : registry) {
            iterated++;
            assertNotNull(value, name);
            assertSame(value, registry.get(value.getKey()), name + " " + value.getKey() + " is cached");
            assertEquals(value.getKey(), registry.getKey(value), name);
        }
        assertEquals(expected, iterated, name + " iterator");
        assertNull(registry.get(NamespacedKey.minecraft("no_such_" + name.replace('/', '_'))), name + " unknown key");
    }

    @Test void builtinRegistriesMatchTheGeneratedReport() throws IOException {
        for (var entry : BUILTIN.entrySet())
            assertConsistent(entry.getKey(), registry(entry.getValue()), reportCount(entry.getKey()));
    }

    @Test void dataDrivenRegistriesMatchTheGeneratedDataPackFiles() throws IOException {
        for (var entry : DATA_DRIVEN.entrySet())
            assertConsistent(entry.getKey(), registry(entry.getValue()), dataFileCount(entry.getKey()));
    }

    @Test void registrySizesAreTheVanillaNumbers() {
        assertEquals(15, registry(RegistryKey.VILLAGER_PROFESSION).size());
        assertEquals(9, registry(RegistryKey.WOLF_VARIANT).size());
        assertEquals(43, registry(RegistryKey.BANNER_PATTERN).size());
        assertEquals(51, registry(RegistryKey.PAINTING_VARIANT).size());
        assertEquals(46, registry(RegistryKey.POTION).size());
        assertEquals(128, registry(RegistryKey.PARTICLE_TYPE).size());
        assertEquals(1286, registry(RegistryKey.BLOCK).size());
    }

    @Test void identityRegistriesResolveNamedEntries() {
        var librarian = registry(RegistryKey.VILLAGER_PROFESSION).get(NamespacedKey.minecraft("librarian"));
        assertNotNull(librarian);
        assertEquals(NamespacedKey.minecraft("librarian"), librarian.getKey());
        assertSame(librarian, registry(RegistryKey.VILLAGER_PROFESSION).get(NamespacedKey.minecraft("librarian")));
        assertTrue(registry(RegistryKey.WOLF_VARIANT).keyStream().anyMatch(NamespacedKey.minecraft("ashen")::equals));
        assertTrue(registry(RegistryKey.PAINTING_VARIANT).keyStream().anyMatch(NamespacedKey.minecraft("kebab")::equals));
        assertNull(registry(RegistryKey.VILLAGER_PROFESSION).get(NamespacedKey.fromString("other:librarian")));
    }

    @Test void classLookupsAndKeyLookupsReturnTheSameRegistry() {
        var access = RegistryAccess.registryAccess();
        assertSame(registry(RegistryKey.PARTICLE_TYPE), access.getRegistry(org.bukkit.Particle.class));
        assertSame(registry(RegistryKey.POTION), access.getRegistry(org.bukkit.potion.PotionType.class));
        assertSame(registry(RegistryKey.INSTRUMENT), access.getRegistry(org.bukkit.MusicInstrument.class));
        assertSame(registry(RegistryKey.VILLAGER_PROFESSION), access.getRegistry(org.bukkit.entity.Villager.Profession.class));
    }

    @Test void enumBackedRegistriesReturnTheEnumConstants() {
        var particles = registry(RegistryKey.PARTICLE_TYPE);
        for (var particle : org.bukkit.Particle.values())
            assertSame(particle, particles.get(particle.getKey()), particle.toString());
        assertSame(org.bukkit.Particle.POOF, particles.get(NamespacedKey.minecraft("poof")));
        var potions = registry(RegistryKey.POTION);
        for (var type : org.bukkit.potion.PotionType.values())
            assertSame(type, potions.get(type.getKey()), type.toString());
        assertSame(org.bukkit.potion.PotionType.SWIFTNESS, potions.get(NamespacedKey.minecraft("swiftness")));
    }

    @Test void musicInstrumentsReadTheirGeneratedDataFile() {
        var ponder = org.bukkit.MusicInstrument.PONDER_GOAT_HORN;
        assertSame(ponder, org.bukkit.Registry.INSTRUMENT.get(NamespacedKey.minecraft("ponder_goat_horn")));
        assertEquals(NamespacedKey.minecraft("ponder_goat_horn"), ponder.getKey());
        assertEquals(7.0f, ponder.getDuration(), 0.0f);
        assertEquals(256.0f, ponder.getRange(), 0.0f);
        assertSame(org.bukkit.Registry.SOUND_EVENT.get(NamespacedKey.minecraft("item.goat_horn.sound.0")), ponder.getSound());
        assertEquals("instrument.minecraft.ponder_goat_horn", ponder.translationKey());
        var description = assertInstanceOf(TranslatableComponent.class, ponder.description());
        assertEquals("instrument.minecraft.ponder_goat_horn", description.key());
        assertEquals(8, org.bukkit.Registry.INSTRUMENT.stream().map(org.bukkit.MusicInstrument::getKey).collect(Collectors.toSet()).size());
    }

    @Test void registriesWithoutGeneratedValueDataFailExplicitly() {
        var gameRules = registry(RegistryKey.GAME_RULE);
        var gameRuleError = assertThrows(UnsupportedOperationException.class, gameRules::size);
        assertTrue(gameRuleError.getMessage().contains("minecraft:game_rule") && gameRuleError.getMessage().contains("value type"),
                gameRuleError.getMessage());
        assertThrows(UnsupportedOperationException.class, () -> gameRules.get(NamespacedKey.minecraft("advance_time")));
        var memory = registry(RegistryKey.MEMORY_MODULE_TYPE);
        var memoryError = assertThrows(UnsupportedOperationException.class, memory::keyStream);
        assertTrue(memoryError.getMessage().contains("minecraft:memory_module_type") && memoryError.getMessage().contains("private constructor"),
                memoryError.getMessage());
        var unknownClass = RegistryAccess.registryAccess().getRegistry(org.bukkit.Keyed.class);
        var classError = assertThrows(UnsupportedOperationException.class, unknownClass::size);
        assertTrue(classError.getMessage().contains("org.bukkit.Keyed"), classError.getMessage());
    }
}
