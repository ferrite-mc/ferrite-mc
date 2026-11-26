// Clean-room Ferrite DFU compatibility code. No Mojang code or assets.
package com.mojang.serialization;

import java.util.function.Function;

/** Serializer for values of type {@code A}. */
public interface Encoder<A> {
    <T> DataResult<T> encode(A input, DynamicOps<T> ops, T prefix);

    default <T> DataResult<T> encodeStart(DynamicOps<T> ops, A input) {
        return encode(input, ops, ops.empty());
    }

    default <B> Encoder<B> comap(Function<? super B, ? extends A> function) {
        return new Encoder<>() {
            @Override
            public <T> DataResult<T> encode(B input, DynamicOps<T> ops, T prefix) {
                return Encoder.this.encode(function.apply(input), ops, prefix);
            }
        };
    }

    default <B> Encoder<B> flatComap(Function<? super B, ? extends DataResult<? extends A>> function) {
        return new Encoder<>() {
            @Override
            public <T> DataResult<T> encode(B input, DynamicOps<T> ops, T prefix) {
                return function.apply(input).flatMap(a -> Encoder.this.encode(a, ops, prefix));
            }
        };
    }

    default MapEncoder<A> fieldOf(String name) {
        return new MapEncoder.FieldEncoder<>(name, this);
    }

    static <A> Encoder<A> empty() {
        return new Encoder<>() {
            @Override
            public <T> DataResult<T> encode(A input, DynamicOps<T> ops, T prefix) {
                return DataResult.success(prefix, Lifecycle.experimental());
            }
        };
    }

    static <A> Encoder<A> error(String message) {
        return new Encoder<>() {
            @Override
            public <T> DataResult<T> encode(A input, DynamicOps<T> ops, T prefix) {
                return DataResult.error(() -> message);
            }
        };
    }
}
