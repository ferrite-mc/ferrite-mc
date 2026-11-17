// Clean-room Ferrite DFU compatibility code. No Mojang code or assets.
package com.mojang.serialization;

import com.mojang.datafixers.util.Either;
import com.mojang.datafixers.util.Pair;
import com.mojang.datafixers.util.Unit;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.function.Function;
import java.util.function.Supplier;

/** Combined {@link Encoder} and {@link Decoder}, matching DataFixerUpper's {@code Codec<A>}. */
public interface Codec<A> extends Encoder<A>, Decoder<A> {

    Codec<Boolean> BOOL = new Codec<>() {
        @Override
        public <T> DataResult<T> encode(Boolean input, DynamicOps<T> ops, T prefix) {
            return ops.mergeToList(prefix, ops.createBoolean(input)).isError()
                    ? DataResult.success(ops.createBoolean(input))
                    : DataResult.success(ops.createBoolean(input));
        }

        @Override
        public <T> DataResult<Pair<Boolean, T>> decode(DynamicOps<T> ops, T input) {
            return ops.getBooleanValue(input).map(b -> Pair.of(b, ops.empty()));
        }
    };

    Codec<Byte> BYTE = new Codec<>() {
        @Override
        public <T> DataResult<T> encode(Byte input, DynamicOps<T> ops, T prefix) {
            return DataResult.success(ops.createByte(input));
        }

        @Override
        public <T> DataResult<Pair<Byte, T>> decode(DynamicOps<T> ops, T input) {
            return ops.getNumberValue(input).map(n -> Pair.of(n.byteValue(), ops.empty()));
        }
    };

    Codec<Short> SHORT = new Codec<>() {
        @Override
        public <T> DataResult<T> encode(Short input, DynamicOps<T> ops, T prefix) {
            return DataResult.success(ops.createShort(input));
        }

        @Override
        public <T> DataResult<Pair<Short, T>> decode(DynamicOps<T> ops, T input) {
            return ops.getNumberValue(input).map(n -> Pair.of(n.shortValue(), ops.empty()));
        }
    };

    Codec<Integer> INT = new Codec<>() {
        @Override
        public <T> DataResult<T> encode(Integer input, DynamicOps<T> ops, T prefix) {
            return DataResult.success(ops.createInt(input));
        }

        @Override
        public <T> DataResult<Pair<Integer, T>> decode(DynamicOps<T> ops, T input) {
            return ops.getNumberValue(input).map(n -> Pair.of(n.intValue(), ops.empty()));
        }
    };

    Codec<Long> LONG = new Codec<>() {
        @Override
        public <T> DataResult<T> encode(Long input, DynamicOps<T> ops, T prefix) {
            return DataResult.success(ops.createLong(input));
        }

        @Override
        public <T> DataResult<Pair<Long, T>> decode(DynamicOps<T> ops, T input) {
            return ops.getNumberValue(input).map(n -> Pair.of(n.longValue(), ops.empty()));
        }
    };

    Codec<Float> FLOAT = new Codec<>() {
        @Override
        public <T> DataResult<T> encode(Float input, DynamicOps<T> ops, T prefix) {
            return DataResult.success(ops.createFloat(input));
        }

        @Override
        public <T> DataResult<Pair<Float, T>> decode(DynamicOps<T> ops, T input) {
            return ops.getNumberValue(input).map(n -> Pair.of(n.floatValue(), ops.empty()));
        }
    };

    Codec<Double> DOUBLE = new Codec<>() {
        @Override
        public <T> DataResult<T> encode(Double input, DynamicOps<T> ops, T prefix) {
            return DataResult.success(ops.createDouble(input));
        }

        @Override
        public <T> DataResult<Pair<Double, T>> decode(DynamicOps<T> ops, T input) {
            return ops.getNumberValue(input).map(n -> Pair.of(n.doubleValue(), ops.empty()));
        }
    };

    Codec<String> STRING = new Codec<>() {
        @Override
        public <T> DataResult<T> encode(String input, DynamicOps<T> ops, T prefix) {
            return DataResult.success(ops.createString(input));
        }

        @Override
        public <T> DataResult<Pair<String, T>> decode(DynamicOps<T> ops, T input) {
            return ops.getStringValue(input).map(s -> Pair.of(s, ops.empty()));
        }
    };

    Codec<Unit> EMPTY = MapCodec.unit(Unit.INSTANCE).codec();

    static <A> Codec<A> of(Encoder<A> encoder, Decoder<A> decoder) {
        return new Codec<>() {
            @Override
            public <T> DataResult<T> encode(A input, DynamicOps<T> ops, T prefix) {
                return encoder.encode(input, ops, prefix);
            }

            @Override
            public <T> DataResult<Pair<A, T>> decode(DynamicOps<T> ops, T input) {
                return decoder.decode(ops, input);
            }
        };
    }

    static <A> Codec<A> unit(A value) {
        return unit(() -> value);
    }

    static <A> Codec<A> unit(Supplier<A> value) {
        return MapCodec.unit(value).codec();
    }

    static <E> Codec<List<E>> list(Codec<E> elementCodec) {
        return new Codec<>() {
            @Override
            public <T> DataResult<T> encode(List<E> input, DynamicOps<T> ops, T prefix) {
                ListBuilder<T> builder = ops.listBuilder();
                for (E element : input) {
                    builder.add(elementCodec.encodeStart(ops, element));
                }
                return builder.build(prefix);
            }

            @Override
            public <T> DataResult<Pair<List<E>, T>> decode(DynamicOps<T> ops, T input) {
                return ops.getStream(input).flatMap(stream -> {
                    List<E> out = new ArrayList<>();
                    for (T element : stream.toList()) {
                        DataResult<E> parsed = elementCodec.parse(ops, element);
                        if (parsed.isError()) {
                            return DataResult.error(
                                    () -> parsed.error().map(DataResult.Error::message).orElse("list element error"));
                        }
                        out.add(parsed.getOrThrow());
                    }
                    return DataResult.success(Pair.of(List.copyOf(out), ops.empty()));
                });
            }
        };
    }

