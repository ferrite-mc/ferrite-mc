// Clean-room Ferrite DFU test. No Mojang code or assets.
package dev.ferrite.nms;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.gson.JsonElement;
import com.mojang.datafixers.DSL;
import com.mojang.datafixers.DataFixer;
import com.mojang.datafixers.util.Either;
import com.mojang.datafixers.util.Pair;
import com.mojang.datafixers.util.Unit;
import com.mojang.serialization.Codec;
import com.mojang.serialization.DataResult;
import com.mojang.serialization.Dynamic;
import com.mojang.serialization.JsonOps;
import com.mojang.serialization.codecs.RecordCodecBuilder;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.Tag;
import org.junit.jupiter.api.Test;

class DataFixerSerializationTest {

    record SelectionRegion(String world, int minX, int minY, int minZ, Optional<String> label) {
        static final Codec<SelectionRegion> CODEC = RecordCodecBuilder.create(instance -> instance.group(
                Codec.STRING.fieldOf("world").forGetter(SelectionRegion::world),
                Codec.INT.fieldOf("min_x").forGetter(SelectionRegion::minX),
                Codec.intRange(-64, 320).fieldOf("min_y").forGetter(SelectionRegion::minY),
                Codec.INT.fieldOf("min_z").forGetter(SelectionRegion::minZ),
                Codec.STRING.optionalFieldOf("label").forGetter(SelectionRegion::label))
                .apply(instance, SelectionRegion::new));
    }

    @Test
    void eitherPairAndDataResultContracts() {
        Either<String, Integer> left = Either.left("ok");
        Either<String, Integer> right = Either.right(42);
        assertEquals("ok", left.orThrow());
        assertEquals(Optional.of(42), right.right());
        assertEquals(Either.left(42), right.swap());
        assertThrows(IllegalStateException.class, right::orThrow);

        Pair<String, Integer> pair = Pair.of("x", 7);
        assertEquals(Pair.of(7, "x"), pair.swap());
        assertEquals(Unit.INSTANCE.toString(), "Unit");

        DataResult<Integer> ok = DataResult.success(10).flatMap(v -> DataResult.success(v * 2));
        assertEquals(20, ok.getOrThrow());
        DataResult<Integer> err = DataResult.error(() -> "bad value");
        assertTrue(err.isError());
        assertThrows(IllegalStateException.class, err::getOrThrow);
    }

    @Test
    void recordCodecBuilderAndCrossOpsConversionRoundTrip() {
        SelectionRegion region = new SelectionRegion("minecraft:overworld", -10, 64, 20, Optional.of("spawn"));
        JsonElement json = SelectionRegion.CODEC.encodeStart(JsonOps.INSTANCE, region).getOrThrow();
        SelectionRegion fromJson = SelectionRegion.CODEC.parse(JsonOps.INSTANCE, json).getOrThrow();
        assertEquals(region, fromJson);

        Dynamic<Tag> nbtDyn = new Dynamic<>(JsonOps.INSTANCE, json).convert(NbtOps.INSTANCE);
        SelectionRegion fromNbt = SelectionRegion.CODEC.parse(nbtDyn).getOrThrow();
        assertEquals(region, fromNbt);

        SelectionRegion outOfRange = new SelectionRegion("minecraft:overworld", 0, 999, 0, Optional.empty());
        assertFalse(SelectionRegion.CODEC.encodeStart(JsonOps.INSTANCE, outOfRange).isSuccess());
    }

    @Test
    void listMapEitherCodecsAndDataFixerContract() {
        Codec<List<Integer>> listCodec = Codec.INT.listOf();
        JsonElement encodedList = listCodec.encodeStart(JsonOps.INSTANCE, List.of(1, 2, 3)).getOrThrow();
        assertEquals(List.of(1, 2, 3), listCodec.parse(JsonOps.INSTANCE, encodedList).getOrThrow());

        Codec<Map<String, Integer>> mapCodec = Codec.unboundedMap(Codec.STRING, Codec.INT);
        JsonElement encodedMap = mapCodec.encodeStart(JsonOps.INSTANCE, Map.of("a", 1, "b", 2)).getOrThrow();
        assertEquals(Map.of("a", 1, "b", 2), mapCodec.parse(JsonOps.INSTANCE, encodedMap).getOrThrow());

        Codec<Either<Integer, String>> eitherCodec = Codec.either(Codec.INT, Codec.STRING);
        assertEquals(Either.right("stone"), eitherCodec.parse(JsonOps.INSTANCE, JsonOps.INSTANCE.createString("stone")).getOrThrow());

        DataFixer identityFixer = new DataFixer() {
            @Override
            public <T> Dynamic<T> update(DSL.TypeReference type, Dynamic<T> input, int version, int newVersion) {
                assertEquals("block_name", type.typeName());
                return input;
            }
        };
        Dynamic<JsonElement> dyn = new Dynamic<>(JsonOps.INSTANCE, JsonOps.INSTANCE.createString("minecraft:stone"));
        assertEquals(dyn, identityFixer.update(() -> "block_name", dyn, 4000, 5023));
    }
}
