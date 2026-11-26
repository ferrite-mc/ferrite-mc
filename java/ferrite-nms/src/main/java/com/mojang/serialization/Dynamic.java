// Clean-room Ferrite DFU compatibility code. No Mojang code or assets.
package com.mojang.serialization;

import java.util.Objects;

/** Immutable value paired with its {@link DynamicOps}. */
public final class Dynamic<T> {
    private final DynamicOps<T> ops;
    private final T value;

    public Dynamic(DynamicOps<T> ops) {
        this(ops, ops.empty());
    }

    public Dynamic(DynamicOps<T> ops, T value) {
        this.ops = Objects.requireNonNull(ops, "ops");
        this.value = value == null ? ops.empty() : value;
    }

    public DynamicOps<T> getOps() {
        return ops;
    }

    public T getValue() {
        return value;
    }

    public <U> Dynamic<U> convert(DynamicOps<U> outOps) {
        return new Dynamic<>(outOps, ops.convertTo(outOps, value));
    }

    public <A> DataResult<A> decode(Decoder<A> decoder) {
        return decoder.parse(ops, value);
    }

    public DataResult<Number> asNumber() {
        return ops.getNumberValue(value);
    }

    public DataResult<String> asString() {
        return ops.getStringValue(value);
    }

    public DataResult<Boolean> asBoolean() {
        return ops.getBooleanValue(value);
    }

    public Dynamic<T> remove(String key) {
        return new Dynamic<>(ops, ops.remove(value, key));
    }

    @Override
    public boolean equals(Object o) {
        return this == o || o instanceof Dynamic<?> d && Objects.equals(ops, d.ops) && Objects.equals(value, d.value);
    }

    @Override
    public int hashCode() {
        return Objects.hash(ops, value);
    }

    @Override
    public String toString() {
        return "Dynamic[" + value + "]";
    }
}
