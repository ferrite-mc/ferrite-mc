// Clean-room Ferrite DFU compatibility code. No Mojang code or assets.
package com.mojang.serialization;

import com.mojang.datafixers.util.Pair;
import java.util.function.Function;

/** Deserializer for values of type {@code A}. */
public interface Decoder<A> {
    <T> DataResult<Pair<A, T>> decode(DynamicOps<T> ops, T input);

    default <T> DataResult<A> parse(DynamicOps<T> ops, T input) {
        return decode(ops, input).map(Pair::getFirst);
    }

    default <T> DataResult<A> parse(Dynamic<T> input) {
        return parse(input.getOps(), input.getValue());
    }

    default <B> Decoder<B> map(Function<? super A, ? extends B> function) {
        return new Decoder<>() {
            @Override
            public <T> DataResult<Pair<B, T>> decode(DynamicOps<T> ops, T input) {
                return Decoder.this.decode(ops, input).map(p -> p.mapFirst(function));
            }
        };
    }

    default <B> Decoder<B> flatMap(Function<? super A, ? extends DataResult<? extends B>> function) {
        return new Decoder<>() {
            @Override
            public <T> DataResult<Pair<B, T>> decode(DynamicOps<T> ops, T input) {
                return Decoder.this.decode(ops, input)
                        .flatMap(p -> function.apply(p.getFirst()).map(b -> Pair.of(b, p.getSecond())));
            }
        };
    }

    default MapDecoder<A> fieldOf(String name) {
        return new MapDecoder.FieldDecoder<>(name, this);
    }

    static <A> Decoder<A> unit(A instance) {
        return new Decoder<>() {
            @Override
            public <T> DataResult<Pair<A, T>> decode(DynamicOps<T> ops, T input) {
                return DataResult.success(Pair.of(instance, input), Lifecycle.stable());
            }
        };
    }

    static <A> Decoder<A> error(String message) {
        return new Decoder<>() {
            @Override
            public <T> DataResult<Pair<A, T>> decode(DynamicOps<T> ops, T input) {
                return DataResult.error(() -> message);
            }
        };
    }
}
