// Clean-room Ferrite DFU compatibility code. No Mojang code or assets.
package com.mojang.serialization;

import com.mojang.datafixers.kinds.App;
import com.mojang.datafixers.kinds.K1;
import com.mojang.datafixers.util.Pair;
import java.util.function.Function;
import java.util.function.Supplier;

/** Combined map-level encoder and decoder, matching DataFixerUpper's {@code MapCodec<A>}. */
public abstract class MapCodec<A> implements MapEncoder<A>, MapDecoder<A> {

    public static <A> MapCodec<A> of(MapEncoder<A> encoder, MapDecoder<A> decoder) {
        return new MapCodec<>() {
            @Override
            public <T> RecordBuilder<T> encode(A input, DynamicOps<T> ops, RecordBuilder<T> prefix) {
                return encoder.encode(input, ops, prefix);
            }

            @Override
            public <T> DataResult<A> decode(DynamicOps<T> ops, MapLike<T> input) {
                return decoder.decode(ops, input);
            }
        };
    }

    public static <A> MapCodec<A> unit(A value) {
        return unit(() -> value);
    }

    public static <A> MapCodec<A> unit(Supplier<A> value) {
        return new MapCodec<>() {
            @Override
            public <T> RecordBuilder<T> encode(A input, DynamicOps<T> ops, RecordBuilder<T> prefix) {
                return prefix;
            }

            @Override
            public <T> DataResult<A> decode(DynamicOps<T> ops, MapLike<T> input) {
                return DataResult.success(value.get(), Lifecycle.stable());
            }
        };
    }

    public <S> MapCodec<S> xmap(Function<? super A, ? extends S> to, Function<? super S, ? extends A> from) {
        return of(comap(from), map(to));
    }

    public <S> MapCodec<S> flatXmap(
            Function<? super A, ? extends DataResult<? extends S>> to,
            Function<? super S, ? extends DataResult<? extends A>> from) {
        return of(flatComap(from), flatMap(to));
    }

    public Codec<A> codec() {
        return new Codec<>() {
            @Override
            public <T> DataResult<T> encode(A input, DynamicOps<T> ops, T prefix) {
                return MapCodec.this.encode(input, ops, ops.mapBuilder()).build(prefix);
            }

            @Override
            public <T> DataResult<Pair<A, T>> decode(DynamicOps<T> ops, T input) {
                return ops.getMap(input)
                        .flatMap(map -> MapCodec.this.decode(ops, map))
                        .map(a -> Pair.of(a, input));
            }
        };
    }

    public <O> RecordCodecField<O, A> forGetter(Function<O, A> getter) {
        return new RecordCodecField<>(this, getter);
    }

    /** Bound field + getter pair used by {@link com.mojang.serialization.codecs.RecordCodecBuilder}. */
    public static final class RecordCodecField<O, A> implements App<RecordCodecField.Mu<O>, A> {
        public static final class Mu<O> implements K1 {}

        private final MapCodec<A> codec;
        private final Function<O, A> getter;

        public RecordCodecField(MapCodec<A> codec, Function<O, A> getter) {
            this.codec = codec;
            this.getter = getter;
        }

        public MapCodec<A> codec() {
            return codec;
        }

        public Function<O, A> getter() {
            return getter;
        }
    }
}
