package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;
import io.papermc.paper.entity.poi.PoiTypes;
import org.junit.jupiter.api.Test;

class PoiRegistryValuesTest {
    private static com.google.gson.JsonObject decoded() throws Exception {
        try (var input = PoiRegistryValuesTest.class.getResourceAsStream("/ferrite/poi-types.json")) {
            return com.google.gson.JsonParser.parseString(new String(input.readAllBytes(), java.nio.charset.StandardCharsets.UTF_8)).getAsJsonObject();
        }
    }

    @Test void allPoiPredicatesMatchOfficialStatesUsingIndependentBlockReports() throws Exception {
        var json = decoded().getAsJsonObject("types");
        var registry = org.bukkit.Registry.POINT_OF_INTEREST_TYPE;
        var positiveStates = new java.util.HashMap<io.papermc.paper.entity.poi.PoiType, java.util.Set<Integer>>();
        var allPositive = new java.util.HashSet<Integer>();
        assertEquals(json.size(), registry.size());
        for (var type : registry) {
            var raw = json.getAsJsonObject(type.getKey().toString());
            assertEquals(raw.get("max_tickets").getAsInt() != 0, type.hasOccupants());
            var states = raw.getAsJsonArray("states").asList().stream().map(com.google.gson.JsonElement::getAsInt).collect(java.util.stream.Collectors.toSet());
            positiveStates.put(type, states);
            allPositive.addAll(states);
            assertSame(type, registry.get(type.getKey()));
        }
        var blocks = com.google.gson.JsonParser.parseString(java.nio.file.Files.readString(java.nio.file.Path.of(System.getProperty("test.vanilla.data"), "reports/blocks.json"))).getAsJsonObject();
        var visited = new java.util.HashSet<Integer>();
        for (var block : blocks.entrySet()) {
            var states = block.getValue().getAsJsonObject().getAsJsonArray("states");
            boolean relevant = states.asList().stream().anyMatch(s -> allPositive.contains(s.getAsJsonObject().get("id").getAsInt()));
            for (var stateJson : states) {
                var state = stateJson.getAsJsonObject();
                if (!relevant && !state.has("default")) continue;
                int id = state.get("id").getAsInt();
                visited.add(id);
                String properties = state.has("properties") ? state.getAsJsonObject("properties").entrySet().stream()
                        .map(p -> p.getKey() + "=" + p.getValue().getAsString()).collect(java.util.stream.Collectors.joining(",", "[", "]")) : "";
                var data = FerriteBlockData.create(block.getKey() + properties);
                for (var entry : positiveStates.entrySet())
                    assertEquals(entry.getValue().contains(id), entry.getKey().is(data), entry.getKey().getKey() + " " + data.getAsString());
            }
        }
        assertTrue(visited.containsAll(allPositive));
        assertNull(registry.get(org.bukkit.NamespacedKey.minecraft("unknown_poi")));
    }

    @Test void malformedDataInputsAndForeignBlockErrorsRemainExplicit() throws Exception {
        var valid = decoded();
        var keys = java.util.Set.copyOf(valid.getAsJsonObject("types").keySet());
        assertThrows(IllegalStateException.class, () -> FerritePoiData.load(null, keys));
        for (String data : java.util.List.of("{", "{}", "null"))
            assertThrows(IllegalStateException.class, () -> FerritePoiData.load(stream(data), keys));
        for (String field : java.util.List.of("max_tickets", "states")) {
            var invalid = valid.deepCopy();
            invalid.getAsJsonObject("types").getAsJsonObject("minecraft:home").remove(field);
            assertThrows(IllegalStateException.class, () -> FerritePoiData.load(stream(invalid.toString()), keys));
        }
        for (String states : java.util.List.of("[-1]", "[2147483647]", "[1.5]", "[\"1\"]", "[1,1]")) {
            var invalid = valid.deepCopy();
            invalid.getAsJsonObject("types").getAsJsonObject("minecraft:home").add("states", com.google.gson.JsonParser.parseString(states));
            assertThrows(IllegalStateException.class, () -> FerritePoiData.load(stream(invalid.toString()), keys), states);
        }
        var missing = valid.deepCopy();
        missing.getAsJsonObject("types").remove("minecraft:home");
        assertThrows(IllegalStateException.class, () -> FerritePoiData.load(stream(missing.toString()), keys));
        var snapshot = FerritePoiData.load(stream(valid.toString()), keys);
        assertThrows(UnsupportedOperationException.class, snapshot::clear);
        assertThrows(UnsupportedOperationException.class, () -> snapshot.get(org.bukkit.NamespacedKey.minecraft("home")).states().clear());
        assertThrows(NullPointerException.class, () -> PoiTypes.HOME.is(null));
        var marker = new IllegalStateException("foreign block failure");
        var foreign = (org.bukkit.block.data.BlockData) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(),
                new Class<?>[]{org.bukkit.block.data.BlockData.class}, (proxy, method, args) -> { throw marker; });
        assertSame(marker, assertThrows(IllegalStateException.class, () -> PoiTypes.HOME.is(foreign)));
        var invalidBlock = (org.bukkit.block.data.BlockData) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(),
                new Class<?>[]{org.bukkit.block.data.BlockData.class}, (proxy, method, args) -> "minecraft:stone[not_a_property=1]");
        assertThrows(IllegalArgumentException.class, () -> PoiTypes.HOME.is(invalidBlock));
    }

    @Test void concurrentStaticReadsPreserveCanonicalValueIdentity() {
        java.util.stream.IntStream.range(0, 512).parallel().forEach(i -> {
            assertSame(PoiTypes.HOME, org.bukkit.Registry.POINT_OF_INTEREST_TYPE.get(PoiTypes.HOME.getKey()));
            assertTrue(PoiTypes.HOME.hasOccupants());
            assertTrue(PoiTypes.ARMORER.is(FerriteBlockData.create("minecraft:blast_furnace")));
            assertEquals(PoiTypes.HOME, new FerritePoiType(PoiTypes.HOME.getKey()));
            assertEquals(PoiTypes.HOME.getKey().hashCode(), PoiTypes.HOME.hashCode());
        });
        assertNotEquals(PoiTypes.HOME, PoiTypes.ARMORER);
    }

    private static java.io.InputStream stream(String value) {
        return new java.io.ByteArrayInputStream(value.getBytes(java.nio.charset.StandardCharsets.UTF_8));
    }

    @Test void actualPoiApisReadStaticDefinitionsWithoutHost() {
        var old = HostBridge.transport;
        HostBridge.transport = (owner, subject, method, args) -> { throw new AssertionError("host call " + owner + method); };
        try {
            assertTrue(PoiTypes.HOME.hasOccupants());
            assertFalse(PoiTypes.NETHER_PORTAL.hasOccupants());
            assertTrue(PoiTypes.ARMORER.is(FerriteBlockData.create("minecraft:blast_furnace")));
            assertFalse(PoiTypes.ARMORER.is(FerriteBlockData.create("minecraft:stone")));
            assertTrue(PoiTypes.HOME.is(FerriteBlockData.create("minecraft:red_bed[part=head]")));
            assertFalse(PoiTypes.HOME.is(FerriteBlockData.create("minecraft:red_bed[part=foot]")));
        } finally { HostBridge.transport = old; }
    }
}
