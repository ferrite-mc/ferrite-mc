package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;
import com.google.gson.JsonParser;
import java.nio.file.Files;
import java.nio.file.Path;
import org.bukkit.Registry;
import org.bukkit.NamespacedKey;
import org.bukkit.damage.*;
import org.junit.jupiter.api.Test;

class DamageTypeRegistryTest {
    private static com.google.gson.JsonObject decoded() throws Exception {
        try (var input = DamageTypeRegistryTest.class.getResourceAsStream("/ferrite/damage-types.json")) {
            return JsonParser.parseString(new String(input.readAllBytes(), java.nio.charset.StandardCharsets.UTF_8)).getAsJsonObject();
        }
    }

    @Test void optionalDefaultsAndEveryEffectMatchTheOfficialCodecAndSoundRegistry() throws Exception {
        var data = decoded();
        var defaults = data.getAsJsonObject("defaults"); // independently read official constructor defaults
        var directory = Path.of(System.getProperty("test.vanilla.data"), "data/minecraft/damage_type");
        try (var files = Files.list(directory)) {
            var paths = files.toList();
            assertEquals(paths.size(), Registry.DAMAGE_TYPE.size());
            for (var path : paths) {
                var raw = JsonParser.parseString(Files.readString(path)).getAsJsonObject();
                var value = Registry.DAMAGE_TYPE.get(NamespacedKey.minecraft(path.getFileName().toString().replace(".json", "")));
                var effectName = (raw.has("effects") ? raw : defaults).get("effects").getAsString();
                var effect = (DamageEffect) DamageEffect.class.getField(effectName.toUpperCase(java.util.Locale.ROOT)).get(null);
                assertNotNull(effect);
                assertSame(effect, value.getDamageEffect());
                var death = (raw.has("death_message_type") ? raw : defaults).get("death_message_type").getAsString();
                assertEquals(DeathMessageType.valueOf(death.toUpperCase(java.util.Locale.ROOT)), value.getDeathMessageType());
            }
        }
        for (var entry : data.getAsJsonObject("effects").entrySet()) {
            var effect = (DamageEffect) DamageEffect.class.getField(entry.getKey().toUpperCase(java.util.Locale.ROOT)).get(null);
            assertSame(Registry.SOUND_EVENT.get(NamespacedKey.fromString(entry.getValue().getAsString())), effect.getSound());
            assertSame(effect, new FerriteInternalBridge().getDamageEffect(entry.getKey()));
        }
        assertNotEquals(DamageEffect.HURT, DamageEffect.THORNS); // same sound, distinct effect identity
        assertThrows(IllegalArgumentException.class, () -> new FerriteInternalBridge().getDamageEffect("unknown"));
    }

    @Test void malformedMissingAndUnknownDecodedDataFailExplicitly() throws Exception {
        var valid = decoded();
        var expected = java.util.Set.copyOf(valid.getAsJsonObject("types").keySet());
        assertThrows(IllegalStateException.class, () -> FerriteDamageData.load(null, expected));
        for (String raw : java.util.List.of("{", "{}", "null"))
            assertThrows(IllegalStateException.class, () -> FerriteDamageData.load(stream(raw), expected));
        for (String field : java.util.List.of("exhaustion", "message_id", "scaling", "effects", "death_message_type")) {
            var broken = valid.deepCopy();
            broken.getAsJsonObject("types").getAsJsonObject("minecraft:fall").remove(field);
            assertThrows(IllegalStateException.class, () -> FerriteDamageData.load(stream(broken.toString()), expected), field);
        }
        for (String field : java.util.List.of("scaling", "effects", "death_message_type")) {
            var broken = valid.deepCopy();
            broken.getAsJsonObject("types").getAsJsonObject("minecraft:fall").addProperty(field, "not_a_real_value");
            assertThrows(IllegalStateException.class, () -> FerriteDamageData.load(stream(broken.toString()), expected), field);
        }
        for (String number : java.util.List.of("\"0.1\"", "1e100")) {
            var broken = valid.deepCopy();
            broken.getAsJsonObject("types").getAsJsonObject("minecraft:fall").add("exhaustion", JsonParser.parseString(number));
            assertThrows(IllegalStateException.class, () -> FerriteDamageData.load(stream(broken.toString()), expected));
        }
        var missing = valid.deepCopy();
        missing.getAsJsonObject("types").remove("minecraft:fall");
        assertThrows(IllegalStateException.class, () -> FerriteDamageData.load(stream(missing.toString()), expected));
        var sound = valid.deepCopy();
        sound.getAsJsonObject("effects").addProperty("hurt", "minecraft:missing_sound");
        assertThrows(IllegalStateException.class, () -> FerriteDamageData.load(stream(sound.toString()), expected));
        var snapshot = FerriteDamageData.load(stream(valid.toString()), expected);
        valid.getAsJsonObject("types").getAsJsonObject("minecraft:fall").addProperty("exhaustion", 99);
        assertEquals(0f, snapshot.definition(NamespacedKey.minecraft("fall")).exhaustion());
    }