    static <K, V> Codec<Map<K, V>> unboundedMap(Codec<K> keyCodec, Codec<V> valueCodec) {
        return new Codec<>() {
            @Override
            public <T> DataResult<T> encode(Map<K, V> input, DynamicOps<T> ops, T prefix) {
                RecordBuilder<T> builder = ops.mapBuilder();
                for (Map.Entry<K, V> entry : input.entrySet()) {
                    builder.add(
                            keyCodec.encodeStart(ops, entry.getKey()),
                            valueCodec.encodeStart(ops, entry.getValue()));
                }
                return builder.build(prefix);
            }

            @Override
            public <T> DataResult<Pair<Map<K, V>, T>> decode(DynamicOps<T> ops, T input) {
                return ops.getMapValues(input).flatMap(stream -> {
                    Map<K, V> out = new LinkedHashMap<>();
                    for (Pair<T, T> pair : stream.toList()) {
                        DataResult<K> k = keyCodec.parse(ops, pair.getFirst());
                        if (k.isError()) {
                            return DataResult.error(() -> k.error().map(DataResult.Error::message).orElse("map key error"));
                        }
                        DataResult<V> v = valueCodec.parse(ops, pair.getSecond());
                        if (v.isError()) {
                            return DataResult.error(() -> v.error().map(DataResult.Error::message).orElse("map value error"));
                        }
                        out.put(k.getOrThrow(), v.getOrThrow());
                    }
                    return DataResult.success(Pair.of(Map.copyOf(out), ops.empty()));
                });
            }
        };
    }

    static <L, R> Codec<Either<L, R>> either(Codec<L> left, Codec<R> right) {
        return new Codec<>() {
            @Override
            public <T> DataResult<T> encode(Either<L, R> input, DynamicOps<T> ops, T prefix) {
                return input.map(
                        l -> left.encode(l, ops, prefix),
                        r -> right.encode(r, ops, prefix));
            }

            @Override
            public <T> DataResult<Pair<Either<L, R>, T>> decode(DynamicOps<T> ops, T input) {
                DataResult<Pair<Either<L, R>, T>> l = left.decode(ops, input).map(p -> p.mapFirst(Either::left));
                if (l.isSuccess()) {
                    return l;
                }
                DataResult<Pair<Either<L, R>, T>> r = right.decode(ops, input).map(p -> p.mapFirst(Either::right));
                if (r.isSuccess()) {
                    return r;
                }
                return DataResult.error(() -> "Failed to decode either: "
                        + l.error().map(DataResult.Error::message).orElse("")
                        + "; " + r.error().map(DataResult.Error::message).orElse(""));
            }
        };
    }

    default Codec<List<A>> listOf() {
        return list(this);
    }

    default <S> Codec<S> xmap(Function<? super A, ? extends S> to, Function<? super S, ? extends A> from) {
        return of(comap(from), map(to));
    }

    default <S> Codec<S> comapFlatMap(
            Function<? super A, ? extends DataResult<? extends S>> to,
            Function<? super S, ? extends A> from) {
        return of(comap(from), flatMap(to));
    }

    default <S> Codec<S> flatComapMap(
            Function<? super A, ? extends S> to,
            Function<? super S, ? extends DataResult<? extends A>> from) {
        return of(flatComap(from), map(to));
    }

    default <S> Codec<S> flatXmap(
            Function<? super A, ? extends DataResult<? extends S>> to,
            Function<? super S, ? extends DataResult<? extends A>> from) {
        return of(flatComap(from), flatMap(to));
    }

    @Override
    default MapCodec<A> fieldOf(String name) {
        return MapCodec.of(
                Encoder.super.fieldOf(name),
                Decoder.super.fieldOf(name));
    }

    default MapCodec<Optional<A>> optionalFieldOf(String name) {
        return new MapCodec<>() {
            @Override
            public <T> RecordBuilder<T> encode(Optional<A> input, DynamicOps<T> ops, RecordBuilder<T> prefix) {
                return input.map(a -> prefix.add(name, Codec.this.encodeStart(ops, a))).orElse(prefix);
            }

            @Override
            public <T> DataResult<Optional<A>> decode(DynamicOps<T> ops, MapLike<T> input) {
                T val = input.get(name);
                if (val == null) {
                    return DataResult.success(Optional.empty(), Lifecycle.stable());
                }
                return Codec.this.parse(ops, val).map(Optional::of);
            }
        };
    }

    default MapCodec<A> optionalFieldOf(String name, A defaultValue) {
        return optionalFieldOf(name).xmap(
                opt -> opt.orElse(defaultValue),
                val -> val == null || val.equals(defaultValue) ? Optional.empty() : Optional.of(val));
    }

    static Codec<Integer> intRange(int minInclusive, int maxInclusive) {
        Function<Integer, DataResult<Integer>> checker = v -> v >= minInclusive && v <= maxInclusive
                ? DataResult.success(v)
                : DataResult.error(
                        () -> "Value " + v + " outside range [" + minInclusive + "," + maxInclusive + "]");
        return INT.flatXmap(checker, checker);
    }
}
