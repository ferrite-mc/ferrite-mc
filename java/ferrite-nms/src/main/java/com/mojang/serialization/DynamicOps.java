// Clean-room Ferrite DFU compatibility code. No Mojang code or assets.
package com.mojang.serialization;

import com.mojang.datafixers.util.Pair;
import java.nio.ByteBuffer;
import java.util.List;
import java.util.Map;
import java.util.function.Consumer;
import java.util.stream.IntStream;
import java.util.stream.LongStream;
import java.util.stream.Stream;

/** Abstract data-format operations matching DataFixerUpper's {@code DynamicOps<T>} contract. */
public interface DynamicOps<T> {
    T empty();

    default T emptyMap() {
        return createMap(Map.of());
    }

    default T emptyList() {
        return createList(Stream.empty());
    }

    <U> U convertTo(DynamicOps<U> outOps, T input);

    DataResult<Number> getNumberValue(T input);

    default Number getNumberValue(T input, Number defaultValue) {
        return getNumberValue(input).result().orElse(defaultValue);
    }

    T createNumeric(Number i);

    default T createByte(byte value) {
        return createNumeric(value);
    }

    default T createShort(short value) {
        return createNumeric(value);
    }

    default T createInt(int value) {
        return createNumeric(value);
    }

    default T createLong(long value) {
        return createNumeric(value);
    }

    default T createFloat(float value) {
        return createNumeric(value);
    }

    default T createDouble(double value) {
        return createNumeric(value);
    }

    default DataResult<Boolean> getBooleanValue(T input) {
        return getNumberValue(input).map(n -> n.byteValue() != 0);
    }

    default T createBoolean(boolean value) {
        return createByte((byte) (value ? 1 : 0));
    }

    DataResult<String> getStringValue(T input);

    T createString(String value);

    DataResult<T> mergeToList(T list, T value);

    default DataResult<T> mergeToList(T list, List<T> values) {
        DataResult<T> result = DataResult.success(list);
        for (T value : values) {
            result = result.flatMap(r -> mergeToList(r, value));
        }
        return result;
    }

    DataResult<T> mergeToMap(T map, T key, T value);

    default DataResult<T> mergeToMap(T map, Map<T, T> values) {
        DataResult<T> result = DataResult.success(map);
        for (Map.Entry<T, T> entry : values.entrySet()) {
            result = result.flatMap(r -> mergeToMap(r, entry.getKey(), entry.getValue()));
        }
        return result;
    }

    default DataResult<T> mergeToMap(T map, MapLike<T> values) {
        DataResult<T> result = DataResult.success(map);
        for (Pair<T, T> entry : values.entries().toList()) {
            result = result.flatMap(r -> mergeToMap(r, entry.getFirst(), entry.getSecond()));
        }
        return result;
    }

    DataResult<Stream<Pair<T, T>>> getMapValues(T input);

    default DataResult<Consumer<Consumer<Pair<T, T>>>> getMapEntries(T input) {
        return getMapValues(input).map(s -> s::forEach);
    }

    default DataResult<MapLike<T>> getMap(T input) {
        return getMapValues(input).flatMap(s -> {
            try {
                return DataResult.success(MapLike.forMap(
                        s.collect(Pair.toMap()), this));
            } catch (IllegalStateException e) {
                return DataResult.error(() -> "Error while building map: " + e.getMessage());
            }
        });
    }

    T createMap(Stream<Pair<T, T>> map);

    default T createMap(Map<T, T> map) {
        return createMap(map.entrySet().stream().map(e -> Pair.of(e.getKey(), e.getValue())));
    }

    DataResult<Stream<T>> getStream(T input);

    default DataResult<Consumer<Consumer<T>>> getList(T input) {
        return getStream(input).map(s -> s::forEach);
    }

    T createList(Stream<T> input);

    default DataResult<ByteBuffer> getByteBuffer(T input) {
        return getStream(input).flatMap(s -> {
            List<T> list = s.toList();
            ByteBuffer buf = ByteBuffer.allocate(list.size());
            for (T item : list) {
                DataResult<Number> num = getNumberValue(item);
                if (num.isError()) {
                    return DataResult.error(() -> "Invalid byte element: " + num);
                }
                buf.put(num.getOrThrow().byteValue());
            }
            buf.flip();
            return DataResult.success(buf);
        });
    }

    default T createByteList(ByteBuffer input) {
        ByteBuffer dup = input.duplicate();
        byte[] arr = new byte[dup.remaining()];
        dup.get(arr);
        return createList(IntStream.range(0, arr.length).mapToObj(i -> createByte(arr[i])));
    }

    default DataResult<IntStream> getIntStream(T input) {
        return getStream(input).flatMap(s -> {
            List<T> list = s.toList();
            int[] arr = new int[list.size()];
            for (int i = 0; i < list.size(); i++) {
                DataResult<Number> num = getNumberValue(list.get(i));
                if (num.isError()) {
                    return DataResult.error(() -> "Invalid int element: " + num);
                }
                arr[i] = num.getOrThrow().intValue();
            }
            return DataResult.success(IntStream.of(arr));
        });
    }

    default T createIntList(IntStream input) {
        return createList(input.mapToObj(this::createInt));
    }

    default DataResult<LongStream> getLongStream(T input) {
        return getStream(input).flatMap(s -> {
            List<T> list = s.toList();
            long[] arr = new long[list.size()];
            for (int i = 0; i < list.size(); i++) {
                DataResult<Number> num = getNumberValue(list.get(i));
                if (num.isError()) {
                    return DataResult.error(() -> "Invalid long element: " + num);
                }
                arr[i] = num.getOrThrow().longValue();
            }
            return DataResult.success(LongStream.of(arr));
        });
    }

    default T createLongList(LongStream input) {
        return createList(input.mapToObj(this::createLong));
    }

    T remove(T input, String key);

    default boolean compressMaps() {
        return false;
    }

    default DataResult<T> get(T input, String key) {
        return getMap(input).flatMap(map -> {
            T val = map.get(key);
            return val == null
                    ? DataResult.error(() -> "No element " + key + " in map " + input)
                    : DataResult.success(val);
        });
    }

    default RecordBuilder<T> mapBuilder() {
        return new RecordBuilder.MapBuilder<>(this);
    }

    default ListBuilder<T> listBuilder() {
        return new ListBuilder.Builder<>(this);
    }

    default <R> R convertList(DynamicOps<R> outOps, T input) {
        return outOps.createList(
                getStream(input).result().orElseGet(Stream::empty)
                        .map(e -> convertTo(outOps, e)));
    }

    default <R> R convertMap(DynamicOps<R> outOps, T input) {
        return outOps.createMap(
                getMapValues(input).result().orElseGet(Stream::empty)
                        .map(e -> Pair.of(
                                convertTo(outOps, e.getFirst()),
                                convertTo(outOps, e.getSecond()))));
    }
}
