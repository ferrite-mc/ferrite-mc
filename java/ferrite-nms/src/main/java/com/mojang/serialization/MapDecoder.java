// Clean-room Ferrite DFU compatibility code. No Mojang code or assets.
package com.mojang.serialization;

import java.util.function.Function;

/** Map-level deserializer for values of type {@code A}. */
public interface MapDecoder<A> {
    <T> DataResult<A> decode(DynamicOps<T> ops, MapLike<T> input);

    default <B> MapDecoder<B> map(Function<? super A, ? extends B> function) {
        return new MapDecoder<>() {
            @Override
            public <T> DataResult<B> decode(DynamicOps<T> ops, MapLike<T> input) {
                return MapDecoder.this.decode(ops, input).map(function);
            }
        };
    }

    default <B> MapDecoder<B> flatMap(Function<? super A, ? extends DataResult<? extends B>> function) {
        return new MapDecoder<>() {
            @Override
            public <T> DataResult<B> decode(DynamicOps<T> ops, MapLike<T> input) {
                return MapDecoder.this.decode(ops, input)
                        .flatMap(a -> function.apply(a).map(b -> b));
            }
        };
    }

    final class FieldDecoder<A> implements MapDecoder<A> {
        private final String name;
        private final Decoder<A> elementCodec;

        public FieldDecoder(String name, Decoder<A> elementCodec) {
            this.name = name;
            this.elementCodec = elementCodec;
        }

        @Override
        public <T> DataResult<A> decode(DynamicOps<T> ops, MapLike<T> input) {
            T value = input.get(name);
            if (value == null) {
                return DataResult.error(() -> "No key " + name + " in " + input);
            }
            return elementCodec.parse(ops, value);
        }
    }
}
