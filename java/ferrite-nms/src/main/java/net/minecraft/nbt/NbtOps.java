// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.nbt;

import com.mojang.datafixers.util.Pair;
import com.mojang.serialization.DataResult;
import com.mojang.serialization.DynamicOps;
import java.nio.ByteBuffer;
import java.util.ArrayList;
import java.util.List;
import java.util.stream.IntStream;
import java.util.stream.LongStream;
import java.util.stream.Stream;

/** {@link DynamicOps} implementation for {@link Tag}. */
public final class NbtOps implements DynamicOps<Tag> {
    public static final NbtOps INSTANCE = new NbtOps();

    private NbtOps() {}

    @Override
    public Tag empty() {
        return EndTag.INSTANCE;
    }

    @Override
    public <U> U convertTo(DynamicOps<U> outOps, Tag input) {
        if (input == null || input instanceof EndTag) {
            return outOps.empty();
        }
        return switch (input) {
            case CompoundTag c -> convertMap(outOps, c);
            case ListTag l -> convertList(outOps, l);
            case ByteArrayTag b -> outOps.createByteList(ByteBuffer.wrap(b.getAsByteArray()));
            case IntArrayTag i -> outOps.createIntList(IntStream.of(i.getAsIntArray()));
            case LongArrayTag l -> outOps.createLongList(LongStream.of(l.getAsLongArray()));
            case ByteTag b -> outOps.createByte(b.getAsByte());
            case ShortTag s -> outOps.createShort(s.getAsShort());
            case IntTag i -> outOps.createInt(i.getAsInt());
            case LongTag l -> outOps.createLong(l.getAsLong());
            case FloatTag f -> outOps.createFloat(f.getAsFloat());
            case DoubleTag d -> outOps.createDouble(d.getAsDouble());
            case StringTag s -> outOps.createString(s.asString());
            default -> outOps.empty();
        };
    }

    @Override
    public DataResult<Number> getNumberValue(Tag input) {
        if (input instanceof NumericTag n) {
            return DataResult.success(n.getAsNumber());
        }
        return DataResult.error(() -> "Not a number: " + input);
    }

    @Override
    public Tag createNumeric(Number i) {
        return DoubleTag.valueOf(i.doubleValue());
    }

    @Override
    public Tag createByte(byte value) {
        return ByteTag.valueOf(value);
    }

    @Override
    public Tag createShort(short value) {
        return ShortTag.valueOf(value);
    }

    @Override
    public Tag createInt(int value) {
        return IntTag.valueOf(value);
    }

    @Override
    public Tag createLong(long value) {
        return LongTag.valueOf(value);
    }

    @Override
    public Tag createFloat(float value) {
        return FloatTag.valueOf(value);
    }

    @Override
    public Tag createDouble(double value) {
        return DoubleTag.valueOf(value);
    }

    @Override
    public Tag createBoolean(boolean value) {
        return ByteTag.valueOf(value);
    }

    @Override
    public DataResult<String> getStringValue(Tag input) {
        if (input instanceof StringTag s) {
            return DataResult.success(s.asString());
        }
        return DataResult.error(() -> "Not a string: " + input);
    }

    @Override
    public Tag createString(String value) {
        return StringTag.valueOf(value);
    }

    @Override
    public DataResult<Tag> mergeToList(Tag list, Tag value) {
        if (!(list instanceof ListTag) && !(list instanceof EndTag)) {
            return DataResult.error(() -> "mergeToList called with not a list: " + list, list);
        }
        ListTag out = list instanceof ListTag l ? l.copy() : new ListTag();
        out.add(value);
        return DataResult.success(out);
    }

    @Override
    public DataResult<Tag> mergeToMap(Tag map, Tag key, Tag value) {
        if (!(map instanceof CompoundTag) && !(map instanceof EndTag)) {
            return DataResult.error(() -> "mergeToMap called with not a map: " + map, map);
        }
        if (!(key instanceof StringTag s)) {
            return DataResult.error(() -> "key is not a string: " + key, map);
        }
        CompoundTag out = map instanceof CompoundTag c ? c.copy() : new CompoundTag();
        out.put(s.asString(), value);
        return DataResult.success(out);
    }

    @Override
    public DataResult<Stream<Pair<Tag, Tag>>> getMapValues(Tag input) {
        if (!(input instanceof CompoundTag c)) {
            return DataResult.error(() -> "Not a compound tag: " + input);
        }
        return DataResult.success(c.keySet().stream()
                .map(k -> Pair.of(StringTag.valueOf(k), c.get(k))));
    }

    @Override
    public Tag createMap(Stream<Pair<Tag, Tag>> map) {
        CompoundTag out = new CompoundTag();
        map.forEach(p -> out.put(p.getFirst().asString(), p.getSecond()));
        return out;
    }

    @Override
    public DataResult<Stream<Tag>> getStream(Tag input) {
        if (input instanceof ListTag l) {
            List<Tag> copy = new ArrayList<>(l);
            return DataResult.success(copy.stream());
        }
        return DataResult.error(() -> "Not a list tag: " + input);
    }

    @Override
    public Tag createList(Stream<Tag> input) {
        ListTag out = new ListTag();
        input.forEach(out::add);
        return out;
    }

    @Override
    public Tag remove(Tag input, String key) {
        if (input instanceof CompoundTag c) {
            CompoundTag copy = c.copy();
            copy.remove(key);
            return copy;
        }
        return input;
    }
}
