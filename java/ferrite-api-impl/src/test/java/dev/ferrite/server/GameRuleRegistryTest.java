package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;
import com.google.gson.JsonParser;
import io.papermc.paper.registry.RegistryAccess;
import io.papermc.paper.registry.RegistryKey;
import java.nio.file.Files;
import java.nio.file.Path;
import org.bukkit.GameRule;
import org.bukkit.NamespacedKey;
import org.junit.jupiter.api.Test;

class GameRuleRegistryTest {
    @org.junit.jupiter.api.BeforeAll static void startApi() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
    }
    @Test void everyTypeDefaultAndTranslationMatchesIndependentOfficialConstants() throws Exception {
        var reference = JsonParser.parseString(Files.readString(Path.of(System.getProperty("test.gamerule.reference"))))
                .getAsJsonObject().getAsJsonObject("rules");
        var registry = org.bukkit.Registry.GAME_RULE;
        assertEquals(reference.size(), registry.size());
        for (var rule : registry) {
            var expected = reference.getAsJsonObject(rule.getKey().toString());
            assertEquals(expected.get("type").getAsString(), rule.getType().getName());
            Object value = rule.getType() == Boolean.class ? expected.get("default").getAsBoolean() : expected.get("default").getAsInt();
            assertEquals(value, rule.getDefaultValue(), rule.getKey().toString());
            assertEquals(expected.get("translation_key").getAsString(), rule.translationKey());
            assertEquals(rule.getKey().toString(), rule.getName());
            assertEquals(expected.getAsJsonArray("required_features").asList().stream().map(v -> v.getAsString()).collect(java.util.stream.Collectors.toSet()),
                    rule.requiredFeatures().stream().map(flag -> flag.getKey().toString()).collect(java.util.stream.Collectors.toSet()));
        }
        assertThrows(UnsupportedOperationException.class, () -> org.bukkit.GameRules.MAX_MINECART_SPEED.requiredFeatures().clear());
    }

    @Test void allInstalledConstantsAndSupportedLegacyConversionsInitialize() throws Exception {
        for (var field : org.bukkit.GameRules.class.getFields()) {
            if (field.getType() != GameRule.class) continue;
            var value = (GameRule<?>) field.get(null);
            assertNotNull(value, field.getName());
            assertSame(value, org.bukkit.Registry.GAME_RULE.get(value.getKey()));
        }
        for (var field : GameRule.class.getFields()) {
            if (field.getType() != GameRule.class) continue;
            var value = (GameRule<?>) field.get(null);
            assertNotNull(value, field.getName());
            assertInstanceOf(value.getType(), value.getDefaultValue(), field.getName());
        }
        assertEquals(Boolean.FALSE, GameRule.DISABLE_PLAYER_MOVEMENT_CHECK.getDefaultValue());
        assertEquals(Boolean.FALSE, GameRule.DISABLE_RAIDS.getDefaultValue());
        assertEquals(Boolean.TRUE, GameRule.DO_FIRE_TICK.getDefaultValue());
        assertEquals(Boolean.class, GameRule.DO_FIRE_TICK.getType());
        assertEquals(Integer.class, org.bukkit.GameRules.FIRE_SPREAD_RADIUS_AROUND_PLAYER.getType());
        assertSame(org.bukkit.GameRules.ADVANCE_TIME, GameRule.getByName("advance_time"));
        assertSame(org.bukkit.GameRules.ADVANCE_TIME, GameRule.getByName("minecraft:advance_time"));
        assertNull(GameRule.getByName("doDaylightCycle")); // API itself accepts canonical keys only.
        var copy = GameRule.values();
        copy[0] = null;
        assertNotNull(GameRule.values()[0]);
        assertEquals(59, GameRule.values().length);
    }

    @Test void parallelReadsAreInternedAndEnumerationIsStable() {
        var registry = org.bukkit.Registry.GAME_RULE;
        java.util.stream.IntStream.range(0, 512).parallel().forEach(i -> {
            assertSame(org.bukkit.GameRules.RANDOM_TICK_SPEED, registry.get(NamespacedKey.minecraft("random_tick_speed")));
            assertEquals(3, org.bukkit.GameRules.RANDOM_TICK_SPEED.getDefaultValue());
            assertEquals(59, registry.stream().count());
        });
        assertEquals(registry.keyStream().sorted(java.util.Comparator.comparing(Object::toString)).toList(), registry.keyStream().toList());
        assertThrows(NullPointerException.class, () -> registry.get((NamespacedKey) null));
        assertNotEquals(org.bukkit.GameRules.ADVANCE_TIME, org.bukkit.GameRules.ADVANCE_WEATHER);
        assertEquals(org.bukkit.GameRules.ADVANCE_TIME.getKey().hashCode(), org.bukkit.GameRules.ADVANCE_TIME.hashCode());
    }

    @Test void malformedMissingAndUnfaithfulDefinitionsAreRejected() throws Exception {
        var key = "minecraft:advance_time";
        var valid = JsonParser.parseString(Files.readString(Path.of(System.getProperty("test.gamerule.reference")))).getAsJsonObject();
        assertThrows(IllegalStateException.class, () -> FerriteGameRuleData.load(null, java.util.Set.of(key)));
        for (String data : java.util.List.of("{", "{}", "null", "{\"version\":\"old\",\"rules\":{}}"))
            assertThrows(IllegalStateException.class, () -> FerriteGameRuleData.load(stream(data), java.util.Set.of(key)));
        var expectedKeys = java.util.Set.copyOf(valid.getAsJsonObject("rules").keySet());
        for (String field : java.util.List.of("type", "default", "translation_key", "required_features")) {
            var broken = valid.deepCopy();
            broken.getAsJsonObject("rules").getAsJsonObject(key).remove(field);
            assertThrows(IllegalStateException.class, () -> FerriteGameRuleData.load(stream(broken.toString()), expectedKeys), field);
        }
        for (String definition : java.util.List.of(
                "{\"type\":\"java.lang.String\",\"default\":true}",
                "{\"type\":\"java.lang.Boolean\",\"default\":1}",
                "{\"type\":\"java.lang.Integer\",\"default\":\"3\"}",
                "{\"type\":\"java.lang.Integer\",\"default\":2.5}",
                "{\"type\":\"java.lang.Integer\",\"default\":2147483648}")) {
            var broken = valid.deepCopy();
            for (var field : JsonParser.parseString(definition).getAsJsonObject().entrySet())
                broken.getAsJsonObject("rules").getAsJsonObject(key).add(field.getKey(), field.getValue());
            assertThrows(IllegalStateException.class, () -> FerriteGameRuleData.load(stream(broken.toString()), expectedKeys));
        }
        var unknownFeature = valid.deepCopy();
        unknownFeature.getAsJsonObject("rules").getAsJsonObject(key).add("required_features", JsonParser.parseString("[\"minecraft:unknown_feature\"]"));
        assertThrows(IllegalStateException.class, () -> FerriteGameRuleData.load(stream(unknownFeature.toString()), expectedKeys));
        var fewer = valid.deepCopy();
        fewer.getAsJsonObject("rules").remove(key);
        assertThrows(IllegalStateException.class, () -> FerriteGameRuleData.load(stream(fewer.toString()), expectedKeys));
        var snapshot = FerriteGameRuleData.load(stream(valid.toString()), expectedKeys);
        assertThrows(UnsupportedOperationException.class, snapshot::clear);
    }

    private static java.io.InputStream stream(String value) {
        return new java.io.ByteArrayInputStream(value.getBytes(java.nio.charset.StandardCharsets.UTF_8));
    }

    @Test void actualBukkitRegistryEnumeratesTypedDefaultsWithoutHost() throws Exception {
        var previous = HostBridge.transport;
        HostBridge.transport = (owner, subject, method, args) -> { throw new AssertionError("host call " + owner + method); };
        try {
            var registry = RegistryAccess.registryAccess().getRegistry(RegistryKey.GAME_RULE);
            assertEquals(59, registry.size());
            var report = JsonParser.parseString(Files.readString(Path.of(System.getProperty("test.vanilla.data"), "reports/registries.json")))
                    .getAsJsonObject().getAsJsonObject("minecraft:game_rule").getAsJsonObject("entries");
            assertEquals(report.keySet(), registry.keyStream().map(Object::toString).collect(java.util.stream.Collectors.toSet()));
            for (var rule : registry) {
                assertSame(rule, registry.get(rule.getKey()));
                assertEquals(rule.getKey(), registry.getKey(rule));
                assertTrue(rule.getType() == Boolean.class || rule.getType() == Integer.class);
                assertInstanceOf(rule.getType(), rule.getDefaultValue());
            }
            assertSame(registry, RegistryAccess.registryAccess().getRegistry(GameRule.class));
            assertSame(org.bukkit.GameRules.ADVANCE_TIME, registry.get(NamespacedKey.minecraft("advance_time")));
            assertSame(GameRule.DO_DAYLIGHT_CYCLE, org.bukkit.GameRules.ADVANCE_TIME);
            assertEquals(Boolean.TRUE, org.bukkit.GameRules.ADVANCE_TIME.getDefaultValue());
            assertEquals(3, org.bukkit.GameRules.RANDOM_TICK_SPEED.getDefaultValue());
            assertNull(registry.get(NamespacedKey.minecraft("missing")));
            assertNull(registry.get(NamespacedKey.fromString("other:advance_time")));
        } finally { HostBridge.transport = previous; }
    }
}