    @Test void concurrentReadsRemainInternedAndImmutable() {
        java.util.stream.IntStream.range(0, 512).parallel().forEach(i -> {
            assertSame(DamageType.FALL, Registry.DAMAGE_TYPE.get(NamespacedKey.minecraft("fall")));
            assertEquals(0f, DamageType.FALL.getExhaustion());
            assertSame(DamageEffect.BURNING, DamageType.IN_FIRE.getDamageEffect());
            assertSame(org.bukkit.Sound.ENTITY_PLAYER_HURT_ON_FIRE, DamageEffect.BURNING.getSound());
            assertEquals(DamageType.FALL, new FerriteDamageType(DamageType.FALL.getKey()));
            assertEquals(DamageType.FALL.getKey().hashCode(), DamageType.FALL.hashCode());
        });
        assertNotEquals(DamageType.FALL, DamageType.IN_FIRE);
    }

    private static java.io.InputStream stream(String value) {
        return new java.io.ByteArrayInputStream(value.getBytes(java.nio.charset.StandardCharsets.UTF_8));
    }

    @Test void actualDamageTypePropertiesUseCanonicalDataWithoutHost() throws Exception {
        var previous = HostBridge.transport;
        HostBridge.transport = (owner, subject, method, args) -> { throw new AssertionError("host call " + owner + method); };
        try {
            assertEquals(DamageScaling.WHEN_CAUSED_BY_LIVING_NON_PLAYER, DamageType.FALL.getDamageScaling());
            assertEquals(DeathMessageType.FALL_VARIANTS, DamageType.FALL.getDeathMessageType());
            assertEquals(0f, DamageType.FALL.getExhaustion());
            assertSame(DamageEffect.BURNING, DamageType.IN_FIRE.getDamageEffect());
            assertEquals("death.attack.player", DamageType.PLAYER_ATTACK.getTranslationKey());
            assertNotNull(DamageEffect.HURT);
            assertSame(org.bukkit.Sound.ENTITY_PLAYER_HURT_DROWN, DamageEffect.DROWNING.getSound());
            var directory = Path.of(System.getProperty("test.vanilla.data"), "data/minecraft/damage_type");
            try (var files = Files.list(directory)) {
                for (var file : files.toList()) {
                    var json = JsonParser.parseString(Files.readString(file)).getAsJsonObject();
                    var key = NamespacedKey.minecraft(file.getFileName().toString().replace(".json", ""));
                    var type = Registry.DAMAGE_TYPE.get(key);
                    assertNotNull(type);
                    assertSame(type, Registry.DAMAGE_TYPE.get(key));
                    assertEquals(json.get("exhaustion").getAsFloat(), type.getExhaustion(), key.toString());
                    assertEquals(DamageScaling.valueOf(json.get("scaling").getAsString().toUpperCase(java.util.Locale.ROOT)), type.getDamageScaling(), key.toString());
                    assertEquals("death.attack." + json.get("message_id").getAsString(), type.getTranslationKey());
                }
            }
            assertNull(Registry.DAMAGE_TYPE.get(NamespacedKey.minecraft("missing")));
        } finally { HostBridge.transport = previous; }
    }
}
