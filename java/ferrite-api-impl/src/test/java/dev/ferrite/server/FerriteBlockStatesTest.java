package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.gson.JsonParser;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.stream.Collectors;
import org.junit.jupiter.api.Test;

/** Global block-state ids against the generated {@code reports/blocks.json}, read here without the generator. */
class FerriteBlockStatesTest {
    private static final Path REPORT = Path.of(System.getProperty("test.vanilla.data"), "reports/blocks.json");

    @Test void everyStateInTheReportGetsItsReportedId() throws IOException {
        var blocks = JsonParser.parseString(Files.readString(REPORT, StandardCharsets.UTF_8)).getAsJsonObject();
        int checked = 0;
        for (var block : blocks.entrySet()) {
            for (var state : block.getValue().getAsJsonObject().getAsJsonArray("states")) {
                var object = state.getAsJsonObject();
                var properties = object.has("properties") ? object.getAsJsonObject("properties") : null;
                var text = block.getKey() + (properties == null ? "" : properties.entrySet().stream()
                        .map(property -> property.getKey() + "=" + property.getValue().getAsString())
                        .collect(Collectors.joining(",", "[", "]")));
                assertEquals(object.get("id").getAsInt(), FerriteBlockStates.stateId(text), text);
                checked++;
            }
        }
        assertEquals(35723, checked);
    }

    @Test void unnamedPropertiesKeepTheirDefaults() {
        assertEquals(0, FerriteBlockStates.stateId("minecraft:air"));
        assertEquals(1, FerriteBlockStates.stateId("stone"));
        // chest defaults: type=single, facing=north, waterlogged=false (report: default state id 5544)
        assertEquals(5544, FerriteBlockStates.stateId("minecraft:chest"));
        assertEquals(5543, FerriteBlockStates.stateId("minecraft:chest[waterlogged=true]"));
        assertEquals(5545, FerriteBlockStates.stateId("minecraft:chest[type=left,waterlogged=true]"));
    }

    @Test void unknownBlocksPropertiesAndValuesAreRejected() {
        assertTrue(assertThrows(IllegalArgumentException.class, () -> FerriteBlockStates.stateId("minecraft:no_such_block")).getMessage().contains("no_such_block"));
        assertThrows(IllegalArgumentException.class, () -> FerriteBlockStates.stateId("minecraft:stone[facing=north]"));
        assertThrows(IllegalArgumentException.class, () -> FerriteBlockStates.stateId("minecraft:chest[type=middle]"));
        assertThrows(IllegalArgumentException.class, () -> FerriteBlockStates.stateId("minecraft:chest[type=left"));
        assertThrows(IllegalArgumentException.class, () -> FerriteBlockStates.stateId("minecraft:chest[type=left,type=right]"));
    }
}
