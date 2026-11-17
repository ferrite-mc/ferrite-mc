// Clean-room Ferrite DFU compatibility code. No Mojang code or assets.
package com.mojang.serialization;

import java.util.function.Function;

/** Map-level serializer for values of type {@code A}. */
public interface MapEncoder<A> {
    <T> RecordBuilder<T> encode(A input, DynamicOps<T> ops, RecordBuilder<T> prefix);

    default <B> MapEncoder<B> comap(Function<? super B, ? extends A> function) {
        return new MapEncoder<>() {
            @Override
            public <T> RecordBuilder<T> encode(B input, DynamicOps<T> ops, RecordBuilder<T> prefix) {
                return MapEncoder.this.encode(function.apply(input), ops, prefix);
            }
        };
    }

    default <B> MapEncoder<B> flatComap(Function<? super B, ? extends DataResult<? extends A>> function) {
        return new MapEncoder<>() {
            @Override
            public <T> RecordBuilder<T> encode(B input, DynamicOps<T> ops, RecordBuilder<T> prefix) {
                DataResult<? extends A> result = function.apply(input);
                return result.result()
                        .map(a -> MapEncoder.this.encode(a, ops, prefix))
                        .orElseGet(() -> prefix.withErrorsFrom(result));
            }
        };
    }

    final class FieldEncoder<A> implements MapEncoder<A> {
        private final String name;
        private final Encoder<A> elementCodec;

        public FieldEncoder(String name, Encoder<A> elementCodec) {
            this.name = name;
            this.elementCodec = elementCodec;
        }

        @Override
        public <T> RecordBuilder<T> encode(A input, DynamicOps<T> ops, RecordBuilder<T> prefix) {
            return prefix.add(name, elementCodec.encodeStart(ops, input));
        }
    }
}
