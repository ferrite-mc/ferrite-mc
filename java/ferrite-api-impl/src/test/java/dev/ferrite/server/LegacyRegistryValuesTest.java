package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;
import com.google.gson.JsonParser;
import java.nio.file.Files;
import java.nio.file.Path;
import org.bukkit.NamespacedKey;
import org.junit.jupiter.api.Test;

class LegacyRegistryValuesTest {
    @org.junit.jupiter.api.BeforeAll static void startApi() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
    }

    @Test void legacyValuesArraysFollowOrdinalAndKeepCanonicalConversions() {
        for (var values : java.util.List.of(org.bukkit.Sound.values(), org.bukkit.entity.Villager.Profession.values(), org.bukkit.entity.Villager.Type.values())) {
            for (int i = 0; i < values.length; i++) assertEquals(i, values[i].ordinal());
        }
        for (var sound : org.bukkit.Sound.values()) {
            assertSame(sound, org.bukkit.Sound.valueOf(sound.getKey().toString()));
            assertSame(sound, org.bukkit.Sound.valueOf(sound.name()));
        }
        for (var value : org.bukkit.entity.Villager.Profession.values())
            assertSame(value, org.bukkit.entity.Villager.Profession.valueOf(value.name()));
        for (var value : org.bukkit.entity.Villager.Type.values())
            assertSame(value, org.bukkit.entity.Villager.Type.valueOf(value.name()));
        assertThrows(IllegalArgumentException.class, () -> org.bukkit.Sound.valueOf("missing_sound"));
        assertThrows(IllegalArgumentException.class, () -> org.bukkit.entity.Villager.Type.valueOf("missing_type"));
        var copy = org.bukkit.Sound.values();
        copy[0] = null;
        assertNotNull(org.bukkit.Sound.values()[0]);
    }

    @Test void comparisonsAndFactoriesRejectUnknownOrWrongTypes() {
        assertThrows(NullPointerException.class, () -> org.bukkit.Sound.ENTITY_PLAYER_HURT.compareTo(null));
        assertThrows(ClassCastException.class, () -> ((Comparable) org.bukkit.Sound.ENTITY_PLAYER_HURT).compareTo(org.bukkit.entity.Villager.Type.DESERT));
        assertThrows(IllegalArgumentException.class, () -> FerriteLegacyRegistryValues.create("unknown", NamespacedKey.minecraft("missing"), org.bukkit.Sound.class));
        assertThrows(IllegalArgumentException.class, () -> FerriteLegacyRegistryValues.create("sound_event", NamespacedKey.minecraft("missing"), org.bukkit.Sound.class));
        assertNotEquals(org.bukkit.entity.Villager.Type.DESERT, org.bukkit.entity.Villager.Profession.LIBRARIAN);
    }

    @Test void concurrentReadsAndTypedKeyEqualityAreStable() {
        var registry = org.bukkit.Registry.SOUND_EVENT;
        var value = org.bukkit.Sound.ENTITY_PLAYER_HURT;
        java.util.stream.IntStream.range(0, 512).parallel().forEach(i -> {
            assertSame(value, registry.get(value.getKey()));
            assertEquals(0, value.compareTo(value));
            var independent = FerriteLegacyRegistryValues.create("sound_event", value.getKey(), org.bukkit.Sound.class);
            assertEquals(value, independent);
            assertEquals(independent, value);
            assertEquals(value.hashCode(), independent.hashCode());
        });
    }

    @Test void legacyApisMatchAllOfficialBuiltinIdsWithoutHost() throws Exception {
        var prior = HostBridge.transport;
        HostBridge.transport = (owner, subject, method, args) -> { throw new AssertionError("host call " + owner + method); };
        try {
            verify("sound_event", org.bukkit.Sound.class);
            verify("villager_profession", org.bukkit.entity.Villager.Profession.class);
            verify("villager_type", org.bukkit.entity.Villager.Type.class);
            assertEquals("entity.minecraft.villager.librarian", org.bukkit.entity.Villager.Profession.LIBRARIAN.translationKey());
        } finally { HostBridge.transport = prior; }
    }
    private static <T extends org.bukkit.Keyed & org.bukkit.util.OldEnum<T>> void verify(String kind, Class<T> type) throws Exception {
        var report = JsonParser.parseString(Files.readString(Path.of(System.getProperty("test.vanilla.data"), "reports/registries.json")))
                .getAsJsonObject().getAsJsonObject("minecraft:" + kind).getAsJsonObject("entries");
        var registry = io.papermc.paper.registry.RegistryAccess.registryAccess().getRegistry(type);
        assertEquals(report.size(), registry.size());
        var first = registry.iterator().next();
        for (var value : registry) {
            assertSame(value, registry.get(value.getKey()));
            assertEquals(report.getAsJsonObject(value.getKey().toString()).get("protocol_id").getAsInt(), value.ordinal());
            assertEquals(0, value.compareTo(value));
            assertEquals(Integer.signum(value.ordinal() - first.ordinal()), Integer.signum(value.compareTo(first)));
            assertEquals(value.getKey(), value.key());
            assertEquals(value.getKey().getKey().toUpperCase(java.util.Locale.ROOT).replace('.', '_'), value.name());
        }
        assertNull(registry.get(NamespacedKey.minecraft("missing_value")));
        assertNull(registry.get(NamespacedKey.fromString("other:librarian")));
    }
}
