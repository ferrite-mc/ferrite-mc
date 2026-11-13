package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;
import java.nio.file.Files;
import java.nio.file.Path;
import org.bukkit.NamespacedKey;
import org.bukkit.block.Biome;
import org.junit.jupiter.api.Test;

class BiomeRegistryValuesTest {
    @org.junit.jupiter.api.BeforeAll static void startApi() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
    }
    @Test void allBiomesUseTheHostsCanonicalSortedWireOrderAndApiTranslation() throws Exception {
        var previous = HostBridge.transport;
        HostBridge.transport = (owner, subject, method, args) -> { throw new AssertionError("host call " + owner + method); };
        try {
            java.util.List<String> keys;
            var directory = Path.of(System.getProperty("test.vanilla.data"), "data/minecraft/worldgen/biome");
            try (var files = Files.walk(directory)) {
                keys = files.filter(p -> p.toString().endsWith(".json"))
                        .map(p -> "minecraft:" + directory.relativize(p).toString().replace('\\', '/').replace(".json", "")).sorted().toList();
            }
            var values = Biome.values();
            assertEquals(keys.size(), values.length);
            for (int i = 0; i < values.length; i++) {
                var value = values[i];
                assertEquals(keys.get(i), value.getKey().toString());
                assertEquals(i, value.ordinal());
                assertEquals("biome.minecraft." + value.getKey().getKey(), value.translationKey());
                assertSame(value, Biome.valueOf(value.name()));
                assertSame(value, org.bukkit.Registry.BIOME.get(value.getKey()));
                assertEquals(0, value.compareTo(value));
                assertEquals(Integer.signum(i), Integer.signum(value.compareTo(values[0])));
            }
            assertNull(org.bukkit.Registry.BIOME.get(NamespacedKey.minecraft("custom")));
        } finally { HostBridge.transport = previous; }
    }

    @Test void deprecatedCustomIsNotInventedAsARealRegistryEntry() {
        assertSame(Biome.CUSTOM, Biome.valueOf("CUSTOM"));
        assertEquals("CUSTOM", Biome.CUSTOM.name());
        assertEquals("biome.minecraft.custom", Biome.CUSTOM.translationKey());
        assertThrows(UnsupportedOperationException.class, Biome.CUSTOM::ordinal);
        assertThrows(UnsupportedOperationException.class, () -> Biome.CUSTOM.compareTo(Biome.PLAINS));
        assertThrows(IllegalArgumentException.class, () -> Biome.PLAINS.compareTo(Biome.CUSTOM));
    }

    @Test void parallelBiomeReadsStayCanonicalAndArraysAreDefensive() {
        java.util.stream.IntStream.range(0, 512).parallel().forEach(i -> {
            assertSame(Biome.PLAINS, org.bukkit.Registry.BIOME.get(NamespacedKey.minecraft("plains")));
            assertEquals("biome.minecraft.plains", Biome.PLAINS.translationKey());
        });
        var copy = Biome.values();
        copy[0] = null;
        assertNotNull(Biome.values()[0]);
    }
}
